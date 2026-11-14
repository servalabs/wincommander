// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only experiment; not wired into production or an approved disk format.
//! Each independently authenticated chunk supports bounded random access.
use aes_gcm::{
    aead::{consts::U12, Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use sha2::Sha256;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use zeroize::{Zeroize, Zeroizing};

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

fn encrypt<W: Write>(mut output: W, key: &[u8; 32], name: &str, plaintext: &[u8]) -> io::Result<()> {
    if plaintext.len().div_ceil(CHUNK) > MAX_FILE_CHUNKS {
        return Err(invalid());
    }
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
        if len.div_ceil(CHUNK) > MAX_FILE_CHUNKS || footer[8..20] != footer_nonce() {
            return Err(invalid());
        }
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
        if range.start > range.end || range.end > self.len || range.end - range.start > MAX_READ_RANGE {
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
    let mut first = Vec::new();
    let mut second = Vec::new();
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
    for r in [0..0, 0..1, CHUNK - 20..CHUNK + 30, CHUNK * 2..text.len(), 0..text.len()] {
        assert_eq!(file.read(r.clone()).unwrap(), text[r]);
    }
    assert!(ProtectedFile::open(io::Cursor::new(bytes.clone()), &[43; 32], "a.idx").is_err());
    assert!(ProtectedFile::open(io::Cursor::new(bytes.clone()), &key, "b.idx").is_err());
    assert!(ProtectedFile::open(io::Cursor::new(bytes[..bytes.len() - 1].to_vec()), &key, "a.idx").is_err());
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
    let mut input = ProtectedFile::open(std::fs::File::open(path).unwrap(), &key, "segment.idx").unwrap();
    assert_eq!(input.read(CHUNK - 9..CHUNK + 10).unwrap(), b"private-search-term");
}

// Deliberately test-only: key management, migration and atomic manifest limits
// are not ready for production. It exercises Tantivy's real Directory contract.
#[cfg(windows)]
pub(crate) mod tantivy_trial;
