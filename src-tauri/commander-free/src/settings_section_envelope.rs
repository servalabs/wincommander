// SPDX-License-Identifier: AGPL-3.0-or-later
//! Self-contained section envelope. Key protection is supplied by the caller.
use super::{decode_section, encode_section, B64, OsRng, RngCore};
use base64::Engine;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub(super) const PREFIX: &str = "enc:section-v3:";
const MAX_WRAPPED: usize = 65_536;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope { wrapped_key: String, ciphertext: String }

pub(super) fn seal(section: &str, data: &[u8], protect: impl Fn(&[u8]) -> Result<Vec<u8>, String>, unprotect: impl Fn(&[u8]) -> Result<Vec<u8>, String>) -> Result<String, String> {
    let mut key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(key.as_mut());
    let wrapped = protect(key.as_ref())?;
    if wrapped.is_empty() || wrapped.len() > MAX_WRAPPED { return Err("invalid protected section key".into()); }
    let check = Zeroizing::new(unprotect(&wrapped)?);
    if check.as_slice() != key.as_ref() { return Err("protected section key did not reopen".into()); }
    let ciphertext = encode_section(&key, data, &format!("section-v3:{section}"))?;
    let envelope = Envelope { wrapped_key: B64.encode(wrapped), ciphertext };
    Ok(format!("{PREFIX}{}", serde_json::to_string(&envelope).map_err(|_| "invalid section envelope")?))
}

pub(super) fn open(section: &str, value: &str, unprotect: impl Fn(&[u8]) -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    let body = value.strip_prefix(PREFIX).ok_or("invalid section envelope")?;
    let envelope: Envelope = serde_json::from_str(body).map_err(|_| "invalid section envelope")?;
    if envelope.wrapped_key.len() > MAX_WRAPPED * 2 || !envelope.ciphertext.starts_with("enc:v2:") { return Err("invalid section envelope".into()); }
    let wrapped = B64.decode(&envelope.wrapped_key).map_err(|_| "invalid section key")?;
    if wrapped.is_empty() || wrapped.len() > MAX_WRAPPED { return Err("invalid section key".into()); }
    let key = Zeroizing::new(unprotect(&wrapped)?);
    let key: &[u8; 32] = key.as_slice().try_into().map_err(|_| "invalid section key")?;
    decode_section(key, &envelope.ciphertext, &format!("section-v3:{section}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity(value: &[u8]) -> Result<Vec<u8>, String> { Ok(value.to_vec()) }
    #[test]
    fn fresh_keys_roundtrip_and_section_swaps_fail() {
        let first = seal("settings", b"private", identity, identity).unwrap();
        let next = seal("settings", b"private", identity, identity).unwrap();
        assert_ne!(first, next);
        assert_eq!(open("settings", &first, identity).unwrap(), b"private");
        assert!(open("other", &first, identity).is_err());
        let user = seal("user-install:settings", b"private", identity, identity).unwrap();
        assert!(open("machine:settings", &user, identity).is_err());
    }
    #[test]
    fn corrupted_keys_and_ciphertext_fail_closed() {
        let original = seal("settings", b"private", identity, identity).unwrap();
        let mut body: Envelope = serde_json::from_str(original.strip_prefix(PREFIX).unwrap()).unwrap();
        body.wrapped_key = B64.encode([7u8; 32]);
        let changed = format!("{PREFIX}{}", serde_json::to_string(&body).unwrap());
        assert!(open("settings", &changed, identity).is_err());
        body.ciphertext = "enc:v1:AAAA".into();
        let legacy = format!("{PREFIX}{}", serde_json::to_string(&body).unwrap());
        assert!(open("settings", &legacy, identity).is_err());
        assert!(seal("settings", b"private", identity, |_| Err("unavailable".into())).is_err());
        assert!(seal("settings", b"private", identity, |_| Ok(vec![1; 32])).is_err());
    }
    #[test]
    fn legacy_section_survives_failed_publish_and_new_section_reopens() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.dat");
        let legacy_key = [42u8; 32];
        let legacy = encode_section(&legacy_key, br#"{"setting":true}"#, "machine:settings").unwrap();
        std::fs::write(&path, &legacy).unwrap();
        let decoded = decode_section(&legacy_key, &std::fs::read_to_string(&path).unwrap(), "machine:settings").unwrap();
        let candidate = seal("settings", &decoded, identity, identity).unwrap();
        assert!(crate::datastore_io::atomic_write(&directory.path().join("missing").join("settings.dat"), candidate.as_bytes()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), legacy);
        crate::datastore_io::atomic_write(&path, candidate.as_bytes()).unwrap();
        assert_eq!(open("settings", &std::fs::read_to_string(&path).unwrap(), identity).unwrap(), decoded);
    }

    #[cfg(windows)]
    #[test]
    fn native_dpapi_machine_and_user_envelopes_roundtrip() {
        use super::super::{dpapi_protect, dpapi_unprotect, user_dpapi_protect, user_dpapi_unprotect};
        let machine = seal("machine:settings", b"machine value", dpapi_protect, dpapi_unprotect).unwrap();
        assert_eq!(open("machine:settings", &machine, dpapi_unprotect).unwrap(), b"machine value");
        let user = seal("user-install:settings", b"user value", user_dpapi_protect, user_dpapi_unprotect).unwrap();
        assert_eq!(open("user-install:settings", &user, user_dpapi_unprotect).unwrap(), b"user value");
        assert!(open("machine:settings", &user, user_dpapi_unprotect).is_err());
    }

}
