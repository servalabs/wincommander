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
const FENCE_PROTOCOL: &str = "wincommander-search-fence-v1";
/// The gated service must be built as a pair with one independently reviewed
/// Pro binary. Missing build pins fail closed rather than trusting Hello claims.
fn expected_pair<'a>(hash: Option<&'a str>, app: Option<&str>, protocol: Option<&str>) -> io::Result<&'a str> {
    let hash = hash.ok_or_else(denied)?;
    let expected_app = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || app != Some(expected_app) || protocol != Some(FENCE_PROTOCOL) { return Err(denied()); }
    Ok(hash)
}
pub(super) fn capability() -> serde_json::Value {
    let paired = expected_pair(option_env!("WINCMD_FENCED_PRO_SHA256"),
        option_env!("WINCMD_FENCED_PRO_APP_DIR"), option_env!("WINCMD_FENCED_PRO_PROTOCOL")).is_ok();
    serde_json::json!({
        "protocol": 1,
        "broker_user_fence": paired,
        "system_search_disabled": paired,
        "app_dir": option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander"),
    })
}
pub(super) struct PinnedPro { _parents: Vec<File>, file: File }
impl PinnedPro {
    pub(super) fn matches(&self, other: &File) -> bool {
        fn identity(file: &File) -> Option<(u32, u32, u32)> {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 { return None; }
            Some((info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
        }
        let left = identity(&self.file);
        left.is_some() && left == identity(other)
    }
}
pub(super) fn pin_expected_pro(path: &Path) -> io::Result<PinnedPro> {
    let expected = expected_pair(option_env!("WINCMD_FENCED_PRO_SHA256"),
        option_env!("WINCMD_FENCED_PRO_APP_DIR"), option_env!("WINCMD_FENCED_PRO_PROTOCOL"))?;
    pin_and_verify(path, expected)
}
fn pin_and_verify(path: &Path, expected: &str) -> io::Result<PinnedPro> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek, SeekFrom};
    let parents = pin_directories(path.parent().ok_or_else(denied)?)?;
    let mut file = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
    let metadata = file.metadata()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 || info.nNumberOfLinks != 1 { return Err(denied()); }
    let mut digest = Sha256::new();
    let mut block = [0u8; 65536];
    loop {
        let read = file.read(&mut block)?;
        if read == 0 { break; }
        digest.update(&block[..read]);
    }
    if !super::bytes_to_hex(&digest.finalize()).eq_ignore_ascii_case(expected) { return Err(denied()); }
    file.seek(SeekFrom::Start(0))?;
    Ok(PinnedPro { _parents: parents, file })
}
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
fn pin_directories(parent: &Path) -> io::Result<Vec<File>> {
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
    Ok(pins)
}
fn acquire_at(parent: &Path, create: bool) -> io::Result<SearchBrokerFence> {
    let pins = pin_directories(parent)?;
    // Creation is permitted only while impersonating the authenticated user.
    let file = OpenOptions::new().read(true).write(true).create(create).truncate(false)
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
    provision_under_token(token, None)
}
fn provision_under_token(token: HANDLE, fixture_parent: Option<&Path>) -> io::Result<SearchBrokerFence> {
    use windows_sys::Win32::Security::{DuplicateTokenEx, ImpersonateLoggedOnUser, RevertToSelf,
        SecurityImpersonation, TokenImpersonation, TOKEN_IMPERSONATE, TOKEN_QUERY};
    let mut impersonation: HANDLE = std::ptr::null_mut();
    if unsafe { DuplicateTokenEx(token, TOKEN_QUERY | TOKEN_IMPERSONATE, std::ptr::null_mut(),
        SecurityImpersonation, TokenImpersonation, &mut impersonation) } == 0 { return Err(denied()); }
    struct UserToken(HANDLE);
    impl Drop for UserToken { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
    let impersonation = UserToken(impersonation);
    let parent = match fixture_parent {
        Some(path) => path.to_path_buf(),
        None => user_search_root(impersonation.0)?,
    };
    let app = parent.parent().ok_or_else(denied)?;
    let base = app.parent().ok_or_else(denied)?;
    if unsafe { ImpersonateLoggedOnUser(impersonation.0) } == 0 { return Err(denied()); }
    struct RevertGuard;
    impl Drop for RevertGuard { fn drop(&mut self) { if unsafe { RevertToSelf() } == 0 { std::process::abort(); } } }
    let revert = RevertGuard;
    let result = (|| {
        let _base_pins = pin_directories(base)?;
        match std::fs::create_dir(app) { Ok(()) => {}, Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}, Err(e) => return Err(e) }
        let _app_pins = pin_directories(app)?;
        match std::fs::create_dir(&parent) { Ok(()) => {}, Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}, Err(e) => return Err(e) }
        acquire_at(&parent, true)
    })();
    drop(revert);
    result
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
/// Retain a lease until service exit if OS process termination cannot be
/// confirmed; a leaked lease is safer than allowing cleanup under a live child.
fn preserve_if_unconfirmed<T>(lease: T, confirmed: bool) {
    if !confirmed { std::mem::forget(lease); }
}
fn stop_child_or_preserve(slot: &Arc<Mutex<Option<SearchBrokerFence>>>, process: HANDLE) {
    unsafe { TerminateProcess(process, 1); }
    let confirmed = unsafe { WaitForSingleObject(process, 5_000) } == WAIT_OBJECT_0;
    preserve_if_unconfirmed(Arc::clone(slot), confirmed);
    unsafe { CloseHandle(process); }
}
/// Move the exact held lock to a supervisor waiting on a duplicate child handle.
/// Failure kills and waits for the child while the original lock is still held.
pub(super) fn hold_until_process_exit(fence: SearchBrokerFence, process: HANDLE) -> io::Result<()> {
    let slot = Arc::new(Mutex::new(Some(fence)));
    let clone = Arc::clone(&slot);
    let mut duplicate: HANDLE = std::ptr::null_mut();
    let current = unsafe { GetCurrentProcess() };
    if unsafe { DuplicateHandle(current, process, current, &mut duplicate, 0, 0, DUPLICATE_SAME_ACCESS) } == 0 {
        stop_child_or_preserve(&slot, process);
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
        stop_child_or_preserve(&slot, process);
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
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join(".encrypted-search-maintenance.lock"), b"").unwrap();
        root
    }
    fn open_probe(root: &Path) -> File {
        OpenOptions::new().read(true).write(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(root.join(".encrypted-search-maintenance.lock")).unwrap()
    }
    #[test]
    fn capability_is_false_when_pair_inputs_are_missing() {
        if option_env!("WINCMD_FENCED_PRO_SHA256").is_none() {
            let capability = capability();
            assert_eq!(capability["protocol"], 1);
            assert_eq!(capability["broker_user_fence"], false);
            assert_eq!(capability["system_search_disabled"], false);
        }
    }
    #[test]
    fn paired_pin_requires_exact_protocol_and_app_directory() {
        let hash = "a".repeat(64);
        let app = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
        assert!(expected_pair(Some(&hash), Some(app), Some(FENCE_PROTOCOL)).is_ok());
        assert!(expected_pair(None, Some(app), Some(FENCE_PROTOCOL)).is_err());
        assert!(expected_pair(Some(&hash), Some("other"), Some(FENCE_PROTOCOL)).is_err());
        assert!(expected_pair(Some(&hash), Some(app), Some("old-protocol")).is_err());
        assert!(expected_pair(Some("not-a-hash"), Some(app), Some(FENCE_PROTOCOL)).is_err());
    }
    #[test]
    fn binary_pin_rejects_wrong_hash_and_denies_replacement() {
        use sha2::{Digest, Sha256};
        let root = fixture(); let path = root.join("fenced-pro.exe");
        std::fs::write(&path, b"reviewed binary").unwrap();
        let hash = super::super::bytes_to_hex(&Sha256::digest(b"reviewed binary"));
        assert!(pin_and_verify(&path, &"0".repeat(64)).is_err());
        let held = pin_and_verify(&path, &hash).unwrap();
        assert!(std::fs::write(&path, b"replacement").is_err());
        assert!(std::fs::remove_file(&path).is_err());
        let moved_root = root.with_extension("moved");
        assert!(std::fs::rename(&root, &moved_root).is_err());
        let other = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(&path).unwrap();
        assert!(held.matches(&other));
        drop(other);
        drop(held);
        assert!(std::fs::remove_file(&path).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn pinned_binary_can_launch_and_matches_child_image() {
        use sha2::{Digest, Sha256};
        use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;
        let root = fixture(); let path = root.join("fenced-pro.exe");
        let system = std::env::var_os("SystemRoot").unwrap();
        std::fs::copy(PathBuf::from(system).join("System32").join("cmd.exe"), &path).unwrap();
        let hash = super::super::bytes_to_hex(&Sha256::digest(std::fs::read(&path).unwrap()));
        let held = pin_and_verify(&path, &hash).unwrap();
        let mut child = Command::new(&path).args(["/C", "ping -n 20 127.0.0.1 >nul"]).stdout(Stdio::null()).spawn().unwrap();
        let mut image = vec![0u16; 32768]; let mut len = image.len() as u32;
        assert_ne!(unsafe { QueryFullProcessImageNameW(child.as_raw_handle(), 0, image.as_mut_ptr(), &mut len) }, 0);
        let actual = PathBuf::from(std::ffi::OsString::from_wide(&image[..len as usize]));
        let file = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(actual).unwrap();
        assert!(held.matches(&file));
        child.kill().unwrap(); child.wait().unwrap();
        drop(file); drop(held);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn unconfirmed_child_exit_retains_lease() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct DropProbe(Arc<AtomicUsize>);
        impl Drop for DropProbe { fn drop(&mut self) { self.0.fetch_add(1, Ordering::SeqCst); } }
        let count = Arc::new(AtomicUsize::new(0));
        preserve_if_unconfirmed(DropProbe(Arc::clone(&count)), false);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        preserve_if_unconfirmed(DropProbe(Arc::clone(&count)), true);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn broker_does_not_create_a_missing_user_lock() {
        let root = fixture();
        std::fs::remove_file(root.join(".encrypted-search-maintenance.lock")).unwrap();
        assert!(acquire_at(&root, false).is_err());
        assert!(!root.join(".encrypted-search-maintenance.lock").exists());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn user_token_provisions_disposable_missing_profile_lock() {
        use windows_sys::Win32::{Security::{TOKEN_DUPLICATE, TOKEN_QUERY}, System::Threading::OpenProcessToken};
        let base = std::env::temp_dir().join(format!("wc-broker-token-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&base).unwrap();
        let parent = base.join("app").join("file-search");
        let mut token: HANDLE = std::ptr::null_mut();
        assert_ne!(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut token) }, 0);
        let lease = provision_under_token(token, Some(&parent)).unwrap();
        unsafe { CloseHandle(token); }
        let probe = open_probe(&parent);
        assert!(probe.try_lock().is_err());
        drop(lease);
        probe.try_lock().unwrap(); drop(probe);
        let _ = std::fs::remove_dir_all(base);
    }
    #[test]
    fn broker_lease_excludes_cleanup_and_releases_on_drop() {
        let root = fixture();
        let lease = acquire_at(&root, false).unwrap();
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
        let lease = acquire_at(&root, false).unwrap();
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
