use super::*;
use crate::read_only::{AccessMode, CheckedDirectory};
use std::{
    fmt,
    io::BufWriter,
    path::Path,
    sync::{Arc, Mutex},
};
use tantivy::{
    directory::{
        error::{DeleteError, LockError, OpenReadError, OpenWriteError},
        AntiCallToken, Directory, DirectoryLock, FileHandle, Lock, MmapDirectory, OwnedBytes, TerminatingWrite, WatchCallback, WatchHandle,
        WritePtr,
    },
    HasLen,
};

// Trial cap; release sizing must be supported by real large-index measurements.
const MAX_ATOMIC_PLAINTEXT: usize = 8 * 1024 * 1024;
const MAX_ATOMIC_CIPHERTEXT: usize = HEADER + MAX_ATOMIC_PLAINTEXT + MAX_ATOMIC_PLAINTEXT.div_ceil(CHUNK) * OVERHEAD + FOOTER;

enum KeyOwner {
    Fixture([u8; 32]),
    Protected(crate::encrypted_key_prototype::PinnedIndexKey),
}
impl KeyOwner {
    fn key(&self) -> &[u8; 32] {
        match self {
            Self::Fixture(key) => key,
            Self::Protected(key) => key.key(),
        }
    }
}
#[derive(Clone)]
pub(crate) struct TrialDirectory {
    inner: CheckedDirectory,
    key: Arc<KeyOwner>,
    // Experimental lifetime pin: blocks rename of all ancestors while open.
    _root_anchor: Arc<Vec<std::fs::File>>,
}
impl TrialDirectory {
    fn open(root: &Path, mode: AccessMode, key: [u8; 32]) -> Self {
        Self::open_after_pin(root, mode, key, || {})
    }
    fn open_after_pin(root: &Path, mode: AccessMode, key: [u8; 32], after_pin: impl FnOnce()) -> Self {
        let anchor = CheckedDirectory::pin_root_path(root).unwrap();
        after_pin();
        let inner = CheckedDirectory::open(root, mode).unwrap();
        Self {
            inner,
            key: Arc::new(KeyOwner::Fixture(key)),
            _root_anchor: Arc::new(anchor),
        }
    }
}
impl TrialDirectory {
    // Caller must authorize root and hold the migration lease while
    // determining the independently trusted index identity.
    pub(crate) fn open_protected(root: &Path, mode: AccessMode, index_id: &[u8; 16], has_index: bool) -> io::Result<Self> {
        if mode != AccessMode::Writable && !has_index {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "read-only index cannot create a key",
            ));
        }
        let anchor = CheckedDirectory::pin_root_path(root)?;
        // Conservative defense against a caller falsely reporting a fresh
        // index: any existing entry requires an already-protected key.
        // This observation needs an external cross-process writer lease;
        // it cannot by itself prevent files appearing after enumeration.
        let mut entries = std::fs::read_dir(root)?;
        let existing_data = match entries.next() {
            Some(entry) => {
                entry?;
                true
            }
            None => false,
        };
        let inner = CheckedDirectory::open(root, mode).map_err(|e| io::Error::other(e.to_string()))?;
        let key = crate::encrypted_key_prototype::PinnedIndexKey::open(root, index_id, has_index || existing_data)?;
        Ok(Self {
            inner,
            key: Arc::new(KeyOwner::Protected(key)),
            _root_anchor: Arc::new(anchor),
        })
    }
}
impl fmt::Debug for TrialDirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TrialDirectory(redacted)")
    }
}
fn name(path: &Path) -> io::Result<&str> {
    path.to_str().ok_or_else(invalid)
}
struct HandleReader {
    inner: Arc<dyn FileHandle>,
    offset: usize,
}
impl Read for HandleReader {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        let count = dst.len().min(self.inner.len().saturating_sub(self.offset));
        if count == 0 {
            return Ok(0);
        }
        let bytes = self.inner.read_bytes(self.offset..self.offset + count)?;
        dst[..count].copy_from_slice(bytes.as_ref());
        self.offset += count;
        Ok(count)
    }
}
impl Seek for HandleReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let offset = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => self.offset as i128 + i128::from(n),
            SeekFrom::End(n) => self.inner.len() as i128 + i128::from(n),
        };
        self.offset = usize::try_from(offset).map_err(|_| invalid())?;
        Ok(self.offset as u64)
    }
}
struct TrialHandle {
    file: Mutex<ProtectedFile<std::fs::File>>,
    len: usize,
    _key_owner: Arc<KeyOwner>,
}
impl fmt::Debug for TrialHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TrialHandle")
    }
}
impl HasLen for TrialHandle {
    fn len(&self) -> usize {
        self.len
    }
}
impl FileHandle for TrialHandle {
    fn read_bytes(&self, range: Range<usize>) -> io::Result<OwnedBytes> {
        Ok(OwnedBytes::new(self.file.lock().map_err(|_| invalid())?.read(range)?))
    }
}
struct TrialWriter {
    sink: Option<WritePtr>,
    _key_owner: Arc<KeyOwner>,
    cipher: Aes256Gcm,
    path: String,
    id: [u8; 32],
    chunks: u64,
    total: u64,
    plain: Vec<u8>,
    failed: bool,
}
impl TrialWriter {
    fn new(mut sink: WritePtr, owner: Arc<KeyOwner>, path: String) -> io::Result<Self> {
        let mut id = [0; 32];
        OsRng.fill_bytes(&mut id);
        sink.write_all(MAGIC)?;
        sink.write_all(&id)?;
        Ok(Self {
            sink: Some(sink),
            cipher: file_cipher(owner.key(), &id)?,
            _key_owner: owner,
            path,
            id,
            chunks: 0,
            total: 0,
            plain: Vec::with_capacity(CHUNK),
            failed: false,
        })
    }
    fn write_chunk(&mut self) -> io::Result<()> {
        if self.plain.is_empty() {
            return Ok(());
        }
        if self.chunks >= MAX_FILE_CHUNKS as u64 {
            return Err(invalid());
        }
        let n = chunk_nonce(self.chunks);
        let encrypted = self
            .cipher
            .encrypt(
                nonce(&n)?,
                Payload {
                    msg: &self.plain,
                    aad: &aad(&self.path, &self.id, b"chunk", self.chunks),
                },
            )
            .map_err(|_| invalid())?;
        let sink = self.sink.as_mut().ok_or_else(invalid)?;
        sink.write_all(&n)?;
        sink.write_all(&encrypted)?;
        self.chunks = self.chunks.checked_add(1).ok_or_else(invalid)?;
        self.plain.zeroize();
        self.plain.clear();
        Ok(())
    }
}
impl Drop for TrialWriter {
    fn drop(&mut self) {
        self.plain.zeroize();
    }
}
impl Write for TrialWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(invalid());
        }
        self.total = match self.total.checked_add(data.len() as u64) {
            Some(total) => total,
            None => {
                self.failed = true;
                return Err(invalid());
            }
        };
        for part in data.chunks(CHUNK) {
            let mut left = part;
            while !left.is_empty() {
                let n = (CHUNK - self.plain.len()).min(left.len());
                self.plain.extend_from_slice(&left[..n]);
                left = &left[n..];
                if self.plain.len() == CHUNK {
                    if let Err(error) = self.write_chunk() {
                        self.failed = true;
                        return Err(error);
                    }
                }
            }
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.failed {
            return Err(invalid());
        }
        let result = self.sink.as_mut().ok_or_else(invalid)?.flush();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}
impl TerminatingWrite for TrialWriter {
    fn terminate_ref(&mut self, _: AntiCallToken) -> io::Result<()> {
        if self.failed {
            return Err(invalid());
        }
        if let Err(error) = self.write_chunk() {
            self.failed = true;
            return Err(error);
        }
        let n = footer_nonce();
        let tag = self
            .cipher
            .encrypt(
                nonce(&n)?,
                Payload {
                    msg: &[],
                    aad: &aad(&self.path, &self.id, b"footer", self.total),
                },
            )
            .map_err(|_| invalid())?;
        let mut sink = self.sink.take().ok_or_else(invalid)?;
        sink.write_all(&self.total.to_le_bytes())?;
        sink.write_all(&n)?;
        sink.write_all(&tag)?;
        sink.terminate()
    }
}

include!("tantivy_directory_impl.rs");
#[cfg(test)]
include!("tantivy_rebuild_tests.rs");
#[cfg(test)]
include!("tantivy_directory_tests.rs");
