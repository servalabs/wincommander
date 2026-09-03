// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/selective_erase.rs
//
// Track A — in-Windows SELECTIVE crypto-erase of ONE encrypted container.
// Surgical: reuses the existing Pro erase handlers for a single resolved
// target and keeps the OS bootable. NEVER routes through the lockdown cascade
// (full_lockdown / lockdown / run_destruct_step). Server-side OS-volume
// derivation and receipt-honesty live here as PURE, unit-tested functions.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EraseTargetInput {
    /// "veracrypt" | "bitlocker"
    pub kind: String,
    /// VeraCrypt: backing container/device path (the erase target).
    pub path: Option<String>,
    /// VeraCrypt: current mount letter (dismount first if present).
    pub mount_letter: Option<String>,
    /// BitLocker: mount point, e.g. "D:".
    pub mount_point: Option<String>,
    /// Baseline checkbox acknowledgement.
    pub confirmed: bool,
    /// Typed identifier from the nuclear ceremony; present only for OS targets.
    pub os_volume_ack: Option<String>,
}

fn capture_identity(
    target: &EraseTargetInput,
) -> Result<wincmd_shared::DestructiveRequestV2, String> {
    match target.kind.trim().to_ascii_lowercase().as_str() {
        "veracrypt" => crate::path_identity::ExpectedFileIdentity::capture(std::path::Path::new(
            target
                .path
                .as_deref()
                .ok_or_else(|| "veracrypt target requires a path".to_string())?,
        ))
        .map(|identity| identity.request()),
        "bitlocker" => crate::path_identity::bitlocker_identity(
            target
                .mount_point
                .as_deref()
                .ok_or_else(|| "bitlocker target requires a mount point".to_string())?,
        ),
        other => Err(format!("unknown target kind: {other}")),
    }
}

pub fn canonical_erase_args(target: &EraseTargetInput) -> Result<String, String> {
    let identity = capture_identity(target)?;
    canonical_erase_args_with_identity(target, &identity)
}

fn canonical_erase_args_with_identity(
    target: &EraseTargetInput,
    identity: &wincmd_shared::DestructiveRequestV2,
) -> Result<String, String> {
    let path = target.path.as_deref().map(crate::authz::canonical_path);
    serde_json::to_string(&(
        "erase_encrypted_container",
        target.kind.trim().to_ascii_lowercase(),
        path,
        identity,
        target.mount_letter.as_deref().map(norm_drive),
        target.mount_point.as_deref().map(norm_drive),
        is_os_target(
            &target.kind,
            target.mount_point.as_deref(),
            target.path.as_deref(),
            &system_drive(),
        ),
        identity,
    ))
    .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EraseReceipt {
    pub kind: String,
    pub label: String,
    pub action: String,
    /// "erased" | "erased_with_caveat" | "failed"
    pub status: String,
    pub verified: bool,
    pub escrow_warning: Option<String>,
    pub recovery_protectors_remaining: Option<i64>,
    pub key_evicted: bool,
    pub detail: String,
}

/// Windows system drive, e.g. "C:". Falls back to "C:" if the env var is absent.
pub fn system_drive() -> String {
    std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string())
}

/// Normalize a drive spec ("C:\\", "c:", " C: ") to "C:".
pub fn norm_drive(s: &str) -> String {
    s.trim().trim_end_matches('\\').to_ascii_uppercase()
}

/// True when a VeraCrypt path targets a RAW device (whole-disk, partition,
/// volume, or system-drive device path) rather than a plain file container.
/// Fail-closed: any `\\.\` / `\\?\` device-namespace path is treated as
/// system/OS-class (requires the nuclear ack) because we cannot cheaply prove
/// it is not the boot disk. A normal file container ("D:\\vault.hc") is data.
fn is_system_device_path(path: &str, sys_drive: &str) -> bool {
    let p = path.trim().to_ascii_uppercase();
    let sys = norm_drive(sys_drive);
    if p == format!("\\\\.\\{sys}") || p == format!("\\\\?\\{sys}") {
        return true;
    }
    p.starts_with("\\\\.\\") || p.starts_with("\\\\?\\")
}

/// Server-side OS/system-volume derivation — NEVER trusts a client flag.
/// BitLocker: the target mount point IS the system drive. VeraCrypt: the path
/// is a raw device path for the system drive. Unknown/garbled ⇒ true
/// (fail-safe: engage the nuclear gate rather than silently treat as data).
pub fn is_os_target(
    kind: &str,
    mount_point: Option<&str>,
    path: Option<&str>,
    sys_drive: &str,
) -> bool {
    let sys = norm_drive(sys_drive);
    match kind {
        "bitlocker" => match mount_point {
            Some(mp) => norm_drive(mp) == sys,
            None => true,
        },
        "veracrypt" => match path {
            Some(p) => is_system_device_path(p, &sys),
            None => true,
        },
        _ => true,
    }
}

/// Build the receipt from a Pro handler's JSON result. HONESTY INVARIANT: a
/// non-empty BitLocker escrow warning (or any remaining recovery protector)
/// can NEVER produce status "erased" — the escrowed key would still unlock it.
pub fn map_receipt(
    kind: &str,
    label: &str,
    handler: &Value,
    key_evicted: bool,
    is_os: bool,
) -> EraseReceipt {
    match kind {
        "veracrypt" => {
            let ok = handler.get("status").and_then(Value::as_str) == Some("destroyed");
            EraseReceipt {
                kind: kind.to_string(),
                label: label.to_string(),
                action: "veracrypt_header_destroy".to_string(),
                status: if ok { "erased" } else { "failed" }.to_string(),
                verified: ok,
                escrow_warning: None,
                recovery_protectors_remaining: None,
                key_evicted,
                detail: if ok {
                    // Honesty: the overwrite was issued + flushed; we do NOT
                    // independently re-mount to verify (see ROADMAP A3 item).
                    "Header (primary + backup) overwritten with random data and flushed; the container can no longer be mounted with its password. (Overwrite issued + flushed; not independently re-mounted to verify.)".to_string()
                } else {
                    format!("VeraCrypt header destroy did not confirm success: {handler}")
                },
            }
        }
        "bitlocker" => {
            let ran_ok = handler.get("status").and_then(Value::as_str) == Some("ok");
            let warn = handler
                .get("escrow_warning")
                .and_then(Value::as_str)
                .unwrap_or("");
            let remaining = handler
                .get("recovery_protectors_remaining")
                .and_then(Value::as_i64);
            let escrow_clean = warn.is_empty() && remaining == Some(0);
            // HONESTY INVARIANT: never claim "erased" when data may survive —
            // whether via an escrowed recovery key OR a still-resident FVEK.
            let (status, detail): (&str, String) = if !ran_ok {
                (
                    "failed",
                    format!("BitLocker protector removal failed: {handler}"),
                )
            } else if !escrow_clean {
                ("erased_with_caveat",
                 "Protectors removed, but a recovery key may survive (escrowed to AD/Entra, or protectors remain) — NOT guaranteed unrecoverable.".to_string())
            } else if is_os {
                // The live OS drive can't be locked; the master key stays in
                // memory until the machine powers off. Not an immediate lock.
                ("erased",
                 "Key protectors removed. The machine will not boot after restart, and the data becomes unrecoverable once this session ends — the drive's key stays in memory until then.".to_string())
            } else if key_evicted {
                ("erased",
                 "All key protectors removed and the volume locked (key evicted from memory); without an escrowed recovery key the data is unrecoverable.".to_string())
            } else {
                // Data volume, protectors gone, but the lock/eviction did not
                // confirm — the key is still resident and the volume readable.
                ("erased_with_caveat",
                 "Protectors removed, but the volume's key could not be evicted from memory — it stays readable until you reboot or dismount it, and is unrecoverable after that.".to_string())
            };
            EraseReceipt {
                kind: kind.to_string(),
                label: label.to_string(),
                action: "bitlocker_protectors_removed".to_string(),
                status: status.to_string(),
                verified: ran_ok && remaining == Some(0),
                escrow_warning: if warn.is_empty() {
                    None
                } else {
                    Some(warn.to_string())
                },
                recovery_protectors_remaining: remaining,
                key_evicted,
                detail,
            }
        }
        _ => EraseReceipt {
            kind: kind.to_string(),
            label: label.to_string(),
            action: "unknown".to_string(),
            status: "failed".to_string(),
            verified: false,
            escrow_warning: None,
            recovery_protectors_remaining: None,
            key_evicted,
            detail: "unknown target kind".to_string(),
        },
    }
}

/// How much of the container's start we read back to independently verify a
/// VeraCrypt header destroy. Matches the documented ~128 KiB primary+backup
/// overwrite window the Pro handler issues. Only the PRIMARY header (start of
/// file) is checked this way — a VeraCrypt backup header near the end of a
/// larger volume is not independently re-read here — a further, separate gap
/// tracked in the internal Free-client weaknesses ledger.
const HEADER_VERIFY_BYTES: usize = 128 * 1024;

/// Best-effort read of the container's header window. `None` on any I/O
/// failure (missing file, permission, moved) or an empty file — verification
/// degrades gracefully rather than blocking the erase result on a read error.
fn read_header_prefix(path: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; HEADER_VERIFY_BYTES];
    let n = f.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    buf.truncate(n);
    Some(buf)
}

/// Independent post-erase verification for VeraCrypt — this closes the "no
/// INDEPENDENT post-erase verification" gap. The Pro handler's "destroyed"
/// status is a self-report of the overwrite it issued; this compares the
/// on-disk header bytes read before vs. after the call and downgrades the
/// receipt when they are unchanged, rather than trusting the self-report
/// alone. Pure and unit-tested — no file I/O here.
fn apply_header_verification(receipt: &mut EraseReceipt, pre: Option<&[u8]>, post: Option<&[u8]>) {
    if receipt.status != "erased" {
        return; // already failed/caveat — nothing to add.
    }
    match (pre, post) {
        (Some(a), Some(b)) if a == b => {
            receipt.verified = false;
            receipt.status = "erased_with_caveat".to_string();
            receipt.detail = format!(
                "{} INDEPENDENT VERIFICATION FAILED: the on-disk header bytes are unchanged from before the erase call — do not treat this container as confirmed erased.",
                receipt.detail
            );
        }
        (Some(_), Some(_)) => {
            receipt.verified = true;
            receipt.detail = format!(
                "{} Independently verified: the on-disk header bytes changed after the erase call.",
                receipt.detail
            );
        }
        _ => {
            receipt.detail = format!(
                "{} Independent verification unavailable (could not read the container header before/after the call); relying on the handler's self-report only.",
                receipt.detail
            );
        }
    }
}

/// Crypto-erase ONE encrypted container. Paid + Secure-persona surface. This is
/// the ONLY entry point and it never touches the lockdown cascade — it dispatches
/// exactly one Pro erase for one resolved target.
///
/// GATE: `require_paid()` below is the source of truth (per AGENTS.md). A native
/// Tauri command like this is never routed through `run_backend_script`, so it
/// needs no `get_command_tier` arm — that table is only consulted for PS commands.
#[tauri::command]
pub async fn erase_encrypted_container(
    target: EraseTargetInput,
    capability_token: Option<String>,
) -> Result<EraseReceipt, String> {
    crate::license::require_paid("selective crypto-erase")?;

    // An examiner must never crypto-erase a seized device.
    if crate::license::is_advanced_mode() {
        return Err("Refused: investigator mode does not crypto-erase containers.".to_string());
    }
    if !target.confirmed {
        return Err("Refusing: erase not confirmed.".to_string());
    }
    let destructive_identity = capture_identity(&target)?;
    crate::authz::consume_required(
        capability_token.as_deref(),
        crate::authz::DestructiveAction::CryptoErase,
        &canonical_erase_args_with_identity(&target, &destructive_identity)?,
    )?;

    let sys = system_drive();
    let is_os = is_os_target(
        &target.kind,
        target.mount_point.as_deref(),
        target.path.as_deref(),
        &sys,
    );

    // OS/system target ⇒ require a typed nuclear ack matching the resolved id.
    if is_os {
        let resolved = match target.kind.as_str() {
            "bitlocker" => norm_drive(target.mount_point.as_deref().unwrap_or(&sys)),
            _ => norm_drive(&sys),
        };
        let ok = target
            .os_volume_ack
            .as_deref()
            .map(|ack| norm_drive(ack) == resolved)
            .unwrap_or(false);
        if !ok {
            return Err(format!(
                "Refusing OS/system volume erase: a typed confirmation matching '{resolved}' is required."
            ));
        }
    }

    match target.kind.as_str() {
        "veracrypt" => {
            let path = crate::path_identity::ExpectedFileIdentity::capture(std::path::Path::new(
                target.path.as_deref().unwrap_or_default(),
            ))?
            .canonical_path()
            .to_string_lossy()
            .into_owned();
            if path.is_empty() {
                return Err("veracrypt target requires a path".to_string());
            }
            // Dismount first if mounted — Destroy-VeraCryptHeader opens the
            // backing file exclusively (FileShare::None). Validate the letter:
            // it flows into a PowerShell dismount command, so only a bare drive
            // letter may pass (injection guard).
            let mut key_evicted = false;
            if let Some(letter) = target.mount_letter.as_deref() {
                let l = letter.trim();
                let valid = matches!(l.len(), 1 | 2)
                    && l.as_bytes()[0].is_ascii_alphabetic()
                    && (l.len() == 1 || l.as_bytes()[1] == b':');
                if !valid {
                    return Err(format!("Invalid mount letter: {letter}"));
                }
                let dismount = crate::sidecar::dispatch_paid_command(
                    "Dismount-EncryptedVolume",
                    serde_json::json!({ "DriveLetter": l }),
                )
                .await;
                // FIX 7: only claim eviction when the dismount actually reported ok.
                key_evicted = matches!(
                    &dismount,
                    Ok(v) if v.get("ok").and_then(Value::as_bool) == Some(true)
                );
            }
            let pre_header = read_header_prefix(&path);
            let feature =
                wincmd_shared::command_strings::join_parts(&["Destroy~-", "VeraCrypt~", "Header~"]);
            let mut payload = serde_json::json!({ "Path": path })
                .as_object()
                .cloned()
                .expect("destructive payload is an object");
            crate::path_identity::insert_request(&mut payload, &destructive_identity)?;
            let res =
                crate::sidecar::dispatch_paid_command(&feature, Value::Object(payload)).await?;
            crate::path_identity::verify_receipt(&res, &destructive_identity)?;
            let label = target
                .mount_letter
                .as_deref()
                .map(|l| format!("VeraCrypt {l}"))
                .unwrap_or_else(|| format!("VeraCrypt container {path}"));
            let mut receipt = map_receipt("veracrypt", &label, &res, key_evicted, is_os);
            let post_header = read_header_prefix(&path);
            apply_header_verification(&mut receipt, pre_header.as_deref(), post_header.as_deref());
            crate::log_message(
                "warn",
                &format!(
                    "container crypto-erase: veracrypt {label} -> {}",
                    receipt.status
                ),
            );
            Ok(receipt)
        }
        "bitlocker" => {
            let mp = target
                .mount_point
                .clone()
                .ok_or_else(|| "bitlocker target requires a mount point".to_string())?;
            let clear = wincmd_shared::command_strings::join_parts(&[
                "Clear~-",
                "Bit~",
                "Locker~",
                "Key~",
                "Protectors~",
            ]);
            let mut clear_payload = serde_json::json!({ "DriveLetter": mp.clone() })
                .as_object()
                .cloned()
                .expect("destructive payload is an object");
            crate::path_identity::insert_request(&mut clear_payload, &destructive_identity)?;
            let res =
                crate::sidecar::dispatch_paid_command(&clear, Value::Object(clear_payload)).await?;
            crate::path_identity::verify_receipt(&res, &destructive_identity)?;
            // Evict the FVEK for a DATA volume; the running OS drive can't be locked.
            // "No key-eviction retry": an open handle (e.g. an
            // Explorer window on the volume) commonly clears within a second,
            // so retry once after a brief delay before accepting the honest
            // `erased_with_caveat` receipt. Never more than 2 attempts total:
            // this is a mitigation for transient contention, not a loop that
            // could mask a genuinely stuck handle.
            let mut key_evicted = false;
            if !is_os {
                for attempt in 0..2 {
                    if attempt > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                    let mut lock_payload = serde_json::json!({ "MountPoint": mp.clone() })
                        .as_object()
                        .cloned()
                        .expect("destructive payload is an object");
                    crate::path_identity::insert_request(&mut lock_payload, &destructive_identity)?;
                    let lock = crate::sidecar::dispatch_paid_command(
                        "Lock-BitLockerVolume",
                        Value::Object(lock_payload),
                    )
                    .await;
                    key_evicted = matches!(
                        &lock,
                        Ok(v) if v.get("status").and_then(Value::as_str) == Some("locked")
                    );
                    if let Ok(value) = &lock {
                        crate::path_identity::verify_receipt(value, &destructive_identity)?;
                    }
                    if key_evicted {
                        break;
                    }
                }
            }
            let label = format!("BitLocker {}", norm_drive(&mp));
            let receipt = map_receipt("bitlocker", &label, &res, key_evicted, is_os);
            crate::log_message(
                "warn",
                &format!(
                    "container crypto-erase: bitlocker {label} -> {}",
                    receipt.status
                ),
            );
            Ok(receipt)
        }
        other => Err(format!("unknown target kind: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitlocker_system_drive_is_os() {
        assert!(is_os_target("bitlocker", Some("C:"), None, "C:"));
        assert!(is_os_target("bitlocker", Some("c:\\"), None, "C:"));
    }

    #[test]
    fn bitlocker_data_drive_is_not_os() {
        assert!(!is_os_target("bitlocker", Some("D:"), None, "C:"));
        assert!(!is_os_target("bitlocker", Some("E:\\"), None, "C:"));
    }

    #[test]
    fn bitlocker_missing_mountpoint_fails_safe_to_os() {
        assert!(is_os_target("bitlocker", None, None, "C:"));
    }

    #[test]
    fn veracrypt_file_container_is_not_os_even_on_c() {
        assert!(!is_os_target("veracrypt", None, Some("C:\\vault.hc"), "C:"));
        assert!(!is_os_target(
            "veracrypt",
            None,
            Some("D:\\secret.hc"),
            "C:"
        ));
    }

    #[test]
    fn veracrypt_system_device_path_is_os() {
        assert!(is_os_target("veracrypt", None, Some("\\\\.\\C:"), "C:"));
    }

    #[test]
    fn veracrypt_raw_physical_disk_is_os() {
        assert!(is_os_target(
            "veracrypt",
            None,
            Some("\\\\.\\PhysicalDrive0"),
            "C:"
        ));
        assert!(is_os_target(
            "veracrypt",
            None,
            Some("\\\\.\\HarddiskVolume3"),
            "C:"
        ));
        assert!(is_os_target(
            "veracrypt",
            None,
            Some("\\\\?\\Volume{abc}"),
            "C:"
        ));
    }

    #[test]
    fn veracrypt_plain_file_container_stays_data() {
        assert!(!is_os_target(
            "veracrypt",
            None,
            Some("D:\\secret.hc"),
            "C:"
        ));
        assert!(!is_os_target(
            "veracrypt",
            None,
            Some("C:\\Users\\me\\v.tc"),
            "C:"
        ));
    }

    #[test]
    fn unknown_kind_fails_safe_to_os() {
        assert!(is_os_target("mystery", Some("D:"), None, "C:"));
    }

    #[test]
    fn norm_drive_strips_trailing_backslash_and_uppercases() {
        assert_eq!(norm_drive(" d:\\ "), "D:");
    }

    use serde_json::json;

    #[test]
    fn veracrypt_destroyed_is_erased() {
        let h = json!({ "status": "destroyed", "path": "D:\\v.hc" });
        let r = map_receipt("veracrypt", "VeraCrypt V:", &h, true, false);
        assert_eq!(r.status, "erased");
        assert!(r.verified);
    }

    #[test]
    fn veracrypt_non_destroyed_is_failed() {
        let h = json!({ "status": "error", "error": "locked" });
        let r = map_receipt("veracrypt", "VeraCrypt V:", &h, false, false);
        assert_eq!(r.status, "failed");
        assert!(!r.verified);
    }

    #[test]
    fn bitlocker_clean_no_escrow_is_erased() {
        let h = json!({ "status": "ok", "escrow_warning": "", "recovery_protectors_remaining": 0 });
        let r = map_receipt("bitlocker", "BitLocker D:", &h, true, false);
        assert_eq!(r.status, "erased");
    }

    #[test]
    fn bitlocker_escrow_warning_never_erased() {
        let h = json!({
            "status": "ok",
            "escrow_warning": "RecoveryPassword protector has BackupUsed=true — escrow confirmed",
            "recovery_protectors_remaining": 0
        });
        let r = map_receipt("bitlocker", "BitLocker D:", &h, true, false);
        assert_eq!(r.status, "erased_with_caveat");
        assert_ne!(r.status, "erased");
        assert!(r.escrow_warning.is_some());
    }

    #[test]
    fn bitlocker_protectors_remaining_is_caveat() {
        let h = json!({ "status": "ok", "escrow_warning": "", "recovery_protectors_remaining": 1 });
        let r = map_receipt("bitlocker", "BitLocker D:", &h, true, false);
        assert_eq!(r.status, "erased_with_caveat");
    }

    #[test]
    fn bitlocker_handler_error_is_failed() {
        let h = json!({ "status": "error", "error": "no volume", "escrow_warning": "" });
        let r = map_receipt("bitlocker", "BitLocker D:", &h, false, false);
        assert_eq!(r.status, "failed");
    }

    #[test]
    fn bitlocker_data_clean_but_lock_failed_is_caveat_not_erased() {
        // Protectors removed cleanly, but the FVEK could not be evicted
        // (key_evicted=false) on a DATA volume ⇒ data still readable ⇒ caveat.
        let h = json!({ "status": "ok", "escrow_warning": "", "recovery_protectors_remaining": 0 });
        let r = map_receipt("bitlocker", "BitLocker D:", &h, false, false);
        assert_eq!(r.status, "erased_with_caveat");
        assert_ne!(r.status, "erased");
        assert!(
            r.detail.to_lowercase().contains("reboot")
                || r.detail.to_lowercase().contains("memory")
        );
    }

    #[test]
    fn bitlocker_os_clean_is_erased_but_detail_is_reboot_honest() {
        // OS volume: lock is intentionally skipped (key_evicted=false). Still
        // "erased" (protectors gone, unrecoverable after reboot) but the detail
        // must NOT claim the volume is locked now.
        let h = json!({ "status": "ok", "escrow_warning": "", "recovery_protectors_remaining": 0 });
        let r = map_receipt("bitlocker", "BitLocker C:", &h, false, true);
        assert_eq!(r.status, "erased");
        assert!(!r.detail.to_lowercase().contains("the volume locked"));
        assert!(
            r.detail.to_lowercase().contains("restart") || r.detail.to_lowercase().contains("boot")
        );
    }

    #[test]
    fn bitlocker_data_clean_and_key_evicted_is_erased() {
        let h = json!({ "status": "ok", "escrow_warning": "", "recovery_protectors_remaining": 0 });
        let r = map_receipt("bitlocker", "BitLocker D:", &h, true, false);
        assert_eq!(r.status, "erased");
        assert!(r.detail.to_lowercase().contains("locked"));
    }

    #[test]
    fn header_verification_downgrades_when_bytes_unchanged() {
        let h = json!({ "status": "destroyed" });
        let mut r = map_receipt("veracrypt", "V:", &h, true, false);
        assert_eq!(r.status, "erased");
        let bytes = vec![7u8; 32];
        apply_header_verification(&mut r, Some(&bytes), Some(&bytes));
        assert_eq!(r.status, "erased_with_caveat");
        assert!(!r.verified);
        assert!(r.detail.contains("INDEPENDENT VERIFICATION FAILED"));
    }

    #[test]
    fn header_verification_confirms_when_bytes_changed() {
        let h = json!({ "status": "destroyed" });
        let mut r = map_receipt("veracrypt", "V:", &h, true, false);
        apply_header_verification(&mut r, Some(&[1, 2, 3]), Some(&[9, 9, 9]));
        assert_eq!(r.status, "erased");
        assert!(r.verified);
        assert!(r.detail.contains("Independently verified"));
    }

    #[test]
    fn header_verification_notes_gap_when_unreadable() {
        let h = json!({ "status": "destroyed" });
        let mut r = map_receipt("veracrypt", "V:", &h, true, false);
        let detail_before = r.detail.clone();
        apply_header_verification(&mut r, None, None);
        assert_eq!(r.status, "erased"); // unchanged — can't downgrade on a read failure
        assert_ne!(r.detail, detail_before);
        assert!(r.detail.contains("Independent verification unavailable"));
    }

    #[test]
    fn header_verification_is_noop_when_already_failed() {
        let h = json!({ "status": "error" });
        let mut r = map_receipt("veracrypt", "V:", &h, false, false);
        assert_eq!(r.status, "failed");
        let detail_before = r.detail.clone();
        apply_header_verification(&mut r, Some(&[1]), Some(&[2]));
        assert_eq!(r.detail, detail_before);
    }

    #[test]
    fn read_header_prefix_reads_bytes_and_returns_none_for_missing_file() {
        let dir = std::env::temp_dir().join(format!("wc-selerase-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hdr.bin");
        std::fs::write(&path, b"hello-header-bytes").unwrap();
        let bytes = read_header_prefix(path.to_str().unwrap());
        assert_eq!(bytes.as_deref(), Some(&b"hello-header-bytes"[..]));

        let missing = dir.join("does-not-exist.bin");
        assert!(read_header_prefix(missing.to_str().unwrap()).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
