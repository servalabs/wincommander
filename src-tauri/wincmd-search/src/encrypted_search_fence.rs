// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental cooperating runtime/maintenance fence. Old binaries do not
//! acquire this lock; installer shutdown and launch control remain necessary.
use std::{fs::File, io, path::Path};
use std::os::windows::{fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetFileInformationByHandle};

pub struct SearchOperationFence { _pins: Vec<File>, _file: File, parent: std::path::PathBuf, exclusive: bool }

impl SearchOperationFence {
    /// Prepare user root and search parent below the caller's verified known folder.
    pub fn prepare_from_known_folder(base: &Path, app_name: &str) -> io::Result<std::path::PathBuf> {
        let mut parts = Path::new(app_name).components();
        if !matches!(parts.next(), Some(std::path::Component::Normal(_))) || parts.next().is_some() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid application directory"));
        }
        let _pins = crate::read_only::CheckedDirectory::pin_root_path(base)?;
        let root = base.join(app_name);
        match std::fs::create_dir(&root) {
            Ok(()) => {}, Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {},
            Err(e) => return Err(e),
        }
        Self::prepare_parent(&root)
    }
    /// Prepare the search parent below an independently verified user root.
    pub fn prepare_parent(user_root: &Path) -> io::Result<std::path::PathBuf> {
        let _pins = crate::read_only::CheckedDirectory::pin_root_path(user_root)?;
        let parent = user_root.join("file-search");
        match std::fs::create_dir(&parent) {
            Ok(()) => {},
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {},
            Err(e) => return Err(e),
        }
        let _parent_pins = crate::read_only::CheckedDirectory::pin_root_path(&parent)?;
        Ok(parent)
    }
    /// The caller must supply its independently verified user-specific parent.
    pub fn acquire_shared(parent: &Path) -> io::Result<Self> { Self::acquire(parent, false) }
    /// Hold for the full offline installer lifetime; old binaries do not cooperate.
    pub fn acquire_exclusive(parent: &Path) -> io::Result<Self> { Self::acquire(parent, true) }
    pub(crate) fn require_exclusive_for(&self, parent: &Path) -> io::Result<()> {
        if self.exclusive && self.parent == parent { Ok(()) } else { Err(io::Error::new(io::ErrorKind::PermissionDenied, "wrong search maintenance fence")) }
    }
    fn acquire(parent: &Path, exclusive: bool) -> io::Result<Self> {
        let denied = || io::Error::new(io::ErrorKind::PermissionDenied, "search fence refused");
        let pins = crate::read_only::CheckedDirectory::pin_root_path(parent)?;
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(parent.join(".encrypted-search-maintenance.lock"))?;
        let metadata = file.metadata()?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the file handle and output structure remain valid.
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
            || info.nNumberOfLinks != 1 { return Err(denied()); }
        let result = if exclusive { file.try_lock() } else { file.try_lock_shared() };
        result.map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => io::Error::new(io::ErrorKind::WouldBlock, "search migration is in progress"),
            std::fs::TryLockError::Error(error) => error,
        })?;
        Ok(Self { _pins: pins, _file: file, parent: parent.to_path_buf(), exclusive })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_parent_is_prepared_inside_verified_folder() {
        let base = tempfile::tempdir().unwrap();
        let parent = SearchOperationFence::prepare_from_known_folder(base.path(), "WinCommander").unwrap();
        assert_eq!(parent, base.path().join("WinCommander").join("file-search"));
        assert!(SearchOperationFence::acquire_shared(&parent).is_ok());
        assert!(SearchOperationFence::prepare_from_known_folder(base.path(), "../outside").is_err());
    }
    #[test]
    fn shared_users_and_exclusive_maintenance_exclude_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let first = SearchOperationFence::acquire_shared(dir.path()).unwrap();
        let second = SearchOperationFence::acquire_shared(dir.path()).unwrap();
        assert!(SearchOperationFence::acquire_exclusive(dir.path()).is_err());
        drop((first, second));
        let exclusive = SearchOperationFence::acquire_exclusive(dir.path()).unwrap();
        assert!(SearchOperationFence::acquire_shared(dir.path()).is_err());
        drop(exclusive);
        assert!(SearchOperationFence::acquire_shared(dir.path()).is_ok());
    }
    #[test]
    fn exclusive_fence_rejects_runtime_in_another_process() {
        const ENV: &str = "WINCMD_TEST_SEARCH_FENCE_PARENT";
        if let Some(parent) = std::env::var_os(ENV) {
            assert!(SearchOperationFence::acquire_shared(std::path::Path::new(&parent)).is_err());
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let _exclusive = SearchOperationFence::acquire_exclusive(dir.path()).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "encrypted_search_fence::tests::exclusive_fence_rejects_runtime_in_another_process"])
            .env(ENV, dir.path()).status().unwrap();
        assert!(status.success());
    }
    #[test]
    fn crashed_shared_process_releases_lease_for_maintenance() {
        use std::time::{Duration, Instant};
        const ENV: &str = "WINCMD_TEST_SHARED_SEARCH_FENCE_PARENT";
        if let Some(parent) = std::env::var_os(ENV) {
            let parent=std::path::PathBuf::from(parent);
            let _shared=SearchOperationFence::acquire_shared(&parent).unwrap();
            std::fs::write(parent.join("shared-ready"),b"ready").unwrap();
            // The parent terminates this disposable child to simulate a crash.
            std::thread::sleep(Duration::from_secs(20));
            return;
        }
        struct Child(std::process::Child);
        impl Drop for Child { fn drop(&mut self) { let _=self.0.kill(); let _=self.0.wait(); } }
        let dir=tempfile::tempdir().unwrap();
        let mut child=Child(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","encrypted_search_fence::tests::crashed_shared_process_releases_lease_for_maintenance"])
            .env(ENV,dir.path()).spawn().unwrap());
        let deadline=Instant::now()+Duration::from_secs(10);
        while !dir.path().join("shared-ready").exists() {
            assert!(child.0.try_wait().unwrap().is_none(),"shared child exited before acquiring the lease");
            assert!(Instant::now()<deadline,"shared child did not acquire its lease");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(SearchOperationFence::acquire_exclusive(dir.path()).is_err());
        child.0.kill().unwrap(); child.0.wait().unwrap();
        assert!(SearchOperationFence::acquire_exclusive(dir.path()).is_ok());
    }
}
