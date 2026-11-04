// SPDX-License-Identifier: AGPL-3.0-or-later
//! Confined Tantivy directory with a read-only mode for externally serialized readers.
use std::{
    io,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tantivy::directory::{
    error::{DeleteError, LockError, OpenReadError, OpenWriteError},
    Directory, DirectoryLock, FileHandle, Lock, MmapDirectory, WatchCallback, WatchHandle,
    WritePtr, META_LOCK,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccessMode {
    Writable,
    SharedReader,
    GuardedReader,
}

#[derive(Clone, Debug)]
pub(crate) struct CheckedDirectory {
    inner: Box<dyn Directory>,
    root: PathBuf,
    mode: AccessMode,
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "index storage access denied",
    )
}

// Windows can briefly reject replacement of Tantivy's managed-file manifest
// while another process opens it. Error 5 can also indicate a permanent denial,
// so bound the retries and return the error if access does not recover.
#[cfg(windows)]
fn retry_manifest_write(mut write: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    for delay_ms in [2, 4, 8, 16, 32, 64, 128] {
        match write() {
            Err(error) if error.raw_os_error() == Some(5) => {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            result => return result,
        }
    }
    write()
}

pub(crate) fn validate_ancestors(path: &Path) -> io::Result<()> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(denied());
    }
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        match std::fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(denied());
                    }
                }
                if meta.file_type().is_symlink() {
                    return Err(denied());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(crate) fn validate_storage(root: &Path) -> io::Result<()> {
    validate_ancestors(root)?;
    if root.is_dir() {
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            validate_ancestors(&entry.path())?;
            // Tantivy storage is flat; reject subtrees before any schema rebuild.
            if !entry.file_type()?.is_file() {
                return Err(denied());
            }
        }
    }
    Ok(())
}

impl CheckedDirectory {
    #[cfg(all(test, windows))]
    pub(crate) fn direct_for_test(root: &Path, mode: AccessMode) -> tantivy::Result<Self> {
        validate_storage(root)?;
        Ok(Self {
            inner: Box::new(crate::direct_directory::DirectDirectory::new(root)),
            root: root.into(),
            mode,
        })
    }

    pub(crate) fn open(root: &Path, mode: AccessMode) -> tantivy::Result<Self> {
        validate_storage(root)?;
        let inner: Box<dyn Directory> = match MmapDirectory::open(root) {
            Ok(directory) => Box::new(directory),
            Err(error) => {
                #[cfg(windows)]
                {
                    // Session-local VeraCrypt drives can lack the DOS mapping canonicalize needs.
                    let missing_mapping = root
                        .canonicalize()
                        .is_err_and(|e| e.raw_os_error() == Some(2));
                    if missing_mapping && root.is_absolute() && root.is_dir() {
                        Box::new(crate::direct_directory::DirectDirectory::new(root))
                    } else {
                        return Err(error.into());
                    }
                }
                #[cfg(not(windows))]
                return Err(error.into());
            }
        };
        Ok(Self {
            inner,
            root: root.to_owned(),
            mode,
        })
    }

    fn check(&self, path: &Path, write: bool) -> io::Result<()> {
        if write && self.mode != AccessMode::Writable {
            return Err(denied());
        }
        let mut parts = path.components();
        let Some(Component::Normal(name)) = parts.next() else {
            return Err(denied());
        };
        if parts.next().is_some() {
            return Err(denied());
        }
        let name = name.to_string_lossy();
        if name.contains([':', '/', '\\']) || name.ends_with(['.', ' ']) {
            return Err(denied());
        }
        validate_ancestors(&self.root.join(path))
    }
}

impl Directory for CheckedDirectory {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        self.check(path, false)
            .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
        self.inner.get_file_handle(path)
    }
    fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
        self.check(path, false)
            .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
        self.inner.exists(path)
    }
    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        self.check(path, false)
            .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
        self.inner.atomic_read(path)
    }
    fn delete(&self, path: &Path) -> Result<(), DeleteError> {
        self.check(path, true).map_err(|e| DeleteError::IoError {
            io_error: Arc::new(e),
            filepath: path.into(),
        })?;
        self.inner.delete(path)
    }
    fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        self.check(path, true)
            .map_err(|e| OpenWriteError::wrap_io_error(e, path.into()))?;
        self.inner.open_write(path)
    }
    fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.check(path, true)?;
        #[cfg(windows)]
        if path == Path::new(".managed.json") {
            return retry_manifest_write(|| {
                self.check(path, true)?;
                self.inner.atomic_write(path, data)
            });
        }
        self.inner.atomic_write(path, data)
    }
    fn sync_directory(&self) -> io::Result<()> {
        if self.mode != AccessMode::Writable {
            return Err(denied());
        }
        validate_ancestors(&self.root)?;
        self.inner.sync_directory()
    }
    fn acquire_lock(&self, lock: &Lock) -> Result<DirectoryLock, LockError> {
        self.check(&lock.filepath, false)
            .map_err(LockError::wrap_io_error)?;
        if self.mode == AccessMode::Writable {
            return self.inner.acquire_lock(lock);
        }
        if lock.filepath != META_LOCK.filepath {
            return Err(LockError::wrap_io_error(denied()));
        }
        match self.mode {
            AccessMode::SharedReader => self.inner.acquire_lock(lock),
            // The caller holds an external cross-process guard for this reader's lifetime.
            AccessMode::GuardedReader => Ok(DirectoryLock::from(Box::new(()))),
            AccessMode::Writable => unreachable!(),
        }
    }
    fn watch(&self, callback: WatchCallback) -> tantivy::Result<WatchHandle> {
        if self.mode != AccessMode::Writable {
            Ok(WatchHandle::empty())
        } else {
            self.inner.watch(callback)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn manifest_write_retries_only_windows_access_denied() {
        let mut attempts = 0;
        retry_manifest_write(|| {
            attempts += 1;
            if attempts < 3 {
                Err(io::Error::from_raw_os_error(5))
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert_eq!(attempts, 3);
        let mut attempts = 0;
        let error = retry_manifest_write(|| {
            attempts += 1;
            Err(io::Error::from_raw_os_error(2))
        })
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(2));
        assert_eq!(attempts, 1);
        let mut attempts = 0;
        assert!(retry_manifest_write(|| {
            attempts += 1;
            Err(io::Error::from_raw_os_error(5))
        }).is_err());
        assert_eq!(attempts, 8);
    }

    #[cfg(windows)]
    #[test]
    fn locked_manifest_remains_intact_and_can_be_replaced_after_unlock() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = tempfile::TempDir::new().unwrap();
        let directory = CheckedDirectory::open(root.path(), AccessMode::Writable).unwrap();
        let manifest = Path::new(".managed.json");
        directory.atomic_write(manifest, b"old").unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.path().join(manifest))
            .unwrap();
        assert!(directory.atomic_write(manifest, b"new").is_err());
        drop(locked);
        assert_eq!(directory.atomic_read(manifest).unwrap(), b"old");
        directory.atomic_write(manifest, b"new").unwrap();
        assert_eq!(directory.atomic_read(manifest).unwrap(), b"new");
    }

    #[test]
    fn directory_rejects_absolute_parent_nested_and_stream_paths() {
        let root = tempfile::TempDir::new().unwrap();
        let directory = CheckedDirectory::open(root.path(), AccessMode::Writable).unwrap();
        for path in ["../outside", "nested/file", "file:stream", "ambiguous."] {
            assert!(directory.atomic_read(Path::new(path)).is_err());
            assert!(directory.atomic_write(Path::new(path), b"content").is_err());
        }
        assert!(directory.atomic_read(root.path()).is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn schema_rebuild_rejects_subtrees_before_deleting_anything() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("unrelated")).unwrap();
        std::fs::write(root.path().join("unrelated/keep.txt"), "keep").unwrap();
        std::fs::write(root.path().join("meta.json"), "old metadata").unwrap();
        assert!(crate::index::ContentIndex::open_or_create(root.path()).is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("unrelated/keep.txt")).unwrap(),
            "keep"
        );
    }
}
