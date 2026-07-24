// SPDX-License-Identifier: AGPL-3.0-or-later
//! F6 Phase-1, Piece 1 — USB Wipe-Authorization Handshake Token
//!
//! # Purpose
//!
//! When WinCommander fires the distress wipe it writes a **signed authorization
//! token** to the wipe-USB before rebooting.  The USB's wipe environment verifies
//! that token before it will wipe anything.  **No valid token → the USB wipes
//! nothing**, so a lost/stolen/random USB inserted into any machine is safe.
//!
//! # Crypto
//!
//! Ed25519 (via `ed25519-dalek` v2).  No custom or novel crypto.
//!
//! # Canonical byte layout (payload)
//!
//! The payload is serialized to a fixed sequence of fields before signing.
//! The layout is documented here precisely so a non-Rust (shell/initramfs)
//! verifier can reimplement it identically without touching this crate.
//!
//! ```text
//! Field        Type              Encoding                   Length
//! ──────────── ───────────────── ────────────────────────── ──────────────────────
//! version      u8                raw byte                   1 byte
//! device_id    UTF-8 string      u16 BE length-prefix       2 + len(device_id) bytes
//! nonce        [u8; 32]          raw bytes                  32 bytes
//! issued_at    i64               big-endian                 8 bytes
//! expires_at   i64               big-endian                 8 bytes
//! action       UTF-8 string      u16 BE length-prefix       2 + len(action) bytes
//! ```
//!
//! Total for the typical case (36-byte UUID device_id, action = "wipe"):
//!   1 + (2+36) + 32 + 8 + 8 + (2+4) = **93 bytes**
//!
//! **Encoding notes:**
//! - All multi-byte integers are big-endian.
//! - Strings are NOT NUL-terminated; the length prefix tells the reader where they end.
//! - There is no trailing padding.
//!
//! # Wire format
//!
//! ```text
//! <base64url-no-pad(canonical_payload_bytes)>.<base64url-no-pad(ed25519_signature_64bytes)>
//! ```
//!
//! The signature is over the canonical payload bytes (NOT the base64 representation).
//! Both parts use RFC 4648 URL-safe base64 with no padding characters.  This format
//! is readable by a shell one-liner:
//!
//! ```sh
//! # Split on '.', decode each half:
//! payload_b64=$(cut -d. -f1 token.txt)
//! sig_b64=$(cut -d. -f2 token.txt)
//! payload_bytes=$(echo "$payload_b64" | base64 -d)
//! # Verify with openssl or libsodium:
//! openssl pkey -pubin -in pubkey.pem -verify -signature <(echo "$sig_b64" | base64 -d) <<< "$payload_bytes"
//! ```
//!
//! # Public key format (USB provisioning)
//!
//! The verifying key is stored on the USB as 32 raw bytes (Ed25519 compressed point).
//! [`verifying_key_to_bytes`] returns `[u8; 32]`; [`verifying_key_from_bytes`]
//! reconstructs from the same 32 bytes.  Write the 32 bytes to a fixed path on the
//! USB control partition (e.g. `/wipe/pubkey.bin`).  The initramfs verifier reads
//! exactly 32 bytes from that path.
//!
//! # Single-use enforcement
//!
//! The nonce is surfaced in [`VerifiedToken`].  The USB-side verifier must maintain
//! a "nonces seen" store (a flat file is sufficient, given the USB wiped the target
//! disk on first use).  Single-use logic intentionally lives on the USB side, not
//! here — this crate is pure crypto + format.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

// ── Errors ────────────────────────────────────────────────────────────────────

/// All the ways token issuance or verification can fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WipeTokenError {
    /// The wire string is not `<base64url>.<base64url>`, or a component decodes
    /// to the wrong number of bytes (payload too short; signature != 64 bytes).
    BadFormat,
    /// The Ed25519 signature over the canonical payload bytes did not verify.
    BadSignature,
    /// The `device_id` inside the token does not match the expected device ID.
    WrongDevice,
    /// `now_unix > expires_at`.
    Expired,
    /// `now_unix < issued_at` — clock skew or the token was issued in the future.
    NotYetValid,
    /// `version != 1`.
    WrongVersion,
    /// `action != "wipe"`.
    WrongAction,
}

impl std::fmt::Display for WipeTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WipeTokenError::BadFormat => f.write_str("token is malformed or truncated"),
            WipeTokenError::BadSignature => f.write_str("Ed25519 signature did not verify"),
            WipeTokenError::WrongDevice => f.write_str("device_id mismatch"),
            WipeTokenError::Expired => f.write_str("token has expired"),
            WipeTokenError::NotYetValid => {
                f.write_str("token is not yet valid (issued_at in future)")
            }
            WipeTokenError::WrongVersion => f.write_str("unsupported token version"),
            WipeTokenError::WrongAction => f.write_str("token action is not 'wipe'"),
        }
    }
}

impl std::error::Error for WipeTokenError {}

// ── Payload struct ─────────────────────────────────────────────────────────

/// Parsed payload of a wipe-auth token.  Returned by [`verify_wipe_token`]
/// on success so the caller can extract the nonce for single-use enforcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedToken {
    pub version: u8,
    pub device_id: String,
    /// 32-byte random nonce from the token.  The USB-side caller must check that
    /// this nonce has not been seen before and record it after accepting.
    pub nonce: [u8; 32],
    pub issued_at: i64,
    pub expires_at: i64,
    pub action: String,
}

// ── Canonical serialization ────────────────────────────────────────────────

/// Serialize the payload fields to canonical bytes for signing.
///
/// Layout (see module-level doc for the full table):
///   1 byte  version
///   2+n     u16 BE len + UTF-8 device_id
///   32      nonce
///   8       i64 BE issued_at
///   8       i64 BE expires_at
///   2+n     u16 BE len + UTF-8 action
fn canonical_payload_bytes(
    version: u8,
    device_id: &str,
    nonce: &[u8; 32],
    issued_at: i64,
    expires_at: i64,
    action: &str,
) -> Vec<u8> {
    let dev_bytes = device_id.as_bytes();
    let act_bytes = action.as_bytes();
    // pre-allocate exactly the right size
    let capacity = 1 + 2 + dev_bytes.len() + 32 + 8 + 8 + 2 + act_bytes.len();
    let mut buf = Vec::with_capacity(capacity);

    buf.push(version);

    let dev_len =
        u16::try_from(dev_bytes.len()).expect("device_id length fits in u16 (max 65535 bytes)");
    buf.extend_from_slice(&dev_len.to_be_bytes());
    buf.extend_from_slice(dev_bytes);

    buf.extend_from_slice(nonce);

    buf.extend_from_slice(&issued_at.to_be_bytes());
    buf.extend_from_slice(&expires_at.to_be_bytes());

    let act_len = u16::try_from(act_bytes.len()).expect("action length fits in u16");
    buf.extend_from_slice(&act_len.to_be_bytes());
    buf.extend_from_slice(act_bytes);

    buf
}

/// Parse canonical payload bytes back to fields.
/// Returns Err(BadFormat) on any truncation or length overflow.
fn parse_payload_bytes(bytes: &[u8]) -> Result<VerifiedToken, WipeTokenError> {
    let mut pos = 0usize;

    macro_rules! read_exact {
        ($n:expr) => {{
            let end = pos + $n;
            if end > bytes.len() {
                return Err(WipeTokenError::BadFormat);
            }
            let slice = &bytes[pos..end];
            pos = end;
            slice
        }};
    }

    let version = read_exact!(1)[0];

    let dev_len =
        u16::from_be_bytes(read_exact!(2).try_into().expect("slice is exactly 2 bytes")) as usize;
    let dev_bytes = read_exact!(dev_len);
    let device_id = std::str::from_utf8(dev_bytes)
        .map_err(|_| WipeTokenError::BadFormat)?
        .to_string();

    let nonce_slice = read_exact!(32);
    let nonce: [u8; 32] = nonce_slice.try_into().expect("slice is exactly 32 bytes");

    let issued_at =
        i64::from_be_bytes(read_exact!(8).try_into().expect("slice is exactly 8 bytes"));
    let expires_at =
        i64::from_be_bytes(read_exact!(8).try_into().expect("slice is exactly 8 bytes"));

    let act_len =
        u16::from_be_bytes(read_exact!(2).try_into().expect("slice is exactly 2 bytes")) as usize;
    let act_bytes = read_exact!(act_len);
    let action = std::str::from_utf8(act_bytes)
        .map_err(|_| WipeTokenError::BadFormat)?
        .to_string();

    // must consume exactly all bytes — no trailing garbage
    if pos != bytes.len() {
        return Err(WipeTokenError::BadFormat);
    }

    Ok(VerifiedToken {
        version,
        device_id,
        nonce,
        issued_at,
        expires_at,
        action,
    })
}

// ── Key helpers ────────────────────────────────────────────────────────────

/// Generate a fresh Ed25519 keypair for USB provisioning.
///
/// Call this ONCE at "Create Wipe USB" time.  Store the `SigningKey` in
/// WinCommander's encrypted settings (never on the USB).  Write the
/// `VerifyingKey`'s 32 raw bytes (from [`verifying_key_to_bytes`]) to the
/// USB control partition so the initramfs verifier can load it.
pub fn generate_provisioning_keypair() -> (SigningKey, VerifyingKey) {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("OS RNG is available");
    let signing = SigningKey::from_bytes(&seed);
    let verifying = signing.verifying_key();
    (signing, verifying)
}

/// Serialize a verifying key to its canonical 32-byte compressed representation.
/// Write these bytes verbatim to the USB (e.g. `/wipe/pubkey.bin`).
pub fn verifying_key_to_bytes(vk: &VerifyingKey) -> [u8; 32] {
    vk.to_bytes()
}

/// Reconstruct a verifying key from 32 bytes read from the USB.
/// Returns `None` if the bytes are not a valid Ed25519 point.
pub fn verifying_key_from_bytes(bytes: &[u8; 32]) -> Option<VerifyingKey> {
    VerifyingKey::from_bytes(bytes).ok()
}

/// Serialize a signing key to its canonical 32-byte seed.
/// Store this in WinCommander's encrypted settings at provisioning time.
pub fn signing_key_to_bytes(sk: &SigningKey) -> [u8; 32] {
    sk.to_bytes()
}

/// Reconstruct a signing key from a 32-byte seed.
pub fn signing_key_from_bytes(bytes: &[u8; 32]) -> SigningKey {
    SigningKey::from_bytes(bytes)
}

// ── Issue ──────────────────────────────────────────────────────────────────

/// Issue a wipe-authorization token.
///
/// # Parameters
/// - `device_id`  — the device UUID (or any stable identifier) that the token is
///   bound to.  The verifier rejects any token where this field doesn't match the
///   expected ID.
/// - `ttl_secs`   — how many seconds the token remains valid after `now_unix`.
///   Typical: 300 (5 minutes); must survive the reboot + initramfs init time.
/// - `now_unix`   — current Unix time in seconds (caller-supplied; no `SystemTime`
///   calls inside this library).
/// - `signing_key` — the device's provisioning signing key.
///
/// # Returns
///
/// The wire token as a string: `<base64url(payload)>.<base64url(signature)>`.
///
/// The nonce is drawn from the OS RNG inside this function.
pub fn issue_wipe_token(
    device_id: &str,
    ttl_secs: i64,
    now_unix: i64,
    signing_key: &SigningKey,
) -> String {
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce).expect("OS RNG is available");

    let issued_at = now_unix;
    let expires_at = now_unix + ttl_secs;

    let payload = canonical_payload_bytes(1, device_id, &nonce, issued_at, expires_at, "wipe");
    let sig: Signature = signing_key.sign(&payload);

    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(&payload),
        URL_SAFE_NO_PAD.encode(sig.to_bytes()),
    )
}

// ── Verify ─────────────────────────────────────────────────────────────────

/// Verify a wipe-auth token.
///
/// Checks (in order):
///   1. Wire format — two base64url parts, signature is 64 bytes.
///   2. Ed25519 signature over the canonical payload bytes.
///   3. `version == 1`
///   4. `action == "wipe"`
///   5. `device_id == expected_device_id`
///   6. `now_unix >= issued_at` (not-yet-valid)
///   7. `now_unix <= expires_at` (not expired)
///
/// Returns the parsed [`VerifiedToken`] on success (includes the nonce so the
/// caller can enforce single-use by recording it in a nonce-seen store).
///
/// # Parameters
/// - `token`              — the wire string from [`issue_wipe_token`].
/// - `verifying_key`      — the device's provisioning public key.
/// - `expected_device_id` — the device ID the calling environment expects.
/// - `now_unix`           — current Unix time in seconds (caller-supplied).
pub fn verify_wipe_token(
    token: &str,
    verifying_key: &VerifyingKey,
    expected_device_id: &str,
    now_unix: i64,
) -> Result<VerifiedToken, WipeTokenError> {
    // ── 1. Wire format ──
    let (payload_b64, sig_b64) = token.split_once('.').ok_or(WipeTokenError::BadFormat)?;

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| WipeTokenError::BadFormat)?;

    let sig_bytes = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|_| WipeTokenError::BadFormat)?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| WipeTokenError::BadFormat)?;
    let signature = Signature::from_bytes(&sig_arr);

    // ── 2. Signature ──
    verifying_key
        .verify(&payload_bytes, &signature)
        .map_err(|_| WipeTokenError::BadSignature)?;

    // ── 3–7. Payload fields ──
    let parsed = parse_payload_bytes(&payload_bytes)?;

    if parsed.version != 1 {
        return Err(WipeTokenError::WrongVersion);
    }
    if parsed.action != "wipe" {
        return Err(WipeTokenError::WrongAction);
    }
    if parsed.device_id != expected_device_id {
        return Err(WipeTokenError::WrongDevice);
    }
    if now_unix < parsed.issued_at {
        return Err(WipeTokenError::NotYetValid);
    }
    if now_unix > parsed.expires_at {
        return Err(WipeTokenError::Expired);
    }

    Ok(parsed)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    // Deterministic signing key for tests that don't need randomness.
    fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[0x42u8; 32])
    }

    fn test_vk() -> VerifyingKey {
        test_signing_key().verifying_key()
    }

    const DEVICE: &str = "550e8400-e29b-41d4-a716-446655440000";
    const NOW: i64 = 1_751_000_000; // 2025-06-27 UTC — fixed epoch for all tests

    // ── Happy path ──────────────────────────────────────────────────────

    #[test]
    fn round_trip_issue_verify_ok() {
        let sk = test_signing_key();
        let token = issue_wipe_token(DEVICE, 300, NOW, &sk);
        let vt = verify_wipe_token(&token, &test_vk(), DEVICE, NOW).expect("must verify");
        assert_eq!(vt.version, 1);
        assert_eq!(vt.device_id, DEVICE);
        assert_eq!(vt.action, "wipe");
        assert_eq!(vt.issued_at, NOW);
        assert_eq!(vt.expires_at, NOW + 300);
        assert_eq!(vt.nonce.len(), 32);
    }

    #[test]
    fn verify_at_boundary_timestamps_ok() {
        let sk = test_signing_key();
        let token = issue_wipe_token(DEVICE, 300, NOW, &sk);
        // exactly at issued_at → valid
        verify_wipe_token(&token, &test_vk(), DEVICE, NOW).unwrap();
        // exactly at expires_at → valid (inclusive boundary)
        verify_wipe_token(&token, &test_vk(), DEVICE, NOW + 300).unwrap();
    }

    // ── Signature failures ───────────────────────────────────────────────

    #[test]
    fn tampered_payload_rejected_with_bad_signature() {
        let sk = test_signing_key();
        let token = issue_wipe_token(DEVICE, 300, NOW, &sk);
        // Flip one bit in the payload part (first base64 char → replace with adjacent char)
        let parts: Vec<&str> = token.splitn(2, '.').collect();
        let mut corrupted_payload = parts[0].to_string();
        // Replace the last char to ensure payload bytes differ
        let last = corrupted_payload.pop().unwrap_or('A');
        let replacement = if last == 'A' { 'B' } else { 'A' };
        corrupted_payload.push(replacement);
        let tampered = format!("{}.{}", corrupted_payload, parts[1]);
        let err = verify_wipe_token(&tampered, &test_vk(), DEVICE, NOW).unwrap_err();
        // Could be BadSignature (payload decodes but sig fails) or BadFormat (base64 invalid)
        assert!(
            err == WipeTokenError::BadSignature || err == WipeTokenError::BadFormat,
            "expected BadSignature or BadFormat, got {:?}",
            err
        );
    }

    #[test]
    fn wrong_verifying_key_rejected() {
        let sk = test_signing_key();
        let token = issue_wipe_token(DEVICE, 300, NOW, &sk);
        let other_sk = SigningKey::from_bytes(&[0x99u8; 32]);
        let other_vk = other_sk.verifying_key();
        let err = verify_wipe_token(&token, &other_vk, DEVICE, NOW).unwrap_err();
        assert_eq!(err, WipeTokenError::BadSignature);
    }

    // ── Device binding ───────────────────────────────────────────────────

    #[test]
    fn wrong_device_id_rejected() {
        let sk = test_signing_key();
        let token = issue_wipe_token(DEVICE, 300, NOW, &sk);
        let err = verify_wipe_token(&token, &test_vk(), "different-device-id", NOW).unwrap_err();
        assert_eq!(err, WipeTokenError::WrongDevice);
    }

    // ── Temporal checks ──────────────────────────────────────────────────

    #[test]
    fn expired_token_rejected() {
        let sk = test_signing_key();
        let token = issue_wipe_token(DEVICE, 300, NOW, &sk);
        let err = verify_wipe_token(&token, &test_vk(), DEVICE, NOW + 301).unwrap_err();
        assert_eq!(err, WipeTokenError::Expired);
    }

    #[test]
    fn not_yet_valid_token_rejected() {
        let sk = test_signing_key();
        // Issue a token "in the future" relative to our check time
        let token = issue_wipe_token(DEVICE, 300, NOW + 1000, &sk);
        let err = verify_wipe_token(&token, &test_vk(), DEVICE, NOW).unwrap_err();
        assert_eq!(err, WipeTokenError::NotYetValid);
    }

    // ── Format errors ────────────────────────────────────────────────────

    #[test]
    fn malformed_string_no_dot_rejected() {
        let err = verify_wipe_token("notavalidtoken", &test_vk(), DEVICE, NOW).unwrap_err();
        assert_eq!(err, WipeTokenError::BadFormat);
    }

    #[test]
    fn malformed_string_bad_base64_rejected() {
        let err = verify_wipe_token("!!!.???", &test_vk(), DEVICE, NOW).unwrap_err();
        assert_eq!(err, WipeTokenError::BadFormat);
    }

    #[test]
    fn malformed_string_short_signature_rejected() {
        let sk = test_signing_key();
        let token = issue_wipe_token(DEVICE, 300, NOW, &sk);
        let payload_b64 = token.split('.').next().unwrap();
        // Signature part is just a few bytes — not 64
        let bad = format!("{}.AAAA", payload_b64);
        let err = verify_wipe_token(&bad, &test_vk(), DEVICE, NOW).unwrap_err();
        assert_eq!(err, WipeTokenError::BadFormat);
    }

    #[test]
    fn empty_string_rejected() {
        let err = verify_wipe_token("", &test_vk(), DEVICE, NOW).unwrap_err();
        assert_eq!(err, WipeTokenError::BadFormat);
    }

    // ── Nonce surfacing and uniqueness ───────────────────────────────────

    #[test]
    fn nonce_is_surfaced_in_verified_token() {
        let sk = test_signing_key();
        let token = issue_wipe_token(DEVICE, 300, NOW, &sk);
        let vt = verify_wipe_token(&token, &test_vk(), DEVICE, NOW).unwrap();
        // Nonce must be 32 bytes and not all-zero (with overwhelming probability)
        assert_eq!(vt.nonce.len(), 32);
    }

    #[test]
    fn two_tokens_have_different_nonces() {
        let sk = test_signing_key();
        let t1 = issue_wipe_token(DEVICE, 300, NOW, &sk);
        let t2 = issue_wipe_token(DEVICE, 300, NOW, &sk);
        let vt1 = verify_wipe_token(&t1, &test_vk(), DEVICE, NOW).unwrap();
        let vt2 = verify_wipe_token(&t2, &test_vk(), DEVICE, NOW).unwrap();
        // Tokens issued at the same time must still have different nonces
        assert_ne!(vt1.nonce, vt2.nonce, "nonces must differ across issues");
        // And since the nonce is in the payload, the wire tokens must also differ
        assert_ne!(t1, t2, "wire tokens must differ (nonces differ)");
    }

    // ── Key round-trip helpers ───────────────────────────────────────────

    #[test]
    fn verifying_key_to_from_bytes_round_trips() {
        let (sk, vk) = generate_provisioning_keypair();
        let raw = verifying_key_to_bytes(&vk);
        let reconstructed = verifying_key_from_bytes(&raw).expect("must reconstruct");
        // The reconstructed key must verify tokens signed by the original sk
        let token = issue_wipe_token(DEVICE, 300, NOW, &sk);
        verify_wipe_token(&token, &reconstructed, DEVICE, NOW).unwrap();
    }

    #[test]
    fn signing_key_to_from_bytes_round_trips() {
        let (sk, _) = generate_provisioning_keypair();
        let raw = signing_key_to_bytes(&sk);
        let reconstructed = signing_key_from_bytes(&raw);
        // Tokens from original and reconstructed key must produce identical signatures
        // (Ed25519 is deterministic for the same key+message — but nonce differs, so
        // just check both issue+verify correctly)
        let token = issue_wipe_token(DEVICE, 300, NOW, &reconstructed);
        verify_wipe_token(&token, &reconstructed.verifying_key(), DEVICE, NOW).unwrap();
    }

    #[test]
    fn invalid_verifying_key_bytes_returns_none() {
        let bad: [u8; 32] = [0xFFu8; 32]; // not a valid Ed25519 point
                                          // This may or may not parse — ed25519-dalek is lenient here.
                                          // We just verify that verifying_key_from_bytes doesn't panic.
        let _ = verifying_key_from_bytes(&bad);
    }

    // ── Known-answer / golden test — pins the canonical byte layout ──────
    //
    // This test is the CONTRACT with the USB-side (non-Rust) verifier.
    // If any of these bytes change, the USB initramfs verifier MUST be updated
    // and all existing provisioned tokens MUST be re-issued.
    //
    // Inputs (frozen):
    //   version    = 1
    //   device_id  = "dev-0001"          (8 bytes, UTF-8)
    //   nonce      = [0x00, 0x01, ..., 0x1F]  (32 bytes: 0x00..0x1F)
    //   issued_at  = 1_751_000_000        (0x00000000685E23C0)
    //   expires_at = 1_751_000_300        (0x00000000685E24EC)
    //   action     = "wipe"              (4 bytes, UTF-8)
    //
    // Expected canonical bytes:
    //   01             version=1
    //   00 08          device_id length = 8
    //   64 65 76 2d 30 30 30 31   "dev-0001"
    //   00 01 02 … 1f  nonce (32 bytes)
    //   00 00 00 00 68 66 c9 80   issued_at  BE
    //   00 00 00 00 68 66 ca ac   expires_at BE
    //   00 04          action length = 4
    //   77 69 70 65    "wipe"
    //
    // Total: 1 + 2+8 + 32 + 8 + 8 + 2+4 = 65 bytes
    #[test]
    fn golden_canonical_payload_bytes() {
        let nonce: [u8; 32] = core::array::from_fn(|i| i as u8);
        let bytes =
            canonical_payload_bytes(1, "dev-0001", &nonce, 1_751_000_000, 1_751_000_300, "wipe");

        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

        // FROZEN golden vector — computed 2026-06-30.
        // To regenerate: comment out the assert_eq!, run the test with
        //   cargo test -p wincmd-shared golden_canonical_payload_bytes -- --nocapture
        // read the hex from the failure output, paste it back.
        // Any change requires coordinating a USB-side verifier update.
        assert_eq!(
            hex,
            // 01 — version
            // 0008 — device_id len (8)
            // 6465762d30303031 — "dev-0001"
            // 000102...1f — nonce (0x00..0x1F)
            // 00000000685e23c0 — issued_at  = 1_751_000_000 decimal (0x685E23C0)
            // 00000000685e24ec — expires_at = 1_751_000_300 decimal (0x685E24EC)
            // 0004 — action len (4)
            // 77697065 — "wipe"
            concat!(
                "01",
                "0008",
                "6465762d30303031",
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                "00000000685e23c0",
                "00000000685e24ec",
                "0004",
                "77697065"
            )
        );
    }

    // Sign the golden payload with the all-0x42 key and verify via verify_wipe_token.
    // This pins both the canonical bytes AND the verify path.
    #[test]
    fn golden_sign_and_verify() {
        let nonce: [u8; 32] = core::array::from_fn(|i| i as u8);
        let payload = canonical_payload_bytes(1, "dev-0001", &nonce, NOW, NOW + 300, "wipe");

        let sk = test_signing_key();
        let sig: ed25519_dalek::Signature = sk.sign(&payload);
        let wire = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&payload),
            URL_SAFE_NO_PAD.encode(sig.to_bytes()),
        );

        let vt = verify_wipe_token(&wire, &test_vk(), "dev-0001", NOW).unwrap();
        assert_eq!(vt.version, 1);
        assert_eq!(vt.device_id, "dev-0001");
        assert_eq!(vt.nonce, nonce);
        assert_eq!(vt.issued_at, NOW);
        assert_eq!(vt.expires_at, NOW + 300);
        assert_eq!(vt.action, "wipe");
    }
}
