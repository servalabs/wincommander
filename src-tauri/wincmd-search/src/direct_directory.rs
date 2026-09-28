// SPDX-License-Identifier: AGPL-3.0-or-later
//! File-backed storage for Windows mounts without a canonical DOS volume mapping.
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tantivy::{
    directory::{
        error::{DeleteError, LockError, OpenReadError, OpenWriteError},
        AntiCallToken, Directory, DirectoryLock, FileHandle, Lock, OwnedBytes, TerminatingWrite,
        WatchCallback, WatchHandle, WritePtr,
    },
    HasLen,
};

#[cfg(test)]
#[path = "direct_directory_tests.rs"]
mod tests;

#[derive(Clone, Debug)]
pub(crate) struct DirectDirectory {
    root: PathBuf,
}

impl DirectDirectory {
    // All callers wrap this backend in CheckedDirectory's per-operation confinement checks.
    pub(crate) fn new(root: &Path) -> Self {
        Self { root: root.into() }
    }
}

#[derive(Debug)]
struct ReadFile {
    file: Mutex<File>,
    len: usize,
}

impl HasLen for ReadFile {
    fn len(&self) -> usize {
        self.len
    }
}

impl FileHandle for ReadFile {
    fn read_bytes(&self, range: Range<usize>) -> io::Result<OwnedBytes> {
        if range.start > range.end || range.end > self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid index byte range",
            ));
        }
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("Index reader lock unavailable"))?;
        let mut bytes = vec![0; range.end - range.start];
        file.seek(SeekFrom::Start(range.start as u64))?;
        file.read_exact(&mut bytes)?;
        Ok(OwnedBytes::new(bytes))
    }
}

struct SyncedWriter(File);
impl Write for SyncedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
impl TerminatingWrite for SyncedWriter {
    fn terminate_ref(&mut self, _: AntiCallToken) -> io::Result<()> {
        self.0.flush()?;
        self.0.sync_data()
    }
}

fn read_error(error: io::Error, path: &Path) -> OpenReadError {
    if error.kind() == io::ErrorKind::NotFound {
        OpenReadError::FileDoesNotExist(path.into())
    } else {
        OpenReadError::wrap_io_error(error, path.into())
    }
}

impl Directory for DirectDirectory {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        let file = File::open(self.root.join(path)).map_err(|e| read_error(e, path))?;
        let len = file.metadata().map_err(|e| read_error(e, path))?.len();
        let len = usize::try_from(len)
            .map_err(|_| read_error(io::Error::other("Index file too large"), path))?;
        Ok(Arc::new(ReadFile {
            file: Mutex::new(file),
            len,
        }))
    }

    fn delete(&self, path: &Path) -> Result<(), DeleteError> {
        fs::remove_file(self.root.join(path)).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                DeleteError::FileDoesNotExist(path.into())
            } else {
                DeleteError::IoError {
                    io_error: Arc::new(error),
                    filepath: path.into(),
                }
            }
        })
    }

    fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
        self.root
            .join(path)
            .try_exists()
            .map_err(|e| read_error(e, path))
    }

    fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.root.join(path))
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    OpenWriteError::FileAlreadyExists(path.into())
                } else {
                    OpenWriteError::wrap_io_error(error, path.into())
                }
            })?;
        Ok(BufWriter::new(Box::new(SyncedWriter(file))))
    }

    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        fs::read(self.root.join(path)).map_err(|e| read_error(e, path))
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..32 {
            let temp = self.root.join(format!(
                ".wincmd-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = match OpenOptions::new().write(true).create_new(true).open(&temp) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            let result = file.write_all(bytes).and_then(|_| file.sync_data());
            drop(file);
            let result = result.and_then(|_| fs::rename(&temp, self.root.join(path)));
            if result.is_err() {
                let _ = fs::remove_file(&temp);
            }
            return result;
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Index temporary file unavailable",
        ))
    }

    fn acquire_lock(&self, lock: &Lock) -> Result<DirectoryLock, LockError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(&lock.filepath))
            .map_err(LockError::wrap_io_error)?;
        if lock.is_blocking {
            file.lock().map_err(LockError::wrap_io_error)?;
        } else {
            file.try_lock().map_err(|error| match error {
                TryLockError::WouldBlock => LockError::LockBusy,
                TryLockError::Error(error) => LockError::wrap_io_error(error),
            })?;
        }
        Ok(DirectoryLock::from(Box::new(file)))
    }

    fn sync_directory(&self) -> io::Result<()> {
        // Windows commits directory entries with the file; directory fsync is unsupported here.
        Ok(())
    }

    fn watch(&self, _: WatchCallback) -> tantivy::Result<WatchHandle> {
        // ContentIndex uses manual reader reloads after every commit.
        Ok(WatchHandle::empty())
    }
}
