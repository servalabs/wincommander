// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opt-in broker lease for the authenticated caller's user search profile.
//! This alone does not fence SYSTEM helpers, old direct Pro copies, or a child
//! surviving a service crash. Do not enable until paired process fencing exists.
use std::{fs::{File, OpenOptions}, io, os::windows::{ffi::OsStringExt, fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle}, path::{Path, PathBuf}, sync::{Arc, Mutex}};
use windows_sys::Win32::{Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE, WAIT_OBJECT_0},
    Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle},
    System::{Com::CoTaskMemFree, Threading::{GetCurrentProcess, TerminateProcess, WaitForSingleObject, INFINITE},},
    UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath}};

pub(super) struct SearchBrokerFence { _pins: Vec<File>, _file: File }
fn denied() -> io::Error { io::Error::new(io::ErrorKind::PermissionDenied, "user search fence unavailable") }
fn user_search_root(token: HANDLE) -> io::Result<PathBuf> {
    if token.is_null() { return Err(denied()); }
    let mut raw = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, token, &mut raw) } < 0 || raw.is_null() {
        if !raw.is_null() { unsafe { CoTaskMemFree(raw.cast()); } }
        return Err(denied());
    }
    let base = unsafe {
        let mut len = 0usize;
        while *raw.add(len) != 0 { len += 1; }
        let path = PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, len)));
        CoTaskMemFree(raw.cast());
        path
    };
    if !base.is_absolute() { return Err(denied()); }
    let app = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
    let mut components = Path::new(app).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_))) || components.next().is_some() { return Err(denied()); }
    Ok(base.join(app).join("file-search"))
}
fn acquire_at(parent: &Path) -> io::Result<SearchBrokerFence> {
    if !parent.is_absolute() { return Err(denied()); }
    let mut ancestors: Vec<_> = parent.ancestors().collect(); ancestors.reverse();
    let mut pins = Vec::new();
    for path in ancestors {
        let directory = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
        let meta = directory.metadata()?;
        if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 { return Err(denied()); }
        pins.push(directory);
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(parent.join(".encrypted-search-maintenance.lock"))?;
    let meta = file.metadata()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 || info.nNumberOfLinks != 1 { return Err(denied()); }
    file.try_lock_shared().map_err(|error| io::Error::other(error.to_string()))?;
    Ok(SearchBrokerFence { _pins: pins, _file: file })
}
pub(super) fn acquire_for_user_token(token: HANDLE, session: u32, sid: &str) -> io::Result<SearchBrokerFence> {
    if !super::token_matches_authenticated_client(token, session, sid) { return Err(denied()); }
    acquire_at(&user_search_root(token)?)
}
pub(super) fn acquire_for_session(session: u32, sid: &str) -> io::Result<SearchBrokerFence> {
    use windows_sys::Win32::System::RemoteDesktop::WTSQueryUserToken;
    if session == 0 { return Err(denied()); }
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { WTSQueryUserToken(session, &mut token) } == 0 { return Err(denied()); }
    let result = acquire_for_user_token(token, session, sid);
    unsafe { CloseHandle(token); }
    result
}
struct Process(HANDLE);
impl Process { fn wait(&self) -> u32 { unsafe { WaitForSingleObject(self.0, INFINITE) } } }
unsafe impl Send for Process {}
impl Drop for Process { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
/// Move the exact held lock to a supervisor waiting on a duplicate child handle.
/// Failure kills and waits for the child while the original lock is still held.
pub(super) fn hold_until_process_exit(fence: SearchBrokerFence, process: HANDLE) -> io::Result<()> {
    let slot = Arc::new(Mutex::new(Some(fence)));
    let clone = Arc::clone(&slot);
    let mut duplicate: HANDLE = std::ptr::null_mut();
    let current = unsafe { GetCurrentProcess() };
    if unsafe { DuplicateHandle(current, process, current, &mut duplicate, 0, 0, DUPLICATE_SAME_ACCESS) } == 0 {
        unsafe { TerminateProcess(process, 1); WaitForSingleObject(process, INFINITE); CloseHandle(process); }
        return Err(denied());
    }
    let handle = Process(duplicate);
    let worker = std::thread::Builder::new().name("pro-search-fence".into()).spawn(move || {
        let result = handle.wait();
        if result != WAIT_OBJECT_0 {
            // If the wait cannot prove termination, hold the lease until the
            // service exits instead of silently allowing destructive cleanup.
            loop { std::thread::park(); }
        }
        drop(clone);
    });
    if worker.is_err() {
        unsafe { TerminateProcess(process, 1); WaitForSingleObject(process, INFINITE); CloseHandle(process); }
        return Err(denied());
    }
    drop(slot);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::windows::io::AsRawHandle, process::{Command, Stdio}, time::{Duration, Instant}};
    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!("wc-broker-fence-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap(); root
    }
    fn open_probe(root: &Path) -> File {
        OpenOptions::new().read(true).write(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(root.join(".encrypted-search-maintenance.lock")).unwrap()
    }
    #[test]
    fn broker_lease_excludes_cleanup_and_releases_on_drop() {
        let root = fixture();
        let lease = acquire_at(&root).unwrap();
        let probe = open_probe(&root);
        assert!(probe.try_lock().is_err());
        drop(lease);
        probe.try_lock().unwrap();
        drop(probe);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn broker_lease_survives_request_owner_until_child_exits() {
        let root = fixture();
        let lease = acquire_at(&root).unwrap();
        let mut child = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 30"]).stdout(Stdio::null()).spawn().unwrap();
        hold_until_process_exit(lease, child.as_raw_handle()).unwrap();
        let probe = open_probe(&root);
        assert!(probe.try_lock().is_err());
        child.kill().unwrap(); child.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if probe.try_lock().is_ok() { break; }
            assert!(Instant::now() < deadline, "lease survived confirmed child exit");
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(probe);
        let _ = std::fs::remove_dir_all(root);
    }
}
