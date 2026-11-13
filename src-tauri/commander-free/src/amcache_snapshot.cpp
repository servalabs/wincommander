#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <vss.h>
#include <vswriter.h>
#include <vsbackup.h>
#include <string>
#include <new>

// The snapshot is owned by this object and is never exposed as a drive letter.
// VSS_CTX_BACKUP is non-persistent: VSS also releases it if this process exits.
struct AmcacheSnapshot {
    IVssBackupComponents* backup = nullptr;
    VSS_ID id = GUID_NULL;
    bool com_initialized = false;
    bool created = false;
    bool prepared = false;
};

static HRESULT wait_for(IVssAsync* operation) {
    if (!operation) return E_UNEXPECTED;
    // Several VSS phases run sequentially. Keep each bounded well below the
    // 180-second collector deadline so a slow phase can be cancelled and the
    // snapshot can be released by close_snapshot before the wrapper times out.
    HRESULT hr = operation->Wait(20000);
    HRESULT result = E_FAIL;
    INT reserved = 0;
    if (SUCCEEDED(hr)) hr = operation->QueryStatus(&result, &reserved);
    if (SUCCEEDED(hr)) {
        if (result == VSS_S_ASYNC_PENDING) { operation->Cancel(); (void)operation->Wait(5000); hr = HRESULT_FROM_WIN32(ERROR_TIMEOUT); }
        else if (result == VSS_S_ASYNC_FINISHED) hr = S_OK;
        else hr = FAILED(result) ? result : E_ABORT;
    } else { (void)operation->Cancel(); (void)operation->Wait(5000); }
    operation->Release();
    return hr;
}

static HRESULT verify_registry_writer(IVssBackupComponents* backup) {
    // The registry writer flushes registered system hives before the snapshot.
    // Its success does not imply that every application-managed hive was flushed.
    static const VSS_ID registry_writer = {0xafbab4a2, 0x367d, 0x4d15, {0xa5, 0x86, 0x71, 0xdb, 0xb1, 0x8f, 0x84, 0x85}};
    IVssAsync* operation = nullptr;
    HRESULT hr = backup->GatherWriterStatus(&operation);
    if (SUCCEEDED(hr)) hr = wait_for(operation);
    if (FAILED(hr)) return hr;
    UINT count = 0;
    hr = backup->GetWriterStatusCount(&count);
    bool found = false;
    for (UINT i = 0; SUCCEEDED(hr) && i < count; ++i) {
        VSS_ID instance = GUID_NULL, writer = GUID_NULL;
        BSTR name = nullptr; VSS_WRITER_STATE state = VSS_WS_UNKNOWN; HRESULT failure = E_FAIL;
        hr = backup->GetWriterStatus(i, &instance, &writer, &name, &state, &failure);
        if (name) SysFreeString(name);
        if (SUCCEEDED(hr) && IsEqualGUID(writer, registry_writer)) {
            found = true;
            if (FAILED(failure) || (state != VSS_WS_STABLE && state != VSS_WS_WAITING_FOR_BACKUP_COMPLETE)) hr = E_FAIL;
        }
    }
    (void)backup->FreeWriterStatus();
    if (SUCCEEDED(hr) && !found) hr = E_FAIL;
    return hr;
}

static HRESULT close_snapshot(AmcacheSnapshot* s) {
    if (!s) return S_OK;
    HRESULT cleanup = S_OK;
    if (s->backup) {
        if (s->created) {
            if (s->prepared) {
                IVssAsync* operation = nullptr;
                if (SUCCEEDED(s->backup->BackupComplete(&operation))) (void)wait_for(operation);
            }
        } else if (s->prepared) {
            (void)s->backup->AbortBackup();
        }
        if (!IsEqualGUID(s->id, GUID_NULL)) {
            LONG deleted = 0; VSS_ID failed = GUID_NULL;
            HRESULT hr = s->backup->DeleteSnapshots(s->id, VSS_OBJECT_SNAPSHOT, TRUE, &deleted, &failed);
            if (s->created && (FAILED(hr) || deleted != 1)) cleanup = FAILED(hr) ? hr : E_FAIL;
        }
        s->backup->Release();
    }
    if (s->com_initialized) CoUninitialize();
    delete s;
    return cleanup;
}

extern "C" __declspec(dllexport) HRESULT wc_amcache_snapshot_close(void* handle) {
    return close_snapshot(static_cast<AmcacheSnapshot*>(handle));
}

extern "C" __declspec(dllexport) HRESULT wc_amcache_snapshot_open(void** handle, wchar_t* output, DWORD capacity) {
    if (!handle || !output || capacity == 0) return E_INVALIDARG;
    *handle = nullptr;
    auto* s = new (std::nothrow) AmcacheSnapshot();
    if (!s) return E_OUTOFMEMORY;
    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (SUCCEEDED(hr)) s->com_initialized = true;
    else { close_snapshot(s); return hr; }
    hr = CoInitializeSecurity(nullptr, -1, nullptr, nullptr, RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
                              RPC_C_IMP_LEVEL_IMPERSONATE, nullptr, EOAC_DYNAMIC_CLOAKING, nullptr);
    if (hr == RPC_E_TOO_LATE) hr = S_OK; // COM security was already established by this process.
    if (FAILED(hr)) { close_snapshot(s); return hr; }

    wchar_t windows_dir[MAX_PATH + 1] = {};
    wchar_t volume_root[MAX_PATH + 1] = {};
    wchar_t volume_name[MAX_PATH + 1] = {};
    if (!GetWindowsDirectoryW(windows_dir, MAX_PATH) ||
        !GetVolumePathNameW(windows_dir, volume_root, MAX_PATH) ||
        !GetVolumeNameForVolumeMountPointW(volume_root, volume_name, MAX_PATH)) {
        hr = HRESULT_FROM_WIN32(GetLastError()); close_snapshot(s); return hr;
    }
    ULARGE_INTEGER available = {};
    if (!GetDiskFreeSpaceExW(volume_root, &available, nullptr, nullptr)) {
        hr = HRESULT_FROM_WIN32(GetLastError()); close_snapshot(s); return hr;
    }
    // Avoid starting a shadow operation on an almost full system volume.
    if (available.QuadPart < 1024ULL * 1024 * 1024) {
        close_snapshot(s); return HRESULT_FROM_WIN32(ERROR_DISK_FULL);
    }
    std::wstring root(volume_root), win(windows_dir);
    if (win.size() < root.size() || _wcsnicmp(win.c_str(), root.c_str(), root.size()) != 0) {
        close_snapshot(s); return E_FAIL;
    }
    hr = CreateVssBackupComponents(&s->backup);
    if (SUCCEEDED(hr)) hr = s->backup->InitializeForBackup();
    if (SUCCEEDED(hr)) hr = s->backup->SetContext(VSS_CTX_BACKUP);
    if (SUCCEEDED(hr)) hr = s->backup->SetBackupState(FALSE, FALSE, VSS_BT_COPY, FALSE);
    IVssAsync* operation = nullptr;
    if (SUCCEEDED(hr)) { hr = s->backup->GatherWriterMetadata(&operation); if (SUCCEEDED(hr)) hr = wait_for(operation); }
    VSS_ID set_id = GUID_NULL;
    if (SUCCEEDED(hr)) hr = s->backup->StartSnapshotSet(&set_id);
    if (SUCCEEDED(hr)) hr = s->backup->AddToSnapshotSet(volume_name, GUID_NULL, &s->id);
    if (SUCCEEDED(hr)) {
        operation = nullptr; hr = s->backup->PrepareForBackup(&operation);
        if (SUCCEEDED(hr)) { s->prepared = true; hr = wait_for(operation); }
    }
    if (SUCCEEDED(hr)) {
        operation = nullptr; hr = s->backup->DoSnapshotSet(&operation);
        if (SUCCEEDED(hr)) hr = wait_for(operation);
        if (SUCCEEDED(hr)) s->created = true;
    }
    if (SUCCEEDED(hr)) hr = verify_registry_writer(s->backup);
    VSS_SNAPSHOT_PROP properties = {};
    if (SUCCEEDED(hr)) {
        hr = s->backup->GetSnapshotProperties(s->id, &properties);
        if (SUCCEEDED(hr)) {
            if (!properties.m_pwszSnapshotDeviceObject || !*properties.m_pwszSnapshotDeviceObject) hr = E_FAIL;
            else {
                std::wstring path = properties.m_pwszSnapshotDeviceObject;
                path += L"\\" + win.substr(root.size()) + L"\\AppCompat\\Programs\\Amcache.hve";
                if (path.size() + 1 > capacity) hr = HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER);
                else wcscpy_s(output, capacity, path.c_str());
            }
            VssFreeSnapshotProperties(&properties);
        }
    }
    if (FAILED(hr)) { close_snapshot(s); return hr; }
    *handle = s;
    return S_OK;
}
