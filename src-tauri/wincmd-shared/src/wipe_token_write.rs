// SPDX-License-Identifier: AGPL-3.0-or-later
//! F6 Phase-1, Piece 2 — Token + pubkey write helper.
//!
//! Writes the two files the USB wipe environment needs onto the USB control
//! area at provisioning / distress-trigger time:
//!
//! ```text
//! <usb_root>/wipe/token.txt   — signed wipe-auth wire token (UTF-8 text)
//! <usb_root>/wipe/pubkey.bin  — 32 raw bytes of the Ed25519 verifying key
//! ```
//!
//! # What this does
//!
//! * Creates `<usb_root>/wipe/` if it does not exist.
//! * Writes `token.txt` (the wire string from `wipe_auth::issue_wipe_token`).
//! * Writes `pubkey.bin` (the 32 raw bytes from `wipe_auth::verifying_key_to_bytes`).
//!
//! # What this does NOT do
//!
//! * It does NOT call any crypto-erase.
//! * It does NOT set `BootNext`.
//! * It does NOT trigger a reboot.
//! * It does NOT issue the token itself — the caller provides the pre-issued
//!   token string so the signing key never has to cross the module boundary.
//!
//! The write is a plain filesystem operation — it is REVERSIBLE (delete the
//! files to undo), safe (no system-level side effects), and idempotent
//! (writing the same token twice leaves the same file on disk).
//!
//! # File layout rationale
//!
//! The `wipe/` subdirectory groups both files so the USB-side initramfs can
//! mount the control partition and check for `wipe/token.txt` in one stat.
//! The verifying key is written at provisioning time so the initramfs never
//! needs to network-fetch it.

use std::path::{Path, PathBuf};

use ed25519_dalek::VerifyingKey;

use crate::wipe_auth::verifying_key_to_bytes;

/// The control subdirectory inside the USB root.
const WIPE_DIR: &str = "wipe";
/// Token file name.
const TOKEN_FILE: &str = "token.txt";
/// Verifying-key file name.
const PUBKEY_FILE: &str = "pubkey.bin";

/// Write the wipe token and verifying key to the USB control area.
///
/// # Parameters
/// - `usb_root`      — path to the USB volume root (e.g. `D:\` on Windows or
///   `/mnt/wipe-usb` on Linux).
/// - `token`         — wire token string from `wipe_auth::issue_wipe_token`.
/// - `verifying_key` — the device's provisioning Ed25519 verifying key.
///
/// # Returns
///
/// `Ok(WipeUsbPaths)` with the two absolute paths that were written.
/// `Err(String)` if directory creation or either write fails.
///
/// # Safety
///
/// Callers must validate that `usb_root` is a removable/USB volume before
/// calling.  This function does not validate the mount path.
pub fn write_wipe_token_to_usb(
    usb_root: &Path,
    token: &str,
    verifying_key: &VerifyingKey,
) -> Result<WipeUsbPaths, String> {
    let wipe_dir: PathBuf = usb_root.join(WIPE_DIR);

    // Create the control directory (idempotent).
    std::fs::create_dir_all(&wipe_dir)
        .map_err(|e| format!("create wipe dir '{}': {e}", wipe_dir.display()))?;

    // Write token.txt.
    let token_path = wipe_dir.join(TOKEN_FILE);
    std::fs::write(&token_path, token.as_bytes())
        .map_err(|e| format!("write token to '{}': {e}", token_path.display()))?;

    // Write pubkey.bin (32 raw bytes, no encoding).
    let pubkey_bytes: [u8; 32] = verifying_key_to_bytes(verifying_key);
    let pubkey_path = wipe_dir.join(PUBKEY_FILE);
    std::fs::write(&pubkey_path, pubkey_bytes)
        .map_err(|e| format!("write pubkey to '{}': {e}", pubkey_path.display()))?;

    Ok(WipeUsbPaths {
        token_path,
        pubkey_path,
    })
}

/// Absolute paths of the two files written to the USB.
#[derive(Debug, Clone)]
pub struct WipeUsbPaths {
    /// `<usb_root>/wipe/token.txt`
    pub token_path: PathBuf,
    /// `<usb_root>/wipe/pubkey.bin`
    pub pubkey_path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wipe_auth::{
        generate_provisioning_keypair, issue_wipe_token, verify_wipe_token,
        verifying_key_from_bytes,
    };
    use tempfile::TempDir;

    const DEVICE: &str = "test-device-f6-p1p2";
    const NOW: i64 = 1_751_000_000;
    const TTL: i64 = 300;

    /// Helper: generate a keypair, issue a token, write to a temp dir.
    fn setup() -> (TempDir, String, VerifyingKey, WipeUsbPaths) {
        let dir = TempDir::new().expect("tempdir");
        let (sk, vk) = generate_provisioning_keypair();
        let token = issue_wipe_token(DEVICE, TTL, NOW, &sk);
        let paths = write_wipe_token_to_usb(dir.path(), &token, &vk)
            .expect("write_wipe_token_to_usb must succeed");
        (dir, token, vk, paths)
    }

    #[test]
    fn writes_token_and_pubkey() {
        let (dir, token, _vk, paths) = setup();

        // token.txt must exist and contain the wire token exactly.
        let read_token = std::fs::read_to_string(&paths.token_path).expect("read token.txt");
        assert_eq!(read_token, token, "token.txt must match issued wire token");

        // pubkey.bin must be exactly 32 bytes.
        let pubkey_bytes = std::fs::read(&paths.pubkey_path).expect("read pubkey.bin");
        assert_eq!(pubkey_bytes.len(), 32, "pubkey.bin must be 32 bytes");

        // Paths must be inside the temp dir.
        assert!(paths.token_path.starts_with(dir.path()));
        assert!(paths.pubkey_path.starts_with(dir.path()));
    }

    #[test]
    fn creates_wipe_subdirectory() {
        let dir = TempDir::new().expect("tempdir");
        let (sk, vk) = generate_provisioning_keypair();
        let token = issue_wipe_token(DEVICE, TTL, NOW, &sk);

        // The `wipe/` subdir does NOT exist yet.
        let wipe_dir = dir.path().join("wipe");
        assert!(!wipe_dir.exists(), "wipe/ must not exist before write");

        write_wipe_token_to_usb(dir.path(), &token, &vk).expect("write must succeed");

        assert!(wipe_dir.is_dir(), "wipe/ must be created by the write");
    }

    #[test]
    fn pubkey_round_trips_to_verifying_key() {
        let (_dir, _token, vk, paths) = setup();

        // Read raw bytes back from disk.
        let raw: Vec<u8> = std::fs::read(&paths.pubkey_path).expect("read pubkey.bin");
        let raw_arr: [u8; 32] = raw.as_slice().try_into().expect("must be 32 bytes");

        // Reconstruct the verifying key from the raw bytes.
        let reconstructed = verifying_key_from_bytes(&raw_arr)
            .expect("raw bytes from disk must yield a valid VerifyingKey");

        // The reconstructed key must verify the token written to disk.
        let token_on_disk = std::fs::read_to_string(&paths.token_path).expect("read token.txt");
        verify_wipe_token(&token_on_disk, &reconstructed, DEVICE, NOW)
            .expect("token from disk must verify with pubkey from disk");

        // And both keys must match (same public point).
        assert_eq!(vk.to_bytes(), reconstructed.to_bytes());
    }

    #[test]
    fn idempotent_write_overwrites_on_second_call() {
        let dir = TempDir::new().expect("tempdir");
        let (sk, vk) = generate_provisioning_keypair();

        // First write.
        let token1 = issue_wipe_token(DEVICE, TTL, NOW, &sk);
        let paths1 = write_wipe_token_to_usb(dir.path(), &token1, &vk).expect("first write");

        // Second write with a different token (new nonce).
        let token2 = issue_wipe_token(DEVICE, TTL, NOW + 1, &sk);
        write_wipe_token_to_usb(dir.path(), &token2, &vk).expect("second write must not error");

        // The file must contain the SECOND token now.
        let on_disk = std::fs::read_to_string(&paths1.token_path).expect("read");
        assert_eq!(on_disk, token2, "second write must overwrite first");
        assert_ne!(on_disk, token1);
    }

    #[test]
    fn token_path_and_pubkey_path_are_expected_names() {
        let (dir, _token, _vk, paths) = setup();
        let expected_token = dir.path().join("wipe").join("token.txt");
        let expected_pubkey = dir.path().join("wipe").join("pubkey.bin");
        assert_eq!(paths.token_path, expected_token);
        assert_eq!(paths.pubkey_path, expected_pubkey);
    }
}
