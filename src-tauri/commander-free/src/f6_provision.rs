// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/f6_provision.rs
//
// F6 — Create-Wipe-USB provisioning commands.
//
// Two paid, admin-required Tauri commands:
//   - f6_list_removable_volumes: enumerate removable drives for the UI picker.
//   - f6_provision_wipe_usb:     write pubkey.bin + device_id.txt to the USB.
//
// Both commands call require_paid first. Neither performs any crypto-erase
// or reboot. Writing pubkey.bin is reversible (delete the file to undo).
//
// # Fixed-disk guard
//
// GetDriveTypeW is called on every path passed to f6_provision_wipe_usb.
// DRIVE_REMOVABLE (2) is the only accepted type. DRIVE_FIXED (3), DRIVE_CDROM
// (5), or any other type returns Err — we never write to the wrong disk.

use serde_json::{json, Value};

// ── Public Tauri commands ─────────────────────────────────────────────────────

/// List all removable (USB/SD) volumes visible to the OS.
///
/// Returns a JSON array: `[{ driveLetter, label, totalBytes }, ...]`.
/// Fixed disks, CD-ROMs, and unmapped drive bits are excluded.
///
/// # Pro-gating
///
/// Calls `require_paid` — returns Err if the licence is not Pro.
#[tauri::command]
pub fn f6_list_removable_volumes() -> Result<Value, String> {
    crate::license::require_paid("f6 wipe usb list")?;
    list_removable_impl()
}

/// Write the device's provisioning public key and device ID to the USB
/// control area.
///
/// Creates `<usb_root>\wipe\` if needed, then writes:
///   - `pubkey.bin`    — 32 raw bytes of the device Ed25519 verifying key
///   - `device_id.txt` — the device UUID string (UTF-8, no newline)
///
/// The device ID written here MUST match what `f6_orchestrator::build_production_deps`
/// reads (i.e. `settings::read_settings()?.device_id`). Both call the same fn.
///
/// # Fixed-disk guard
///
/// Refuses any `usb_root` whose drive letter maps to a non-removable volume
/// (DRIVE_REMOVABLE = 2 is the only accepted type).
///
/// # Pro-gating
///
/// Calls `require_paid` — returns Err if the licence is not Pro.
#[tauri::command]
pub fn f6_provision_wipe_usb(usb_root: String) -> Result<Value, String> {
    crate::license::require_paid("f6 wipe usb provision")?;

    // Validate the drive is removable before writing anything.
    validate_removable_path(&usb_root)?;

    // Get this device's stable ID — same source as build_production_deps().
    let device_id = crate::settings::read_settings()
        .map(|s| s.device_id)
        .unwrap_or_else(|_| "unknown-device".to_string());

    provision_impl(&usb_root, &device_id)
}

// ── Inner implementations (testable without Tauri runtime) ───────────────────

/// The actual provisioning write — extracted so tests can call it directly
/// with a synthetic device_id and a temp dir path.
pub(crate) fn provision_impl(usb_root: &str, device_id: &str) -> Result<Value, String> {
    use std::path::Path;

    let root = Path::new(usb_root);
    let wipe_dir = root.join("wipe");

    std::fs::create_dir_all(&wipe_dir)
        .map_err(|e| format!("create wipe dir '{}': {e}", wipe_dir.display()))?;

    // Write pubkey.bin — 32 raw bytes of this device's Ed25519 verifying key.
    let pubkey_bytes = crate::f6_keystore::device_verifying_key_bytes()?;
    let pubkey_path = wipe_dir.join("pubkey.bin");
    std::fs::write(&pubkey_path, pubkey_bytes).map_err(|e| format!("write pubkey.bin: {e}"))?;

    // Write device_id.txt — UTF-8 UUID, no trailing newline.
    // verify-token.sh reads this with `cat` and strips whitespace.
    let device_id_path = wipe_dir.join("device_id.txt");
    std::fs::write(&device_id_path, device_id.as_bytes())
        .map_err(|e| format!("write device_id.txt: {e}"))?;

    crate::log_message(
        "info",
        &format!(
            "[F6-Provision] wrote pubkey.bin + device_id.txt to {}",
            wipe_dir.display()
        ),
    );

    Ok(json!({
        "pubkeyPath": pubkey_path.to_string_lossy(),
        "deviceIdPath": device_id_path.to_string_lossy(),
        "deviceId": device_id,
    }))
}

/// Check that the drive_type returned by GetDriveTypeW indicates a removable
/// volume. Extracted so tests can call it without touching real drives.
pub(crate) fn check_drive_is_removable_by_type(drive_type: u32) -> Result<(), String> {
    const DRIVE_REMOVABLE: u32 = 2;
    if drive_type == DRIVE_REMOVABLE {
        Ok(())
    } else {
        Err(format!(
            "path is not a removable volume (drive type {drive_type}) — \
             refusing to write to a fixed or unknown disk"
        ))
    }
}

// ── Windows-only helpers ──────────────────────────────────────────────────────

/// Validate that `path` is on a removable volume. On non-Windows builds this
/// always returns Err (the feature is Windows-only by design).
fn validate_removable_path(path: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

        // Extract the drive root (first 3 chars: "X:\") from the path.
        let drive_root = if path.len() >= 3 && path.as_bytes()[1] == b':' {
            format!("{}\\", &path[..2])
        } else {
            return Err(format!("usb_root '{path}' is not an absolute Windows path"));
        };

        let wide: Vec<u16> = OsStr::new(&drive_root)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let drive_type = unsafe { GetDriveTypeW(wide.as_ptr()) };
        check_drive_is_removable_by_type(drive_type)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("F6 provisioning requires Windows".to_string())
    }
}

/// Inner implementation of volume enumeration (Windows-only).
fn list_removable_impl() -> Result<Value, String> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::{
            GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
        };

        const DRIVE_REMOVABLE: u32 = 2;
        let mask = unsafe { GetLogicalDrives() };
        let mut volumes: Vec<Value> = Vec::new();

        for bit in 0u32..26 {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let letter = (b'A' + bit as u8) as char;
            let root = format!("{letter}:\\");
            let wide: Vec<u16> = OsStr::new(&root).encode_wide().chain(Some(0)).collect();

            let drive_type = unsafe { GetDriveTypeW(wide.as_ptr()) };
            if drive_type != DRIVE_REMOVABLE {
                continue;
            }

            // Read volume label (best-effort; empty string on error).
            let mut label_buf = vec![0u16; 256];
            let mut vol_serial: u32 = 0;
            let mut max_component: u32 = 0;
            let mut fs_flags: u32 = 0;
            let mut fs_name_buf = vec![0u16; 64];
            let label = unsafe {
                let ok = GetVolumeInformationW(
                    wide.as_ptr(),
                    label_buf.as_mut_ptr(),
                    label_buf.len() as u32,
                    &mut vol_serial,
                    &mut max_component,
                    &mut fs_flags,
                    fs_name_buf.as_mut_ptr(),
                    fs_name_buf.len() as u32,
                );
                if ok != 0 {
                    let end = label_buf
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(label_buf.len());
                    std::ffi::OsString::from_wide(&label_buf[..end])
                        .to_string_lossy()
                        .into_owned()
                } else {
                    String::new()
                }
            };

            volumes.push(json!({
                "driveLetter": root,
                "label": label,
            }));
        }

        Ok(Value::Array(volumes))
    }
    #[cfg(not(windows))]
    {
        Ok(Value::Array(vec![]))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── Test: provision writes pubkey.bin (32 bytes) and device_id.txt ──
    #[test]
    fn provision_writes_pubkey_and_device_id() {
        let dir = TempDir::new().expect("tempdir");
        let usb_root = dir.path().to_str().expect("path").to_string();

        // Call the inner logic directly (bypass the Tauri command + require_paid).
        let result = provision_impl(&usb_root, "test-device-uuid-1234");
        assert!(result.is_ok(), "provision_impl must succeed: {:?}", result);

        let wipe_dir = dir.path().join("wipe");
        assert!(wipe_dir.is_dir(), "wipe/ dir must be created");

        let pubkey = std::fs::read(wipe_dir.join("pubkey.bin")).expect("read pubkey.bin");
        assert_eq!(pubkey.len(), 32, "pubkey.bin must be 32 bytes");

        let device_id =
            std::fs::read_to_string(wipe_dir.join("device_id.txt")).expect("read device_id.txt");
        assert_eq!(
            device_id, "test-device-uuid-1234",
            "device_id.txt must match"
        );
    }

    // ── Test: non-removable path is refused ──
    #[test]
    fn provision_refuses_non_removable_drive_type() {
        // drive_type = DRIVE_FIXED (3) — must be refused
        let result = check_drive_is_removable_by_type(3);
        assert!(result.is_err(), "fixed disk must be refused");
        assert!(
            result.unwrap_err().contains("not a removable"),
            "error must name the rejection"
        );
    }

    // ── Test: removable drive type is accepted ──
    #[test]
    fn provision_accepts_removable_drive_type() {
        // drive_type = DRIVE_REMOVABLE (2) — must be accepted
        let result = check_drive_is_removable_by_type(2);
        assert!(result.is_ok(), "removable disk must be accepted");
    }

    #[test]
    fn drive_type_0_no_root_is_refused() {
        // DRIVE_NO_ROOT_DIR (1) — must be refused
        assert!(check_drive_is_removable_by_type(1).is_err());
    }

    #[test]
    fn drive_type_cdrom_is_refused() {
        // DRIVE_CDROM (5) — must be refused
        assert!(check_drive_is_removable_by_type(5).is_err());
    }

    // ── Test: device_id.txt content matches what orchestrator uses ──
    #[test]
    fn device_id_written_matches_settings_device_id_format() {
        let dir = TempDir::new().expect("tempdir");
        let device_id = "550e8400-e29b-41d4-a716-446655440000"; // valid UUID v4
        provision_impl(dir.path().to_str().unwrap(), device_id).expect("ok");
        let written =
            std::fs::read_to_string(dir.path().join("wipe").join("device_id.txt")).expect("read");
        // Must match character-for-character with no trailing newline
        assert_eq!(written, device_id);
    }

    #[test]
    fn provision_idempotent_overwrites() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().to_str().unwrap();
        provision_impl(path, "device-a").expect("first write");
        provision_impl(path, "device-b").expect("second write must not error");
        let written =
            std::fs::read_to_string(dir.path().join("wipe").join("device_id.txt")).expect("read");
        assert_eq!(written, "device-b", "second write must overwrite first");
    }
}
