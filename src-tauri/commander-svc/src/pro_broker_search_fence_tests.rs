// SPDX-License-Identifier: AGPL-3.0-or-later
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
        assert_eq!(capability["epoch"], "wincommander-paired-search-fence-2026-11-v1");
        assert_eq!(capability["broker_user_fence"], false);
        assert_eq!(capability["system_search_disabled"], false);
    }
}
#[test]
fn capability_requires_the_complete_guarded_pair() {
    let hash = "a".repeat(64);
    let app = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
    let valid = capability_for(Some(&hash), Some(app), Some(FENCE_PROTOCOL));
    assert_eq!(valid["broker_user_fence"], true);
    assert_eq!(valid["system_search_disabled"], true);
    assert_eq!(valid["pro_sha256"], hash);
    for invalid in [capability_for(None, Some(app), Some(FENCE_PROTOCOL)),
        capability_for(Some(&hash), Some("other"), Some(FENCE_PROTOCOL)),
        capability_for(Some(&hash), Some(app), Some("other"))] {
        assert_eq!(invalid["broker_user_fence"], false);
        assert_eq!(invalid["system_search_disabled"], false);
        assert!(invalid["pro_sha256"].is_null());
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
    assert!(expected_pair(Some(&"0".repeat(64)), Some(app), Some(FENCE_PROTOCOL)).is_err());
}
#[test]
fn binary_pin_rejects_wrong_hash_and_denies_replacement() {
    use sha2::{Digest, Sha256};
    let root = fixture(); let path = root.join("fenced-pro.exe");
    std::fs::write(&path, b"reviewed binary").unwrap();
    let hash = super::super::bytes_to_hex(&Sha256::digest(b"reviewed binary"));
    assert!(pin_and_verify(&path, &"0".repeat(64)).is_err());
    let app = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
    let advertised = capability_for(Some(&hash), Some(app), Some(FENCE_PROTOCOL));
    let advertised_hash = advertised["pro_sha256"].as_str().unwrap();
    let held = pin_and_verify(&path, advertised_hash).unwrap();
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
#[ignore = "requires a disposable second Windows account and ACL fixture"]
fn second_user_token_creates_own_search_fence() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::{LogonUserW, LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT};
    let username = std::env::var_os("WINCMD_TEST_BROKER_USER").expect("missing fixture account");
    let password = std::env::var_os("WINCMD_TEST_BROKER_PASSWORD").expect("missing fixture credential");
    let parent = PathBuf::from(std::env::var_os("WINCMD_TEST_BROKER_PARENT").expect("missing fixture parent"));
    let wide = |value: &std::ffi::OsStr| value.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let user = wide(&username); let pass = wide(&password); let domain = wide(std::ffi::OsStr::new("."));
    let mut token: HANDLE = std::ptr::null_mut();
    assert_ne!(unsafe { LogonUserW(user.as_ptr(), domain.as_ptr(), pass.as_ptr(), LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, &mut token) }, 0);
    let first = provision_under_token(token, Some(&parent)).unwrap();
    let second = provision_under_token(token, Some(&parent)).unwrap();
    unsafe { CloseHandle(token); }
    let probe = open_probe(&parent);
    assert!(probe.try_lock().is_err());
    drop((first, second));
    probe.try_lock().unwrap();
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
