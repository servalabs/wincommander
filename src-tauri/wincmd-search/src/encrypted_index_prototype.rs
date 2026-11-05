// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only experiment; not wired into production or an approved disk format.
//! Each independently authenticated chunk supports bounded random access.
use aes_gcm::{
    aead::{consts::U12, Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;

const MAGIC: &[u8; 8] = b"WCIDX001";
const CHUNK: usize = 64 * 1024;
const HEADER: usize = 24;
const OVERHEAD: usize = 28; // 96-bit random nonce and 128-bit tag
const FOOTER: usize = 8 + OVERHEAD;

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "index authentication failed")
}

fn aad(name: &str, id: &[u8], label: &[u8], number: u64) -> Vec<u8> {
    let mut result = Vec::new();
    result.extend_from_slice(b"wincommander-search-index-test-v1");
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

fn encrypt<W: Write>(
    mut output: W,
    key: &[u8; 32],
    name: &str,
    plaintext: &[u8],
) -> io::Result<()> {
    let cipher = Aes256Gcm::new(key.into());
    let mut id = [0u8; 16];
    OsRng.fill_bytes(&mut id);
    output.write_all(MAGIC)?;
    output.write_all(&id)?;
    for (i, chunk) in plaintext.chunks(CHUNK).enumerate() {
        let mut n = [0u8; 12];
        OsRng.fill_bytes(&mut n);
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
    let mut n = [0u8; 12];
    OsRng.fill_bytes(&mut n);
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
    id: [u8; 16],
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
        let id: [u8; 16] = header[8..].try_into().map_err(|_| invalid())?;
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
        let cipher = Aes256Gcm::new(key.into());
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
        if range.start > range.end || range.end > self.len {
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
mod tantivy_trial {
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

    #[derive(Clone)]
    struct TrialDirectory {
        inner: CheckedDirectory,
        key: [u8; 32],
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
        file: Mutex<ProtectedFile<HandleReader>>,
        len: usize,
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
        cipher: Aes256Gcm,
        path: String,
        id: [u8; 16],
        chunks: u64,
        total: u64,
        plain: Vec<u8>,
        failed: bool,
    }
    impl TrialWriter {
        fn new(mut sink: WritePtr, key: &[u8; 32], path: String) -> io::Result<Self> {
            let mut id = [0; 16];
            OsRng.fill_bytes(&mut id);
            sink.write_all(MAGIC)?;
            sink.write_all(&id)?;
            Ok(Self {
                sink: Some(sink),
                cipher: Aes256Gcm::new(key.into()),
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
            let mut n = [0; 12];
            OsRng.fill_bytes(&mut n);
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
            self.plain.fill(0);
            self.plain.clear();
            Ok(())
        }
    }
    impl Drop for TrialWriter {
        fn drop(&mut self) {
            self.plain.fill(0);
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
            let mut n = [0; 12];
            OsRng.fill_bytes(&mut n);
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
            let source = self.inner.get_file_handle(path)?;
            let file = ProtectedFile::open(
                HandleReader {
                    inner: source,
                    offset: 0,
                },
                &self.key,
                name(path).map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?,
            )
            .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
            let len = file.len;
            Ok(Arc::new(TrialHandle {
                file: Mutex::new(file),
                len,
            }))
        }
        fn delete(&self, path: &Path) -> Result<(), DeleteError> {
            self.inner.delete(path)
        }
        fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
            self.inner.exists(path)
        }
        fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
            let path_name = name(path)
                .map_err(|e| OpenWriteError::wrap_io_error(e, path.into()))?
                .to_owned();
            let sink = self.inner.open_write(path)?;
            let writer = TrialWriter::new(sink, &self.key, path_name)
                .map_err(|e| OpenWriteError::wrap_io_error(e, path.into()))?;
            Ok(BufWriter::new(Box::new(writer)))
        }
        fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
            let bytes = self
                .inner
                .atomic_read_bounded(path, MAX_ATOMIC_CIPHERTEXT)?;
            let mut f = ProtectedFile::open(
                io::Cursor::new(bytes),
                &self.key,
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
            encrypt(&mut encrypted, &self.key, name(path)?, data)?;
            // Windows can briefly refuse atomic replacement while a shared
            // reader has the old manifest open. Preserve a permanent failure.
            #[cfg(windows)]
            for delay in [2, 4, 8, 16, 32, 64, 128] {
                match self.inner.atomic_write(path, &encrypted) {
                    Err(e) if e.raw_os_error() == Some(5) => {
                        std::thread::sleep(std::time::Duration::from_millis(delay))
                    }
                    result => return result,
                }
            }
            self.inner.atomic_write(path, &encrypted)
        }
        fn sync_directory(&self) -> io::Result<()> {
            self.inner.sync_directory()
        }
        fn acquire_lock(&self, lock: &Lock) -> Result<DirectoryLock, LockError> {
            self.inner.acquire_lock(lock)
        }
        fn watch(&self, callback: WatchCallback) -> tantivy::Result<WatchHandle> {
            self.inner.watch(callback)
        }
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
        let directory = TrialDirectory {
            inner: CheckedDirectory::open(dir.path(), AccessMode::Writable).unwrap(),
            key: [9; 32],
        };
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
            TrialWriter::new(inner.open_write(path).unwrap(), &key, "stream.idx".into()).unwrap();
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
        let mut writer = TrialWriter::new(sink, &key, "failed.idx".into()).unwrap();
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
        let writer_dir = TrialDirectory {
            inner: CheckedDirectory::open(dir.path(), AccessMode::Writable).unwrap(),
            key: [23; 32],
        };
        let mut schema = Schema::builder();
        let body = schema.add_text_field("body", TEXT);
        let index = Index::create(writer_dir.clone(), schema.build(), Default::default()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();
        writer
            .add_document(doc!(body => "sharedreadtoken"))
            .unwrap();
        writer.commit().unwrap();
        let reader_dir = TrialDirectory {
            inner: CheckedDirectory::open(dir.path(), AccessMode::SharedReader).unwrap(),
            key: [23; 32],
        };
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
        let directory = TrialDirectory {
            inner: CheckedDirectory::open(dir.path(), AccessMode::Writable).unwrap(),
            key: [24; 32],
        };
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
        let writer = TrialDirectory {
            inner: CheckedDirectory::open(dir.path(), AccessMode::Writable).unwrap(),
            key: [25; 32],
        };
        let reader = TrialDirectory {
            inner: CheckedDirectory::open(dir.path(), AccessMode::SharedReader).unwrap(),
            key: [25; 32],
        };
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
            let reader = TrialDirectory {
                inner: CheckedDirectory::open(&root, AccessMode::SharedReader).unwrap(),
                key: [31; 32],
            };
            std::fs::write(root.join("child-started"), b"ready").unwrap();
            let _lock = reader.acquire_lock(&META_LOCK).unwrap();
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let writer = TrialDirectory {
            inner: CheckedDirectory::open(dir.path(), AccessMode::Writable).unwrap(),
            key: [31; 32],
        };
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
        let writable = TrialDirectory {
            inner: CheckedDirectory::open(dir.path(), AccessMode::Writable).unwrap(),
            key: [27; 32],
        };
        let shared = TrialDirectory {
            inner: CheckedDirectory::open(dir.path(), AccessMode::SharedReader).unwrap(),
            key: [27; 32],
        };
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
