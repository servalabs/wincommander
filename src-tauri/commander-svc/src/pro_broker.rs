// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pro sidecar broker owned by the SYSTEM service.

#![allow(dead_code)]

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct HashAcceptance {
    current: Option<String>,
    previous: Option<String>,
    install_metadata_hash: Option<String>,
    install_path_hash: Option<String>,
}

fn random_session_token() -> String {
    let mut buf = [0u8; 32];
    fill_random(&mut buf);
    bytes_to_hex(&buf)
}

fn random_pipe_name() -> String {
    let mut buf = [0u8; 8];
    fill_random(&mut buf);
    format!(r"\\.\pipe\wincmd-pro-{}", bytes_to_hex(&buf))
}

fn fill_random(buf: &mut [u8]) {
    use rand::rngs::OsRng;
    use rand::RngCore;

    OsRng.fill_bytes(buf);
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }

    out
}

fn verify_pro_binary_hash_release(actual: &str, accepted: &HashAcceptance) -> Result<(), String> {
    let actual = actual.trim();

    if actual.is_empty() {
        return Err(
            "Pro did not report a binary hash in Hello ack (handshake refused)".to_string(),
        );
    }

    if accepted_hash_matches(&accepted.current, actual)
        || accepted_hash_matches(&accepted.previous, actual)
    {
        return Ok(());
    }

    if accepted_hash_matches(&accepted.install_metadata_hash, actual)
        && accepted_hash_matches(&accepted.install_path_hash, actual)
    {
        return Ok(());
    }

    Err(format!(
        "Pro binary hash {} is not in the accepted set - refuse handshake. \
         If you just updated Pro, reinstall it from Settings > Pro.",
        actual
    ))
}

fn accepted_hash_matches(expected: &Option<String>, actual: &str) -> bool {
    expected
        .as_deref()
        .map(|hash| !hash.trim().is_empty() && hash.trim().eq_ignore_ascii_case(actual))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_session_token_is_32_bytes_hex_encoded() {
        let token = random_session_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn random_pipe_name_uses_per_session_wincmd_pro_namespace() {
        let pipe = random_pipe_name();
        assert!(pipe.starts_with(r"\\.\pipe\wincmd-pro-"));
        assert_eq!(pipe.len(), r"\\.\pipe\wincmd-pro-".len() + 16);
    }

    #[test]
    fn release_hash_verifier_refuses_empty_hash() {
        assert_eq!(
            verify_pro_binary_hash_release("", &HashAcceptance::default()),
            Err("Pro did not report a binary hash in Hello ack (handshake refused)".to_string())
        );
    }

    #[test]
    fn release_hash_verifier_accepts_current_or_previous_pin() {
        let pins = HashAcceptance {
            current: Some(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            ),
            previous: Some(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            ),
            install_metadata_hash: None,
            install_path_hash: None,
        };

        assert!(verify_pro_binary_hash_release(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            &pins
        )
        .is_ok());
        assert!(verify_pro_binary_hash_release(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            &pins
        )
        .is_ok());
    }

    #[test]
    fn release_hash_verifier_requires_metadata_and_disk_hash_to_match() {
        let actual = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let mut pins = HashAcceptance {
            current: None,
            previous: None,
            install_metadata_hash: Some(actual.to_string()),
            install_path_hash: None,
        };
        assert!(verify_pro_binary_hash_release(actual, &pins).is_err());

        pins.install_path_hash = Some(actual.to_string());
        assert!(verify_pro_binary_hash_release(actual, &pins).is_ok());
    }
}
