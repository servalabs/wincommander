// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unwired ownership seam for a future settings-generation cutover.
//! Callers must authorize and pin the directory and require all participating
//! writers to use this guard. It cannot fence older binaries or detect a crash.
#![allow(dead_code)]
use std::{fs::{File, OpenOptions}, io, path::Path};

pub(super) struct SettingsGenerationLock { _file: File }

impl SettingsGenerationLock {
    pub(super) fn try_acquire(root: &Path) -> io::Result<Self> {
        let path = root.join(".settings-generation.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
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
        file.try_lock().map_err(|e| io::Error::other(format!("settings generation is busy: {e}")))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guard_excludes_other_owners_and_releases_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let guard = SettingsGenerationLock::try_acquire(dir.path()).unwrap();
        assert!(SettingsGenerationLock::try_acquire(dir.path()).is_err());
        drop(guard);
        SettingsGenerationLock::try_acquire(dir.path()).unwrap();
    }
    #[test]
    fn lock_child_probe() {
        let Some(root) = std::env::var_os("WC_ROTATION_LOCK_TEST_ROOT") else { return; };
        let expect_available = std::env::var_os("WC_ROTATION_LOCK_EXPECT_AVAILABLE").is_some();
        assert_eq!(SettingsGenerationLock::try_acquire(Path::new(&root)).is_ok(), expect_available);
    }
    #[test]
    fn guard_excludes_a_separate_process() {
        let dir = tempfile::tempdir().unwrap();
        let guard = SettingsGenerationLock::try_acquire(dir.path()).unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let guard = SettingsGenerationLock::try_acquire(dir.path()).unwrap();
        assert!(std::fs::remove_file(dir.path().join(".settings-generation.lock")).is_err());
        drop(guard);
        std::fs::remove_file(dir.path().join(".settings-generation.lock")).unwrap();
    }
}
