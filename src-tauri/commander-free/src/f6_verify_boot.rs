// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/f6_verify_boot.rs
//
// F6 Phase-1, Piece 3 (tools/wipe-usb/README.md §6.6, "P1 Piece 3") — USB boot
// chain self-test.
//
// ════════════════════════════════════════════════════════════════════════
// DANGER — READ BEFORE USING OR EXTENDING THIS MODULE
// ════════════════════════════════════════════════════════════════════════
//
// `f6_verify_usb_boot_arm` writes a REAL, validly-signed wipe token to the
// USB and sets a REAL one-shot UEFI BootNext entry. The wire format has no
// "test mode" flag — the USB-side `wipe-autorun.sh` cannot distinguish a
// self-test token from a genuine distress token. If the armed machine
// reboots for ANY reason (Windows Update, power loss, an unrelated manual
// reboot) while the token is still valid and boots into the USB, the
// pipeline WILL crypto-erase and firmware-sanitize that machine's internal
// disk(s) for real.
//
// The short TTL bounds the exposure window; it does NOT make the action
// non-destructive. This command must only ever be run against a disposable
// test machine (a Proxmox/VM with a throwaway virtual disk) — NEVER a
// production device. `f6_verify_usb_boot_disarm` exists specifically so an
// operator can back out before rebooting if they armed by mistake.
//
// ════════════════════════════════════════════════════════════════════════
// WHY THIS EXISTS
// ════════════════════════════════════════════════════════════════════════
//
// Every other F6 piece is unit-tested (token crypto, provisioning writes,
// BootNext helpers, the orchestrator's gating/ordering logic) but nothing
// exercises the real chain: reboot → firmware picks the USB → SystemRescue
// boots → `wipe-autorun.sh` runs → `verify-token.sh` accepts the token and
// appends its nonce to `/wipe/consumed-nonces`. That loop can only be
// closed by an actual reboot on real (or virtualized) firmware, which is
// why this is deliberately split into three manual, independently-invoked
// steps rather than one command that reboots automatically:
//
//   1. `f6_verify_usb_boot_arm`    — issue a short-TTL token, write it to
//                                     the USB, set BootNext. Returns the
//                                     nonce the operator must check for
//                                     after they manually reboot.
//   2. (operator reboots manually, lets SystemRescue run, boots back into
//      Windows)
//   3. `f6_verify_usb_boot_check`  — read `<usb_root>/wipe/consumed-nonces`
//                                     back and report whether the armed
//                                     nonce is present. Also clears
//                                     BootNext as a courtesy so a stale
//                                     one-shot entry doesn't linger.
//
// `f6_verify_usb_boot_disarm` clears BootNext and invalidates the token on
// the USB immediately, for use between steps 1 and 2 if the operator wants
// to cancel before rebooting.
//
// This module performs NO crypto-erase itself and never calls a reboot API
// — it only reuses the already-reversible primitives from `reboot_usb.rs`
// (BootNext set/clear) and `wincmd_shared::wipe_token_write` (a plain
// filesystem write, delete-to-undo).

use serde_json::{json, Value};
use wincmd_shared::wipe_auth::{issue_wipe_token, verify_wipe_token};
use wincmd_shared::wipe_token_write::write_wipe_token_to_usb;

/// Matches the production stage-2 TTL (`f6_orchestrator.rs`) so the self-test
/// exercises the same timing budget a real trigger would have to survive
/// (reboot + firmware POST + SystemRescue init + autorun).
const TEST_TOKEN_TTL_SECS: i64 = 300;

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Armed-state persistence (survives app restart / a real reboot) ────────────
//
// The whole point of splitting arm/reboot/check into three manual steps is
// that the operator reboots the real machine in between -- which necessarily
// kills the Tauri process and any in-memory (React) state along with it.
// Without a durable record, `f6_verify_usb_boot_check` was unreachable
// through the UI after a genuine reboot -- the one thing this tool exists to
// validate -- because the frontend had nowhere to recover `usbRoot`/
// `nonceHex` from. This marker file closes that gap: written on arm, read on
// dialog-open to rehydrate the UI, deleted on disarm/check. Best-effort --
// a write failure only degrades UX (the operator has to note the values down
// from the arm response) and never blocks arming, since the real token and
// BootNext entry are already committed by the time this is written.

fn armed_marker_path() -> Result<std::path::PathBuf, String> {
    crate::paths::machine_state_file("f6-verify-boot-armed.json")
}

fn legacy_armed_marker_path() -> Result<std::path::PathBuf, String> {
    crate::paths::legacy_user_state_file("f6-verify-boot-armed.json")
}

fn write_armed_marker(
    usb_root: &std::path::Path,
    entry_id: &str,
    nonce_hex: &str,
    expires_at: i64,
) {
    let Ok(_lock) = crate::paths::acquire_machine_state_lock("f6-verify-boot") else {
        return;
    };
    let path = match armed_marker_path() {
        Ok(p) => p,
        Err(_) => return,
    };
    let marker = json!({
        "usbRoot": usb_root.to_string_lossy(),
        "bootEntryId": entry_id,
        "nonceHex": nonce_hex,
        "expiresAtUnix": expires_at,
    });
    if let Err(e) = crate::paths::atomic_write_machine_state(&path, marker.to_string().as_bytes()) {
        crate::log_message(
            "warn",
            &format!(
                "[F6-VerifyBoot] failed to persist armed marker \
                 (Check will not survive an app restart/real reboot): {e}"
            ),
        );
    }
}

fn clear_armed_marker() {
    let Ok(_lock) = crate::paths::acquire_machine_state_lock("f6-verify-boot") else {
        return;
    };
    if let Ok(path) = armed_marker_path() {
        let _ = std::fs::remove_file(path);
    }
    // A cancelled self-test must also clear the pre-machine-scope marker; a
    // stale per-user marker must not make a later operator think BootNext is armed.
    if let Ok(path) = legacy_armed_marker_path() {
        let _ = std::fs::remove_file(path);
    }
}

fn read_armed_marker() -> Option<Value> {
    let _lock = crate::paths::acquire_machine_state_lock("f6-verify-boot").ok()?;
    let machine_path = armed_marker_path().ok()?;
    if machine_path.exists() {
        return std::fs::read_to_string(&machine_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok());
    }

    let legacy_path = legacy_armed_marker_path().ok()?;
    let raw = std::fs::read_to_string(&legacy_path).ok()?;
    let marker: Value = serde_json::from_str(&raw).ok()?;
    // A marker is only recovery metadata, but it tracks a real BootNext action.
    // Promote a valid marker so any administrator can safely disarm/check after
    // reboot; keep the legacy source if the ProgramData write fails.
    if crate::paths::atomic_write_machine_state(&machine_path, marker.to_string().as_bytes())
        .is_ok()
    {
        let _ = std::fs::remove_file(legacy_path);
    }
    Some(marker)
}

/// Report whether a self-test is currently armed, per the durable marker
/// (not React state) -- lets the UI rehydrate after a real reboot or an app
/// restart instead of losing track of an armed wipe trigger.
#[tauri::command]
pub fn f6_verify_usb_boot_status() -> Result<Value, String> {
    crate::license::require_paid("f6 verify usb boot")?;
    match read_armed_marker() {
        Some(mut marker) => {
            marker["armed"] = json!(true);
            Ok(marker)
        }
        // Corrupt/unreadable marker -- treat as "nothing armed" rather than
        // erroring the whole status check.
        None => Ok(json!({ "armed": false })),
    }
}

/// Arm a USB boot self-test: issue a short-TTL wipe token, write it (plus
/// the device pubkey) to the provisioned wipe USB, and set UEFI BootNext to
/// that USB's firmware entry. Does NOT reboot — the operator reboots
/// manually. See the module-level DANGER section before calling this.
///
/// # Errors
/// - No provisioned wipe USB is present (`wipe/pubkey.bin` marker missing).
/// - No removable/USB firmware boot entry is visible to `bcdedit` (the
///   machine may not see the USB, or Secure Boot may be blocking it).
/// - `bcdedit` fails to set the boot sequence (not elevated, or
///   `SeSystemEnvironmentPrivilege` unavailable).
#[tauri::command]
pub fn f6_verify_usb_boot_arm() -> Result<Value, String> {
    crate::license::require_paid("f6 verify usb boot")?;

    let usb_root = crate::f6_orchestrator::find_wipe_usb().ok_or_else(|| {
        "no provisioned wipe USB found (looked for wipe/pubkey.bin on a removable drive) \
         — provision one first with \"Create Wipe USB\""
            .to_string()
    })?;

    let device_id = crate::settings::read_settings()
        .map(|s| s.device_id)
        .unwrap_or_else(|_| "unknown-device".to_string());
    let signing_key = crate::f6_keystore::get_or_create_device_signing_key()?;
    let now = now_unix();

    let token = issue_wipe_token(&device_id, TEST_TOKEN_TTL_SECS, now, &signing_key);
    // Re-verify our own freshly-issued token to recover its nonce as hex,
    // rather than adding a second nonce-extraction API to wipe_auth just
    // for this self-test's bookkeeping.
    let verified = verify_wipe_token(&token, &signing_key.verifying_key(), &device_id, now)
        .map_err(|e| format!("self-issued token failed self-verification: {e}"))?;
    let nonce_hex = hex::encode(verified.nonce);

    write_wipe_token_to_usb(&usb_root, &token, &signing_key.verifying_key())?;

    let entry = crate::reboot_usb::enum_usb_boot_entry()?.ok_or_else(|| {
        "no removable/USB firmware boot entry found — is the USB recognized by firmware? \
         (some systems only enumerate it after a first boot attempt)"
            .to_string()
    })?;
    let entry_id = crate::reboot_usb::set_boot_next_usb(&entry.id)?;

    let expires_at = now + TEST_TOKEN_TTL_SECS;
    write_armed_marker(&usb_root, &entry_id, &nonce_hex, expires_at);
    crate::log_message(
        "warn",
        &format!(
            "[F6-VerifyBoot] ARMED a real self-test wipe token: usb={} boot_entry={} \
             nonce={}... expires_at_unix={} — rebooting THIS machine before it expires \
             will trigger a real wipe. Disarm with f6_verify_usb_boot_disarm if unintended.",
            usb_root.display(),
            entry_id,
            &nonce_hex[..16],
            expires_at
        ),
    );

    Ok(json!({
        "usbRoot": usb_root.to_string_lossy(),
        "bootEntryId": entry_id,
        "nonceHex": nonce_hex,
        "expiresAtUnix": expires_at,
        "warning": "A real wipe token is now armed on this USB and BootNext points at it. \
                    If THIS machine reboots for any reason before expiry, it will really \
                    wipe. Only run this test against a disposable machine. Reboot manually \
                    to continue the test, or call f6_verify_usb_boot_disarm to cancel.",
    }))
}

/// Immediately cancel an armed self-test: clear UEFI BootNext and, if the
/// USB is still present, overwrite its `token.txt` with an invalid value so
/// even a stray boot into it (via BIOS boot-menu override, say) fails
/// `verify-token.sh`'s signature check rather than succeeding.
#[tauri::command]
pub fn f6_verify_usb_boot_disarm() -> Result<Value, String> {
    crate::license::require_paid("f6 verify usb boot")?;

    crate::reboot_usb::clear_boot_next()?;
    clear_armed_marker();

    let usb_invalidated = if let Some(usb_root) = crate::f6_orchestrator::find_wipe_usb() {
        let token_path = usb_root.join("wipe").join("token.txt");
        std::fs::write(&token_path, b"disarmed").is_ok()
    } else {
        false
    };

    crate::log_message(
        "info",
        &format!(
            "[F6-VerifyBoot] disarmed self-test: BootNext cleared, usb_token_invalidated={}",
            usb_invalidated
        ),
    );

    Ok(json!({
        "bootNextCleared": true,
        "usbTokenInvalidated": usb_invalidated,
    }))
}

/// After the operator has manually rebooted this machine, let SystemRescue
/// run the wipe pipeline, and booted back into Windows: check whether the
/// nonce issued by `f6_verify_usb_boot_arm` shows up in
/// `<usb_root>/wipe/consumed-nonces` (written by `verify-token.sh` only
/// after it accepts a valid token). Also clears BootNext as a courtesy so a
/// completed test doesn't leave a stale one-shot boot entry behind.
#[tauri::command]
pub fn f6_verify_usb_boot_check(usb_root: String, nonce_hex: String) -> Result<Value, String> {
    crate::license::require_paid("f6 verify usb boot")?;

    let consumed_path = std::path::Path::new(&usb_root)
        .join("wipe")
        .join("consumed-nonces");

    let result = check_consumed_nonces(&consumed_path, &nonce_hex);

    // Best-effort cleanup — the test is considered over the moment Check is
    // called, whether or not the real reboot actually happened yet (calling
    // Check before rebooting is indistinguishable from here, and correctly
    // reports consumed=false either way). Clearing now is fail-safe: it can
    // only make an unrebooted machine safer, never trigger anything. The
    // "bootNextCleared" flag is always true so the UI can tell an operator
    // who checked too early that re-arming is required to actually complete
    // the test, rather than concluding the pipeline itself is broken.
    let _ = crate::reboot_usb::clear_boot_next();
    clear_armed_marker();

    result.map(|mut v| {
        v["bootNextCleared"] = json!(true);
        v
    })
}

/// Inner check, extracted so the file-parsing logic can be unit-tested
/// without a Tauri runtime or a real BootNext/USB present.
fn check_consumed_nonces(
    consumed_path: &std::path::Path,
    nonce_hex: &str,
) -> Result<Value, String> {
    if !consumed_path.exists() {
        return Ok(json!({
            "consumed": false,
            "reason": "consumed-nonces file not found on the USB control partition \
                       (the pipeline may not have reached verify-token.sh, or the USB \
                       wasn't remounted at this path)",
        }));
    }
    let contents =
        std::fs::read_to_string(consumed_path).map_err(|e| format!("read consumed-nonces: {e}"))?;
    let consumed = contents
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case(nonce_hex.trim()));
    Ok(json!({ "consumed": consumed }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── check_consumed_nonces: pure file-parsing logic, no hardware needed ──

    #[test]
    fn check_reports_false_when_file_missing() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("wipe").join("consumed-nonces");
        let result = check_consumed_nonces(&path, "deadbeef").expect("must not error");
        assert_eq!(result["consumed"], false);
        assert!(result["reason"].as_str().unwrap().contains("not found"));
    }

    #[test]
    fn check_finds_matching_nonce_among_many_lines() {
        let dir = TempDir::new().expect("tempdir");
        let wipe_dir = dir.path().join("wipe");
        std::fs::create_dir_all(&wipe_dir).unwrap();
        let path = wipe_dir.join("consumed-nonces");
        std::fs::write(&path, "aaaa1111\nbbbb2222\ncccc3333\n").unwrap();

        let result = check_consumed_nonces(&path, "bbbb2222").expect("must not error");
        assert_eq!(result["consumed"], true);
    }

    #[test]
    fn check_reports_false_for_nonce_not_present() {
        let dir = TempDir::new().expect("tempdir");
        let wipe_dir = dir.path().join("wipe");
        std::fs::create_dir_all(&wipe_dir).unwrap();
        let path = wipe_dir.join("consumed-nonces");
        std::fs::write(&path, "aaaa1111\nbbbb2222\n").unwrap();

        let result = check_consumed_nonces(&path, "ffffffff").expect("must not error");
        assert_eq!(result["consumed"], false);
    }

    #[test]
    fn check_is_case_insensitive_and_trims_whitespace() {
        let dir = TempDir::new().expect("tempdir");
        let wipe_dir = dir.path().join("wipe");
        std::fs::create_dir_all(&wipe_dir).unwrap();
        let path = wipe_dir.join("consumed-nonces");
        // verify-token.sh writes lowercase hex with a trailing newline; be
        // tolerant of stray whitespace/case in case the file is hand-edited
        // during manual testing.
        std::fs::write(&path, "  AaBb1234  \n").unwrap();

        let result = check_consumed_nonces(&path, "aabb1234").expect("must not error");
        assert_eq!(result["consumed"], true);
    }

    #[test]
    fn check_handles_empty_file() {
        let dir = TempDir::new().expect("tempdir");
        let wipe_dir = dir.path().join("wipe");
        std::fs::create_dir_all(&wipe_dir).unwrap();
        let path = wipe_dir.join("consumed-nonces");
        std::fs::write(&path, "").unwrap();

        let result = check_consumed_nonces(&path, "anything").expect("must not error");
        assert_eq!(result["consumed"], false);
    }

    // ── Token issue + self-verify round trip that f6_verify_usb_boot_arm relies on ──

    #[test]
    fn issued_token_self_verifies_and_nonce_hex_is_64_chars() {
        use wincmd_shared::wipe_auth::signing_key_from_bytes;

        let sk = signing_key_from_bytes(&[0x11u8; 32]);
        let device_id = "test-device-verify-boot";
        let now = 1_751_000_000i64;

        let token = issue_wipe_token(device_id, TEST_TOKEN_TTL_SECS, now, &sk);
        let verified = verify_wipe_token(&token, &sk.verifying_key(), device_id, now)
            .expect("self-issued token must self-verify");
        let nonce_hex = hex::encode(verified.nonce);

        assert_eq!(nonce_hex.len(), 64, "32-byte nonce hex-encodes to 64 chars");
        assert_eq!(verified.expires_at, now + TEST_TOKEN_TTL_SECS);
    }

    // ── write_wipe_token_to_usb + check_consumed_nonces integration, simulating
    //    what verify-token.sh does on acceptance (appends nonce hex + newline) ──

    #[test]
    fn simulated_full_arm_and_check_roundtrip() {
        use wincmd_shared::wipe_auth::{generate_provisioning_keypair, signing_key_from_bytes};

        let dir = TempDir::new().expect("tempdir");
        let sk = signing_key_from_bytes(&[0x22u8; 32]);
        let (_other_sk, other_vk) = generate_provisioning_keypair();
        let device_id = "test-device-roundtrip";
        let now = 1_751_000_100i64;

        // Simulates f6_verify_usb_boot_arm's body (minus the Tauri/license/USB-
        // enumeration/BootNext parts, which need a real OS).
        let token = issue_wipe_token(device_id, TEST_TOKEN_TTL_SECS, now, &sk);
        let verified = verify_wipe_token(&token, &sk.verifying_key(), device_id, now).unwrap();
        let nonce_hex = hex::encode(verified.nonce);
        write_wipe_token_to_usb(dir.path(), &token, &sk.verifying_key()).unwrap();

        // Sanity: a DIFFERENT device's pubkey must NOT verify this token —
        // guards against a copy-paste mistake wiring the wrong key in.
        assert!(verify_wipe_token(&token, &other_vk, device_id, now).is_err());

        // Simulates verify-token.sh accepting the token and recording the nonce.
        let consumed_path = dir.path().join("wipe").join("consumed-nonces");
        std::fs::write(&consumed_path, format!("{nonce_hex}\n")).unwrap();

        // Simulates f6_verify_usb_boot_check's body.
        let result = check_consumed_nonces(&consumed_path, &nonce_hex).unwrap();
        assert_eq!(result["consumed"], true);

        // A DIFFERENT (not-yet-issued) nonce must not be reported as consumed.
        let other_nonce = hex::encode([0xEEu8; 32]);
        let result2 = check_consumed_nonces(&consumed_path, &other_nonce).unwrap();
        assert_eq!(result2["consumed"], false);
    }
}
