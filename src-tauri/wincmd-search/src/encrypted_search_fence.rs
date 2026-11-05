// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental cooperating runtime/maintenance fence. Old binaries do not
//! acquire this lock; installer shutdown and launch control remain necessary.
use std::{fs::File, io, path::Path};
use std::os::windows::{fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetFileInformationByHandle};

pub struct SearchOperationFence { _pins: Vec<File>, _file: File }

impl SearchOperationFence {
    /// The caller must supply its independently verified user-specific parent.
    pub fn acquire_shared(parent: &Path) -> io::Result<Self> { Self::acquire(parent, false) }
    pub(crate) fn acquire_exclusive(parent: &Path) -> io::Result<Self> { Self::acquire(parent, true) }
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
        result.map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self { _pins: pins, _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
