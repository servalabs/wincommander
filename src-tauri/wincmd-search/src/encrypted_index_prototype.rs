// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only experiment; not wired into production or an approved disk format.
//! Each independently authenticated chunk supports bounded random access.
use aes_gcm::{
    aead::{consts::U12, Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;

const MAGIC: &[u8; 8] = b"WCIDX002";
const CHUNK: usize = 64 * 1024;
// Experimental denial-of-service bound. Large-index support must measure the
// largest ranges Tantivy actually requests before choosing a release value.
const MAX_READ_RANGE: usize = 8 * 1024 * 1024;
const HEADER: usize = 40;
const MAX_FILE_CHUNKS: usize = 65_536; // at most 4 GiB and 65,537 GCM invocations per file key
const OVERHEAD: usize = 28; // 96-bit per-file counter nonce and 128-bit tag
const FOOTER: usize = 8 + OVERHEAD;

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "index authentication failed")
}

fn aad(name: &str, id: &[u8], label: &[u8], number: u64) -> Vec<u8> {
    let mut result = Vec::new();
    result.extend_from_slice(b"wincommander-search-index-test-v2");
    result.extend_from_slice(&(name.len() as u64).to_le_bytes());
    result.extend_from_slice(name.as_bytes());
    result.extend_from_slice(id);
    result.extend_from_slice(label);
    result.extend_from_slice(&number.to_le_bytes());
    result
}
fn nonce(n: &[u8]) -> io::Result<&Nonce<U12>> {
    n.try_into().map_err(|_| invalid())
}

// Each file gets a pseudorandom subkey under the protected index key. A fresh
// 256-bit ID makes accidental cross-file key/nonce reuse negligible; the input
// length and domain are fixed to avoid ambiguous derivation contexts.
fn file_cipher(master: &[u8; 32], id: &[u8; 32]) -> io::Result<Aes256Gcm> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(master).map_err(|_| invalid())?;
    mac.update(b"wincommander-search-file-key-v2");
    mac.update(id);
    let mut key = Zeroizing::new([0u8; 32]);
    let mut derived = mac.finalize().into_bytes();
    key.copy_from_slice(&derived);
    derived.zeroize();
    Ok(Aes256Gcm::new((&*key).into()))
}

fn chunk_nonce(number: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&number.to_le_bytes());
    nonce
}
fn footer_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[0] = 1;
    nonce
}

fn encrypt<W: Write>(
    mut output: W,
    key: &[u8; 32],
    name: &str,
    plaintext: &[u8],
) -> io::Result<()> {
    if plaintext.len().div_ceil(CHUNK) > MAX_FILE_CHUNKS { return Err(invalid()); }
    let mut id = [0u8; 32];
    OsRng.fill_bytes(&mut id);
    let cipher = file_cipher(key, &id)?;
    output.write_all(MAGIC)?;
    output.write_all(&id)?;
    for (i, chunk) in plaintext.chunks(CHUNK).enumerate() {
        let n = chunk_nonce(i as u64);
        let encrypted = cipher
            .encrypt(
                nonce(&n)?,
                Payload {
                    msg: chunk,
                    aad: &aad(name, &id, b"chunk", i as u64),
                },
            )
            .map_err(|_| invalid())?;
        output.write_all(&n)?;
        output.write_all(&encrypted)?;
    }
    let len = plaintext.len() as u64;
    let n = footer_nonce();
    let tag = cipher
        .encrypt(
            nonce(&n)?,
            Payload {
                msg: &[],
                aad: &aad(name, &id, b"footer", len),
            },
        )
        .map_err(|_| invalid())?;
    output.write_all(&len.to_le_bytes())?;
    output.write_all(&n)?;
    output.write_all(&tag)
}

struct ProtectedFile<R> {
    input: R,
    cipher: Aes256Gcm,
    name: String,
    id: [u8; 32],
    len: usize,
}
impl<R: Read + Seek> ProtectedFile<R> {
    fn open(mut input: R, key: &[u8; 32], name: &str) -> io::Result<Self> {
        let file_len = input.seek(SeekFrom::End(0))?;
        if file_len < (HEADER + FOOTER) as u64 {
            return Err(invalid());
        }
        input.seek(SeekFrom::Start(0))?;
        let mut header = [0u8; HEADER];
        input.read_exact(&mut header)?;
        if &header[..8] != MAGIC {
            return Err(invalid());
        }
        let id: [u8; 32] = header[8..].try_into().map_err(|_| invalid())?;
        input.seek(SeekFrom::End(-(FOOTER as i64)))?;
        let mut footer = [0u8; FOOTER];
        input.read_exact(&mut footer)?;
        let len_u64 = u64::from_le_bytes(footer[..8].try_into().map_err(|_| invalid())?);
        let len = usize::try_from(len_u64).map_err(|_| invalid())?;
        let expected = HEADER
            .checked_add(len)
            .and_then(|n| n.checked_add(len.div_ceil(CHUNK).checked_mul(OVERHEAD)?))
            .and_then(|n| n.checked_add(FOOTER))
            .ok_or_else(invalid)?;
        if expected as u64 != file_len {
            return Err(invalid());
        }
        if len.div_ceil(CHUNK) > MAX_FILE_CHUNKS || footer[8..20] != footer_nonce() { return Err(invalid()); }
        let cipher = file_cipher(key, &id)?;
        cipher
            .decrypt(
                nonce(&footer[8..20])?,
                Payload {
                    msg: &footer[20..],
                    aad: &aad(name, &id, b"footer", len_u64),
                },
            )
            .map_err(|_| invalid())?;
        Ok(Self {
            input,
            cipher,
            name: name.into(),
            id,
            len,
        })
    }
    fn read(&mut self, range: Range<usize>) -> io::Result<Vec<u8>> {
        if range.start > range.end
            || range.end > self.len
            || range.end - range.start > MAX_READ_RANGE
        {
            return Err(invalid());
        }
        if range.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(range.end - range.start);
        for i in range.start / CHUNK..=(range.end - 1) / CHUNK {
            let plain_len = (self.len - i * CHUNK).min(CHUNK);
            let offset = HEADER + i * (CHUNK + OVERHEAD);
            self.input.seek(SeekFrom::Start(offset as u64))?;
            let mut data = vec![0u8; plain_len + OVERHEAD];
            self.input.read_exact(&mut data)?;
            let plain = self
                .cipher
                .decrypt(
                    nonce(&data[..12])?,
                    Payload {
                        msg: &data[12..],
                        aad: &aad(&self.name, &self.id, b"chunk", i as u64),
                    },
                )
                .map_err(|_| invalid())?;
            let start = range.start.saturating_sub(i * CHUNK);
            let end = (range.end - i * CHUNK).min(plain_len);
            out.extend_from_slice(&plain[start..end]);
        }
        Ok(out)
    }
}

#[test]
fn experimental_cipher_clears_sensitive_state_on_drop() {
    fn require_zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}
    require_zeroize_on_drop::<Aes256Gcm>();
}
#[test]
fn distinct_file_keys_use_bounded_disjoint_counter_nonces() {
    let key = [14; 32];
    let plaintext = vec![42; CHUNK + 1];
    let mut first = Vec::new(); let mut second = Vec::new();
    encrypt(&mut first, &key, "same-name", &plaintext).unwrap();
    encrypt(&mut second, &key, "same-name", &plaintext).unwrap();
    assert_ne!(&first[8..HEADER], &second[8..HEADER]);
    assert_eq!(&first[HEADER..HEADER + 12], &chunk_nonce(0));
    assert_eq!(&first[HEADER + CHUNK + OVERHEAD..HEADER + CHUNK + OVERHEAD + 12], &chunk_nonce(1));
    assert_eq!(&first[first.len() - 28..first.len() - 16], &footer_nonce());
    assert_ne!(chunk_nonce(0), footer_nonce());
    // Substitution across fresh per-file keys must fail, even at equal offsets.
    second[HEADER..HEADER + CHUNK + OVERHEAD].copy_from_slice(&first[HEADER..HEADER + CHUNK + OVERHEAD]);
    let mut file = ProtectedFile::open(io::Cursor::new(second), &key, "same-name").unwrap();
    assert!(file.read(0..1).is_err());
}
#[test]
fn random_access_and_tampering_are_detected() {
    let key = [42; 32];
    let mut text = vec![b'A'; CHUNK * 2 + 173];
    text[CHUNK - 10..CHUNK + 12].copy_from_slice(b"search secret 12345678");
    let mut bytes = Vec::new();
    encrypt(&mut bytes, &key, "a.idx", &text).unwrap();
    assert!(!bytes.windows(13).any(|w| w == b"search secret"));
    let mut file = ProtectedFile::open(io::Cursor::new(bytes.clone()), &key, "a.idx").unwrap();
    for r in [
        0..0,
        0..1,
        CHUNK - 20..CHUNK + 30,
        CHUNK * 2..text.len(),
        0..text.len(),
    ] {
        assert_eq!(file.read(r.clone()).unwrap(), text[r]);
    }
    assert!(ProtectedFile::open(io::Cursor::new(bytes.clone()), &[43; 32], "a.idx").is_err());
    assert!(ProtectedFile::open(io::Cursor::new(bytes.clone()), &key, "b.idx").is_err());
    assert!(ProtectedFile::open(
        io::Cursor::new(bytes[..bytes.len() - 1].to_vec()),
        &key,
        "a.idx"
    )
    .is_err());
    bytes[HEADER + 30] ^= 1;
    let mut file = ProtectedFile::open(io::Cursor::new(bytes), &key, "a.idx").unwrap();
    assert!(file.read(0..1).is_err());
}

#[test]
fn reordering_and_cross_file_chunk_substitution_are_rejected() {
    let key = [8; 32];
    let plain = vec![3; CHUNK * 2];
    let mut first = Vec::new();
    encrypt(&mut first, &key, "same-name", &plain).unwrap();
    let mut second = Vec::new();
    encrypt(&mut second, &key, "same-name", &plain).unwrap();
    let width = CHUNK + OVERHEAD;
    let chunk = first[HEADER..HEADER + width].to_vec();
    first[HEADER..HEADER + width].copy_from_slice(&second[HEADER + width..HEADER + width * 2]);
    let mut file = ProtectedFile::open(io::Cursor::new(first), &key, "same-name").unwrap();
    assert!(file.read(0..1).is_err());
    second[HEADER + width..HEADER + width * 2].copy_from_slice(&chunk);
    let mut file = ProtectedFile::open(io::Cursor::new(second), &key, "same-name").unwrap();
    assert!(file.read(CHUNK..CHUNK + 1).is_err());
}

#[test]
fn empty_file_and_footer_authentication() {
    let key = [1; 32];
    let mut bytes = Vec::new();
    encrypt(&mut bytes, &key, "empty", &[]).unwrap();
    let mut file = ProtectedFile::open(io::Cursor::new(bytes.clone()), &key, "empty").unwrap();
    assert_eq!(file.read(0..0).unwrap(), b"");
    *bytes.last_mut().unwrap() ^= 1;
    assert!(ProtectedFile::open(io::Cursor::new(bytes), &key, "empty").is_err());
}

#[test]
fn oversized_range_is_rejected_without_allocating_the_requested_output() {
    let key = [2; 32];
    let plain = vec![7; MAX_READ_RANGE + 1];
    let mut bytes = Vec::new();
    encrypt(&mut bytes, &key, "big-segment", &plain).unwrap();
    let mut file = ProtectedFile::open(io::Cursor::new(bytes), &key, "big-segment").unwrap();
    assert!(file.read(0..MAX_READ_RANGE + 1).is_err());
    assert_eq!(file.read(MAX_READ_RANGE..MAX_READ_RANGE + 1).unwrap(), [7]);
}

#[test]
fn native_disk_roundtrip_does_not_write_indexed_text_as_plaintext() {
    let key = [6; 32];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("segment.idx");
    let mut original = vec![b'Q'; CHUNK + 20];
    original[CHUNK - 9..CHUNK + 10].copy_from_slice(b"private-search-term");
    let mut output = std::fs::File::create(&path).unwrap();
    encrypt(&mut output, &key, "segment.idx", &original).unwrap();
    output.sync_all().unwrap();
    drop(output);
    let disk = std::fs::read(&path).unwrap();
    assert!(!disk.windows(19).any(|s| s == b"private-search-term"));
    let mut input =
        ProtectedFile::open(std::fs::File::open(path).unwrap(), &key, "segment.idx").unwrap();
    assert_eq!(
        input.read(CHUNK - 9..CHUNK + 10).unwrap(),
        b"private-search-term"
    );
}

// Deliberately test-only: key management, migration and atomic manifest limits
// are not ready for production. It exercises Tantivy's real Directory contract.
#[cfg(windows)]
pub(crate) mod tantivy_trial {
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
            AntiCallToken, Directory, DirectoryLock, FileHandle, Lock, MmapDirectory, OwnedBytes,
            TerminatingWrite, WatchCallback, WatchHandle, WritePtr,
        },
        HasLen,
    };

    // Trial cap; release sizing must be supported by real large-index measurements.
    const MAX_ATOMIC_PLAINTEXT: usize = 8 * 1024 * 1024;
    const MAX_ATOMIC_CIPHERTEXT: usize =
        HEADER + MAX_ATOMIC_PLAINTEXT + MAX_ATOMIC_PLAINTEXT.div_ceil(CHUNK) * OVERHEAD + FOOTER;

    enum KeyOwner {
        Fixture([u8; 32]),
        Protected(crate::encrypted_key_prototype::PinnedIndexKey),
    }
    impl KeyOwner {
        fn key(&self) -> &[u8; 32] {
            match self { Self::Fixture(key) => key, Self::Protected(key) => key.key() }
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
            Self { inner, key: Arc::new(KeyOwner::Fixture(key)), _root_anchor: Arc::new(anchor) }
        }
    }
    impl TrialDirectory {
        // Caller must authorize root and hold the migration lease while
        // determining the independently trusted index identity.
        pub(crate) fn open_protected(root: &Path, mode: AccessMode, index_id: &[u8; 16], has_index: bool) -> io::Result<Self> {
            if mode != AccessMode::Writable && !has_index {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "read-only index cannot create a key"));
            }
            let anchor = CheckedDirectory::pin_root_path(root)?;
            // Conservative defense against a caller falsely reporting a fresh
            // index: any existing entry requires an already-protected key.
            // This observation needs an external cross-process writer lease;
            // it cannot by itself prevent files appearing after enumeration.
            let mut entries = std::fs::read_dir(root)?;
            let existing_data = match entries.next() {
                Some(entry) => { entry?; true },
                None => false,
            };
            let inner = CheckedDirectory::open(root, mode).map_err(|e| io::Error::other(e.to_string()))?;
            let key = crate::encrypted_key_prototype::PinnedIndexKey::open(root, index_id, has_index || existing_data)?;
            Ok(Self { inner, key: Arc::new(KeyOwner::Protected(key)), _root_anchor: Arc::new(anchor) })
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
            Ok(OwnedBytes::new(
                self.file.lock().map_err(|_| invalid())?.read(range)?,
            ))
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
            if self.chunks >= MAX_FILE_CHUNKS as u64 { return Err(invalid()); }
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
    impl Directory for TrialDirectory {
        fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
            let source = self.inner.open_read_pinned(path)?;
            let file = ProtectedFile::open(
                source,
                self.key.key(),
                name(path).map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?,
            )
            .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
            let len = file.len;
            Ok(Arc::new(TrialHandle {
                file: Mutex::new(file),
                len,
                _key_owner: Arc::clone(&self.key),
            }))
        }
        fn delete(&self, path: &Path) -> Result<(), DeleteError> {
            self.inner.delete_pinned(path)
        }
        fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
            self.inner.exists(path)
        }
        fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
            let path_name = name(path)
                .map_err(|e| OpenWriteError::wrap_io_error(e, path.into()))?
                .to_owned();
            let sink = self.inner.open_write_pinned(path)?;
            let writer = TrialWriter::new(sink, Arc::clone(&self.key), path_name)
                .map_err(|e| OpenWriteError::wrap_io_error(e, path.into()))?;
            Ok(BufWriter::new(Box::new(writer)))
        }
        fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
            let bytes = self
                .inner
                .atomic_read_bounded(path, MAX_ATOMIC_CIPHERTEXT)?;
            let mut f = ProtectedFile::open(
                io::Cursor::new(bytes),
                self.key.key(),
                name(path).map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?,
            )
            .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
            if f.len > MAX_ATOMIC_PLAINTEXT {
                return Err(OpenReadError::wrap_io_error(
                    io::Error::other("index manifest exceeds limit"),
                    path.into(),
                ));
            }
            f.read(0..f.len)
                .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))
        }
        fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
            if data.len() > MAX_ATOMIC_PLAINTEXT {
                return Err(io::Error::other("index manifest exceeds limit"));
            }
            let mut encrypted = Vec::new();
            encrypt(&mut encrypted, self.key.key(), name(path)?, data)?;
            // Windows can briefly refuse atomic replacement while a shared
            // reader has the old manifest open. Preserve a permanent failure.
            #[cfg(windows)]
            for delay in [2, 4, 8, 16, 32, 64, 128] {
                match self.inner.atomic_write_pinned(path, &encrypted) {
                    Err(e) if e.raw_os_error() == Some(5) => {
                        std::thread::sleep(std::time::Duration::from_millis(delay))
                    }
                    result => return result,
                }
            }
            self.inner.atomic_write_pinned(path, &encrypted)
        }
        fn sync_directory(&self) -> io::Result<()> {
            self.inner.sync_directory()
        }
        fn acquire_lock(&self, lock: &Lock) -> Result<DirectoryLock, LockError> {
            self.inner.acquire_lock_pinned(lock)
        }
        fn watch(&self, callback: WatchCallback) -> tantivy::Result<WatchHandle> {
            self.inner.watch(callback)
        }
    }

    #[test]
    fn existing_index_data_cannot_be_mistaken_for_fresh_storage() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("meta.json");
        std::fs::write(&original, b"existing index").unwrap();
        assert!(TrialDirectory::open_protected(root.path(), AccessMode::Writable, &[6; 16], false).is_err());
        assert!(!root.path().join("protected-search-key.v1").exists());
        assert_eq!(std::fs::read(original).unwrap(), b"existing index");
    }
    #[test]
    fn protected_read_only_directories_never_create_a_key() {
        let root = tempfile::tempdir().unwrap();
        for mode in [AccessMode::SharedReader, AccessMode::GuardedReader] {
            assert!(TrialDirectory::open_protected(root.path(), mode, &[6; 16], false).is_err());
            assert!(!root.path().join("protected-search-key.v1").exists());
            assert!(TrialDirectory::open_protected(root.path(), mode, &[6; 16], true).is_err());
            assert!(!root.path().join("protected-search-key.v1").exists());
        }
    }
    #[test]
    fn protected_key_remains_pinned_until_directory_clones_and_handles_close() {
        let root = tempfile::tempdir().unwrap();
        let directory = TrialDirectory::open_protected(root.path(), AccessMode::Writable, &[6; 16], false).unwrap();
        directory.atomic_write(Path::new("segment"), b"secret").unwrap();
        let clone = directory.clone();
        let reader = clone.get_file_handle(Path::new("segment")).unwrap();
        let writer = clone.open_write(Path::new("other-segment")).unwrap();
        let key_path = root.path().join("protected-search-key.v1");
        assert!(std::fs::remove_file(&key_path).is_err());
        drop(directory);
        drop(clone);
        assert!(std::fs::remove_file(&key_path).is_err());
        drop(reader);
        assert!(std::fs::remove_file(&key_path).is_err());
        drop(writer);
        std::fs::remove_file(&key_path).unwrap();
    }
    #[cfg(feature = "experimental-encrypted-search")]
    #[test]
    fn production_schema_can_rebuild_and_reopen_in_a_separate_encrypted_index() {
        use crate::{index::ContentIndex, types::{ContentQuery, DocProps, FileMeta}};
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("legacy");
        let new = root.path().join("encrypted");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        std::fs::write(old.join("meta.json"), b"legacy intact").unwrap();
        let index = ContentIndex::open_experimental_encrypted(&new, [33; 16], false).unwrap();
        let meta = FileMeta { doc_id: 1, path: root.path().join("document.txt"), name: "document.txt".into(), ext: "txt".into(), mtime: 1, size: 24 };
        let mut writer = index.writer().unwrap();
        index.upsert(&mut writer, &meta, "document", "confidential schemaindexsecret", &DocProps::default()).unwrap();
        index.commit(&mut writer).unwrap();
        drop(writer);
        drop(index);
        for entry in std::fs::read_dir(&new).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let bytes = std::fs::read(entry.path()).unwrap();
                assert!(!bytes.windows(b"schemaindexsecret".len()).any(|w| w == b"schemaindexsecret"));
            }
        }
        let reopened = ContentIndex::open_experimental_encrypted(&new, [33; 16], true).unwrap();
        let query = ContentQuery { terms: "schemaindexsecret".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        assert_eq!(reopened.search_keyword(&query).unwrap().len(), 1);
        assert_eq!(std::fs::read(old.join("meta.json")).unwrap(), b"legacy intact");
    }
    #[test]
    fn isolated_rebuild_uses_user_key_and_preserves_legacy_index() {
        use tantivy::{collector::Count, doc, query::QueryParser, schema::{Schema, STORED, TEXT}, Index};
        let sandbox = tempfile::tempdir().unwrap();
        let old_index = sandbox.path().join("old-index");
        let fresh_index = sandbox.path().join("new-index");
        std::fs::create_dir(&old_index).unwrap();
        std::fs::create_dir(&fresh_index).unwrap();
        let legacy_file = old_index.join("meta.json");
        std::fs::write(&legacy_file, b"legacy index retained").unwrap();
        let document = sandbox.path().join("document.txt");
        std::fs::write(&document, "confidential rebuildneedleword example").unwrap();

        let directory = TrialDirectory::open_protected(&fresh_index, AccessMode::Writable, &[13; 16], false).unwrap();
        let mut schema = Schema::builder();
        let body = schema.add_text_field("body", TEXT | STORED);
        let index = Index::create(directory, schema.build(), Default::default()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();
        let contents = std::fs::read_to_string(&document).unwrap();
        writer.add_document(doc!(body => contents)).unwrap();
        writer.commit().unwrap();
        drop(writer);
        drop(index);
        assert_eq!(std::fs::read(&legacy_file).unwrap(), b"legacy index retained");
        assert!(fresh_index.join("protected-search-key.v1").is_file());
        for entry in std::fs::read_dir(&fresh_index).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let bytes = std::fs::read(entry.path()).unwrap();
                assert!(!bytes.windows(b"rebuildneedleword".len()).any(|w| w == b"rebuildneedleword"));
            }
        }
        let directory = TrialDirectory::open_protected(&fresh_index, AccessMode::SharedReader, &[13; 16], true).unwrap();
        let reopened = Index::open(directory).unwrap();
        let query = QueryParser::for_index(&reopened, vec![body]).parse_query("rebuildneedleword").unwrap();
        assert_eq!(reopened.reader().unwrap().searcher().search(&query, &Count).unwrap(), 1);
        assert_eq!(std::fs::read(&legacy_file).unwrap(), b"legacy index retained");
    }
    #[test]
    fn real_tantivy_index_search_and_reopen_are_encrypted() {
        use tantivy::{
            collector::Count,
            doc,
            query::QueryParser,
            schema::{Schema, STORED, TEXT},
            Index,
        };
        let dir = tempfile::tempdir().unwrap();
        let directory = TrialDirectory::open(dir.path(), AccessMode::Writable, [9; 32]);
        let mut schema = Schema::builder();
        let body = schema.add_text_field("body", TEXT | STORED);
        let schema = schema.build();
        let index = Index::create(directory.clone(), schema, Default::default()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();
        writer
            .add_document(doc!(body => "confidential gossamersearchword example"))
            .unwrap();
        writer.commit().unwrap();
        drop(writer);
        drop(index);
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
            if !entry.file_type().unwrap().is_file() {
                continue;
            }
            let bytes = std::fs::read(entry.path()).unwrap();
            assert!(!bytes.windows(18).any(|w| w == b"gossamersearchword"));
        }
        let reopened = Index::open(directory).unwrap();
        let query = QueryParser::for_index(&reopened, vec![body])
            .parse_query("gossamersearchword")
            .unwrap();
        let reader = reopened.reader().unwrap();
        assert_eq!(reader.searcher().search(&query, &Count).unwrap(), 1);
    }

    #[test]
    fn streaming_writer_and_file_handle_use_bounded_chunks() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        #[derive(Debug)]
        struct MeasuredHandle {
            inner: Arc<dyn FileHandle>,
            largest: Arc<AtomicUsize>,
        }
        impl HasLen for MeasuredHandle {
            fn len(&self) -> usize {
                self.inner.len()
            }
        }
        impl FileHandle for MeasuredHandle {
            fn read_bytes(&self, range: Range<usize>) -> io::Result<OwnedBytes> {
                self.largest.fetch_max(range.len(), Ordering::Relaxed);
                self.inner.read_bytes(range)
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let inner = MmapDirectory::open(dir.path()).unwrap();
        let path = Path::new("stream.idx");
        let key = [11; 32];
        let mut writer =
            TrialWriter::new(inner.open_write(path).unwrap(), Arc::new(KeyOwner::Fixture(key)), "stream.idx".into()).unwrap();
        let bytes = vec![42; CHUNK * 8 + 333];
        for part in bytes.chunks(971) {
            writer.write_all(part).unwrap();
            assert!(writer.plain.len() < CHUNK);
        }
        writer.terminate().unwrap();
        let largest = Arc::new(AtomicUsize::new(0));
        let inner_file = inner.get_file_handle(path).unwrap();
        let measured = Arc::new(MeasuredHandle {
            inner: inner_file,
            largest: largest.clone(),
        });
        let mut file = ProtectedFile::open(
            HandleReader {
                inner: measured,
                offset: 0,
            },
            &key,
            "stream.idx",
        )
        .unwrap();
        assert_eq!(
            file.read(CHUNK - 7..CHUNK + 7).unwrap(),
            bytes[CHUNK - 7..CHUNK + 7]
        );
        assert_eq!(file.read(0..bytes.len()).unwrap(), bytes);
        assert!(largest.load(Ordering::Relaxed) <= CHUNK + OVERHEAD);
    }

    #[test]
    fn failed_stream_cannot_be_finalized_or_read_as_a_complete_file() {
        struct FailAfter {
            bytes: Arc<Mutex<Vec<u8>>>,
            limit: usize,
        }
        impl Write for FailAfter {
            fn write(&mut self, input: &[u8]) -> io::Result<usize> {
                let mut bytes = self.bytes.lock().unwrap();
                let available = self.limit.saturating_sub(bytes.len());
                if available == 0 {
                    return Err(io::Error::other("injected write failure"));
                }
                let n = available.min(input.len());
                bytes.extend_from_slice(&input[..n]);
                Ok(n)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        impl TerminatingWrite for FailAfter {
            fn terminate_ref(&mut self, _: AntiCallToken) -> io::Result<()> {
                Ok(())
            }
        }
        let output = Arc::new(Mutex::new(Vec::new()));
        let sink: WritePtr = BufWriter::new(Box::new(FailAfter {
            bytes: output.clone(),
            limit: 100,
        }));
        let key = [17; 32];
        let mut writer = TrialWriter::new(sink, Arc::new(KeyOwner::Fixture(key)), "failed.idx".into()).unwrap();
        assert!(writer.write_all(&vec![4; CHUNK]).is_err());
        assert!(writer.write_all(b"retry").is_err());
        assert!(writer.terminate().is_err());
        let stored = output.lock().unwrap().clone();
        assert!(ProtectedFile::open(io::Cursor::new(stored), &key, "failed.idx").is_err());
    }

    #[test]
    fn checked_shared_reader_searches_but_cannot_mutate() {
        use tantivy::{
            collector::Count,
            doc,
            query::QueryParser,
            schema::{Schema, TEXT},
            Index,
        };
        let dir = tempfile::tempdir().unwrap();
        let writer_dir = TrialDirectory::open(dir.path(), AccessMode::Writable, [23; 32]);
        let mut schema = Schema::builder();
        let body = schema.add_text_field("body", TEXT);
        let index = Index::create(writer_dir.clone(), schema.build(), Default::default()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();
        writer
            .add_document(doc!(body => "sharedreadtoken"))
            .unwrap();
        writer.commit().unwrap();
        let reader_dir = TrialDirectory::open(dir.path(), AccessMode::SharedReader, [23; 32]);
        assert!(reader_dir.open_write(Path::new("forbidden")).is_err());
        assert!(reader_dir
            .atomic_write(Path::new("meta.json"), b"forbidden")
            .is_err());
        assert!(reader_dir.delete(Path::new("meta.json")).is_err());
        let read_index = Index::open(reader_dir).unwrap();
        let query = QueryParser::for_index(&read_index, vec![body])
            .parse_query("sharedreadtoken")
            .unwrap();
        assert_eq!(
            read_index
                .reader()
                .unwrap()
                .searcher()
                .search(&query, &Count)
                .unwrap(),
            1
        );
    }

    #[test]
    fn bounded_atomic_manifest_rejects_large_input_without_replacing_existing() {
        let dir = tempfile::tempdir().unwrap();
        let directory = TrialDirectory::open(dir.path(), AccessMode::Writable, [24; 32]);
        let path = Path::new("manifest.json");
        directory.atomic_write(path, b"valid manifest").unwrap();
        assert!(directory
            .atomic_write(path, &vec![0; MAX_ATOMIC_PLAINTEXT + 1])
            .is_err());
        assert_eq!(directory.atomic_read(path).unwrap(), b"valid manifest");
        directory
            .inner
            .atomic_write(path, &vec![0; MAX_ATOMIC_CIPHERTEXT + 1])
            .unwrap();
        assert!(directory.atomic_read(path).is_err());
    }

    #[test]
    fn checked_shared_reader_waits_for_writer_metadata_lock() {
        use std::{sync::mpsc, thread, time::Duration};
        use tantivy::directory::META_LOCK;
        let dir = tempfile::tempdir().unwrap();
        // The other participant uses the existing production CheckedDirectory lock.
        let writer = CheckedDirectory::open(dir.path(), AccessMode::Writable).unwrap();
        let reader = TrialDirectory::open(dir.path(), AccessMode::SharedReader, [25; 32]);
        let held = writer.acquire_lock(&META_LOCK).unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            tx.send(reader.acquire_lock(&META_LOCK).is_ok()).unwrap();
        });
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(held);
        assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        handle.join().unwrap();
    }

    #[test]
    fn root_anchor_blocks_replacement_during_directory_lifetime() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("index"); std::fs::create_dir(&root).unwrap();
        let directory = TrialDirectory::open_after_pin(&root, AccessMode::Writable, [26; 32], || {
            assert!(std::fs::rename(&root, parent.path().join("moved")).is_err());
        });
        assert!(std::fs::rename(&root, parent.path().join("moved")).is_err());
        drop(directory);
        std::fs::rename(&root, parent.path().join("moved")).unwrap();
    }

    #[test]
    fn pinned_lock_refuses_symlink_and_hardlink() {
        use tantivy::directory::META_LOCK;
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("index"); std::fs::create_dir(&root).unwrap();
        let outside = parent.path().join("outside"); std::fs::write(&outside, b"untouched").unwrap();
        let directory = TrialDirectory::open(&root, AccessMode::Writable, [26; 32]);
        let lock_path = root.join(&META_LOCK.filepath);
        std::fs::hard_link(&outside, &lock_path).unwrap();
        assert!(directory.acquire_lock(&META_LOCK).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"untouched");
        std::fs::remove_file(&lock_path).unwrap();
        std::os::windows::fs::symlink_file(&outside, &lock_path).unwrap();
        assert!(directory.acquire_lock(&META_LOCK).is_err());
        assert_eq!(std::fs::read(outside).unwrap(), b"untouched");
    }

    #[test]
    fn checked_shared_reader_waits_for_writer_in_another_process() {
        use std::{
            process::Command,
            thread,
            time::{Duration, Instant},
        };
        use tantivy::directory::META_LOCK;
        const ENV: &str = "WINCOM_SEARCH_METADATA_LOCK_TEST";
        if let Some(path) = std::env::var_os(ENV) {
            let root = std::path::PathBuf::from(path);
            let reader = TrialDirectory::open(&root, AccessMode::SharedReader, [31; 32]);
            std::fs::write(root.join("child-started"), b"ready").unwrap();
            let _lock = reader.acquire_lock(&META_LOCK).unwrap();
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let writer = TrialDirectory::open(dir.path(), AccessMode::Writable, [31; 32]);
        let held = writer.acquire_lock(&META_LOCK).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "encrypted_index_prototype::tantivy_trial::checked_shared_reader_waits_for_writer_in_another_process"])
            .env(ENV, dir.path()).spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !dir.path().join("child-started").exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        let started = dir.path().join("child-started").exists();
        thread::sleep(Duration::from_millis(150));
        let was_blocked = child.try_wait().unwrap().is_none();
        drop(held);
        let deadline = Instant::now() + Duration::from_secs(15);
        let result = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            thread::sleep(Duration::from_millis(20));
        };
        assert!(started && was_blocked && result.is_some_and(|s| s.success()));
    }

    #[test]
    fn atomic_manifest_reads_never_observe_a_partial_replacement() {
        use std::thread;
        let dir = tempfile::tempdir().unwrap();
        let writable = TrialDirectory::open(dir.path(), AccessMode::Writable, [27; 32]);
        let shared = TrialDirectory::open(dir.path(), AccessMode::SharedReader, [27; 32]);
        let path = Path::new("manifest.json");
        writable.atomic_write(path, &vec![1; 64 * 1024]).unwrap();
        let writer = thread::spawn(move || {
            for n in 0..20 {
                writable
                    .atomic_write(path, &vec![if n % 2 == 0 { 2 } else { 1 }; 64 * 1024])
                    .unwrap();
            }
        });
        for _ in 0..40 {
            let value = shared.atomic_read(path).unwrap();
            assert_eq!(value.len(), 64 * 1024);
            assert!(value.iter().all(|b| *b == 1) || value.iter().all(|b| *b == 2));
        }
        writer.join().unwrap();
    }
}
