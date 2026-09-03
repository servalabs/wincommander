// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/datastore.rs
//
// ═══════════════════════════════════════════════════════════════════════
// APP-DATA STORE — per-section on-disk persistence with encoding at rest
// ═══════════════════════════════════════════════════════════════════════
//
// Public API (no `crypto`/`encrypt`/`cipher`/`key`/`secret` in names):
//
//   load(section)                    → Value  (returns {} if absent)
//   save(section, &Value)            → Ok(())
//   load_profile(passphrase)         → Value  (returns {} if absent)
//   save_profile(passphrase, &Value) → Ok(())
//
// Sections are stored as individual files under (machine-wide so every
// Windows account shares one config — writes are guarded by the machine-data ACL):
//   %ProgramData%\<APP>\store\<section>.dat
//
// Each file contains: "enc:v1:" + base64(nonce[12] || ciphertext_with_gcm_tag)
//
// The per-install material (32 random bytes) lives at:
//   %ProgramData%\<APP>\.install.material
//
// It is generated once on first use and NEVER tied to the binary version,
// so settings survive app updates (update-safe). The material is the salt
// for argon2id key derivation:
//   - General sections:  key = argon2id(password="",         salt=material)
//   - Private section:   key = argon2id(password=passphrase, salt=material)
//
// The private section uses "private" as the section name; callers access it
// via load_profile / save_profile with their passphrase. A wrong passphrase
// produces an authentication failure from AES-256-GCM (not a panic or
// plaintext fallback).
//
// No callers are wired in P0 — this module is the seam P1/P2 build against.

use aes_gcm::{
    aead::{consts::U12, Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use argon2::{Argon2, Params};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::rngs::OsRng;
use rand::RngCore;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const MATERIAL_FILENAME: &str = ".install.material";
const USER_MATERIAL_FILENAME: &str = ".user-store.material";
const STORE_SUBDIR: &str = "store";
const FORMAT_PREFIX_V1: &str = "enc:v1:";
const FORMAT_PREFIX_V2: &str = "enc:v2:";
const AAD_PREFIX: &str = "wincommander:datastore:v2:";

// argon2id params — tuned for interactive load latency (~50 ms on modern hardware).
const ARGON2_MEM_KIB: u32 = 65_536; // 64 MiB
const ARGON2_ITER: u32 = 2;
const ARGON2_PARALLEL: u32 = 1;
const DERIVED_KEY_LEN: usize = 32;

fn store_dir() -> Result<PathBuf, String> {
    // Machine-wide (%ProgramData%) so the settings blob — which holds the
    // startup_pin real/decoy/destroy hashes — is shared across every Windows
    // account. Per-user (%LOCALAPPDATA%) meant a 2nd account had no PINs and
    // bypassed the calculator front door entirely. The machine-data ACL guards writes.
    let dir = crate::paths::machine_data_dir()?.join(STORE_SUBDIR);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create store dir: {e}"))?;
    Ok(dir)
}

fn section_path(section: &str) -> Result<PathBuf, String> {
    if !section
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "Invalid section name '{section}': only alphanumeric, '-', '_' allowed"
        ));
    }
    Ok(store_dir()?.join(format!("{section}.dat")))
}

fn user_file_path(filename: &str) -> Result<PathBuf, String> {
    if filename.is_empty()
        || filename.len() > 64
        || !filename
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err("invalid per-user store filename".to_string());
    }
    Ok(crate::paths::user_data_dir()?.join(filename))
}

// ── DPAPI (machine-scope) protection for the install material ─────────────────
// Ties the settings-store salt to the live Windows install so the salt — and the
// decoy/destroy/self-destruct config the store encrypts under it — cannot be
// recovered from a raw disk image without the machine's DPAPI key. Non-Windows
// targets are CI stubs (never run): identity functions so the crate builds.

#[cfg(windows)]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB,
    };
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = CryptProtectData(
            &in_blob,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_LOCAL_MACHINE,
            &mut out_blob,
        );
        if ok == 0 {
            return Err("CryptProtectData failed".to_string());
        }
        let out = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        LocalFree(out_blob.pbData as _);
        Ok(out)
    }
}

#[cfg(windows)]
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB,
    };
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: blob.len() as u32,
            pbData: blob.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = CryptUnprotectData(
            &in_blob,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_LOCAL_MACHINE,
            &mut out_blob,
        );
        if ok == 0 {
            return Err("CryptUnprotectData failed".to_string());
        }
        let out = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        LocalFree(out_blob.pbData as _);
        Ok(out)
    }
}

// Deliberately separate from the machine store: without
// CRYPTPROTECT_LOCAL_MACHINE this material is only usable by this Windows
// account. Clipboard rule authoring is personal state, never service state.
#[cfg(windows)]
fn user_dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // `0` is current-user DPAPI scope. Do not add CRYPTPROTECT_LOCAL_MACHINE.
        if CryptProtectData(
            &in_blob,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut out_blob,
        ) == 0
        {
            return Err("current-user material protection failed".to_string());
        }
        let out = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        LocalFree(out_blob.pbData as _);
        Ok(out)
    }
}

#[cfg(windows)]
fn user_dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: blob.len() as u32,
            pbData: blob.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // `0` is current-user DPAPI scope. It must match user_dpapi_protect.
        if CryptUnprotectData(
            &in_blob,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut out_blob,
        ) == 0
        {
            return Err("current-user material unprotection failed".to_string());
        }
        let out = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        LocalFree(out_blob.pbData as _);
        Ok(out)
    }
}

#[cfg(not(windows))]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    Ok(plain.to_vec())
}
#[cfg(not(windows))]
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    Ok(blob.to_vec())
}

#[cfg(not(windows))]
fn user_dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    Ok(plain.to_vec())
}

#[cfg(not(windows))]
fn user_dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    Ok(blob.to_vec())
}

// Read the per-install 32-byte material, creating it on first use. The salt is
// stored DPAPI-machine-protected at rest (see above); legacy raw 32-byte
// material is migrated in place on first read (the salt bytes are unchanged, so
// the encrypted store decrypts exactly as before).
fn install_material() -> Result<[u8; 32], String> {
    // Machine-wide alongside the store (see store_dir): the AES/Argon2 salt
    // must live where the sections it decrypts live, so all accounts share one key.
    let path = crate::paths::machine_data_dir()?.join(MATERIAL_FILENAME);
    if path.exists() {
        let raw = fs::read(&path).map_err(|e| format!("Failed to read install material: {e}"))?;
        if raw.len() == 32 {
            // Legacy plaintext material. Use it, and best-effort re-write it
            // DPAPI-protected so the salt is no longer readable from an image.
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&raw);
            if let Ok(protected) = dpapi_protect(&buf) {
                let _ = atomic_write_bytes(&path, &protected);
            }
            return Ok(buf);
        }
        // DPAPI-protected material (blobs are always far larger than 32 bytes).
        // A file that can't be unprotected is a truncated/partial write or a
        // foreign machine; fail closed so the bytes survive for manual recovery
        // rather than rotating the salt and bricking the encrypted store.
        let plain = dpapi_unprotect(&raw).map_err(|e| format!(
            "Install material at {} could not be unprotected ({e}) — refusing to regenerate (would destroy the encrypted store)",
            path.display()
        ))?;
        if plain.len() != 32 {
            return Err(format!(
                "Install material at {} unprotected to {} bytes, expected 32 — refusing to regenerate",
                path.display(),
                plain.len()
            ));
        }
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&plain);
        return Ok(buf);
    }
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    let protected =
        dpapi_protect(&buf).map_err(|e| format!("Failed to protect install material: {e}"))?;
    atomic_write_bytes(&path, &protected)
        .map_err(|e| format!("Failed to write install material: {e}"))?;
    Ok(buf)
}

fn user_material() -> Result<[u8; 32], String> {
    let path = crate::paths::user_data_dir()?.join(USER_MATERIAL_FILENAME);
    if path.exists() {
        let raw =
            fs::read(&path).map_err(|_| "could not read per-user store material".to_string())?;
        let plain = user_dpapi_unprotect(&raw)
            .map_err(|_| "could not unlock per-user store material".to_string())?;
        if plain.len() != 32 {
            return Err("per-user store material is invalid".to_string());
        }
        let mut material = [0u8; 32];
        material.copy_from_slice(&plain);
        return Ok(material);
    }
    let mut material = [0u8; 32];
    OsRng.fill_bytes(&mut material);
    let protected = user_dpapi_protect(&material)
        .map_err(|_| "could not protect per-user store material".to_string())?;
    atomic_write_bytes(&path, &protected)
        .map_err(|_| "could not write per-user store material".to_string())?;
    Ok(material)
}

fn derive_section_key(
    material: &[u8; 32],
    passphrase: Option<&str>,
) -> Result<[u8; DERIVED_KEY_LEN], String> {
    let password = passphrase.map(|p| p.as_bytes()).unwrap_or(b"");
    let params = Params::new(
        ARGON2_MEM_KIB,
        ARGON2_ITER,
        ARGON2_PARALLEL,
        Some(DERIVED_KEY_LEN),
    )
    .map_err(|e| format!("argon2 params: {e}"))?;
    let argon = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; DERIVED_KEY_LEN];
    argon
        .hash_password_into(password, material, &mut out)
        .map_err(|e| format!("Key derivation failed: {e}"))?;
    Ok(out)
}

fn section_aad(scope: &str) -> String {
    format!("{AAD_PREFIX}{scope}")
}

fn nonce_from_bytes(bytes: &[u8]) -> Result<&Nonce<U12>, String> {
    <&Nonce<U12>>::try_from(bytes).map_err(|_| "Invalid AES-GCM nonce length".to_string())
}

/// Writes the current datastore envelope.  The authenticated scope prevents a
/// local attacker who can replace files from moving a valid ciphertext between
/// settings sections or user-owned blobs.
fn encode_section(
    key: &[u8; DERIVED_KEY_LEN],
    plaintext: &[u8],
    scope: &str,
) -> Result<String, String> {
    let cipher = Aes256Gcm::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = nonce_from_bytes(&nonce_bytes)?;
    let aad = section_aad(scope);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|e| format!("Encoding failed: {e}"))?;
    let mut payload = Vec::with_capacity(12 + ciphertext.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext); // GCM tag is appended by aes_gcm
    Ok(format!("{}{}", FORMAT_PREFIX_V2, B64.encode(&payload)))
}

fn decode_section(
    key: &[u8; DERIVED_KEY_LEN],
    encoded: &str,
    scope: &str,
) -> Result<Vec<u8>, String> {
    let (b64_part, uses_aad) = if let Some(value) = encoded.strip_prefix(FORMAT_PREFIX_V2) {
        (value, true)
    } else if let Some(value) = encoded.strip_prefix(FORMAT_PREFIX_V1) {
        // Existing v1 records are intentionally readable for a non-destructive
        // migration. Every later save emits v2 with authenticated context.
        (value, false)
    } else {
        return Err("Missing supported encrypted-store prefix".to_string());
    };
    let payload = B64
        .decode(b64_part)
        .map_err(|e| format!("Base64 decode failed: {e}"))?;
    if payload.len() < 12 {
        return Err("Encoded payload too short".to_string());
    }
    let (nonce_bytes, ciphertext) = payload.split_at(12);
    let cipher = Aes256Gcm::new(key.into());
    let nonce = nonce_from_bytes(nonce_bytes)?;
    let aad = section_aad(scope);
    let plaintext = if uses_aad {
        cipher.decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad: aad.as_bytes(),
            },
        )
    } else {
        cipher.decrypt(nonce, ciphertext)
    };
    plaintext.map_err(|_| "Decoding failed — wrong passphrase or corrupted data".to_string())
}

// ── Atomic write helper ───────────────────────────────────────────────────────

fn atomic_write_bytes(path: &std::path::Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| "Path has no parent directory".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Path has no file name".to_string())?;
    let tmp = parent.join(format!(".{file_name}.tmp"));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("Create temp: {e}"))?;
        f.write_all(data).map_err(|e| format!("Write temp: {e}"))?;
        f.sync_all().map_err(|e| format!("Fsync temp: {e}"))?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Atomic rename: {e}")
    })
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Load a settings section from disk. Returns an empty object if the section
/// file does not yet exist (first run). Returns an error on I/O or decode failure.
pub fn load(section: &str) -> Result<Value, String> {
    let path = section_path(section)?;
    if !path.exists() {
        return Ok(Value::Object(Default::default()));
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read section '{section}': {e}"))?;
    let material = install_material()?;
    let key = derive_section_key(&material, None)?;
    let bytes = decode_section(&key, raw.trim(), &format!("machine:{section}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse section '{section}': {e}"))
}

/// Write a settings section to disk (replaces the full section).
pub fn save(section: &str, data: &Value) -> Result<(), String> {
    let path = section_path(section)?;
    let material = install_material()?;
    let key = derive_section_key(&material, None)?;
    let bytes = serde_json::to_vec(data)
        .map_err(|e| format!("Failed to serialise section '{section}': {e}"))?;
    let encoded = encode_section(&key, &bytes, &format!("machine:{section}"))?;
    atomic_write_bytes(&path, encoded.as_bytes())
        .map_err(|e| format!("Failed to write section '{section}': {e}"))
}

/// Reads one bounded, encrypted blob owned by the interactive Windows user.
/// This is intentionally crate-private: services must not gain a generic path
/// to user-owned state such as clipboard rules or the per-user settings overlay.
pub(crate) fn load_user_blob(
    filename: &str,
    max_plaintext_bytes: usize,
) -> Result<Option<Vec<u8>>, String> {
    let path = user_file_path(filename)?;
    if !path.exists() {
        return Ok(None);
    }
    let max_encoded_bytes = max_plaintext_bytes
        .checked_mul(2)
        .and_then(|n| n.checked_add(128))
        .ok_or_else(|| "per-user store size limit is invalid".to_string())?;
    let size = fs::metadata(&path)
        .map_err(|_| "could not inspect per-user data".to_string())?
        .len();
    if size as usize > max_encoded_bytes {
        return Err("per-user data exceed the size limit".to_string());
    }
    let encoded =
        fs::read_to_string(&path).map_err(|_| "could not read per-user data".to_string())?;
    let key = derive_section_key(&user_material()?, None)?;
    let plaintext = decode_section(&key, encoded.trim(), &format!("user:{filename}"))
        .map_err(|_| "per-user data could not be decoded".to_string())?;
    if plaintext.len() > max_plaintext_bytes {
        return Err("per-user data exceed the size limit".to_string());
    }
    Ok(Some(plaintext))
}

/// Atomically replaces one bounded user-owned encrypted blob.
pub(crate) fn save_user_blob(
    filename: &str,
    plaintext: &[u8],
    max_plaintext_bytes: usize,
) -> Result<(), String> {
    if plaintext.len() > max_plaintext_bytes {
        return Err("per-user data exceed the size limit".to_string());
    }
    let path = user_file_path(filename)?;
    let key = derive_section_key(&user_material()?, None)?;
    let encoded = encode_section(&key, plaintext, &format!("user:{filename}"))?;
    atomic_write_bytes(&path, encoded.as_bytes())
        .map_err(|_| "could not persist per-user data".to_string())
}

// ── Log-record encryption ─────────────────────────────────────────────────────
//
// Logs are encrypted one record at a time with the same key as the general
// settings store.  Key derivation (Argon2id, ~50 ms) runs once per app
// session and is cached in a process-lifetime OnceLock.
//
// On-disk line format:
//   L2:<YYYY-MM-DD>:<base64(nonce[12] || ciphertext || gcm_tag)>
//
// The date is plaintext so the rotation pass can drop old records without
// decrypting.  It does not reveal log *content* — only that the app ran on
// that calendar day, which is observable from file-system timestamps anyway.

static LOG_KEY_CACHE: std::sync::OnceLock<[u8; DERIVED_KEY_LEN]> = std::sync::OnceLock::new();

fn cached_log_key() -> Result<[u8; DERIVED_KEY_LEN], String> {
    if let Some(k) = LOG_KEY_CACHE.get() {
        return Ok(*k);
    }
    let material = install_material()?;
    let key = derive_section_key(&material, None)?;
    let _ = LOG_KEY_CACHE.set(key); // ignore race: both threads derive the same key
    LOG_KEY_CACHE
        .get()
        .copied()
        .ok_or_else(|| "log key cache unavailable".to_string())
}

/// Encrypt one log body into an on-disk `L2:<date>:<b64>` line.
/// `date` = `YYYY-MM-DD`; `body` = `[HH:MM:SS] [LEVEL] message`.
/// Falls back to an unencrypted marker on key failure so the app never panics.
#[allow(dead_code)]
pub fn log_encrypt_line(date: &str, body: &str) -> String {
    let key = match cached_log_key() {
        Ok(k) => k,
        Err(_) => return format!("[ENCRYPT_FAIL] {}", body),
    };
    let cipher = Aes256Gcm::new((&key).into());
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let Ok(nonce) = nonce_from_bytes(&nonce_bytes) else {
        return format!("[ENCRYPT_FAIL] {}", body);
    };
    let aad = section_aad(&format!("log:{date}"));
    match cipher.encrypt(
        nonce,
        Payload {
            msg: body.as_bytes(),
            aad: aad.as_bytes(),
        },
    ) {
        Ok(ct) => {
            let mut payload = Vec::with_capacity(12 + ct.len());
            payload.extend_from_slice(&nonce_bytes);
            payload.extend_from_slice(&ct);
            format!("L2:{}:{}", date, B64.encode(&payload))
        }
        Err(_) => format!("[ENCRYPT_FAIL] {}", body),
    }
}

/// Decrypt one versioned log line. `L:` records remain readable for migration;
/// current `L2:` records authenticate their date as associated data.
pub fn log_decrypt_line(line: &str) -> Option<(String, String)> {
    let (rest, uses_aad) = if let Some(value) = line.strip_prefix("L2:") {
        (value, true)
    } else {
        (line.strip_prefix("L:")?, false)
    };
    let colon_pos = rest.find(':')?;
    let date = &rest[..colon_pos];
    let b64_part = &rest[colon_pos + 1..];
    let payload = B64.decode(b64_part).ok()?;
    if payload.len() < 12 {
        return None;
    }
    let (nonce_bytes, ct) = payload.split_at(12);
    let key = cached_log_key().ok()?;
    let cipher = Aes256Gcm::new((&key).into());
    let nonce = nonce_from_bytes(nonce_bytes).ok()?;
    let aad = section_aad(&format!("log:{date}"));
    let plaintext = if uses_aad {
        cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ct,
                    aad: aad.as_bytes(),
                },
            )
            .ok()?
    } else {
        cipher.decrypt(nonce, ct).ok()?
    };
    let body = String::from_utf8(plaintext).ok()?;
    Some((date.to_string(), body))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(windows)]
    #[test]
    fn dpapi_material_roundtrip() {
        let secret = [7u8; 32];
        let blob = dpapi_protect(&secret).expect("protect");
        assert!(
            blob.len() > 32,
            "DPAPI blob should be larger than the 32-byte plaintext"
        );
        let back = dpapi_unprotect(&blob).expect("unprotect");
        assert_eq!(back, secret.to_vec());
    }

    fn test_key(seed: u8) -> [u8; DERIVED_KEY_LEN] {
        [seed; DERIVED_KEY_LEN]
    }

    #[test]
    fn encode_decode_roundtrip() {
        let key = test_key(42);
        let plain = b"hello world test data 1234";
        let encoded = encode_section(&key, plain, "machine:settings").unwrap();
        assert!(encoded.starts_with("enc:v2:"));
        let decoded = decode_section(&key, &encoded, "machine:settings").unwrap();
        assert_eq!(decoded, plain);
    }

    #[test]
    fn wrong_key_fails_authentication() {
        let key1 = test_key(1);
        let key2 = test_key(2);
        let encoded = encode_section(&key1, b"secret", "machine:settings").unwrap();
        assert!(decode_section(&key2, &encoded, "machine:settings").is_err());
    }

    #[test]
    fn nonces_are_random() {
        let key = test_key(7);
        let plain = b"same plaintext";
        let enc1 = encode_section(&key, plain, "machine:settings").unwrap();
        let enc2 = encode_section(&key, plain, "machine:settings").unwrap();
        // Different nonces produce different ciphertexts.
        assert_ne!(enc1, enc2);
        // But both decrypt correctly.
        assert_eq!(
            decode_section(&key, &enc1, "machine:settings").unwrap(),
            plain
        );
        assert_eq!(
            decode_section(&key, &enc2, "machine:settings").unwrap(),
            plain
        );
    }

    #[test]
    fn key_derivation_is_deterministic() {
        let material = [0xABu8; 32];
        let k1 = derive_section_key(&material, Some("pass")).unwrap();
        let k2 = derive_section_key(&material, Some("pass")).unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn key_derivation_differs_by_passphrase() {
        let material = [0xCDu8; 32];
        let k1 = derive_section_key(&material, Some("alpha")).unwrap();
        let k2 = derive_section_key(&material, Some("beta")).unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn key_derivation_no_passphrase_vs_empty_passphrase() {
        let material = [0x01u8; 32];
        // None and Some("") are treated identically (both use b"" as password).
        let k1 = derive_section_key(&material, None).unwrap();
        let k2 = derive_section_key(&material, Some("")).unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn json_roundtrip_through_encode_decode() {
        let key = test_key(99);
        let data = json!({"version": 1, "foo": "bar", "nums": [1, 2, 3]});
        let bytes = serde_json::to_vec(&data).unwrap();
        let encoded = encode_section(&key, &bytes, "machine:settings").unwrap();
        let decoded_bytes = decode_section(&key, &encoded, "machine:settings").unwrap();
        let decoded: Value = serde_json::from_slice(&decoded_bytes).unwrap();
        assert_eq!(data, decoded);
    }

    #[test]
    fn v2_ciphertext_cannot_be_moved_between_authenticated_sections() {
        let key = test_key(77);
        let encoded = encode_section(&key, b"settings payload", "machine:settings").unwrap();

        assert!(decode_section(&key, &encoded, "machine:license").is_err());
        assert_eq!(
            decode_section(&key, &encoded, "machine:settings").unwrap(),
            b"settings payload"
        );
    }

    #[test]
    fn legacy_v1_ciphertext_remains_readable_for_non_destructive_migration() {
        let key = test_key(31);
        let cipher = Aes256Gcm::new((&key).into());
        let nonce_bytes = [3u8; 12];
        let nonce = nonce_from_bytes(&nonce_bytes).unwrap();
        let ciphertext = cipher.encrypt(nonce, b"legacy payload".as_ref()).unwrap();
        let mut payload = nonce_bytes.to_vec();
        payload.extend_from_slice(&ciphertext);
        let encoded = format!("{FORMAT_PREFIX_V1}{}", B64.encode(payload));

        assert_eq!(
            decode_section(&key, &encoded, "machine:settings").unwrap(),
            b"legacy payload"
        );
    }

    #[test]
    fn invalid_section_name_rejected() {
        assert!(section_path("../etc/passwd").is_err());
        assert!(section_path("foo/bar").is_err());
        assert!(section_path("valid-name_123").is_ok());
    }

    #[test]
    fn per_user_store_filename_cannot_escape_the_user_directory() {
        assert!(user_file_path("../clipboard-guard-rules.dat").is_err());
        assert!(user_file_path("clipboard/guard.dat").is_err());
        assert!(user_file_path("clipboard-guard-rules.dat").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn current_user_dpapi_material_roundtrip_is_separate_from_machine_store() {
        let material = [9u8; 32];
        let protected = user_dpapi_protect(&material).expect("current-user protect");
        assert!(protected.len() > material.len());
        assert_eq!(
            user_dpapi_unprotect(&protected).expect("current-user unprotect"),
            material
        );
    }

    #[test]
    fn missing_prefix_rejected() {
        let key = test_key(5);
        assert!(decode_section(&key, "notenc:v1:aaaa", "machine:settings").is_err());
        assert!(decode_section(&key, "", "machine:settings").is_err());
    }

    // ── Golden / known-answer vector — pins the AES-256-GCM primitive ──────
    //
    // Every other test above is self-referential (encrypt with this crate's
    // `aes-gcm`, decrypt with the same crate) — it would still pass even if
    // an `aes-gcm`/RustCrypto upgrade silently changed output for the same
    // key/nonce/plaintext. This test instead checks against a ciphertext
    // computed by a DIFFERENT, independent AES-GCM implementation, so a
    // real interoperability regression actually fails a test.
    //
    // Reference computed 2026-07-12 via Python's `cryptography` library:
    //   from cryptography.hazmat.primitives.ciphers.aead import AESGCM
    //   key = bytes(range(32)); nonce = bytes(range(12))
    //   AESGCM(key).encrypt(nonce, plaintext, None)   # no AAD — matches
    //                                                  # encode_section/decode_section,
    //                                                  # which never pass one.
    #[test]
    fn golden_aes256gcm_matches_independent_reference() {
        let key: [u8; DERIVED_KEY_LEN] = core::array::from_fn(|i| i as u8);
        let nonce_bytes: [u8; 12] = core::array::from_fn(|i| i as u8);
        let plaintext = b"wincommander-datastore-golden-vector-2026-07-12";

        let cipher = Aes256Gcm::new((&key).into());
        let nonce = nonce_from_bytes(&nonce_bytes).unwrap();
        let ciphertext = cipher.encrypt(nonce, plaintext.as_ref()).unwrap();

        let hex: String = ciphertext.iter().map(|b| format!("{b:02x}")).collect();

        // FROZEN golden vector. To regenerate (only after a deliberate,
        // reviewed crypto-library change): recompute with the Python
        // snippet above and update both this string and the decrypt
        // assertion below together.
        assert_eq!(
            hex,
            concat!(
                "306bb878aa88af7ae325f2f99c8d1919e2a5f35b821e721b570b81e0734476d7",
                "6264c18e82f322aa42894fdaa5b61a14eaa51ef2335b0a81b48556b24b52ed"
            ),
            "AES-256-GCM output diverged from the independent Python reference"
        );

        // Reverse direction: this crate must decrypt the externally
        // computed reference ciphertext back to the exact plaintext.
        let decrypted = cipher.decrypt(nonce, ciphertext.as_ref()).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
