// SPDX-License-Identifier: AGPL-3.0-or-later
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use ed25519_dalek::VerifyingKey;
use sha2::{Digest, Sha256};

pub(super) fn canonical_generation(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
        && value.bytes().any(|b| b != b'0' && b != b'-')
}

pub(super) fn strong_canonical_key(value: &str) -> bool {
    let Ok(bytes) = B64.decode(value) else {
        return false;
    };
    let Ok(array) = <[u8; 32]>::try_from(bytes.as_slice()) else {
        return false;
    };
    B64.encode(array) == value && VerifyingKey::from_bytes(&array).is_ok_and(|key| !key.is_weak())
}

pub(super) fn canonical_signature(value: &str) -> bool {
    B64.decode(value)
        .is_ok_and(|bytes| bytes.len() == 64 && B64.encode(bytes) == value)
}

pub(super) fn hash_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn windows_bytes_sha256(bytes: &[u8]) -> String {
    B64.encode(Sha256::digest(bytes))
}

pub(super) fn hash_shape(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
