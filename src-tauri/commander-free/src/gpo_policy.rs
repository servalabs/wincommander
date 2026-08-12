// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/gpo_policy.rs
//
// F9 — Group Policy / ADMX support (Free-side, read-only overlay)
// ──────────────────────────────────────────────────────────────────
// WinCommander reads managed policy from the standard GPO Policies hive:
//
//   HKLM\SOFTWARE\Policies\ServaLabs\WinCommander
//
// An AD admin can set these values via Group Policy (using commander.admx +
// en-US/commander.adml from resources/gpo/) or by pushing registry values
// with any MDM/RMM tool that writes to the Policies hive.
//
// v1 scope: READ and SURFACE the managed policy (read-only overlay + UI
// indicator).  Hard enforcement (locking individual toggles in the settings
// engine so the user cannot override them) is a documented phase-2 follow-on.
// This file deliberately does NOT wire up any enforcement: the UI indicator
// and the `managed` flag are informational only.
//
// ── Recognised policy value names ───────────────────────────────────────────
//
//   LockTelemetryOff       DWORD  (0/1 → bool)  Force all Windows telemetry
//                                                settings to "off" and prevent
//                                                the user from re-enabling them.
//                                                Phase-2: locks the toggle.
//
//   ForceSecureDNS         REG_SZ (string)       Override the DNS-over-HTTPS
//                                                resolver URL (e.g.
//                                                "https://1.1.1.1/dns-query").
//                                                Phase-2: replaces the user
//                                                configurable resolver.
//
//   DisableSelfDestruct    DWORD  (0/1 → bool)  Prevent the panic / self-
//                                                destruct flow from firing.
//                                                Phase-2: locks the button.
//
//   RequireStartupPin      DWORD  (0/1 → bool)  Force the startup PIN gate to
//                                                be enabled on every launch.
//                                                Phase-2: greys out "disable
//                                                PIN" in settings.
//
//   ManagedBannerText      REG_SZ (string)       Optional custom organisation
//                                                message shown in the managed-
//                                                policy banner (e.g. company
//                                                name or IT helpdesk contact).
//
// Adding a new policy value requires only:
//   1. Add its name to RECOGNISED_POLICY_KEYS below.
//   2. Document it in this header.
//   3. Add a <policy> element in resources/gpo/commander.admx and the
//      matching <string> entries in en-US/commander.adml.
//
// ── Phase-2 note ────────────────────────────────────────────────────────────
// Enforcement (toggling UI elements to read-only, blocking Tauri command
// handlers for locked settings) MUST NOT be half-wired in this file.  The
// correct approach for phase-2 is:
//   - settings::is_setting_locked() should call get_managed_policy() and check
//     whether a specific value overrides the requested setting.
//   - The frontend checks is_setting_locked() before rendering each toggle and
//     replaces the control with a locked icon + "managed by org" tooltip.
// None of that wiring exists yet; this comment is the SSOT for that plan.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Recognised policy value names ────────────────────────────────────────────

/// Stable, documented set of registry value names that WinCommander reads
/// from the Policies hive.  Values outside this list are silently ignored.
/// Extend this constant (and the ADMX template) to add new policies.
const RECOGNISED_POLICY_KEYS: &[&str] = &[
    "LockTelemetryOff",
    "ForceSecureDNS",
    "DisableSelfDestruct",
    "RequireStartupPin",
    "ManagedBannerText",
];

// ── Public types ─────────────────────────────────────────────────────────────

/// Serialised result returned by `get_managed_policy`.
///
/// `managed` is `true` iff at least one recognised policy value was present
/// in the registry.  When `managed` is `false` the UI shows nothing.
///
/// `source` is a human-readable string indicating where the policy came from
/// (e.g. `"HKLM\\SOFTWARE\\Policies\\ServaLabs\\WinCommander"`).  It is the
/// empty string when no policy is present or on non-Windows.
///
/// `values` is a camelCase BTreeMap of all recognised keys that were found and
/// their normalised values:
///   - DWORD 0/1  → `serde_json::Value::Bool(false/true)`
///   - REG_SZ     → `serde_json::Value::String(...)`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct ManagedPolicy {
    pub managed: bool,
    pub source: String,
    pub values: BTreeMap<String, serde_json::Value>,
}

// ── Registry path ─────────────────────────────────────────────────────────────

const POLICY_SOURCE: &str = "HKLM\\SOFTWARE\\Policies\\ServaLabs\\WinCommander";

/// Registry subkey relative to HKLM.
const POLICY_SUBKEY: &str = "SOFTWARE\\Policies\\ServaLabs\\WinCommander";

// ── Pure, unit-testable core ─────────────────────────────────────────────────

/// Normalise raw registry reads into a typed `ManagedPolicy`.
///
/// Rules:
///   - Values whose name is not in `RECOGNISED_POLICY_KEYS` are dropped.
///   - Values that are already a `serde_json::Value::Bool` (DWORD normalised
///     by the caller to bool) pass through unchanged.
///   - Values that are `serde_json::Value::Number(n)` are treated as DWORDs:
///     `n != 0 → true`, `0 → false`.
///   - Values that are `serde_json::Value::String` pass through unchanged
///     (REG_SZ).
///   - Any other JSON variant is dropped.
///
/// `managed` is set to `true` iff at least one recognised key is present after
/// filtering.
pub fn parse_policy(values: &[(String, serde_json::Value)]) -> ManagedPolicy {
    let mut map: BTreeMap<String, serde_json::Value> = BTreeMap::new();

    for (name, raw) in values {
        // Only accept values we explicitly recognise.
        if !RECOGNISED_POLICY_KEYS.contains(&name.as_str()) {
            continue;
        }

        let normalised = match raw {
            serde_json::Value::Bool(b) => serde_json::Value::Bool(*b),
            serde_json::Value::Number(n) => {
                // DWORD: any non-zero value → true.
                let as_u = n.as_u64().unwrap_or_else(|| n.as_i64().unwrap_or(0) as u64);
                serde_json::Value::Bool(as_u != 0)
            }
            serde_json::Value::String(s) => serde_json::Value::String(s.clone()),
            _ => continue, // null, array, object — not a valid registry type
        };

        map.insert(name.clone(), normalised);
    }

    let managed = !map.is_empty();
    ManagedPolicy {
        managed,
        source: if managed {
            POLICY_SOURCE.to_string()
        } else {
            String::new()
        },
        values: map,
    }
}

// ── Registry reader ───────────────────────────────────────────────────────────

/// Read all values from the Policies hive key.  Returns an empty `Vec` if the
/// key is absent (machine is unmanaged) or on non-Windows platforms.
///
/// Uses `windows-sys` via `RegOpenKeyExW` / `RegEnumValueW` / `RegQueryValueExW`
/// — the same crate and pattern already used throughout `runtime_visibility::registry`.
///
/// DWORDs are returned as `serde_json::Value::Number`.
/// REG_SZ / REG_EXPAND_SZ are returned as `serde_json::Value::String`.
/// All other registry types are skipped (not exposed as policy values).
pub fn read_registry_policy() -> Vec<(String, serde_json::Value)> {
    #[cfg(windows)]
    {
        read_registry_policy_windows()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn from_wide_nul(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(windows)]
fn read_registry_policy_windows() -> Vec<(String, serde_json::Value)> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD,
        REG_EXPAND_SZ, REG_SZ,
    };

    let mut out: Vec<(String, serde_json::Value)> = Vec::new();

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let wide_sub = to_wide(POLICY_SUBKEY);
        let rc = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            wide_sub.as_ptr(),
            0,
            KEY_READ,
            &mut hkey,
        );
        if rc != ERROR_SUCCESS {
            // Key absent — unmanaged machine.
            return out;
        }

        let mut index: u32 = 0;
        loop {
            let mut name_buf: [u16; 256] = [0u16; 256];
            let mut name_len: u32 = name_buf.len() as u32;
            // Use a generous data buffer; policy strings are short.
            let mut data_buf: [u8; 4096] = [0u8; 4096];
            let mut data_len: u32 = data_buf.len() as u32;
            let mut value_type: u32 = 0;

            let rc = RegEnumValueW(
                hkey,
                index,
                name_buf.as_mut_ptr(),
                &mut name_len,
                std::ptr::null_mut(),
                &mut value_type,
                data_buf.as_mut_ptr(),
                &mut data_len,
            );
            if rc != ERROR_SUCCESS {
                break; // ERROR_NO_MORE_ITEMS (or error) — done.
            }
            index += 1;

            let name = from_wide_nul(&name_buf[..name_len as usize]);

            match value_type {
                REG_SZ | REG_EXPAND_SZ => {
                    let char_count = (data_len as usize) / 2;
                    let slice =
                        std::slice::from_raw_parts(data_buf.as_ptr() as *const u16, char_count);
                    let s = from_wide_nul(slice);
                    out.push((name, serde_json::Value::String(s)));
                }
                REG_DWORD if data_len >= 4 => {
                    let v =
                        u32::from_le_bytes([data_buf[0], data_buf[1], data_buf[2], data_buf[3]]);
                    out.push((name, serde_json::Value::Number(v.into())));
                }
                _ => {
                    // Binary, QWORD, multi-sz, etc. — not used as policy values.
                }
            }
        }

        RegCloseKey(hkey);
    }

    out
}

// ── Tauri command ─────────────────────────────────────────────────────────────

/// Read the managed policy overlay from the Group Policy registry hive and
/// return a normalised `ManagedPolicy`.
///
/// This command is ungated (no `require_paid`): the presence or absence of
/// managed policy is always visible to the user so they know whether their
/// organisation controls any settings.
#[tauri::command]
pub fn get_managed_policy() -> ManagedPolicy {
    let raw = read_registry_policy();
    parse_policy(&raw)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kv(name: &str, v: serde_json::Value) -> (String, serde_json::Value) {
        (name.to_string(), v)
    }

    #[test]
    fn empty_input_yields_not_managed() {
        let p = parse_policy(&[]);
        assert!(!p.managed);
        assert!(p.values.is_empty());
        assert!(p.source.is_empty());
    }

    #[test]
    fn dword_one_becomes_true() {
        let input = [kv("LockTelemetryOff", json!(1u32))];
        let p = parse_policy(&input);
        assert!(p.managed);
        assert_eq!(p.values["LockTelemetryOff"], json!(true));
    }

    #[test]
    fn dword_zero_becomes_false() {
        let input = [kv("DisableSelfDestruct", json!(0u32))];
        let p = parse_policy(&input);
        assert!(p.managed);
        assert_eq!(p.values["DisableSelfDestruct"], json!(false));
    }

    #[test]
    fn reg_sz_passes_through_as_string() {
        let input = [kv("ForceSecureDNS", json!("https://1.1.1.1/dns-query"))];
        let p = parse_policy(&input);
        assert!(p.managed);
        assert_eq!(
            p.values["ForceSecureDNS"],
            json!("https://1.1.1.1/dns-query")
        );
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let input = [
            kv("LockTelemetryOff", json!(1u32)),
            kv("SomeUnknownFuturePolicyKey", json!(1u32)),
        ];
        let p = parse_policy(&input);
        assert!(p.managed);
        assert_eq!(p.values.len(), 1);
        assert!(p.values.contains_key("LockTelemetryOff"));
        assert!(!p.values.contains_key("SomeUnknownFuturePolicyKey"));
    }

    #[test]
    fn mixed_types_all_recognised() {
        let input = [
            kv("LockTelemetryOff", json!(1u32)),
            kv("ForceSecureDNS", json!("https://1.1.1.1/dns-query")),
            kv("DisableSelfDestruct", json!(0u32)),
            kv("RequireStartupPin", json!(1u32)),
            kv("ManagedBannerText", json!("Contact IT: helpdesk@acme.com")),
        ];
        let p = parse_policy(&input);
        assert!(p.managed);
        assert_eq!(p.values.len(), 5);
        assert_eq!(p.values["LockTelemetryOff"], json!(true));
        assert_eq!(p.values["DisableSelfDestruct"], json!(false));
        assert_eq!(p.values["RequireStartupPin"], json!(true));
        assert_eq!(
            p.values["ManagedBannerText"],
            json!("Contact IT: helpdesk@acme.com")
        );
    }

    #[test]
    fn bool_variant_passes_through() {
        // Covers the case where the caller pre-normalised a DWORD to bool.
        let input = [kv("RequireStartupPin", json!(true))];
        let p = parse_policy(&input);
        assert!(p.managed);
        assert_eq!(p.values["RequireStartupPin"], json!(true));
    }

    #[test]
    fn source_populated_only_when_managed() {
        let managed = parse_policy(&[kv("LockTelemetryOff", json!(1u32))]);
        assert!(!managed.source.is_empty());

        let not_managed = parse_policy(&[]);
        assert!(not_managed.source.is_empty());
    }
}
