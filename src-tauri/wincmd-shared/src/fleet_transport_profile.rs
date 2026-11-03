// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure comparison of environment inputs that can affect Fleet HTTP transport.
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

/// Both sides collect these in this order; values are never persisted here.
pub const ENV_KEYS: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "REQWEST_CA_BUNDLE",
    "CURL_CA_BUNDLE",
];

/// `None` distinguishes an absent variable from a present empty value. Length
/// framing prevents concatenation ambiguity. The digest is not a credential.
pub fn fingerprint(values: &[Option<&str>]) -> Option<String> {
    if values.len() != ENV_KEYS.len() {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update(b"wincommander:fleet:transport-environment:v1\0");
    for (key, value) in ENV_KEYS.iter().zip(values) {
        hash.update((key.len() as u64).to_be_bytes());
        hash.update(key.as_bytes());
        match value {
            None => hash.update([0]),
            Some(value) => {
                hash.update([1]);
                hash.update((value.len() as u64).to_be_bytes());
                hash.update(value.as_bytes());
            }
        }
    }
    Some(STANDARD.encode(hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprint_binds_names_values_and_presence() {
        let absent = vec![None; ENV_KEYS.len()];
        let first = fingerprint(&absent).unwrap();
        let mut changed = absent.clone();
        changed[0] = Some("");
        assert_ne!(first, fingerprint(&changed).unwrap());
        changed[0] = Some("http://proxy.example");
        assert_ne!(first, fingerprint(&changed).unwrap());
        assert!(fingerprint(&[]).is_none());
    }
}
