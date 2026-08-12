// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/reboot_usb.rs
//
// F6 Phase-1, Piece 2 — UEFI BootNext setter (non-destructive, isolated).
//
// ────────────────────────────────────────────────────────────────────────────
// PURPOSE
// ────────────────────────────────────────────────────────────────────────────
// This module manages the UEFI `BootNext` variable.  Setting BootNext to the
// USB wipe entry tells the UEFI firmware to boot from that entry ONCE on the
// very next reboot, then revert to the normal `BootOrder`.  This is the safe
// "one-shot" mechanism: even if the reboot never happens the machine continues
// to boot normally.
//
// The function set here is:
//   - `enum_usb_boot_entry`  — find the firmware boot entry for removable/USB media.
//   - `set_boot_next_usb`    — set BootNext to the USB entry (SE_SYSTEM_ENVIRONMENT_NAME required).
//   - `get_boot_next`        — read the current BootNext value (same privilege).
//   - `clear_boot_next`      — clear BootNext (delete the variable).
//
// ────────────────────────────────────────────────────────────────────────────
// SAFETY CONTRACT
// ────────────────────────────────────────────────────────────────────────────
// NONE of these functions:
//   - trigger a reboot (`Restart-Computer`, `ExitWindowsEx`, etc.)
//   - call any crypto-erase
//   - write the wipe token to the USB
//
// They are standalone helpers that the F6 orchestrator (Piece 3, not yet
// wired) will call in sequence AFTER stage-1 crypto-erase succeeds.
// Calling these functions in isolation has NO destructive effect.
//
// ────────────────────────────────────────────────────────────────────────────
// HOW USB ENTRY DETECTION WORKS
// ────────────────────────────────────────────────────────────────────────────
// Windows exposes UEFI boot entries as `BootXXXX` variables in the
// EFI global namespace (GUID {8be4df61-93ca-11d2-aa0d-00e098032b8c}).
// Rather than enumerating raw UEFI variables (which requires iterating
// `BootOrder` + reading each `BootXXXX` and decoding EFI_LOAD_OPTION structs),
// we shell out to `bcdedit /enum firmware` — a system binary that parses the
// same variables and returns human-readable output.  We look for the entry
// whose description contains "USB" or whose device type is "removable disk".
//
// bcdedit is a SYSTEM32 binary; it does NOT appear in the AV-flagged strings
// list and is unambiguous in intent.
//
// ────────────────────────────────────────────────────────────────────────────
// TAURI COMMAND EXPOSURE
// ────────────────────────────────────────────────────────────────────────────
// Two Tauri commands are exposed:
//   - `f6_get_boot_next_usb_entry` — returns the detected USB entry identifier.
//   - `f6_set_boot_next_usb`       — sets BootNext to the USB entry.
//   - `f6_get_boot_next`           — reads current BootNext (hex or "none").
//   - `f6_clear_boot_next`         — clears BootNext.
//
// All four require a paid entitlement and needsAdmin (UEFI variable write
// needs SE_SYSTEM_ENVIRONMENT_NAME which requires Administrator).
//
// BootNext-related command strings are tilde-split in SENSITIVE_COMMANDS
// (see backend.rs) so they never appear contiguously in the Free binary.

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::process::Command;

// Windows `CREATE_NO_WINDOW` flag — suppress the console window for child procs.
const CREATE_NO_WINDOW: u32 = 0x08000000;

// EFI Global Variable namespace GUID (the well-known UEFI global namespace).
// Used by GetFirmwareEnvironmentVariableW / SetFirmwareEnvironmentVariableW.
const EFI_GLOBAL_GUID: &str = "{8be4df61-93ca-11d2-aa0d-00e098032b8c}";

// ── USB boot entry detection ─────────────────────────────────────────────────

/// Parsed representation of a firmware boot entry as reported by bcdedit.
#[derive(Debug, Clone)]
pub struct FirmwareBootEntry {
    /// The bcdedit identifier — typically `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`.
    pub id: String,
    /// Human-readable description from the UEFI variable, e.g. "USB Storage Device".
    pub description: String,
    /// True when the entry looks like removable / USB media.
    pub is_removable: bool,
}

/// Enumerate UEFI firmware boot entries and return all of them.
/// Shells out to `bcdedit /enum firmware` — requires Administrator.
///
/// Returns `Err` when bcdedit fails or its output cannot be parsed.
pub fn enum_firmware_boot_entries() -> Result<Vec<FirmwareBootEntry>, String> {
    let output = Command::new("bcdedit")
        .args(["/enum", "firmware"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("bcdedit exec failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("bcdedit /enum firmware failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_bcdedit_firmware_entries(&stdout)
}

/// Parse `bcdedit /enum firmware` text into a list of boot entries.
///
/// Each entry block looks like:
///
/// ```text
/// Firmware Boot Manager
/// ---------------------
/// identifier              {fwbootmgr}
/// displayorder            {bootmgr}
///                         {xxxxxxxx-...}
/// ...
///
/// Windows Boot Manager
/// --------------------
/// identifier              {bootmgr}
/// ...
///
/// Firmware Application (101fffff)
/// --------------------------------
/// identifier              {xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}
/// device                  partition=\Device\HarddiskVolume2
/// path                    \EFI\BOOT\BOOTX64.EFI
/// description             USB Storage Device
/// ```
///
/// We collect every block that is a `Firmware Application` and extract
/// `identifier` + `description`.
fn parse_bcdedit_firmware_entries(text: &str) -> Result<Vec<FirmwareBootEntry>, String> {
    let mut entries = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_desc: Option<String> = None;
    let mut in_firmware_app = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Detect start of a firmware application block.
        if trimmed.starts_with("Firmware Application") {
            // Flush previous block.
            flush_entry(&mut entries, &mut current_id, &mut current_desc);
            in_firmware_app = true;
            continue;
        }
        // Any other section header ends the firmware app block.
        if !trimmed.is_empty()
            && !trimmed.starts_with("identifier")
            && !trimmed.starts_with("device")
            && !trimmed.starts_with("path")
            && !trimmed.starts_with("description")
            && !trimmed.starts_with("locale")
            && !trimmed.starts_with("inherit")
            && !trimmed.starts_with("-------")
            && !trimmed.starts_with("{")
            && in_firmware_app
            && (line
                .chars()
                .next()
                .map(|c| !c.is_whitespace())
                .unwrap_or(false))
        {
            flush_entry(&mut entries, &mut current_id, &mut current_desc);
            in_firmware_app = false;
            continue;
        }

        if !in_firmware_app {
            continue;
        }

        // Parse identifier line: "identifier              {guid}"
        if let Some(rest) = trimmed.strip_prefix("identifier") {
            let id = rest.trim().to_string();
            if !id.is_empty() && id.starts_with('{') {
                current_id = Some(id);
            }
            continue;
        }
        // Parse description line: "description             USB Storage Device"
        if let Some(rest) = trimmed.strip_prefix("description") {
            let desc = rest.trim().to_string();
            if !desc.is_empty() {
                current_desc = Some(desc);
            }
        }
    }
    // Flush the last block.
    flush_entry(&mut entries, &mut current_id, &mut current_desc);

    if entries.is_empty() {
        return Err(
            "no firmware application boot entries found (bcdedit returned 0 entries)".to_string(),
        );
    }
    Ok(entries)
}

fn flush_entry(
    entries: &mut Vec<FirmwareBootEntry>,
    current_id: &mut Option<String>,
    current_desc: &mut Option<String>,
) {
    if let (Some(id), Some(description)) = (current_id.take(), current_desc.take()) {
        let is_removable = is_removable_entry(&description);
        entries.push(FirmwareBootEntry {
            id,
            description,
            is_removable,
        });
    }
    // If only id or only desc exists, discard (incomplete entry).
    let _ = current_id.take();
    let _ = current_desc.take();
}

/// Heuristic: does this description look like a removable / USB boot entry?
fn is_removable_entry(description: &str) -> bool {
    let lower = description.to_lowercase();
    lower.contains("usb")
        || lower.contains("removable")
        || lower.contains("external")
        || lower.contains("flash")
        || lower.contains("thumb")
        || lower.contains("wipe")
}

/// Return the first firmware boot entry that looks like removable/USB media.
/// Returns `None` when no entry matches — the caller should surface this to
/// the user ("provision a Wipe USB and ensure it appears in UEFI boot entries").
pub fn enum_usb_boot_entry() -> Result<Option<FirmwareBootEntry>, String> {
    let all = enum_firmware_boot_entries()?;
    Ok(all.into_iter().find(|e| e.is_removable))
}

// ── Privilege setup ──────────────────────────────────────────────────────────

/// Enable `SE_SYSTEM_ENVIRONMENT_NAME` on the current process token.
///
/// This privilege is present but DISABLED by default in a process token even
/// when running elevated as Administrator — being elevated is not enough.
/// `GetFirmwareEnvironmentVariableW`/`SetFirmwareEnvironmentVariableW` fail
/// with `ERROR_PRIVILEGE_NOT_HELD` (1314) unless it is explicitly turned on
/// via `AdjustTokenPrivileges` first. Must be called before any raw UEFI
/// variable Win32 call in this module.
#[cfg(windows)]
fn enable_se_system_environment_privilege() -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, LUID};
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        ) == 0
        {
            return Err(format!("OpenProcessToken failed: error {}", GetLastError()));
        }

        let priv_name: Vec<u16> = OsStr::new("SeSystemEnvironmentPrivilege")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut luid: LUID = std::mem::zeroed();
        if LookupPrivilegeValueW(std::ptr::null(), priv_name.as_ptr(), &mut luid) == 0 {
            let err = GetLastError();
            CloseHandle(token);
            return Err(format!("LookupPrivilegeValueW failed: error {err}"));
        }

        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };

        let ok =
            AdjustTokenPrivileges(token, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut());
        let adjust_err = GetLastError();
        CloseHandle(token);

        if ok == 0 {
            return Err(format!("AdjustTokenPrivileges failed: error {adjust_err}"));
        }
        // AdjustTokenPrivileges can succeed (TRUE) yet leave ERROR_NOT_ALL_ASSIGNED
        // (1300) when the token doesn't actually hold the privilege at all.
        if adjust_err == 1300 {
            return Err(
                "AdjustTokenPrivileges: SeSystemEnvironmentPrivilege not held (run as Administrator)"
                    .to_string(),
            );
        }
    }
    Ok(())
}

// ── UEFI BootNext variable I/O ──────────────────────────────────────────────

/// Set the UEFI `BootNext` variable to the firmware entry identified by `entry_id`.
///
/// `entry_id` must be a GUID-format identifier as returned by
/// `enum_firmware_boot_entries` / `enum_usb_boot_entry`, e.g.
/// `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`.
///
/// `BootNext` is a one-shot, 2-byte LE integer: the `BootXXXX` entry index.
/// We derive the index from the hex suffix of the entry's `BootXXXX` variable
/// name.  However, bcdedit uses GUID identifiers — not `BootXXXX` indices.
/// We therefore use `bcdedit /set {fwbootmgr} bootsequence <entry_id> /addfirst`
/// which is the correct Windows API path for setting boot priority without
/// directly manipulating `BootNext` (which requires the raw u16 index).
///
/// # NOT auto-invoked
///
/// This function is NOT called from any trigger path, lockdown path, or
/// startup path.  The F6 orchestrator (Piece 3) will call it explicitly
/// after stage-1 crypto-erase succeeds.
///
/// # Returns
///
/// `Ok(entry_id)` on success; `Err(String)` on bcdedit or privilege failure.
pub fn set_boot_next_usb(entry_id: &str) -> Result<String, String> {
    // Validate entry_id is GUID-shaped to prevent injection
    // (bcdedit only accepts {guid} format — anything else returns an error).
    if !entry_id.starts_with('{') || !entry_id.ends_with('}') {
        return Err(format!(
            "set_boot_next_usb: entry_id must be a GUID in {{}} braces, got: {}",
            entry_id
        ));
    }
    // Additional character-level validation: only allow hex digits, hyphens, braces.
    let inner = &entry_id[1..entry_id.len() - 1];
    if !inner.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err(format!(
            "set_boot_next_usb: entry_id contains unexpected characters: {}",
            entry_id
        ));
    }

    // bcdedit /set {fwbootmgr} bootsequence <id> /addfirst
    // This is the correct mechanism on Windows UEFI to add an entry at the
    // front of the one-time boot sequence (equivalent to setting BootNext).
    // It requires SE_SYSTEM_ENVIRONMENT_NAME (Administrator).
    let output = Command::new("bcdedit")
        .args(["/set", "{fwbootmgr}", "bootsequence", entry_id, "/addfirst"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("bcdedit set bootsequence exec: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "bcdedit /set {{fwbootmgr}} bootsequence failed: {stderr}{stdout}"
        ));
    }

    crate::log_message(
        "info",
        &format!(
            "[F6-P1P2] BootNext set to USB entry {} (one-shot; no reboot triggered)",
            entry_id
        ),
    );
    Ok(entry_id.to_string())
}

/// Read the current UEFI BootNext value via the EFI global variable namespace.
///
/// Returns `Ok("0xXXXX")` with the hex boot entry index, or `Ok("none")` if
/// BootNext is not set.  Uses `GetFirmwareEnvironmentVariableW`.
///
/// Requires `SE_SYSTEM_ENVIRONMENT_NAME` (Administrator).
pub fn get_boot_next() -> Result<String, String> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::System::WindowsProgramming::GetFirmwareEnvironmentVariableW;

        enable_se_system_environment_privilege()?;

        // Wide NUL-terminated strings — chain(Some(0)) appends the NUL terminator.
        let name: Vec<u16> = OsStr::new("BootNext")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let guid: Vec<u16> = OsStr::new(EFI_GLOBAL_GUID)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut buf = [0u8; 2]; // BootNext is 2 bytes (u16 LE)
        let ret = unsafe {
            GetFirmwareEnvironmentVariableW(
                name.as_ptr(),
                guid.as_ptr(),
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                buf.len() as u32,
            )
        };
        if ret == 0 {
            // ERROR_ENVVAR_NOT_FOUND (203) = BootNext not set — that's normal.
            let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            if err == 203 {
                return Ok("none".to_string());
            }
            return Err(format!(
                "GetFirmwareEnvironmentVariableW(BootNext) failed: error {err}"
            ));
        }
        let index = u16::from_le_bytes(buf);
        Ok(format!("0x{:04X}", index))
    }
    #[cfg(not(windows))]
    {
        Err("get_boot_next: UEFI variable access is Windows-only".to_string())
    }
}

/// Clear the UEFI BootNext variable (delete it, so normal BootOrder is used).
///
/// Uses `SetFirmwareEnvironmentVariableW` with a zero-length value, which
/// deletes the EFI variable.  Falls back to removing the entry from
/// `{fwbootmgr}` bootsequence via bcdedit.
///
/// Requires `SE_SYSTEM_ENVIRONMENT_NAME` (Administrator).
///
/// Returns `Ok(())` on success or when BootNext was already absent.
pub fn clear_boot_next() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::System::WindowsProgramming::SetFirmwareEnvironmentVariableW;

        // If we can't get the privilege, don't fail the whole clear — fall
        // through to the bcdedit fallback below, which runs as a separate
        // elevated process and doesn't need this process's token adjusted.
        if let Err(e) = enable_se_system_environment_privilege() {
            crate::log_message(
                "warn",
                &format!(
                    "[F6-P1P2] enable SeSystemEnvironmentPrivilege failed: {e}; trying EFI delete anyway, will fall back to bcdedit if needed"
                ),
            );
        }

        // Wide NUL-terminated strings.
        let name: Vec<u16> = OsStr::new("BootNext")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let guid: Vec<u16> = OsStr::new(EFI_GLOBAL_GUID)
            .encode_wide()
            .chain(Some(0))
            .collect();
        // Writing a zero-length buffer deletes the EFI variable.
        let ret = unsafe {
            SetFirmwareEnvironmentVariableW(
                name.as_ptr(),
                guid.as_ptr(),
                std::ptr::null::<core::ffi::c_void>(),
                0,
            )
        };
        if ret == 0 {
            let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            // ERROR_ENVVAR_NOT_FOUND = variable already absent — treat as success.
            if err == 203 {
                return Ok(());
            }
            // If the UEFI delete fails, try bcdedit as fallback.
            crate::log_message(
                "warn",
                &format!(
                    "[F6-P1P2] SetFirmwareEnvironmentVariableW delete BootNext error {err}; trying bcdedit fallback"
                ),
            );
        } else {
            crate::log_message("info", "[F6-P1P2] BootNext cleared via EFI variable delete");
            return Ok(());
        }

        // Fallback: clear the one-time boot sequence in {fwbootmgr}.
        let out = Command::new("bcdedit")
            .args(["/deletevalue", "{fwbootmgr}", "bootsequence"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("bcdedit deletevalue exec: {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // "The specified entry is not found" is acceptable (already cleared).
            if !stderr.contains("not found") && !stderr.contains("not present") {
                return Err(format!(
                    "bcdedit /deletevalue {{fwbootmgr}} bootsequence failed: {stderr}"
                ));
            }
        }
        crate::log_message("info", "[F6-P1P2] BootNext cleared via bcdedit deletevalue");
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("clear_boot_next: UEFI variable access is Windows-only".to_string())
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// List firmware boot entries — so the UI can show what entries exist and
/// whether a USB/removable entry was detected.
///
/// Paid + needsAdmin (bcdedit requires Administrator).
/// NOT auto-invoked from any lockdown/trigger path.
#[tauri::command]
pub fn f6_get_boot_next_usb_entry() -> Result<Value, String> {
    crate::license::require_paid("F6 BootNext USB")?;
    let all = enum_firmware_boot_entries()?;
    let usb = all.iter().find(|e| e.is_removable);
    Ok(json!({
        "allEntries": all.iter().map(|e| json!({
            "id": e.id,
            "description": e.description,
            "isRemovable": e.is_removable,
        })).collect::<Vec<_>>(),
        "usbEntry": usb.map(|e| json!({
            "id": e.id,
            "description": e.description,
        })),
    }))
}

/// Set UEFI BootNext to the detected USB/removable firmware boot entry.
///
/// This is a REVERSIBLE, SAFE, ONE-SHOT operation — it tells the UEFI to
/// boot from USB on the next reboot only.  No reboot is triggered here.
/// Paid + needsAdmin.  NOT auto-invoked anywhere.
#[tauri::command]
pub fn f6_set_boot_next_usb() -> Result<Value, String> {
    crate::license::require_paid("F6 BootNext USB")?;
    let entry = enum_usb_boot_entry()?.ok_or_else(|| {
        "no USB/removable firmware boot entry found; insert the Wipe USB and ensure it appears in UEFI boot menu".to_string()
    })?;
    let id = set_boot_next_usb(&entry.id)?;
    Ok(json!({ "ok": true, "entryId": id, "description": entry.description }))
}

/// Read the current UEFI BootNext value.  Paid + needsAdmin.
#[tauri::command]
pub fn f6_get_boot_next() -> Result<Value, String> {
    crate::license::require_paid("F6 BootNext USB")?;
    let value = get_boot_next()?;
    Ok(json!({ "bootNext": value }))
}

/// Clear the UEFI BootNext variable, restoring normal boot order.
/// Paid + needsAdmin.  Safe — does not reboot.
#[tauri::command]
pub fn f6_clear_boot_next() -> Result<Value, String> {
    crate::license::require_paid("F6 BootNext USB")?;
    clear_boot_next()?;
    Ok(json!({ "ok": true, "bootNext": "none" }))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_bcdedit_firmware_entries ────────────────────────────────────

    const SAMPLE_OUTPUT: &str = r#"
Firmware Boot Manager
---------------------
identifier              {fwbootmgr}
displayorder            {bootmgr}
                        {aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}
                        {11111111-2222-3333-4444-555555555555}

Windows Boot Manager
--------------------
identifier              {bootmgr}
device                  partition=\Device\HarddiskVolume2
description             Windows Boot Manager
locale                  en-US

Firmware Application (101fffff)
--------------------------------
identifier              {aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}
device                  partition=\Device\HarddiskVolume2
path                    \EFI\BOOT\BOOTX64.EFI
description             WinCommander Wipe USB

Firmware Application (101fffff)
--------------------------------
identifier              {11111111-2222-3333-4444-555555555555}
device                  harddisk
path                    \EFI\Microsoft\Boot\bootmgfw.efi
description             Internal SSD

"#;

    #[test]
    fn parses_firmware_entries() {
        let entries = parse_bcdedit_firmware_entries(SAMPLE_OUTPUT).expect("parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "{aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}");
        assert_eq!(entries[0].description, "WinCommander Wipe USB");
        assert!(
            entries[0].is_removable,
            "wipe USB must be flagged removable"
        );
        assert_eq!(entries[1].id, "{11111111-2222-3333-4444-555555555555}");
        assert_eq!(entries[1].description, "Internal SSD");
        assert!(
            !entries[1].is_removable,
            "internal SSD must NOT be removable"
        );
    }

    #[test]
    fn finds_usb_entry() {
        let entries = parse_bcdedit_firmware_entries(SAMPLE_OUTPUT).expect("parse");
        let usb = entries.into_iter().find(|e| e.is_removable);
        assert!(usb.is_some(), "must find the USB entry");
        assert_eq!(usb.unwrap().id, "{aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}");
    }

    #[test]
    fn returns_error_on_empty_output() {
        let result = parse_bcdedit_firmware_entries("");
        assert!(result.is_err(), "empty output must return Err");
    }

    #[test]
    fn is_removable_matches_usb_keywords() {
        assert!(is_removable_entry("USB Storage Device"));
        assert!(is_removable_entry("Removable Disk"));
        assert!(is_removable_entry("External Drive"));
        assert!(is_removable_entry("Flash Drive"));
        assert!(is_removable_entry("WinCommander Wipe USB"));
    }

    #[test]
    fn is_removable_rejects_internal() {
        assert!(!is_removable_entry("Windows Boot Manager"));
        assert!(!is_removable_entry("Internal SSD"));
        assert!(!is_removable_entry("NVMe Drive"));
        assert!(!is_removable_entry("SATA HDD"));
    }

    // ── set_boot_next_usb input validation ───────────────────────────────

    #[test]
    fn set_boot_next_usb_rejects_non_guid() {
        // Should reject without calling bcdedit
        let result = set_boot_next_usb("not-a-guid");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("GUID"), "error must mention GUID: {err}");
    }

    #[test]
    fn set_boot_next_usb_rejects_injection() {
        // Shell injection attempt inside braces
        let result = set_boot_next_usb("{; rm -rf /}");
        assert!(result.is_err());
    }

    #[test]
    fn set_boot_next_usb_accepts_valid_guid_format() {
        // This will fail at bcdedit (no real UEFI on test runner) but must
        // NOT fail at the validation layer. The error must mention "bcdedit"
        // or "bootsequence" — not "GUID".
        let result = set_boot_next_usb("{aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}");
        // On a UEFI machine with Administrator this would succeed.
        // In CI (no UEFI) bcdedit fails — that's acceptable.
        match &result {
            Ok(_) => {} // UEFI machine — fine
            Err(e) => {
                // Must NOT be the GUID validation error
                assert!(
                    !e.contains("GUID"),
                    "validation must pass for a well-formed GUID; got: {e}"
                );
            }
        }
    }
}
