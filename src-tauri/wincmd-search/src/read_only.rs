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

// Windows can briefly deny index inspection or manifest replacement while
// another process changes files. Error 5 can also be permanent, so repeat the
// full operation and return the error if access does not recover.
#[cfg(windows)]
fn retry_windows_access_denied(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    for delay_ms in [2, 4, 8, 16, 32, 64, 128] {
        match operation() {
            Err(error) if error.raw_os_error() == Some(5) => {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            result => return result,
        }
    }
    operation()
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
    #[cfg(windows)]
    {
        retry_windows_access_denied(|| validate_storage_once(root))
    }
    #[cfg(not(windows))]
    validate_storage_once(root)
}

fn validate_storage_once(root: &Path) -> io::Result<()> {
    validate_ancestors(root)?;
    match std::fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(denied()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        validate_ancestors(&entry.path())?;
        // Tantivy storage is flat; reject subtrees before any schema rebuild.
        if !entry.file_type()?.is_file() {
            return Err(denied());
        }
    }
    Ok(())
}

impl CheckedDirectory {
    #[cfg(all(test, windows))]
    pub(crate) fn pin_root_path(root: &Path) -> io::Result<Vec<std::fs::File>> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        if !root.is_absolute() {
            return Err(denied());
        }
        let mut ancestors: Vec<_> = root.ancestors().collect();
        ancestors.reverse();
        let mut pins = Vec::with_capacity(ancestors.len());
        for ancestor in ancestors {
            let pin = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(ancestor)?;
            let meta = pin.metadata()?;
            if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(denied());
            }
            pins.push(pin);
        }
        Ok(pins)
    }

    #[cfg(all(test, windows))]
    pub(crate) fn pin_root(&self) -> io::Result<Vec<std::fs::File>> {
        Self::pin_root_path(&self.root)
    }

    #[cfg(all(test, windows))]
    fn pin_mutation_root(&self, path: &Path) -> io::Result<Vec<std::fs::File>> {
        self.check(path, true)?;
        self.pin_root()
    }

    #[cfg(all(test, windows))]
    pub(crate) fn acquire_lock_pinned(&self, lock: &Lock) -> Result<DirectoryLock, LockError> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
        self.check(&lock.filepath, false).map_err(LockError::wrap_io_error)?;
        let _pins = self.pin_root().map_err(LockError::wrap_io_error)?;
        if self.mode != AccessMode::Writable && lock.filepath != META_LOCK.filepath {
            return Err(LockError::wrap_io_error(denied()));
        }
        if self.mode == AccessMode::GuardedReader {
            return Ok(DirectoryLock::from(Box::new(())));
        }
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(self.root.join(&lock.filepath)).map_err(LockError::wrap_io_error)?;
        let meta = file.metadata().map_err(LockError::wrap_io_error)?;
        if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(LockError::wrap_io_error(denied()));
        }
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the file handle and output structure remain live for the call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 || info.nNumberOfLinks != 1 {
            return Err(LockError::wrap_io_error(denied()));
        }
        if lock.is_blocking { file.lock().map_err(LockError::wrap_io_error)?; }
        else { file.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => LockError::LockBusy,
            std::fs::TryLockError::Error(e) => LockError::wrap_io_error(e),
        })?; }
        Ok(DirectoryLock::from(Box::new(file)))
    }

    #[cfg(all(test, windows))]
    pub(crate) fn open_write_pinned(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        let _pins = self
            .pin_mutation_root(path)
            .map_err(|e| OpenWriteError::wrap_io_error(e, path.into()))?;
        self.check(path, true)
            .map_err(|e| OpenWriteError::wrap_io_error(e, path.into()))?;
        self.inner.open_write(path)
    }

    #[cfg(all(test, windows))]
    pub(crate) fn atomic_write_pinned(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.atomic_write_pinned_with_hooks(path, data, || {}, || {})
    }

    #[cfg(all(test, windows))]
    fn atomic_write_pinned_with_hooks(&self, path: &Path, data: &[u8], after_pin: impl FnOnce(), before_rename: impl FnOnce()) -> io::Result<()> {
        use rand::{rngs::OsRng, RngCore};
        use std::{io::Write, os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle}};
        use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES,
            FILE_RENAME_INFO, FILE_RENAME_INFO_0, FILE_SHARE_READ, FileDispositionInfo, FileRenameInfo,
            SetFileInformationByHandle};
        use std::os::windows::fs::MetadataExt;
        let pins = self.pin_mutation_root(path)?;
        after_pin();
        self.check(path, true)?;
        let root = pins.last().ok_or_else(denied)?;
        let mut random = [0u8; 16]; OsRng.fill_bytes(&mut random);
        let temp_name = format!(".encrypted-index-{}.tmp", hex::encode(random));
        let mut file = std::fs::OpenOptions::new().access_mode(FILE_GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .create_new(true).open(self.root.join(temp_name))?;
        let result = (|| -> io::Result<()> {
            let meta = file.metadata()?;
            if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 { return Err(denied()); }
            file.write_all(data)?; file.sync_all()?;
            before_rename();
            let name: Vec<u16> = path.as_os_str().encode_wide().collect();
            let name_bytes = name.len().checked_mul(2).ok_or_else(denied)?;
            let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
            let len = offset.checked_add(name_bytes).and_then(|n| n.checked_add(2)).ok_or_else(denied)?;
            let mut buffer = vec![0usize; len.div_ceil(std::mem::size_of::<usize>())];
            let rename = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            // SAFETY: allocation is aligned and covers the header, name and terminator;
            // the source and pinned root handles remain live throughout the call.
            unsafe {
                (*rename).Anonymous = FILE_RENAME_INFO_0 { ReplaceIfExists: true };
                (*rename).RootDirectory = root.as_raw_handle();
                (*rename).FileNameLength = u32::try_from(name_bytes).map_err(|_| denied())?;
                std::ptr::copy_nonoverlapping(name.as_ptr(), std::ptr::addr_of_mut!((*rename).FileName).cast::<u16>(), name.len());
                if SetFileInformationByHandle(file.as_raw_handle(), FileRenameInfo, rename.cast(), u32::try_from(len).map_err(|_| denied())?) == 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        })();
        if result.is_err() {
            let mut disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: this cleanup applies only to our newly-created temp handle.
            unsafe { SetFileInformationByHandle(file.as_raw_handle(), FileDispositionInfo,
                std::ptr::from_mut(&mut disposition).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32); }
        }
        result
    }

    #[cfg(all(test, windows))]
    pub(crate) fn delete_pinned(&self, path: &Path) -> Result<(), DeleteError> {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            FileDispositionInfo, GetFileInformationByHandle, SetFileInformationByHandle,
            BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let wrap = |error| DeleteError::IoError {
            io_error: Arc::new(error),
            filepath: path.into(),
        };
        let _pins = self.pin_mutation_root(path).map_err(wrap)?;
        let file = std::fs::OpenOptions::new()
            .access_mode(DELETE | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(self.root.join(path))
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    DeleteError::FileDoesNotExist(path.into())
                } else {
                    wrap(e)
                }
            })?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the owned file handle and info buffer remain live for the call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(wrap(io::Error::last_os_error()));
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || info.nNumberOfLinks != 1 {
            return Err(wrap(denied()));
        }
        let mut disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: the verified handle and disposition buffer remain live for the call.
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                std::ptr::from_mut(&mut disposition).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(wrap(io::Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(all(test, windows))]
    pub(crate) fn open_read_pinned(&self, path: &Path) -> Result<std::fs::File, OpenReadError> {
        self.open_read_pinned_after_guard(path, || {})
    }

    #[cfg(all(test, windows))]
    fn open_read_pinned_after_guard(
        &self,
        path: &Path,
        before_leaf: impl FnOnce(),
    ) -> Result<std::fs::File, OpenReadError> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let wrap = |e| OpenReadError::wrap_io_error(e, path.into());
        self.check(path, false).map_err(wrap)?;
        if !self.root.is_absolute() {
            return Err(wrap(denied()));
        }
        // Pin each existing parent without delete sharing, from the volume root
        // downward. This prevents renaming a validated ancestor before the
        // final no-follow leaf handle is opened.
        let mut parents: Vec<_> = self.root.ancestors().collect();
        parents.reverse();
        let mut pinned = Vec::with_capacity(parents.len());
        for parent in parents {
            let handle = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(parent)
                .map_err(wrap)?;
            let meta = handle.metadata().map_err(wrap)?;
            if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(wrap(denied()));
            }
            pinned.push(handle);
        }
        before_leaf();
        let leaf = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(self.root.join(path))
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    OpenReadError::FileDoesNotExist(path.into())
                } else {
                    wrap(e)
                }
            })?;
        let meta = leaf.metadata().map_err(wrap)?;
        if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(wrap(denied()));
        }
        // The already-open leaf is a stable snapshot after the parents drop.
        Ok(leaf)
    }

    #[cfg(all(test, windows))]
    pub(crate) fn atomic_read_bounded(
        &self,
        path: &Path,
        max: usize,
    ) -> Result<Vec<u8>, OpenReadError> {
        use std::io::Read;
        // One opened handle preserves atomic-replace snapshot semantics; limit
        // the read itself rather than trusting a racy metadata size check.
        let file = self.open_read_pinned(path)?;
        let limit = max
            .checked_add(1)
            .ok_or_else(|| OpenReadError::wrap_io_error(denied(), path.into()))?;
        let mut bytes = Vec::new();
        file.take(limit as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
        if bytes.len() > max {
            return Err(OpenReadError::wrap_io_error(
                io::Error::other("index manifest exceeds limit"),
                path.into(),
            ));
        }
        Ok(bytes)
    }
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
            return retry_windows_access_denied(|| {
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
    fn mutation_pins_prevent_ancestor_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("index");
        std::fs::create_dir(&root).unwrap();
        let directory = CheckedDirectory::open(&root, AccessMode::Writable).unwrap();
        let pins = directory.pin_mutation_root(Path::new("new-file")).unwrap();
        assert!(std::fs::rename(&root, dir.path().join("moved")).is_err());
        drop(pins);
        std::fs::rename(&root, dir.path().join("moved")).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn atomic_pinned_write_cannot_be_redirected_by_parent_or_leaf_swap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("index"); std::fs::create_dir(&root).unwrap();
        let outside = dir.path().join("outside"); std::fs::write(&outside, b"outside").unwrap();
        let path = Path::new("manifest"); std::fs::write(root.join(path), b"old").unwrap();
        let directory = CheckedDirectory::open(&root, AccessMode::Writable).unwrap();
        directory.atomic_write_pinned_with_hooks(path, b"new", || {
            assert!(std::fs::rename(&root, dir.path().join("moved")).is_err());
        }, || {}).unwrap();
        assert_eq!(std::fs::read(root.join(path)).unwrap(), b"new");
        let result = directory.atomic_write_pinned_with_hooks(path, b"newer", || {}, || {
            std::fs::remove_file(root.join(path)).unwrap();
            std::os::windows::fs::symlink_file(&outside, root.join(path)).unwrap();
        });
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
        if result.is_ok() { assert_eq!(std::fs::read(root.join(path)).unwrap(), b"newer"); }
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
        let outside_handle = std::fs::File::open(&outside).unwrap();
        let identity = || {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            assert_ne!(unsafe { GetFileInformationByHandle(outside_handle.as_raw_handle(), &mut info) }, 0);
            (info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow)
        };
        let before = identity();
        let result = directory.atomic_write_pinned_with_hooks(path, b"third", || {}, || {
            std::fs::remove_file(root.join(path)).unwrap();
            std::fs::hard_link(&outside, root.join(path)).unwrap();
        });
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
        assert_eq!(identity(), before);
        if result.is_ok() { assert_eq!(std::fs::read(root.join(path)).unwrap(), b"third"); }
    }

    #[cfg(windows)]
    #[test]
    fn pinned_delete_rejects_links_and_preserves_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("index");
        std::fs::create_dir(&root).unwrap();
        let outside = dir.path().join("other");
        std::fs::write(&outside, b"do not delete").unwrap();
        let directory = CheckedDirectory::open(&root, AccessMode::Writable).unwrap();
        let link = root.join("segment");
        std::fs::hard_link(&outside, &link).unwrap();
        assert!(directory.delete_pinned(Path::new("segment")).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"do not delete");
        std::fs::remove_file(&link).unwrap();
        std::os::windows::fs::symlink_file(&outside, &link).unwrap();
        assert!(directory.delete_pinned(Path::new("segment")).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"do not delete");
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, b"valid index segment").unwrap();
        directory.delete_pinned(Path::new("segment")).unwrap();
        assert!(!link.exists());
    }

    #[cfg(windows)]
    #[test]
    fn pinned_read_denies_ancestor_replacement_during_open() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("index");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("manifest"), b"expected").unwrap();
        let directory = CheckedDirectory::open(&root, AccessMode::SharedReader).unwrap();
        let leaf = directory
            .open_read_pinned_after_guard(Path::new("manifest"), || {
                assert!(std::fs::rename(&root, parent.path().join("moved")).is_err());
            })
            .unwrap();
        use std::io::Read;
        let mut bytes = Vec::new();
        leaf.take(128).read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"expected");
    }

    #[cfg(windows)]
    #[test]
    fn pinned_read_rejects_leaf_reparse_swapped_after_path_check() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("index");
        std::fs::create_dir(&root).unwrap();
        let leaf = root.join("manifest");
        let outside = parent.path().join("outside");
        std::fs::write(&leaf, b"expected").unwrap();
        std::fs::write(&outside, b"outside secret").unwrap();
        let directory = CheckedDirectory::open(&root, AccessMode::SharedReader).unwrap();
        let result = directory.open_read_pinned_after_guard(Path::new("manifest"), || {
            std::fs::remove_file(&leaf).unwrap();
            std::os::windows::fs::symlink_file(&outside, &leaf).unwrap();
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(outside).unwrap(), b"outside secret");
    }

    #[cfg(windows)]
    #[test]
    fn manifest_write_retries_only_windows_access_denied() {
        let mut attempts = 0;
        retry_windows_access_denied(|| {
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
        let error = retry_windows_access_denied(|| {
            attempts += 1;
            Err(io::Error::from_raw_os_error(2))
        })
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(2));
        assert_eq!(attempts, 1);
        let mut attempts = 0;
        assert!(retry_windows_access_denied(|| {
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

    #[cfg(windows)]
    #[test]
    fn storage_retry_rechecks_the_entire_directory() {
        let root = tempfile::TempDir::new().unwrap();
        let mut attempts = 0;
        let result = retry_windows_access_denied(|| {
            attempts += 1;
            if attempts == 1 {
                std::fs::create_dir(root.path().join("unsafe-subtree"))?;
                return Err(io::Error::from_raw_os_error(5));
            }
            validate_storage_once(root.path())
        });
        assert_eq!(attempts, 2);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
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

    #[test]
    fn storage_validation_rejects_a_file_as_the_index_root() {
        let parent = tempfile::TempDir::new().unwrap();
        let root = parent.path().join("index");
        assert!(validate_storage(&root).is_ok());
        std::fs::write(&root, b"not a directory").unwrap();
        assert_eq!(validate_storage(&root).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(&root).unwrap(), b"not a directory");
    }
}
