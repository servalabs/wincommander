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

// Deliberately test-only: this adapter buffers entire files and is unsuitable for
// production-size indexes. It exercises Tantivy's real Directory contract.
mod tantivy_trial {
    use super::*;
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

    #[derive(Clone)]
    struct TrialDirectory {
        inner: MmapDirectory,
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
    struct TrialHandle {
        file: Mutex<ProtectedFile<io::Cursor<Vec<u8>>>>,
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
        key: [u8; 32],
        path: String,
        plain: Vec<u8>,
    }
    impl Write for TrialWriter {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.plain.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl TerminatingWrite for TrialWriter {
        fn terminate_ref(&mut self, _: AntiCallToken) -> io::Result<()> {
            let mut sink = self.sink.take().ok_or_else(invalid)?;
            encrypt(&mut sink, &self.key, &self.path, &self.plain)?;
            sink.terminate()
        }
    }
    impl Directory for TrialDirectory {
        fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
            let source = self.inner.get_file_handle(path)?;
            let bytes = source
                .read_bytes(0..source.len())
                .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
            let file = ProtectedFile::open(
                io::Cursor::new(bytes.as_ref().to_vec()),
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
            Ok(BufWriter::new(Box::new(TrialWriter {
                sink: Some(sink),
                key: self.key,
                path: path_name,
                plain: Vec::new(),
            })))
        }
        fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
            let bytes = self.inner.atomic_read(path)?;
            let mut f = ProtectedFile::open(
                io::Cursor::new(bytes),
                &self.key,
                name(path).map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?,
            )
            .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
            f.read(0..f.len)
                .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))
        }
        fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
            let mut encrypted = Vec::new();
            encrypt(&mut encrypted, &self.key, name(path)?, data)?;
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
            inner: MmapDirectory::open(dir.path()).unwrap(),
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
}
