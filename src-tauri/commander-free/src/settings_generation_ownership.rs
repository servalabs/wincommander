// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unwired ownership seam for a future settings-generation cutover.
//! Callers must authorize and pin the directory and require all participating
//! writers to use this guard. It cannot fence older binaries or detect a crash.
#![allow(dead_code)]
use std::{fs::{File, OpenOptions}, io, path::Path};

pub(super) struct SettingsGenerationLock { _file: File }

impl SettingsGenerationLock {
    pub(super) fn try_acquire(root: &Path) -> io::Result<Self> { Self::try_acquire_mode(root, false, std::time::Duration::from_secs(2)) }
    pub(super) fn try_acquire_shared(root: &Path) -> io::Result<Self> { Self::try_acquire_mode(root, true, std::time::Duration::from_secs(2)) }
    fn try_acquire_mode(root: &Path, shared: bool, timeout: std::time::Duration) -> io::Result<Self> {
        let path = root.join(".settings-generation.lock");
        let mut options = OpenOptions::new();
        // Lock file must be pre-provisioned with the intended cross-user ACL.
        // Readers only request read access; neither path creates or repairs it.
        options.read(true).write(!shared);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE};
            // Denying delete sharing keeps the lock name bound while held.
            options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        #[cfg(not(windows))]
        if path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(io::Error::other("settings generation lock is a link"));
        }
        let file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() { return Err(io::Error::other("invalid settings generation lock")); }
        #[cfg(windows)]
        {
            use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
            use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT};
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: the handle and information buffer are valid during the call.
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
                || info.nNumberOfLinks != 1 {
                return Err(io::Error::other("invalid settings generation lock"));
            }
        }
        let started = std::time::Instant::now();
        loop {
            let result = if shared { file.try_lock_shared() } else { file.try_lock() };
            match result {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if started.elapsed() < timeout => {
                    std::thread::sleep(std::time::Duration::from_millis(25).min(timeout.saturating_sub(started.elapsed())));
                }
                Err(std::fs::TryLockError::WouldBlock) => return Err(io::Error::new(io::ErrorKind::WouldBlock, "settings are busy; try again")),
                Err(std::fs::TryLockError::Error(e)) => return Err(e),
            }
        }
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Another test's short-lived subprocess may inherit open lock descriptors
    // during process launch, so serialize the parent-side fixture tests.
    static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn immediate(root: &Path, shared: bool) -> io::Result<SettingsGenerationLock> {
        SettingsGenerationLock::try_acquire_mode(root, shared, std::time::Duration::ZERO)
    }
    #[test]
    fn guard_excludes_other_owners_and_releases_on_drop() {
        let _fixture = TEST_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".settings-generation.lock"), b"").unwrap();
        let guard = immediate(dir.path(), false).unwrap();
        assert!(immediate(dir.path(), false).is_err());
        drop(guard);
        immediate(dir.path(), false).unwrap();
    }
    #[test]
    fn shared_readers_coexist_and_exclude_writer() {
        let _fixture = TEST_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        assert!(immediate(dir.path(), true).is_err());
        assert!(immediate(dir.path(), false).is_err());
        assert!(!dir.path().join(".settings-generation.lock").exists());
        std::fs::write(dir.path().join(".settings-generation.lock"), b"").unwrap();
        let first = immediate(dir.path(), true).unwrap();
        let second = immediate(dir.path(), true).unwrap();
        assert!(immediate(dir.path(), false).is_err());
        drop(first);
        assert!(immediate(dir.path(), false).is_err());
        drop(second);
        let writer = immediate(dir.path(), false).unwrap();
        assert!(immediate(dir.path(), true).is_err());
        drop(writer);
        assert!(immediate(dir.path(), true).is_ok());
    }
    #[test]
    fn reader_waits_for_writer_and_timeout_is_reported_as_busy() {
        let _fixture = TEST_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".settings-generation.lock"), b"").unwrap();
        let guard = immediate(dir.path(), false).unwrap();
        let path = dir.path().to_path_buf();
        let reader = std::thread::spawn(move || SettingsGenerationLock::try_acquire_mode(&path, true, std::time::Duration::from_secs(1)));
        std::thread::sleep(std::time::Duration::from_millis(100));
        drop(guard);
        drop(reader.join().unwrap().unwrap());
        let _guard = immediate(dir.path(), false).unwrap();
        let error = match SettingsGenerationLock::try_acquire_mode(dir.path(), true, std::time::Duration::from_millis(50)) {
            Ok(_) => panic!("reader unexpectedly acquired writer lock"),
            Err(e) => e,
        };
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(error.to_string().contains("busy"));
    }
    #[test]
    fn lock_child_probe() {
        let Some(root) = std::env::var_os("WC_ROTATION_LOCK_TEST_ROOT") else { return; };
        let expect_available = std::env::var_os("WC_ROTATION_LOCK_EXPECT_AVAILABLE").is_some();
        assert_eq!(immediate(Path::new(&root), false).is_ok(), expect_available);
    }
    #[test]
    fn guard_excludes_a_separate_process() {
        let _fixture = TEST_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".settings-generation.lock"), b"").unwrap();
        let guard = immediate(dir.path(), false).unwrap();
        let run_child = |available: bool| {
            let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
            cmd.arg("lock_child_probe").env("WC_ROTATION_LOCK_TEST_ROOT", dir.path());
            if available { cmd.env("WC_ROTATION_LOCK_EXPECT_AVAILABLE", "1"); }
            else { cmd.env_remove("WC_ROTATION_LOCK_EXPECT_AVAILABLE"); }
            assert!(cmd.status().unwrap().success());
        };
        run_child(false);
        drop(guard);
        run_child(true);
    }
    #[cfg(windows)]
    #[test]
    fn held_guard_prevents_lock_file_replacement() {
        let _fixture = TEST_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".settings-generation.lock"), b"").unwrap();
        let guard = immediate(dir.path(), false).unwrap();
        assert!(std::fs::remove_file(dir.path().join(".settings-generation.lock")).is_err());
        drop(guard);
        std::fs::remove_file(dir.path().join(".settings-generation.lock")).unwrap();
    }
}
