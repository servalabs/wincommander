// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only cutover record experiment. No filesystem publication or rollback
//! protection; the in-memory key provider below does not model Windows DPAPI.
use aes_gcm::{aead::{Aead, KeyInit, Payload}, Aes256Gcm, Nonce};
use rand::{rngs::OsRng, RngCore};

const MAGIC: &[u8; 8] = b"WCICUT01";
const RECORD_LEN: usize = 8 + 12 + 16 + 16 + 1 + 16;
const AAD: &[u8] = b"wincommander-index-cutover-experiment-v1";

#[derive(Debug, PartialEq, Eq)]
struct Record { index: [u8; 16], generation: [u8; 16], committed: bool }

// Intentionally ephemeral and test-only. A real provider must persist a key
// in user-scoped DPAPI and distinguish missing from inaccessible or corrupt.
struct StubKey([u8; 32]);
impl StubKey {
    fn random() -> Self { let mut key = [0; 32]; OsRng.fill_bytes(&mut key); Self(key) }
}

fn nonce(n: &[u8]) -> Result<&Nonce<aes_gcm::aead::consts::U12>, ()> { n.try_into().map_err(|_| ()) }

fn seal(key: &StubKey, record: &Record) -> Result<Vec<u8>, ()> {
    let mut random_nonce = [0; 12]; OsRng.fill_bytes(&mut random_nonce);
    let mut plain = [0; 33];
    plain[..16].copy_from_slice(&record.index);
    plain[16..32].copy_from_slice(&record.generation);
    plain[32] = u8::from(record.committed);
    let cipher = Aes256Gcm::new((&key.0).into());
    let encrypted = cipher.encrypt(nonce(&random_nonce)?, Payload { msg: &plain, aad: AAD }).map_err(|_| ())?;
    let mut out = Vec::with_capacity(RECORD_LEN);
    out.extend_from_slice(MAGIC); out.extend_from_slice(&random_nonce); out.extend_from_slice(&encrypted);
    Ok(out)
}

fn open(key: &StubKey, bytes: &[u8], expected_index: &[u8; 16]) -> Result<Record, ()> {
    if bytes.len() != RECORD_LEN || &bytes[..8] != MAGIC { return Err(()); }
    let cipher = Aes256Gcm::new((&key.0).into());
    let plain = cipher.decrypt(nonce(&bytes[8..20])?, Payload { msg: &bytes[20..], aad: AAD }).map_err(|_| ())?;
    let index: [u8; 16] = plain[..16].try_into().map_err(|_| ())?;
    if &index != expected_index || plain[32] > 1 { return Err(()); }
    Ok(Record { index, generation: plain[16..32].try_into().map_err(|_| ())?, committed: plain[32] == 1 })
}

// This gates the state transition but does not itself verify the index: the
// caller must check the new generation with a real reader before passing true.
fn commit(key: &StubKey, prepared: &[u8], index: &[u8; 16], verified: bool) -> Result<Vec<u8>, ()> {
    let mut record = open(key, prepared, index)?;
    if record.committed || !verified { return Err(()); }
    record.committed = true;
    seal(key, &record)
}

#[test]
fn staged_record_requires_validation_and_matching_identity() {
    let key = StubKey::random(); let index = [1; 16];
    let record = Record { index, generation: [2; 16], committed: false };
    let staged = seal(&key, &record).unwrap();
    assert_eq!(open(&key, &staged, &index).unwrap(), record);
    assert!(commit(&key, &staged, &index, false).is_err());
    assert!(commit(&key, &staged, &[3; 16], true).is_err());
    let published = commit(&key, &staged, &index, true).unwrap();
    assert!(open(&key, &published, &index).unwrap().committed);
    assert!(commit(&key, &published, &index, true).is_err());
}

#[test]
fn bounded_record_rejects_corruption_wrong_key_and_truncation() {
    let key = StubKey::random(); let other = StubKey::random();
    let record = Record { index: [4; 16], generation: [5; 16], committed: false };
    let bytes = seal(&key, &record).unwrap();
    assert_eq!(bytes.len(), RECORD_LEN);
    assert!(open(&other, &bytes, &record.index).is_err());
    for i in 0..bytes.len() { let mut bad = bytes.clone(); bad[i] ^= 1; assert!(open(&key, &bad, &record.index).is_err()); }
    for len in 0..bytes.len() { assert!(open(&key, &bytes[..len], &record.index).is_err()); }
    let mut oversized = bytes.clone(); oversized.push(0);
    assert!(open(&key, &oversized, &record.index).is_err());
}

#[test]
fn authentication_alone_does_not_prevent_replay() {
    let key = StubKey::random();
    let old = Record { index: [6; 16], generation: [7; 16], committed: true };
    let old_bytes = seal(&key, &old).unwrap();
    let new = Record { generation: [8; 16], ..old };
    assert!(open(&key, &seal(&key, &new).unwrap(), &new.index).is_ok());
    // A separate trusted freshness anchor is required before production use.
    assert_eq!(open(&key, &old_bytes, &new.index).unwrap().generation, [7; 16]);
}
