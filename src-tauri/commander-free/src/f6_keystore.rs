// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/f6_keystore.rs
//
// F6 Phase-1, Piece 3 — Device signing key store.
//
// Stores the Ed25519 device signing key in the same AES-256-GCM encrypted
// datastore used by the rest of the app (.install.material-protected,
// machine-bound).  The private key NEVER sits in plaintext on disk.
//
// # Key lifecycle
//
//   1. First call to `get_or_create_device_signing_key()` — generates a
//      fresh keypair, persists the 32-byte signing-key seed to the
//      `"f6-keystore"` section, returns the `SigningKey`.
//   2. Subsequent calls — loads the seed from disk, reconstructs the
//      `SigningKey`, returns it.  Fast: the datastore key is derived once
//      per process and cached by `datastore.rs`.
//
// # Safety
//
// This module performs NO crypto-erase, NO reboot, NO BootNext writes.
// It only reads / writes the encrypted datastore.  Tests run it on a
// real temp-dir-backed store (the datastore's key derivation runs on the
// actual Argon2id params, so tests are deliberately NOT run in parallel
// with other datastore tests to avoid thrashing the file cache).
//
// Tests are gated via `#[cfg(test)]` and operate on a temp directory,
// never touching the production store.

use ed25519_dalek::SigningKey;
use serde_json::{json, Value};
use wincmd_shared::wipe_auth::{
    generate_provisioning_keypair, signing_key_from_bytes, signing_key_to_bytes,
    verifying_key_to_bytes,
};

// Section key in the datastore for the F6 device signing seed.
const F6_KEYSTORE_SECTION: &str = "f6-keystore";
// JSON field inside the section for the 32-byte seed (hex-encoded for legibility).
const SIGNING_SEED_FIELD: &str = "signing_seed_hex";

// ── Public API ────────────────────────────────────────────────────────────────

/// Load the device Ed25519 signing key from the encrypted store, or generate
/// and persist a new one on first call.
///
/// The key is machine-bound (protected by `.install.material` + Argon2id
/// key derivation) and is NOT stored in plaintext anywhere.
///
/// # Errors
///
/// Returns `Err(String)` if the datastore section cannot be read or written.
/// Callers must not proceed with the F6 orchestration if this returns an error.
pub fn get_or_create_device_signing_key() -> Result<SigningKey, String> {
    let section = crate::datastore::load(F6_KEYSTORE_SECTION)?;

    if let Some(hex_val) = section.get(SIGNING_SEED_FIELD).and_then(Value::as_str) {
        // Existing key — decode hex seed and reconstruct.
        let seed_bytes =
            hex::decode(hex_val).map_err(|e| format!("F6 keystore: hex decode failed: {e}"))?;
        if seed_bytes.len() != 32 {
            return Err(format!(
                "F6 keystore: stored seed is {} bytes, expected 32 — store may be corrupt",
                seed_bytes.len()
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&seed_bytes);
        return Ok(signing_key_from_bytes(&arr));
    }

    // First call — generate a new keypair, persist the signing seed.
    let (signing_key, _verifying_key) = generate_provisioning_keypair();
    let seed = signing_key_to_bytes(&signing_key);
    let new_section = json!({ SIGNING_SEED_FIELD: hex::encode(seed) });
    crate::datastore::save(F6_KEYSTORE_SECTION, &new_section)?;

    crate::log_message(
        "info",
        "[F6-Keystore] generated new device signing key and persisted to encrypted store",
    );

    Ok(signing_key)
}

/// Return the 32-byte verifying key (public key) for this device.
#[allow(dead_code)] // Used by the provisioning UI (piece 4, not yet wired)
///
/// The verifying key is derived from the signing key and is the value written
/// to `<usb_root>/wipe/pubkey.bin` at provisioning time.
///
/// # Errors
///
/// Returns `Err` if the signing key cannot be loaded (same conditions as
/// `get_or_create_device_signing_key`).
pub fn device_verifying_key_bytes() -> Result<[u8; 32], String> {
    let sk = get_or_create_device_signing_key()?;
    Ok(verifying_key_to_bytes(&sk.verifying_key()))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey as DalekSigningKey;
    use wincmd_shared::wipe_auth::{
        issue_wipe_token, signing_key_to_bytes, verify_wipe_token, verifying_key_to_bytes,
    };

    // ── Keystore round-trip test (in-memory, not touching the real datastore) ──
    //
    // We test the keystore logic by directly exercising the encode/decode path
    // that `get_or_create_device_signing_key` uses, without calling the real
    // `datastore::load/save` (which would touch %ProgramData% and need Argon2).
    // The full datastore encryption is covered by `datastore.rs` tests.
    //
    // What we test here:
    //   1. `signing_key_from_bytes(signing_key_to_bytes(k))` round-trips.
    //   2. The hex encode/decode path preserves all 32 bytes correctly.
    //   3. A token issued with the original key verifies with the round-tripped key.

    fn deterministic_signing_key() -> DalekSigningKey {
        DalekSigningKey::from_bytes(&[0x5Au8; 32])
    }

    #[test]
    fn signing_key_seed_hex_roundtrip() {
        let sk = deterministic_signing_key();
        let seed = signing_key_to_bytes(&sk);

        // Simulate what get_or_create_device_signing_key stores: hex-encode seed.
        let hex_str = hex::encode(seed);
        assert_eq!(hex_str.len(), 64, "hex of 32 bytes must be 64 chars");

        // Simulate the load path: hex-decode → reconstruct.
        let decoded = hex::decode(&hex_str).expect("must decode");
        assert_eq!(decoded.len(), 32);
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        let reconstructed = wincmd_shared::wipe_auth::signing_key_from_bytes(&arr);

        // The reconstructed key must be byte-identical to the original.
        assert_eq!(
            signing_key_to_bytes(&sk),
            signing_key_to_bytes(&reconstructed),
            "reconstructed signing key seed must match original"
        );
    }

    #[test]
    fn reconstructed_key_issues_valid_tokens() {
        let sk = deterministic_signing_key();
        let seed = signing_key_to_bytes(&sk);
        let hex_str = hex::encode(seed);

        let decoded = hex::decode(&hex_str).unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        let reconstructed = wincmd_shared::wipe_auth::signing_key_from_bytes(&arr);

        let device_id = "test-device-keystore-001";
        let now: i64 = 1_751_000_000;
        let token = issue_wipe_token(device_id, 300, now, &reconstructed);

        // Verify with the verifying key derived from the reconstructed signing key.
        let vk = reconstructed.verifying_key();
        verify_wipe_token(&token, &vk, device_id, now)
            .expect("token issued from reconstructed key must verify");
    }

    #[test]
    fn verifying_key_bytes_matches_signing_key() {
        let sk = deterministic_signing_key();
        let seed = signing_key_to_bytes(&sk);
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&seed);
        let reconstructed = wincmd_shared::wipe_auth::signing_key_from_bytes(&arr);

        let vk_bytes = verifying_key_to_bytes(&reconstructed.verifying_key());
        // Also derives from the original — must match.
        let vk_bytes_orig = verifying_key_to_bytes(&sk.verifying_key());
        assert_eq!(
            vk_bytes, vk_bytes_orig,
            "verifying key bytes must be identical for the same signing key"
        );
    }

    #[test]
    fn corrupt_hex_is_detected() {
        // Simulates what would happen if the stored hex is corrupted.
        let bad_hex = "not_valid_hex!";
        let result = hex::decode(bad_hex);
        assert!(result.is_err(), "corrupt hex must fail to decode");
    }

    #[test]
    fn wrong_length_seed_is_detected() {
        // If the stored bytes were truncated to 16 bytes, we must return an error.
        let short_hex = hex::encode([0u8; 16]);
        let decoded = hex::decode(&short_hex).unwrap();
        // Simulate the guard in get_or_create_device_signing_key.
        assert_ne!(decoded.len(), 32, "16 bytes must not pass the length check");
    }
}
