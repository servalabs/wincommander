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
// Sections are stored as individual files under the installation's trusted data
// root. A machine install uses %ProgramData% (shared policy/service state); an
// NSIS current-user install uses the caller's %LOCALAPPDATA% so opening the app
// never requires permission to modify ProgramData:
//   %ProgramData%\<APP>\store\<section>.dat       (machine install)
//   %LOCALAPPDATA%\<APP>\store\<section>.dat     (current-user install)
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

#[cfg(any(test, feature = "experimental-settings-generation"))]
#[path = "settings_generation_maintenance.rs"]
mod settings_generation_maintenance;

#[cfg(any(test, feature = "experimental-settings-generation"))]
#[path = "settings_generation_bootstrap.rs"]
mod settings_generation_bootstrap;

#[cfg(any(test, feature = "experimental-settings-generation"))]
#[path = "settings_generation_cutover.rs"]
mod settings_generation_cutover;

#[cfg(all(windows, any(test, feature = "experimental-settings-generation")))]
#[path = "settings_generation_acl_windows.rs"]
mod settings_generation_acl_windows;

#[cfg(any(test, feature = "experimental-settings-generation"))]
#[path = "settings_generation_ownership.rs"]
mod settings_generation_ownership;

#[cfg(any(test, feature = "experimental-settings-generation"))]
#[path = "settings_section_envelope.rs"]
mod settings_section_envelope;

#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(crate) fn settings_generation_lab_status() -> Result<&'static str, String> {
    let (root, _pins, scope) = settings_rotation_trial::experimental_runtime_root()?;
    match settings_rotation_trial::inspect_startup_once(&root, scope)? {
        settings_rotation_trial::StartupView::Legacy => Ok("Legacy settings selected; migration has not run."),
        settings_rotation_trial::StartupView::Selected { .. } => Ok("Protected settings generation selected."),
        settings_rotation_trial::StartupView::RecoveryRequired => Err("Settings generation requires authorized recovery; original files were preserved.".into()),
    }
}
/// Lab desktop startup must never silently return to the legacy store while a
/// selected generation is incomplete or its paired service is stopped.
#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(crate) fn require_selected_settings_lab_startup() -> Result<(), String> {
    if option_env!("WINCMD_SETTINGS_LAB_AUTOROUTE") != Some("1") { return Ok(()); }
    settings_generation_maintenance::wait_for_running_service()?;
    let (root, _pins, scope) = settings_rotation_trial::experimental_runtime_root()?;
    settings_rotation_trial::require_selected_startup(
        settings_rotation_trial::inspect_startup_once(&root, scope)?,
        scope,
        crate::settings::valid_fresh_machine_settings,
    )
}

#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(crate) fn settings_generation_lab_stage_proof() -> Result<(String, bool), String> {
    settings_rotation_trial::stage_machine_with_proof()
}
#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(crate) fn settings_generation_lab_install_proof(proof: &str) -> Result<(), String> {
    settings_rotation_trial::install_machine_with_proof(proof)
}
#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(crate) fn settings_generation_lab_create_new_identity() -> Result<(), String> {
    settings_rotation_trial::create_new_machine_identity_explicitly()
}

#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(crate) fn settings_generation_lab_stage_user(expected_sid: &str) -> Result<(), String> {
    let actual = settings_generation_acl_windows::current_user_sid()
        .map_err(|_| "Cannot verify the Windows user; existing settings preserved.")?;
    if !crate::settings_generation_lab::user_sid_matches(expected_sid, &actual) {
        return Err("The original Windows user must run settings staging; existing settings preserved.".into());
    }
    settings_rotation_trial::stage_existing_explicitly(settings_rotation_trial::StoreScope::CurrentUser)
}

#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(crate) fn hold_search_fence_for_installer(pid: u32) -> Result<(), String> {
    settings_rotation_trial::prepare_installer_search_fence(pid)
}

const MATERIAL_FILENAME: &str = ".install.material";
const USER_MATERIAL_FILENAME: &str = ".user-store.material";
const PERSONAL_SECRETS_KEY_MARKER: &str = "personal-secrets-key.in-use";
const USER_STORE_FILES: [&str; 3] = [
    "user-settings.dat",
    "clipboard-guard-rules.dat",
    PERSONAL_SECRETS_KEY_MARKER,
];
const PERSONAL_SECRETS_SCOPE: &str = "user:personal-settings-secrets";
const MAX_PERSONAL_SECRETS_BYTES: usize = 1_048_576;
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
    let dir = crate::paths::datastore_data_dir()?.join(STORE_SUBDIR);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create store dir: {e}"))?;
    Ok(dir)
}

fn validate_section_name(section: &str) -> Result<(), String> {
    if !section
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "Invalid section name '{section}': only alphanumeric, '-', '_' allowed"
        ));
    }
    Ok(())
}

fn section_path(section: &str) -> Result<PathBuf, String> {
    validate_section_name(section)?;
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
        CryptProtectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
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
            CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN,
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
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    use zeroize::Zeroize;
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
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        );
        if ok == 0 {
            return Err("CryptUnprotectData failed".to_string());
        }
        let len = out_blob.cbData as usize;
        if len != 0 && out_blob.pbData.is_null() {
            return Err("CryptUnprotectData returned invalid output".to_string());
        }
        let out = if len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(out_blob.pbData, len).to_vec()
        };
        if !out_blob.pbData.is_null() {
            // DPAPI owns this allocation until LocalFree; wipe its plaintext
            // separately from the returned copy.
            if len != 0 {
                std::slice::from_raw_parts_mut(out_blob.pbData, len).zeroize();
            }
            LocalFree(out_blob.pbData as _);
        }
        Ok(out)
    }
}

// Deliberately separate from the machine store: without
// CRYPTPROTECT_LOCAL_MACHINE this material is only usable by this Windows
// account. Clipboard rule authoring is personal state, never service state.
#[cfg(windows)]
pub(crate) fn user_dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
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
        // Keep current-user scope, but never allow an unattended launch to show
        // credential/UI prompts (for example from an SSH session).
        if CryptProtectData(
            &in_blob,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
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
pub(crate) fn user_dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    use zeroize::Zeroize;
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
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        ) == 0
        {
            return Err("current-user material unprotection failed".to_string());
        }
        let len = out_blob.cbData as usize;
        if len != 0 && out_blob.pbData.is_null() {
            return Err("current-user material unprotection returned invalid output".to_string());
        }
        let out = if len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(out_blob.pbData, len).to_vec()
        };
        if !out_blob.pbData.is_null() {
            if len != 0 {
                std::slice::from_raw_parts_mut(out_blob.pbData, len).zeroize();
            }
            LocalFree(out_blob.pbData as _);
        }
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
pub(crate) fn user_dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    Ok(plain.to_vec())
}

#[cfg(not(windows))]
pub(crate) fn user_dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    Ok(blob.to_vec())
}

// Read the datastore's 32-byte material, creating it on first use. Machine
// installs use DPAPI-machine protection so their shared store remains readable
// across accounts. A current-user install also uses machine DPAPI for its
// LocalAppData key material: Windows can otherwise reject current-user DPAPI in
// a noninteractive logon (such as SSH). The LocalAppData directory ACL remains
// the per-user access boundary; the DPAPI scope supplies logon compatibility.
fn install_material() -> Result<[u8; 32], String> {
    if crate::paths::current_user_install_uses_local_datastore() {
        return user_material();
    }

    // Machine-wide alongside the store (see store_dir): the AES/Argon2 salt
    // must live where the sections it decrypts live, so all accounts share one key.
    let path = crate::paths::datastore_data_dir()?.join(MATERIAL_FILENAME);
    let raw = match read_material_file(&path)? {
        Some(raw) => raw,
        None => {
            let creation = ensure_material_can_be_created(path.parent().unwrap(), true, &[]);
            match read_material_file(&path)? {
                Some(raw) => raw,
                None => {
                    creation?;
                    let mut material = [0u8; 32];
                    OsRng.fill_bytes(&mut material);
                    let protected = dpapi_protect(&material)
                        .map_err(|e| format!("Failed to protect install material: {e}"))?;
                    crate::datastore_io::publish_new(&path, &protected)?;
                    // Every initializer must use the committed winner's key.
                    fs::read(&path).map_err(|e| format!("Failed to read install material: {e}"))?
                }
            }
        }
    };
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
    Ok(buf)
}

fn read_material_file(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Could not read settings key; existing data preserved: {error}"
        )),
    }
}

fn ensure_material_can_be_created(
    directory: &std::path::Path,
    include_sections: bool,
    filenames: &[&str],
) -> Result<(), String> {
    let missing_key = "Settings key is missing but encrypted data exists; restore the matching key. Existing data preserved.";
    for filename in filenames {
        match fs::symlink_metadata(directory.join(filename)) {
            Ok(_) => return Err(missing_key.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not inspect settings data: {error}")),
        }
    }
    if include_sections {
        match fs::read_dir(directory.join(STORE_SUBDIR)) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry
                        .map_err(|error| format!("Could not inspect settings data: {error}"))?;
                    if entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("dat"))
                    {
                        return Err(missing_key.to_string());
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not inspect settings data: {error}")),
        }
    }
    Ok(())
}

fn protect_user_store_material(
    material: &[u8; 32],
    current_user_install: bool,
) -> Result<Vec<u8>, String> {
    if current_user_install {
        dpapi_protect(material).map_err(|_| "could not protect per-user store material".to_string())
    } else {
        user_dpapi_protect(material)
            .map_err(|_| "could not protect per-user store material".to_string())
    }
}

fn unprotect_user_store_material(
    protected: &[u8],
    current_user_install: bool,
) -> Result<Vec<u8>, String> {
    if current_user_install {
        // Preserve an existing user-DPAPI material if this logon can still
        // unlock it. Older current-user builds wrote that format. If it cannot
        // be opened (notably under SSH), accept the current machine-DPAPI
        // format instead.
        user_dpapi_unprotect(protected).or_else(|_| dpapi_unprotect(protected))
    } else {
        user_dpapi_unprotect(protected)
    }
}

fn write_new_user_material(
    path: &std::path::Path,
    current_user_install: bool,
) -> Result<[u8; 32], String> {
    let mut material = [0u8; 32];
    OsRng.fill_bytes(&mut material);
    let protected = protect_user_store_material(&material, current_user_install)?;
    crate::datastore_io::publish_new(path, &protected)
        .map_err(|_| "could not write per-user store material".to_string())?;
    let committed =
        fs::read(path).map_err(|_| "could not read per-user store material".to_string())?;
    decode_user_material(&committed, current_user_install)
}

fn decode_user_material(raw: &[u8], current_user_install: bool) -> Result<[u8; 32], String> {
    let plain = unprotect_user_store_material(raw, current_user_install).map_err(|_| {
        "SETTINGS_KEY_UNAVAILABLE: Could not unlock settings key; existing key and encrypted data preserved.".to_string()
    })?;
    plain
        .try_into()
        .map_err(|_| "Invalid settings key length; existing data preserved.".to_string())
}

fn user_material() -> Result<[u8; 32], String> {
    let directory = crate::paths::user_data_dir()?;
    load_or_create_user_material(
        &directory,
        crate::paths::current_user_install_uses_local_datastore(),
        write_new_user_material,
    )
}

fn load_or_create_user_material(
    directory: &std::path::Path,
    current_user_install: bool,
    create: impl FnOnce(&std::path::Path, bool) -> Result<[u8; 32], String>,
) -> Result<[u8; 32], String> {
    let path = directory.join(USER_MATERIAL_FILENAME);
    match read_material_file(&path)? {
        Some(raw) => decode_user_material(&raw, current_user_install),
        None => {
            let creation =
                ensure_material_can_be_created(directory, current_user_install, &USER_STORE_FILES);
            match read_material_file(&path)? {
                Some(raw) => decode_user_material(&raw, current_user_install),
                None => {
                    creation?;
                    create(&path, current_user_install)
                }
            }
        }
    }
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

#[cfg(test)]
#[test]
fn section_key_matches_pre_upgrade_argon2_for_existing_stores() {
    let material = [0x42; 32];
    let params = argon2_legacy::Params::new(65_536, 2, 1, Some(32)).unwrap();
    let legacy = argon2_legacy::Argon2::new(
        argon2_legacy::Algorithm::Argon2id,
        argon2_legacy::Version::V0x13,
        params,
    );
    for password in [None, Some("existing-user-passphrase")] {
        let mut expected = [0; 32];
        legacy
            .hash_password_into(password.unwrap_or("").as_bytes(), &material, &mut expected)
            .unwrap();
        assert_eq!(derive_section_key(&material, password).unwrap(), expected);
    }
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
    crate::datastore_io::atomic_write(path, data)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Load a settings section from disk. Returns an empty object if the section
/// file does not yet exist (first run). Returns an error on I/O or decode failure.
pub fn load(section: &str) -> Result<Value, String> {
    validate_section_name(section)?;
    #[cfg(all(windows, feature = "experimental-settings-generation"))]
    if experimental_generation_routing_enabled() {
        let (root, _pins, scope) = settings_rotation_trial::experimental_runtime_root()?;
        return settings_rotation_trial::read_with_legacy(&root, scope, section, || load_legacy_guarded(section));
    }
    load_legacy(section)
}

#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(crate) fn experimental_generation_routing_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // Automatic routing is restricted to explicitly opted-in lab builds. The
    // installer must complete migration before starting the new application.
    *ENABLED.get_or_init(|| generation_lab_routing_requested(
        option_env!("WINCMD_SETTINGS_LAB_AUTOROUTE"),
        std::env::var_os("WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION").as_deref(),
    ))
}

#[cfg(any(test, all(windows, feature = "experimental-settings-generation")))]
fn generation_lab_routing_requested(build_opt_in: Option<&str>, runtime_opt_in: Option<&std::ffi::OsStr>) -> bool {
    build_opt_in == Some("1") || runtime_opt_in.is_some_and(|value| value == "1")
}

fn load_legacy(section: &str) -> Result<Value, String> {
    // Reading a first-run store must not require creating an administrator-owned
    // subdirectory. Only NotFound means defaults; denied/corrupt policy stays an error.
    let path = crate::paths::datastore_data_dir()?
        .join(STORE_SUBDIR)
        .join(format!("{section}.dat"));
    let Some(raw) = read_section_file(&path, section)? else {
        return Ok(Value::Object(Default::default()));
    };
    let material = install_material()?;
    let key = derive_section_key(&material, None)?;
    let bytes = decode_section(&key, raw.trim(), &format!("machine:{section}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse section '{section}': {e}"))
}

#[cfg(all(windows, feature = "experimental-settings-generation"))]
fn load_legacy_guarded(section: &str) -> Result<Value, String> {
    let value = load_legacy(section)?;
    if section.eq_ignore_ascii_case("f6-keystore") {
        let path = crate::paths::datastore_data_dir()?.join(STORE_SUBDIR).join(format!("{section}.dat"));
        let exists = path.try_exists().map_err(|_| "cannot inspect device signing key; existing data preserved")?;
        if exists && !settings_rotation_trial::valid_f6(&value) {
            return Err("device signing key is empty or invalid; existing data preserved".into());
        }
    }
    Ok(value)
}

fn read_section_file(path: &std::path::Path, section: &str) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(Some(raw)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Failed to read section '{section}': {error}")),
    }
}

// Reads may still recover a legacy plaintext material file. New ciphertext must
// not be written unless the material used for it has been protected and can
// still be reopened; otherwise a copied disk exposes both ciphertext and key.
pub(crate) struct SettingsWriteMaterial {
    material: Option<zeroize::Zeroizing<[u8; 32]>>,
    #[cfg(all(windows, feature = "experimental-settings-generation"))]
    runtime: Option<settings_rotation_trial::RuntimeWriteGuard>,
    _material_guard: Option<std::fs::File>,
}

pub(crate) fn settings_write_material() -> Result<SettingsWriteMaterial, String> {
    #[cfg(all(windows, feature = "experimental-settings-generation"))]
    let runtime = if experimental_generation_routing_enabled() {
        Some(settings_rotation_trial::begin_experimental_write()?)
    } else { None };
    #[cfg(all(windows, feature = "experimental-settings-generation"))]
    if runtime.as_ref().is_some_and(|guard| guard.selected()) {
        return Ok(SettingsWriteMaterial { material: None, runtime, _material_guard: None });
    }
    let material = install_material()?;
    #[cfg(not(windows))]
    return Ok(SettingsWriteMaterial {
        material: Some(zeroize::Zeroizing::new(material)),
        _material_guard: None,
    });

    #[cfg(windows)]
    {
        let user_install = crate::paths::current_user_install_uses_local_datastore();
        let path = if user_install {
            crate::paths::user_data_dir()?.join(USER_MATERIAL_FILENAME)
        } else {
            crate::paths::datastore_data_dir()?.join(MATERIAL_FILENAME)
        };
        let file = open_verified_write_material(&path, &material, |bytes| {
            if user_install {
                unprotect_user_store_material(bytes, true)
            } else {
                dpapi_unprotect(bytes)
            }
        })?;
        Ok(SettingsWriteMaterial {
            material: Some(zeroize::Zeroizing::new(material)),
            #[cfg(feature = "experimental-settings-generation")]
            runtime,
            _material_guard: Some(file),
        })
    }
}

#[cfg(windows)]
fn open_verified_write_material(
    path: &std::path::Path,
    material: &[u8; 32],
    unprotect: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<std::fs::File, String> {
    use std::io::Read;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    // Hold a read-only handle that denies writes and replacement until the
    // encrypted section is committed. Refuse a reparse-point key file.
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x0020_0000)
        .open(path)
        .map_err(|_| "Settings key unavailable; existing data preserved.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Settings key unavailable; existing data preserved.")?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 || metadata.len() > 65536 {
        return Err("Settings key unavailable; existing data preserved.".into());
    }
    let mut raw = Vec::new();
    (&mut file)
        .take(65537)
        .read_to_end(&mut raw)
        .map_err(|_| "Settings key unavailable; existing data preserved.")?;
    if raw.len() > 65536 {
        return Err("Settings key unavailable; existing data preserved.".into());
    }
    verify_write_material(material, Some(&raw), unprotect)?;
    Ok(file)
}

#[cfg(any(windows, test))]
fn verify_write_material(
    expected: &[u8; 32],
    raw: Option<&[u8]>,
    unprotect: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let error = "Settings key is not protected or has changed; existing data preserved.";
    let raw = raw.ok_or(error)?;
    if raw.len() == 32 {
        return Err(error.into());
    }
    let actual = zeroize::Zeroizing::new(unprotect(raw).map_err(|_| error)?);
    if actual.as_slice() != expected {
        return Err(error.into());
    }
    Ok(())
}

/// Write a settings section to disk (replaces the full section).
pub fn save(section: &str, data: &Value) -> Result<(), String> {
    validate_section_name(section)?;
    let material = settings_write_material()?;
    save_with_material(section, data, &material)
}

pub(crate) fn save_with_material(
    section: &str,
    data: &Value,
    material: &SettingsWriteMaterial,
) -> Result<(), String> {
    #[cfg(all(windows, feature = "experimental-settings-generation"))]
    if let Some(runtime) = &material.runtime {
        if runtime.selected() { return runtime.write(section, data); }
        if section.eq_ignore_ascii_case("f6-keystore") {
            if !settings_rotation_trial::valid_f6(data) { return Err("device signing key is empty or invalid; existing key preserved".into()); }
            let previous = load_legacy_guarded(section)?;
            if settings_rotation_trial::valid_f6(&previous)
                && !previous["signing_seed_hex"].as_str().unwrap_or("").eq_ignore_ascii_case(data["signing_seed_hex"].as_str().unwrap_or("")) {
                return Err("replacing an existing device signing key is not allowed".into());
            }
        }
        // The same runtime guard remains held throughout this legacy write.
    }
    save_legacy_with_material(section, data, material)
}

/// The experimental F6 path holds the same exclusive guard while inspecting,
/// generating, saving and reopening the committed value in either generation.
#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(crate) fn guarded_f6_get_or_create(create: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    let (root, _pins, scope) = settings_rotation_trial::experimental_runtime_root()?;
    settings_rotation_trial::read_f6_or_create(&root, scope,
        || load_legacy_guarded("f6-keystore"),
        || guarded_f6_create_exclusive(create))
}

#[cfg(all(windows, feature = "experimental-settings-generation"))]
fn guarded_f6_create_exclusive(create: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    // Reacquire exclusive ownership and re-read after releasing the shared
    // guard: another process may have created the key in the intervening time.
    let material = settings_write_material()?;
    let runtime = material.runtime.as_ref().ok_or("settings generation ownership unavailable")?;
    if runtime.selected() { return runtime.get_or_create_f6(create); }
    let current = load_legacy_guarded("f6-keystore")?;
    if settings_rotation_trial::valid_f6(&current) { return Ok(current); }
    if current != Value::Object(Default::default()) { return Err("device signing key is invalid; existing data preserved".into()); }
    let created = create()?;
    if !settings_rotation_trial::valid_f6(&created) { return Err("new device signing key is invalid".into()); }
    save_legacy_with_material("f6-keystore", &created, &material)?;
    let observed = load_legacy_guarded("f6-keystore")?;
    if observed != created { return Err("committed device signing key did not verify".into()); }
    Ok(created)
}

fn save_legacy_with_material(section: &str, data: &Value, material: &SettingsWriteMaterial) -> Result<(), String> {
    let path = section_path(section)?;
    let source = material.material.as_ref().ok_or("legacy settings key unavailable")?;
    let key = derive_section_key(source, None)?;
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
    // Never replace unreadable material: the same key may protect settings,
    // private sections and clipboard rules that remain recoverable.
    let key = derive_section_key(&user_material()?, None)?;
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
    let plaintext = decode_section(&key, encoded.trim(), &format!("user:{filename}"))
        .map_err(|_| "per-user data could not be decoded".to_string())?;
    if plaintext.len() > max_plaintext_bytes {
        return Err("per-user data exceed the size limit".to_string());
    }
    Ok(Some(plaintext))
}

/// Produces ciphertext for the caller's atomic service-record update.
pub(crate) fn encode_user_secrets(
    plaintext: &[u8],
    require_existing: bool,
) -> Result<String, String> {
    if plaintext.len() > MAX_PERSONAL_SECRETS_BYTES {
        return Err("Personal secrets exceed the size limit".to_string());
    }
    // A loaded service envelope proves a key existed even if no legacy file remains.
    let material = if require_existing {
        let path = crate::paths::user_data_dir()?.join(USER_MATERIAL_FILENAME);
        existing_user_material(
            &path,
            crate::paths::current_user_install_uses_local_datastore(),
        )?
    } else {
        user_material()?
    };
    mark_personal_secrets_key_in_use(&crate::paths::user_data_dir()?)?;
    let key = derive_section_key(&material, None)?;
    encode_section(&key, plaintext, PERSONAL_SECRETS_SCOPE)
}

fn mark_personal_secrets_key_in_use(directory: &std::path::Path) -> Result<(), String> {
    // All other user_material consumers must know ciphertext can live only in the service.
    atomic_write_bytes(
        &directory.join(PERSONAL_SECRETS_KEY_MARKER),
        b"personal-secrets-key-v1\n",
    )
    .map_err(|_| "Could not preserve personal secrets key ownership".to_string())
}

fn validate_user_secrets_envelope(encoded: &str) -> Result<(), String> {
    if encoded.len() > MAX_PERSONAL_SECRETS_BYTES * 2 + 128 {
        return Err("Personal secrets exceed the size limit".to_string());
    }
    if !encoded.starts_with(FORMAT_PREFIX_V2) {
        return Err("Personal secrets encryption format is invalid".to_string());
    }
    Ok(())
}

pub(crate) fn decode_user_secrets(encoded: &str) -> Result<Vec<u8>, String> {
    validate_user_secrets_envelope(encoded)?;
    // An existing envelope proves there was a key: never create another on read.
    let path = crate::paths::user_data_dir()?.join(USER_MATERIAL_FILENAME);
    let material = existing_user_material(
        &path,
        crate::paths::current_user_install_uses_local_datastore(),
    )?;
    let key = derive_section_key(&material, None)?;
    let plaintext = decode_section(&key, encoded, PERSONAL_SECRETS_SCOPE)
        .map_err(|_| "Personal secrets could not be decoded".to_string())?;
    if plaintext.len() > MAX_PERSONAL_SECRETS_BYTES {
        return Err("Personal secrets exceed the size limit".to_string());
    }
    Ok(plaintext)
}

fn existing_user_material(
    path: &std::path::Path,
    current_user_install: bool,
) -> Result<[u8; 32], String> {
    let raw = read_material_file(path)?.ok_or_else(|| {
        "SETTINGS_KEY_UNAVAILABLE: Personal secrets key is missing; encrypted data preserved."
            .to_string()
    })?;
    decode_user_material(&raw, current_user_install)
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

struct CachedLogKey {
    key: [u8; DERIVED_KEY_LEN],
    material_hash: [u8; 32],
}
static LOG_KEY_CACHE: std::sync::OnceLock<CachedLogKey> = std::sync::OnceLock::new();

fn material_hash(material: &[u8; 32]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(material).into()
}

fn cached_log_key() -> Result<[u8; DERIVED_KEY_LEN], String> {
    if let Some(k) = LOG_KEY_CACHE.get() {
        return Ok(k.key);
    }
    let user_install = crate::paths::current_user_install_uses_local_datastore();
    let path = if user_install {
        crate::paths::user_data_dir()?.join(USER_MATERIAL_FILENAME)
    } else {
        crate::paths::datastore_data_dir()?.join(MATERIAL_FILENAME)
    };
    let raw = read_material_file(&path)?.ok_or("log material unavailable")?;
    let material: [u8; 32] = if user_install {
        decode_user_material(&raw, true)?
    } else if raw.len() == 32 {
        raw.try_into().map_err(|_| "invalid log material")?
    } else {
        dpapi_unprotect(&raw).map_err(|_| "log material unavailable")?
            .try_into().map_err(|_| "invalid log material")?
    };
    let key = derive_section_key(&material, None)?;
    let _ = LOG_KEY_CACHE.set(CachedLogKey { key, material_hash: material_hash(&material) });
    LOG_KEY_CACHE
        .get()
        .map(|cached| cached.key)
        .ok_or_else(|| "log key cache unavailable".to_string())
}

/// A cached key can still decrypt old records, but writing requires proof that
/// the matching install material remains protected and available now.
fn protected_log_key() -> Result<[u8; DERIVED_KEY_LEN], String> {
    let user_install = crate::paths::current_user_install_uses_local_datastore();
    let path = if user_install {
        crate::paths::user_data_dir()?.join(USER_MATERIAL_FILENAME)
    } else {
        crate::paths::datastore_data_dir()?.join(MATERIAL_FILENAME)
    };
    let raw = read_material_file(&path)?;
    let material = existing_protected_log_material(raw.as_deref(), |bytes| {
        if user_install { unprotect_user_store_material(bytes, true) } else { dpapi_unprotect(bytes) }
    })?;
    if LOG_KEY_CACHE.get().is_none() {
        let key = derive_section_key(&material, None)?;
        let _ = LOG_KEY_CACHE.set(CachedLogKey { key, material_hash: material_hash(&material) });
    }
    let cached = LOG_KEY_CACHE.get().ok_or("log key cache unavailable")?;
    if cached.material_hash != material_hash(&material) {
        return Err("protected log material changed".into());
    }
    Ok(cached.key)
}

// Logging is never allowed to create or migrate a shared key. Other settings
// operations own that lifecycle; a missing/plaintext/unreadable key fails closed.
fn existing_protected_log_material(
    raw: Option<&[u8]>,
    unprotect: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<[u8; 32], String> {
    let raw = raw.ok_or("protected log material unavailable")?;
    if raw.len() == 32 { return Err("log material is not protected".into()); }
    let unprotected = unprotect(raw).map_err(|_| "protected log material unavailable")?;
    unprotected.try_into().map_err(|_| "invalid protected log material".into())
}

/// Writes new structured diagnostic events in the shared D1 envelope. Unlike
/// the legacy general-log helper below, this has no plaintext fallback.
pub(crate) fn diagnostic_encrypt_line(date: &str, body: &str) -> Result<String, String> {
    let key = protected_log_key()?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    wincmd_shared::diagnostics::seal_diagnostic_record(
        &key,
        date,
        "desktop",
        body.as_bytes(),
        nonce,
    )
}

/// Reads D1 diagnostic records and the historic L2/L log envelopes. Existing
/// diagnostic history stays visible while all new diagnostics use D1.
pub(crate) fn diagnostic_decrypt_line(line: &str) -> Option<(String, String)> {
    if let Some(rest) = line.strip_prefix("D1:") {
        let (date, _) = rest.split_once(':')?;
        let key = cached_log_key().ok()?;
        let plaintext = wincmd_shared::diagnostics::open_diagnostic_record(&key, "desktop", line)?;
        return String::from_utf8(plaintext)
            .ok()
            .map(|body| (date.to_string(), body));
    }
    log_decrypt_line(line)
}

/// Encrypt one log body into an on-disk `L2:<date>:<b64>` line.
/// `date` = `YYYY-MM-DD`; `body` = `[HH:MM:SS] [LEVEL] message`.
/// Never return the plaintext when the key or encryption is unavailable.
#[allow(dead_code)]
pub fn log_encrypt_line(date: &str, body: &str) -> Result<String, String> {
    let key = protected_log_key()?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.try_fill_bytes(&mut nonce_bytes).map_err(|_| "log randomness unavailable")?;
    seal_log_line(date, body, &key, nonce_bytes)
}

fn seal_log_line(date: &str, body: &str, key: &[u8; 32], nonce_bytes: [u8; 12]) -> Result<String, String> {
    let cipher = Aes256Gcm::new(key.into());
    let nonce = nonce_from_bytes(&nonce_bytes)?;
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
            Ok(format!("L2:{}:{}", date, B64.encode(&payload)))
        }
        Err(_) => Err("log encryption failed".into()),
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
    #[test]
    fn settings_generation_routing_requires_explicit_lab_opt_in() {
        use std::ffi::OsStr;
        assert!(!super::generation_lab_routing_requested(None, None));
        assert!(!super::generation_lab_routing_requested(Some("true"), Some(OsStr::new("0"))));
        assert!(super::generation_lab_routing_requested(Some("1"), None));
        assert!(super::generation_lab_routing_requested(None, Some(OsStr::new("1"))));
    }

    #[cfg(windows)]
    #[test]
    fn settings_writes_require_the_current_protected_material() {
        let material = [42u8; 32];
        let protected = super::dpapi_protect(&material).unwrap();
        super::verify_write_material(&material, Some(&protected), super::dpapi_unprotect).unwrap();
        assert!(super::verify_write_material(&material, None, super::dpapi_unprotect).is_err());
        assert!(super::verify_write_material(&material, Some(&material), |_| {
            panic!("legacy plaintext must not be accepted for writes")
        }).is_err());
        assert!(super::verify_write_material(&[43u8; 32], Some(&protected), super::dpapi_unprotect).is_err());
        assert!(super::verify_write_material(&material, Some(&[1u8; 64]), super::dpapi_unprotect).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn settings_write_guard_holds_protected_material_against_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        let material = [23u8; 32];
        let protected = super::dpapi_protect(&material).unwrap();
        std::fs::write(&path, &protected).unwrap();
        let guard = super::open_verified_write_material(&path, &material, super::dpapi_unprotect).unwrap();
        assert!(std::fs::write(&path, b"changed").is_err());
        assert!(std::fs::remove_file(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), protected);
        drop(guard);
        std::fs::write(&path, b"changed").unwrap();
    }

    #[test]
    fn log_writes_reject_plaintext_unavailable_or_changed_material() {
        let material = [4u8; 32];
        assert!(super::existing_protected_log_material(None, |_| panic!("missing key must not be created")).is_err());
        assert!(super::existing_protected_log_material(Some(&material), |_| panic!("plaintext key must not be rewrapped")).is_err());
        assert!(super::existing_protected_log_material(Some(&[1u8; 64]), |_| Err("DPAPI unavailable".into())).is_err());
        assert!(super::existing_protected_log_material(Some(&[1u8; 64]), |_| Ok(vec![5u8; 31])).is_err());
        assert_eq!(super::existing_protected_log_material(Some(&[1u8; 64]), |_| Ok(material.to_vec())).unwrap(), material);
        assert_ne!(super::material_hash(&material), super::material_hash(&[5u8; 32]));
    }

    #[test]
    fn log_ciphertext_round_trips_and_authenticates_its_date() {
        use aes_gcm::{aead::{Aead, KeyInit, Payload}, Aes256Gcm};
        use base64::Engine;
        let key = [7u8; 32];
        let body = "private log contents";
        let line = super::seal_log_line("2026-11-05", body, &key, [3; 12]).unwrap();
        assert!(!line.contains(body));
        let payload = super::B64.decode(line.strip_prefix("L2:2026-11-05:").unwrap()).unwrap();
        let cipher = Aes256Gcm::new((&key).into());
        let nonce = super::nonce_from_bytes(&payload[..12]).unwrap();
        let aad = super::section_aad("log:2026-11-05");
        assert_eq!(cipher.decrypt(nonce, Payload { msg: &payload[12..], aad: aad.as_bytes() }).unwrap(), body.as_bytes());
        let wrong = super::section_aad("log:2026-11-06");
        assert!(cipher.decrypt(nonce, Payload { msg: &payload[12..], aad: wrong.as_bytes() }).is_err());
    }

    #[test]
    fn missing_section_read_does_not_create_the_store() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("absent-store").join("settings.dat");
        assert_eq!(super::read_section_file(&path, "settings").unwrap(), None);
        assert!(!path.parent().unwrap().exists());
    }

    #[test]
    fn section_io_errors_are_not_treated_as_empty_policy() {
        let temp = tempfile::tempdir().unwrap();
        assert!(super::read_section_file(temp.path(), "settings").is_err());
        let path = temp.path().join("settings.dat");
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(super::read_section_file(&path, "settings").is_err());
        std::fs::write(&path, "encrypted-settings").unwrap();
        assert_eq!(
            super::read_section_file(&path, "settings")
                .unwrap()
                .as_deref(),
            Some("encrypted-settings")
        );
    }
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
    fn personal_secret_envelope_is_bound_to_its_own_scope() {
        let key = test_key(19);
        let encoded = encode_section(&key, b"synthetic secret", PERSONAL_SECRETS_SCOPE).unwrap();
        validate_user_secrets_envelope(&encoded).unwrap();
        assert_eq!(
            decode_section(&key, &encoded, PERSONAL_SECRETS_SCOPE).unwrap(),
            b"synthetic secret"
        );
        assert!(decode_section(&key, &encoded, "user:user-settings.dat").is_err());
        assert!(decode_section(&key, &encoded, "machine:settings").is_err());
    }

    #[test]
    fn secret_envelope_rejects_legacy_and_oversized_input_before_key_access() {
        assert!(validate_user_secrets_envelope("enc:v1:AAAA").is_err());
        assert!(
            validate_user_secrets_envelope(&"x".repeat(MAX_PERSONAL_SECRETS_BYTES * 2 + 129))
                .is_err()
        );
        assert!(encode_user_secrets(&vec![0; MAX_PERSONAL_SECRETS_BYTES + 1], false).is_err());
    }

    #[test]
    fn service_secret_read_never_creates_missing_profile_key() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(USER_MATERIAL_FILENAME);
        let error = existing_user_material(&path, false).unwrap_err();
        assert!(error.starts_with("SETTINGS_KEY_UNAVAILABLE:"));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn service_secret_marker_prevents_other_consumers_replacing_a_missing_key() {
        let directory = tempfile::tempdir().unwrap();
        mark_personal_secrets_key_in_use(directory.path()).unwrap();
        for current_user_install in [false, true] {
            let error =
                load_or_create_user_material(directory.path(), current_user_install, |_, _| {
                    panic!("service ciphertext must forbid replacement key creation")
                })
                .unwrap_err();
            assert!(error.starts_with("Settings key is missing but encrypted data exists;"));
        }
        assert!(!directory.path().join(USER_MATERIAL_FILENAME).exists());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn fresh_profile_key_creation_remains_available_without_secret_marker() {
        let directory = tempfile::tempdir().unwrap();
        let material = load_or_create_user_material(directory.path(), false, |path, _| {
            assert_eq!(path, directory.path().join(USER_MATERIAL_FILENAME));
            Ok([17; 32])
        })
        .unwrap();
        assert_eq!(material, [17; 32]);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn secret_key_marker_write_failure_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join(PERSONAL_SECRETS_KEY_MARKER)).unwrap();
        assert!(mark_personal_secrets_key_in_use(directory.path()).is_err());
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
        assert!(validate_section_name("../etc/passwd").is_err());
        assert!(validate_section_name("foo/bar").is_err());
        assert!(validate_section_name("valid-name_123").is_ok());
    }

    #[test]
    fn per_user_store_filename_cannot_escape_the_user_directory() {
        assert!(user_file_path("../clipboard-guard-rules.dat").is_err());
        assert!(user_file_path("clipboard/guard.dat").is_err());
        assert!(user_file_path("clipboard-guard-rules.dat").is_ok());
    }

    #[test]
    fn missing_key_does_not_replace_existing_encrypted_sections() {
        let temp = tempfile::tempdir().unwrap();
        let store = temp.path().join(STORE_SUBDIR);
        fs::create_dir(&store).unwrap();
        let path = store.join("settings.dat");
        fs::write(&path, b"preserve encrypted policy").unwrap();
        assert!(ensure_material_can_be_created(temp.path(), true, &[]).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"preserve encrypted policy");
        assert!(!temp.path().join(MATERIAL_FILENAME).exists());
    }

    #[test]
    fn missing_user_key_preserves_overlay_and_local_install_sections() {
        let temp = tempfile::tempdir().unwrap();
        assert!(ensure_material_can_be_created(temp.path(), true, &USER_STORE_FILES).is_ok());
        fs::write(temp.path().join(USER_STORE_FILES[0]), b"saved overlay").unwrap();
        assert!(ensure_material_can_be_created(temp.path(), false, &USER_STORE_FILES).is_err());
        assert!(!temp.path().join(USER_MATERIAL_FILENAME).exists());
    }

    #[test]
    fn key_read_errors_do_not_become_first_run() {
        let temp = tempfile::tempdir().unwrap();
        assert!(read_material_file(temp.path()).is_err());
        assert_eq!(
            read_material_file(&temp.path().join("absent")).unwrap(),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn unreadable_user_key_is_preserved_without_rotation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(USER_MATERIAL_FILENAME);
        fs::write(&path, b"unreadable material").unwrap();
        let raw = read_material_file(&path).unwrap().unwrap();
        for current_user_install in [false, true] {
            let error = decode_user_material(&raw, current_user_install).unwrap_err();
            assert!(error.starts_with("SETTINGS_KEY_UNAVAILABLE:"));
        }
        assert_eq!(fs::read(&path).unwrap(), raw);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn concurrent_user_key_creation_returns_the_committed_key() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(USER_MATERIAL_FILENAME);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    write_new_user_material(&path, false).unwrap()
                })
            })
            .collect();
        let material = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert!(material.iter().all(|key| key == &material[0]));
        assert_eq!(
            decode_user_material(&fs::read(path).unwrap(), false).unwrap(),
            material[0]
        );
    }

    #[cfg(windows)]
    #[test]
    fn current_user_install_material_uses_machine_dpapi_and_reads_legacy_user_dpapi() {
        let material = [9u8; 32];
        let protected = protect_user_store_material(&material, true).expect("machine protect");
        assert!(
            protected.len() > material.len(),
            "DPAPI must protect the material"
        );
        assert_eq!(
            dpapi_unprotect(&protected).expect("machine unprotect"),
            material,
            "current-user installations need machine DPAPI to survive SSH/noninteractive logons"
        );

        let legacy = user_dpapi_protect(&material).expect("legacy current-user protect");
        assert_eq!(
            unprotect_user_store_material(&legacy, true).expect("legacy fallback"),
            material,
            "a readable legacy current-user material must not be reset"
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

#[cfg(any(test, feature = "experimental-settings-generation"))]
#[allow(dead_code)] // Cutover stays disabled pending ownership and recovery validation.
#[path = "settings_rotation_trial.rs"]
mod settings_rotation_trial;
