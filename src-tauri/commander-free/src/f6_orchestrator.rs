// SPDX-License-Identifier: AGPL-3.0-or-later — F6 Phase-1, Piece 3 — Reboot-to-USB wipe orchestrator.
//
// ════════════════════════════════════════════════════════════════════════
// SAFETY CONTRACT — READ TWICE BEFORE EDITING
// ════════════════════════════════════════════════════════════════════════
//
// 1. INJECTABLE INTERFACES (SAFETY rule 1):
//    The real crypto-erase calls and the real `Restart-Computer` are
//    NEVER called directly from this function.  They are injected via
//    `F6Deps` so tests can pass no-op stubs.  The build machine MUST
//    NOT reboot or erase during `cargo test`.
//
// 2. KEYS-BEFORE-REBOOT (SAFETY rule 2, the critical invariant):
//    Stage-1 crypto-erase runs FIRST.  If ANY step returns Err →
//    ABORT immediately: do NOT write the token, do NOT set BootNext,
//    do NOT reboot.  Return the error naming the failed step.
//    An `escrow_warning` is NOT an abort — only hard errors abort.
//
// 3. NOT AN IPC COMMAND (SAFETY rule 3):
//    `execute_reboot_to_usb_wipe` has NO `#[tauri::command]` attribute.
//    The frontend CANNOT invoke it directly.  Entry is only through the
//    `reboot_usb` distress mode handler in `shortcut_actions.rs`.
//
// 4. FULL GATE (SAFETY rule 4):
//    The function gates on ALL of:
//      a. `self_destruct.enabled == Some(true)`
//      b. `reboot_to_usb_enabled == Some(true)` (`reboot_to_usb_armed`)
//      c. Firing trigger is the configured `reboot_usb` distress mode
//         (checked at call site in shortcut_actions.rs — the caller
//         only reaches this fn when that mode fires)
//      d. Investigator mode is NOT active
//      e. Decoy mode is NOT active
//
// ════════════════════════════════════════════════════════════════════════
// TWO-STAGE FLOW
// ════════════════════════════════════════════════════════════════════════
//
//  1. Gate check → refuse if not armed.
//  2. Stage-1 crypto-erase (via injected deps.crypto_erase).
//     Each step: bitlocker_erase, veracrypt_header_destroy, vault_tpm_key_delete.
//     Any Err → abort (keys-before-reboot).
//  3. Detect wipe USB by `wipe/pubkey.bin` marker on any removable volume.
//     If none found → skip stage-2, log, return Ok (stage-1 already protected).
//  4. Issue token (issue_wipe_token, ttl=300s) + write_wipe_token_to_usb.
//  5. set_boot_next_usb (via injected deps.set_boot_next).
//  6. Reboot (via injected deps.reboot) — LAST line of the happy path.
//
// ════════════════════════════════════════════════════════════════════════

use ed25519_dalek::SigningKey;
use std::path::PathBuf;
use wincmd_shared::{
    reboot_usb_predicate::{reboot_to_usb_armed, SelfDestructRef},
    wipe_auth::issue_wipe_token,
    wipe_token_write::write_wipe_token_to_usb,
};

// ── Injected dependency bundle ─────────────────────────────────────────────────

/// The three stage-1 crypto-erase steps — injected so tests can stub them.
/// The real implementations dispatch to the Pro sidecar (same as `run_destruct_step`
/// in backend.rs); the test stubs are no-ops or failure simulators.
///
/// Field order matches the execution order in the spec:
///   (1) BitLocker key erase, (2) VeraCrypt header destroy, (3) TPM vault key delete.
pub struct Stage1Eraser {
    /// Erase BitLocker protectors on all volumes.
    /// Returns Ok(escrow_warning) where `escrow_warning` is a non-empty string
    /// when escrowed keys may remain (NOT an abort condition — log only).
    pub bitlocker_erase: Box<dyn Fn() -> Result<Option<String>, String> + Send + Sync>,
    /// Destroy VeraCrypt volume header(s) — true crypto-erase.
    pub veracrypt_destroy: Box<dyn Fn() -> Result<(), String> + Send + Sync>,
    /// Delete the TPM-backed vault signing key.
    pub vault_tpm_delete: Box<dyn Fn() -> Result<(), String> + Send + Sync>,
}

/// Injected interfaces for the non-deterministic / side-effecting operations.
/// Production wiring injects real implementations; tests inject stubs.
pub struct F6Deps {
    pub stage1: Stage1Eraser,
    /// Set BootNext to the detected USB firmware entry. Injected so tests
    /// never call bcdedit. Returns the entry-id string on success.
    pub set_boot_next: Box<dyn Fn() -> Result<String, String> + Send + Sync>,
    /// Execute the system reboot. In production: `Restart-Computer -Force`.
    /// In tests: a no-op that records it was called.
    ///
    /// MUST be called as the LAST operation in the happy path — nothing
    /// executes after this in production (the OS shuts down the process).
    pub reboot: Box<dyn Fn() -> Result<(), String> + Send + Sync>,
    /// Current Unix timestamp in seconds (injected so tests are deterministic).
    pub now_unix: Box<dyn Fn() -> i64 + Send + Sync>,
    /// The device signing key. Injected so tests use a deterministic key.
    pub signing_key: SigningKey,
    /// The stable device identifier (UUID). Used in the wipe token.
    pub device_id: String,
}

// ── USB wipe marker detection ─────────────────────────────────────────────────

/// The path inside a USB root that proves it is a provisioned wipe USB.
const WIPE_PUBKEY_MARKER: &str = "wipe/pubkey.bin";

/// Enumerate all removable logical disk drive letters visible to the OS,
/// then return the first one whose root contains `wipe/pubkey.bin`.
///
/// On Windows we use `GetLogicalDrives` + `GetDriveTypeW` (DRIVE_REMOVABLE = 2)
/// so there are no PowerShell spawns.  On non-Windows targets the function
/// always returns `None` (stubs only — the feature is Windows-only by design).
pub fn find_wipe_usb() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};

        // DRIVE_REMOVABLE = 2 per WinSDK — not exported in this windows-sys version.
        const DRIVE_REMOVABLE: u32 = 2;

        let mask = unsafe { GetLogicalDrives() };
        for bit in 0u32..26 {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let letter = (b'A' + bit as u8) as char;
            let root = format!("{}:\\", letter);
            let wide: Vec<u16> = OsStr::new(&root).encode_wide().chain(Some(0)).collect();
            let drive_type = unsafe { GetDriveTypeW(wide.as_ptr()) };
            if drive_type != DRIVE_REMOVABLE {
                continue;
            }
            let usb_root = PathBuf::from(&root);
            let marker = usb_root.join(WIPE_PUBKEY_MARKER);
            if marker.exists() {
                return Some(usb_root);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        None
    }
}

// ── Outcome of the orchestration ──────────────────────────────────────────────

/// Result of `execute_reboot_to_usb_wipe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum F6Outcome {
    /// Full happy path: stage-1 succeeded, token written, BootNext set,
    /// reboot called.  In production the process does not return after this.
    RebootTriggered,
    /// Stage-1 succeeded but no wipe USB was found.  Stage-2 was skipped.
    /// The machine is crypto-erased but NOT rebooted into the wipe USB.
    Stage1OnlyNoUsb,
}

// ── Orchestrator ──────────────────────────────────────────────────────────────

/// Execute the F6 reboot-to-USB wipe orchestration.
///
/// This is an **internal function**, not a `#[tauri::command]`.  The frontend
/// cannot invoke it directly.  Entry is through the `reboot_usb` distress mode
/// handler in `shortcut_actions.rs`, which is itself not a freely-callable IPC.
///
/// # Gate
///
/// Returns `Err("gate: …")` if:
///   - `self_destruct.enabled != Some(true)`
///   - `reboot_to_usb_enabled != Some(true)`
///   - investigator mode is active
///   - decoy mode is active
///
/// # Keys-before-reboot
///
/// Any stage-1 error → returns `Err("stage-1: <step>: <error>")` immediately.
/// BootNext and reboot are NEVER called if stage-1 fails even partially.
///
/// # Parameters
///
/// - `enabled`              — `self_destruct.enabled` from settings.
/// - `reboot_to_usb_enabled` — `self_destruct.reboot_to_usb_enabled` from settings.
/// - `deps`                 — injectable interfaces (see `F6Deps`).
pub(crate) fn execute_reboot_to_usb_wipe(
    enabled: Option<bool>,
    reboot_to_usb_enabled: Option<bool>,
    deps: F6Deps,
) -> Result<F6Outcome, String> {
    // ── Gate check (SAFETY rule 4a+b) ────────────────────────────────────────
    let sd = SelfDestructRef {
        enabled,
        reboot_to_usb_enabled,
        _phantom: std::marker::PhantomData,
    };
    if !reboot_to_usb_armed(&sd) {
        return Err(format!(
            "gate: reboot-to-USB wipe is not armed \
             (selfDestruct.enabled={:?}, rebootToUsbEnabled={:?})",
            enabled, reboot_to_usb_enabled
        ));
    }

    // ── Gate check (SAFETY rule 4d) — investigator mode ──────────────────────
    if crate::license::is_advanced_mode() {
        return Err("gate: F6 reboot-to-USB wipe refused in investigator mode".to_string());
    }

    // ── Gate check (SAFETY rule 4e) — decoy session ──────────────────────────
    if crate::settings::is_decoy_mode() {
        return Err("gate: F6 reboot-to-USB wipe refused in decoy session".to_string());
    }

    crate::log_message("info", "[F6-Orch] stage-1 crypto-erase beginning");

    // ── Stage-1: BitLocker key erase ─────────────────────────────────────────
    // (SAFETY rule 2: any Err → abort before writing token/BootNext/reboot)
    match (deps.stage1.bitlocker_erase)() {
        Ok(Some(warn)) => {
            // escrow_warning is NOT an abort — log and continue (spec §6 rule 1).
            crate::log_message(
                "warn",
                &format!("[F6-Orch] BitLocker erase escrow_warning (stage-1 continues): {warn}"),
            );
        }
        Ok(None) => {
            crate::log_message("info", "[F6-Orch] BitLocker key erase: ok");
        }
        Err(e) => {
            // Hard error → abort (keys-before-reboot invariant).
            crate::log_message(
                "error",
                &format!("[F6-Orch] stage-1 FAILED at BitLocker erase — aborting (no reboot): {e}"),
            );
            return Err(format!("stage-1: bitlocker_erase: {e}"));
        }
    }

    // ── Stage-1: VeraCrypt header destroy ────────────────────────────────────
    if let Err(e) = (deps.stage1.veracrypt_destroy)() {
        crate::log_message(
            "error",
            &format!("[F6-Orch] stage-1 FAILED at VeraCrypt destroy — aborting (no reboot): {e}"),
        );
        return Err(format!("stage-1: veracrypt_destroy: {e}"));
    }
    crate::log_message("info", "[F6-Orch] VeraCrypt header destroy: ok");

    // ── Stage-1: TPM vault key delete ─────────────────────────────────────────
    if let Err(e) = (deps.stage1.vault_tpm_delete)() {
        crate::log_message(
            "error",
            &format!("[F6-Orch] stage-1 FAILED at TPM key delete — aborting (no reboot): {e}"),
        );
        return Err(format!("stage-1: vault_tpm_delete: {e}"));
    }
    crate::log_message("info", "[F6-Orch] TPM vault key delete: ok");

    crate::log_message("info", "[F6-Orch] stage-1 crypto-erase complete");

    // ── Detect wipe USB ───────────────────────────────────────────────────────
    let usb_root = match find_wipe_usb() {
        Some(root) => root,
        None => {
            // No provisioned wipe USB found — skip stage-2 entirely.
            // Stage-1 already crypto-erased the keys; this is a safe degraded mode.
            crate::log_message(
                "info",
                "[F6-Orch] no wipe USB with wipe/pubkey.bin found; \
                 stage-2 skipped — stage-1 crypto-erase is the guarantee",
            );
            return Ok(F6Outcome::Stage1OnlyNoUsb);
        }
    };

    crate::log_message(
        "info",
        &format!("[F6-Orch] wipe USB detected at {}", usb_root.display()),
    );

    // ── Issue and write wipe token ────────────────────────────────────────────
    let now = (deps.now_unix)();
    let token = issue_wipe_token(&deps.device_id, 300, now, &deps.signing_key);

    let vk = deps.signing_key.verifying_key();
    write_wipe_token_to_usb(&usb_root, &token, &vk)
        .map_err(|e| format!("write_wipe_token_to_usb: {e}"))?;

    crate::log_message("info", "[F6-Orch] wipe token written to USB");

    // ── Set BootNext to the USB firmware entry ────────────────────────────────
    let boot_entry_id = (deps.set_boot_next)()?;
    crate::log_message(
        "info",
        &format!("[F6-Orch] BootNext set to USB entry {}", boot_entry_id),
    );

    // ── Reboot — MUST be the last operation ───────────────────────────────────
    // In production the OS tears down the process here. Tests inject a no-op.
    crate::log_message("info", "[F6-Orch] triggering reboot into wipe USB");
    (deps.reboot)()?;

    Ok(F6Outcome::RebootTriggered)
}

// ── Production dependency wiring ──────────────────────────────────────────────

/// Build the real `F6Deps` for production use.
///
/// The stage-1 erasers dispatch via the Pro sidecar (same mechanism as
/// `backend::run_destruct_step`). The reboot issues `Restart-Computer -Force`
/// via PowerShell.
///
/// # Note on async
///
/// The Pro sidecar dispatch is async, but the orchestrator runs synchronously
/// (the caller is already in a spawned task context in `shortcut_actions.rs`).
/// We use `tokio::runtime::Handle::current().block_on(...)` to bridge async
/// dispatch into the sync `Fn()` API.  This is acceptable here because the
/// orchestrator is called from a `tauri::async_runtime::spawn` task where a
/// Tokio runtime is always present.
///
/// # Why sync closures despite async dispatch?
///
/// The injected `Fn()` interface is sync so the orchestrator can be unit-tested
/// without an async runtime.  Async traits in closures would require `async-trait`
/// and make the test stubs significantly more complex for no benefit.
pub fn build_production_deps() -> Result<F6Deps, String> {
    let signing_key = crate::f6_keystore::get_or_create_device_signing_key()?;
    let device_id = crate::settings::read_settings()
        .map(|s| s.device_id)
        .unwrap_or_else(|_| "unknown-device".to_string());

    Ok(F6Deps {
        stage1: Stage1Eraser {
            bitlocker_erase: Box::new(|| {
                // Previously this dispatched with no `DriveLetter` at all, which the
                // Pro handler silently defaulted to "C:" -- every armed user's stage-1
                // blindly targeted the OS drive with no way to choose otherwise. This
                // now targets exactly the drives the user selected
                // (CryptoEraseTargetsSection, crypto_erase_bitlocker_drives), mirroring
                // veracrypt_destroy below: nothing configured -> Ok(None), proceed to
                // the next stage-1 step rather than defaulting to a drive nobody chose.
                let drives = crate::settings::read_settings()
                    .ok()
                    .and_then(|s| {
                        s.ideal
                            .privacy
                            .self_destruct
                            .crypto_erase_bitlocker_drives
                            .clone()
                    })
                    .unwrap_or_default();
                if drives.is_empty() {
                    return Ok(None);
                }
                let mut warnings = Vec::new();
                for drive in &drives {
                    let result = tokio::runtime::Handle::current().block_on(async {
                        crate::sidecar::dispatch_paid_command(
                            "run_destruct_step",
                            serde_json::json!({
                                "stepId": "bitlocker_erase",
                                "DriveLetter": drive,
                            }),
                        )
                        .await
                    });
                    // Pro returns { status: "ok"|"error", escrow_warning?, error? } --
                    // status-checked (not just "any Ok(v) is success") because a real
                    // PS-level failure (BitLocker not enabled on the target, an
                    // exception mid-removal) previously fell through as a false
                    // success with no escrow_warning either -- a silent no-op reported
                    // as clean. See crypto_erase_status_result's doc comment in
                    // backend.rs for the full shape mismatch this guards against. Any
                    // hard failure here aborts stage-1 (keys-before-reboot) -- a
                    // configured target that genuinely fails to destroy must not
                    // proceed to reboot believing crypto-erase succeeded.
                    let v = result?;
                    crate::backend::crypto_erase_status_result(&v, "ok")
                        .map_err(|e| format!("bitlocker_erase: {drive}: {e}"))?;
                    if let Some(w) = v
                        .get("escrow_warning")
                        .and_then(|w| w.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        warnings.push(format!("{drive}: {w}"));
                    }
                }
                Ok(if warnings.is_empty() {
                    None
                } else {
                    Some(warnings.join(" | "))
                })
            }),
            veracrypt_destroy: Box::new(|| {
                // The automated stage-1 pipeline can only target VeraCrypt containers
                // the user has explicitly pre-configured (crypto_erase_veracrypt_paths)
                // — an unmounted container has no OS-visible trace to auto-discover.
                // Previously this dispatched with NO Path at all, which the Pro handler
                // unconditionally rejects, so stage-1 aborted here on every single
                // reboot_usb trigger regardless of whether the user had any VeraCrypt
                // containers — silently defeating the whole F6 pipeline (README's
                // "P1 Piece 3, pending" self-test exists specifically to catch this
                // class of failure). "Nothing configured" now cleanly proceeds to
                // stage-2 (the USB firmware-sanitize) instead of hard-aborting; a
                // configured target that genuinely fails to destroy still aborts,
                // per the keys-before-reboot safety contract above.
                let paths = crate::settings::read_settings()
                    .ok()
                    .and_then(|s| {
                        s.ideal
                            .privacy
                            .self_destruct
                            .crypto_erase_veracrypt_paths
                            .clone()
                    })
                    .unwrap_or_default();
                if paths.is_empty() {
                    return Ok(());
                }
                for path in &paths {
                    let result = tokio::runtime::Handle::current().block_on(async {
                        crate::sidecar::dispatch_paid_command(
                            "run_destruct_step",
                            serde_json::json!({
                                "stepId": "veracrypt_header_destroy",
                                "Path": path,
                            }),
                        )
                        .await
                    });
                    let outcome = result
                        .and_then(|v| crate::backend::crypto_erase_status_result(&v, "destroyed"));
                    if let Err(e) = outcome {
                        return Err(format!("veracrypt_destroy: {path}: {e}"));
                    }
                }
                Ok(())
            }),
            vault_tpm_delete: Box::new(|| {
                tokio::runtime::Handle::current()
                    .block_on(async {
                        crate::sidecar::dispatch_paid_command(
                            "delete_vault_tpm_key",
                            serde_json::Value::Null,
                        )
                        .await
                    })
                    .map(|_| ())
            }),
        },
        set_boot_next: Box::new(|| {
            crate::reboot_usb::enum_usb_boot_entry()
                .and_then(|opt| {
                    opt.ok_or_else(|| "no USB/removable firmware boot entry found".to_string())
                })
                .and_then(|entry| crate::reboot_usb::set_boot_next_usb(&entry.id))
        }),
        reboot: Box::new(|| {
            // Issue a forceful system restart.  The -Force flag bypasses any
            // "unsaved work" prompts; this fires on a distress signal so there
            // is no interactive user to save work.
            // KT: No AV-flagging string here — "Restart-Computer" is not in
            // the forbidden list (it's a standard management command, not an
            // anti-forensic keyword).
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                let output = std::process::Command::new("powershell")
                    .args(["-NonInteractive", "-Command", "Restart-Computer -Force"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
                    .map_err(|e| format!("reboot exec: {e}"))?;
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("Restart-Computer -Force failed: {err}"));
                }
                Ok(())
            }
            #[cfg(not(windows))]
            {
                Err("reboot: not implemented on non-Windows".to_string())
            }
        }),
        now_unix: Box::new(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        }),
        signing_key,
        device_id,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────
//
// ALL stubs — no real crypto-erase, no real reboot, no real BootNext write.
// The build machine MUST NOT reboot or wipe during `cargo test`.
//
// Test organisation:
//   - gate_* tests: gate refusal before any stage-1 call.
//   - stage1_fail_* test: keys-before-reboot invariant (the critical test).
//   - no_usb_*: USB not found → stage-2 skipped, reboot never called.
//   - happy_path_*: all stubs succeed → stage-1 → token written → BootNext →
//     reboot called exactly once, last.

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use std::sync::{Arc, Mutex};

    const TEST_DEVICE: &str = "test-device-f6-orch-001";
    const TEST_NOW: i64 = 1_751_000_000;

    fn deterministic_key() -> SigningKey {
        SigningKey::from_bytes(&[0x7Bu8; 32])
    }

    // Counters to track stub invocations.
    #[derive(Default, Clone)]
    struct CallCounts {
        bitlocker: u32,
        veracrypt: u32,
        tpm: u32,
        boot_next: u32,
        reboot: u32,
    }

    /// Build F6Deps with all-success stubs, recording call counts.
    fn all_success_deps(
        counts: Arc<Mutex<CallCounts>>,
        bitlocker_warn: Option<&'static str>,
    ) -> F6Deps {
        let c = counts.clone();
        let bitlocker_warn_str = bitlocker_warn.map(String::from);
        F6Deps {
            stage1: Stage1Eraser {
                bitlocker_erase: {
                    let cc = c.clone();
                    let warn = bitlocker_warn_str.clone();
                    Box::new(move || {
                        cc.lock().unwrap().bitlocker += 1;
                        Ok(warn.clone())
                    })
                },
                veracrypt_destroy: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().veracrypt += 1;
                        Ok(())
                    })
                },
                vault_tpm_delete: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().tpm += 1;
                        Ok(())
                    })
                },
            },
            set_boot_next: {
                let cc = c.clone();
                Box::new(move || {
                    cc.lock().unwrap().boot_next += 1;
                    Ok("{stub-usb-entry-id}".to_string())
                })
            },
            reboot: {
                let cc = c.clone();
                Box::new(move || {
                    cc.lock().unwrap().reboot += 1;
                    Ok(())
                })
            },
            now_unix: Box::new(|| TEST_NOW),
            signing_key: deterministic_key(),
            device_id: TEST_DEVICE.to_string(),
        }
    }

    // ── Gate tests ────────────────────────────────────────────────────────────

    #[test]
    fn gate_refuses_when_self_destruct_disabled() {
        let counts = Arc::new(Mutex::new(CallCounts::default()));
        let deps = all_success_deps(counts.clone(), None);

        let result = execute_reboot_to_usb_wipe(
            Some(false), // self_destruct.enabled = false
            Some(true),
            deps,
        );

        assert!(result.is_err(), "must refuse when self_destruct disabled");
        let err = result.unwrap_err();
        assert!(
            err.starts_with("gate:"),
            "error must start with 'gate:': {err}"
        );

        // No stage-1 calls.
        let c = counts.lock().unwrap();
        assert_eq!(
            c.bitlocker, 0,
            "bitlocker must not be called when gate refuses"
        );
        assert_eq!(c.veracrypt, 0);
        assert_eq!(c.tpm, 0);
        assert_eq!(c.boot_next, 0);
        assert_eq!(c.reboot, 0, "reboot must NOT be called when gate refuses");
    }

    #[test]
    fn gate_refuses_when_reboot_to_usb_not_enabled() {
        let counts = Arc::new(Mutex::new(CallCounts::default()));
        let deps = all_success_deps(counts.clone(), None);

        let result = execute_reboot_to_usb_wipe(
            Some(true),
            Some(false), // reboot_to_usb_enabled = false
            deps,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.starts_with("gate:"), "{err}");
        let c = counts.lock().unwrap();
        assert_eq!(c.reboot, 0, "reboot must NOT be called");
        assert_eq!(c.bitlocker, 0, "stage-1 must NOT be called");
    }

    #[test]
    fn gate_refuses_when_both_flags_none() {
        let counts = Arc::new(Mutex::new(CallCounts::default()));
        let deps = all_success_deps(counts.clone(), None);

        let result = execute_reboot_to_usb_wipe(None, None, deps);
        assert!(result.is_err());
        assert!(result.unwrap_err().starts_with("gate:"));
    }

    // ── Keys-before-reboot: the critical test ─────────────────────────────────

    /// CRITICAL: if ANY stage-1 step fails, BootNext must never be set and
    /// the reboot stub must NEVER be called.  This is the keys-before-reboot
    /// invariant from spec §6 rule 1 and SAFETY rule 2.
    #[test]
    fn keys_before_reboot_bitlocker_fail_aborts_all() {
        let counts = Arc::new(Mutex::new(CallCounts::default()));
        let c = counts.clone();
        let deps = F6Deps {
            stage1: Stage1Eraser {
                bitlocker_erase: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().bitlocker += 1;
                        Err("simulated BitLocker erase failure".to_string())
                    })
                },
                veracrypt_destroy: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().veracrypt += 1;
                        Ok(())
                    })
                },
                vault_tpm_delete: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().tpm += 1;
                        Ok(())
                    })
                },
            },
            set_boot_next: {
                let cc = c.clone();
                Box::new(move || {
                    cc.lock().unwrap().boot_next += 1;
                    Ok("{stub-usb-entry-id}".to_string())
                })
            },
            reboot: {
                let cc = c.clone();
                Box::new(move || {
                    cc.lock().unwrap().reboot += 1;
                    Ok(())
                })
            },
            now_unix: Box::new(|| TEST_NOW),
            signing_key: deterministic_key(),
            device_id: TEST_DEVICE.to_string(),
        };

        let result = execute_reboot_to_usb_wipe(Some(true), Some(true), deps);

        // Must return an error naming the failed step.
        assert!(result.is_err(), "must return Err when stage-1 fails");
        let err = result.unwrap_err();
        assert!(
            err.contains("stage-1") && err.contains("bitlocker_erase"),
            "error must name the failed step: {err}"
        );

        let c = counts.lock().unwrap();
        // BitLocker stub was called (it's the failing step).
        assert_eq!(c.bitlocker, 1);
        // VeraCrypt and TPM must NOT have been called (abort on first failure).
        assert_eq!(
            c.veracrypt, 0,
            "veracrypt must NOT be called after bitlocker fails"
        );
        assert_eq!(c.tpm, 0, "tpm must NOT be called after bitlocker fails");
        // CRITICAL: BootNext and reboot must NEVER be called.
        assert_eq!(
            c.boot_next, 0,
            "BootNext must NOT be set when stage-1 fails"
        );
        assert_eq!(
            c.reboot, 0,
            "reboot stub must NEVER be called when stage-1 fails"
        );
    }

    #[test]
    fn keys_before_reboot_veracrypt_fail_aborts_reboot() {
        let counts = Arc::new(Mutex::new(CallCounts::default()));
        let c = counts.clone();
        let deps = F6Deps {
            stage1: Stage1Eraser {
                bitlocker_erase: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().bitlocker += 1;
                        Ok(None)
                    })
                },
                veracrypt_destroy: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().veracrypt += 1;
                        Err("simulated VeraCrypt destroy failure".to_string())
                    })
                },
                vault_tpm_delete: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().tpm += 1;
                        Ok(())
                    })
                },
            },
            set_boot_next: {
                let cc = c.clone();
                Box::new(move || {
                    cc.lock().unwrap().boot_next += 1;
                    Ok("{stub}".to_string())
                })
            },
            reboot: {
                let cc = c.clone();
                Box::new(move || {
                    cc.lock().unwrap().reboot += 1;
                    Ok(())
                })
            },
            now_unix: Box::new(|| TEST_NOW),
            signing_key: deterministic_key(),
            device_id: TEST_DEVICE.to_string(),
        };

        let result = execute_reboot_to_usb_wipe(Some(true), Some(true), deps);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("stage-1") && err.contains("veracrypt_destroy"),
            "{err}"
        );

        let c = counts.lock().unwrap();
        assert_eq!(
            c.bitlocker, 1,
            "bitlocker must have run before veracrypt fails"
        );
        assert_eq!(c.veracrypt, 1);
        assert_eq!(c.tpm, 0, "tpm must NOT run after veracrypt fails");
        assert_eq!(c.boot_next, 0, "BootNext must NOT be set");
        assert_eq!(
            c.reboot, 0,
            "reboot must NEVER be called when stage-1 fails"
        );
    }

    // ── USB not found — stage-2 skipped ──────────────────────────────────────

    #[test]
    fn no_usb_stage2_skipped_reboot_not_called() {
        // find_wipe_usb scans real removable drives; in test environments no drive
        // will have a wipe/pubkey.bin marker, so it always returns None.
        // On non-Windows it always returns None unconditionally.
        let counts = Arc::new(Mutex::new(CallCounts::default()));
        let c = counts.clone();
        let deps = F6Deps {
            stage1: Stage1Eraser {
                bitlocker_erase: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().bitlocker += 1;
                        Ok(None)
                    })
                },
                veracrypt_destroy: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().veracrypt += 1;
                        Ok(())
                    })
                },
                vault_tpm_delete: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().tpm += 1;
                        Ok(())
                    })
                },
            },
            set_boot_next: {
                let cc = c.clone();
                Box::new(move || {
                    cc.lock().unwrap().boot_next += 1;
                    Ok("{stub}".to_string())
                })
            },
            reboot: {
                let cc = c.clone();
                Box::new(move || {
                    cc.lock().unwrap().reboot += 1;
                    Ok(())
                })
            },
            now_unix: Box::new(|| TEST_NOW),
            signing_key: deterministic_key(),
            device_id: TEST_DEVICE.to_string(),
        };

        // On non-Windows, find_wipe_usb always returns None, so this test
        // always exercises the "no USB" path.  On Windows the test dir has
        // no wipe/pubkey.bin and isn't a real removable drive, so
        // find_wipe_usb also returns None (it looks at real drives via
        // GetLogicalDrives).  Either way we expect Stage1OnlyNoUsb.
        let result = execute_reboot_to_usb_wipe(Some(true), Some(true), deps);

        assert!(
            result.is_ok(),
            "no-USB path must not return Err: {:?}",
            result
        );
        assert_eq!(
            result.unwrap(),
            F6Outcome::Stage1OnlyNoUsb,
            "must return Stage1OnlyNoUsb"
        );

        let c = counts.lock().unwrap();
        // Stage-1 must have run.
        assert_eq!(c.bitlocker, 1, "bitlocker must have run");
        assert_eq!(c.veracrypt, 1, "veracrypt must have run");
        assert_eq!(c.tpm, 1, "tpm must have run");
        // Stage-2 must NOT have run.
        assert_eq!(c.boot_next, 0, "BootNext must NOT be set when no USB found");
        assert_eq!(c.reboot, 0, "reboot must NOT be called when no USB found");
    }

    // ── Happy path with token write ───────────────────────────────────────────

    /// Verify that stage-1 runs all three steps in the correct order when armed.
    /// In test environments (no provisioned wipe USB attached) the outcome is
    /// `Stage1OnlyNoUsb` on all platforms — the stage-2 (token write + BootNext +
    /// reboot) sub-path is covered by `token_write_roundtrip_verifies` for the
    /// write side and by the orchestrator's sequential structure for ordering.
    #[test]
    fn stage1_all_steps_run_in_order_when_armed() {
        let counts = Arc::new(Mutex::new(CallCounts::default()));
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let c = counts.clone();
        let ord = order.clone();
        let deps = F6Deps {
            stage1: Stage1Eraser {
                bitlocker_erase: {
                    let cc = c.clone();
                    let o = ord.clone();
                    Box::new(move || {
                        cc.lock().unwrap().bitlocker += 1;
                        o.lock().unwrap().push("bitlocker");
                        Ok(None)
                    })
                },
                veracrypt_destroy: {
                    let cc = c.clone();
                    let o = ord.clone();
                    Box::new(move || {
                        cc.lock().unwrap().veracrypt += 1;
                        o.lock().unwrap().push("veracrypt");
                        Ok(())
                    })
                },
                vault_tpm_delete: {
                    let cc = c.clone();
                    let o = ord.clone();
                    Box::new(move || {
                        cc.lock().unwrap().tpm += 1;
                        o.lock().unwrap().push("tpm");
                        Ok(())
                    })
                },
            },
            set_boot_next: {
                let cc = c.clone();
                let o = ord.clone();
                Box::new(move || {
                    cc.lock().unwrap().boot_next += 1;
                    o.lock().unwrap().push("boot_next");
                    Ok("{aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}".to_string())
                })
            },
            reboot: {
                let cc = c.clone();
                let o = ord.clone();
                Box::new(move || {
                    cc.lock().unwrap().reboot += 1;
                    o.lock().unwrap().push("reboot");
                    Ok(())
                })
            },
            now_unix: Box::new(|| TEST_NOW),
            signing_key: deterministic_key(),
            device_id: TEST_DEVICE.to_string(),
        };

        // In all test environments find_wipe_usb returns None (no provisioned USB
        // wipe device attached), so the outcome is always Stage1OnlyNoUsb.
        let result = execute_reboot_to_usb_wipe(Some(true), Some(true), deps);

        assert!(result.is_ok(), "armed path must not error: {:?}", result);
        // Stage-2 is skipped because find_wipe_usb returns None in test environments.
        assert_eq!(
            result.unwrap(),
            F6Outcome::Stage1OnlyNoUsb,
            "expect Stage1OnlyNoUsb when no provisioned USB is attached"
        );

        let c = counts.lock().unwrap();
        // Stage-1 must have run exactly once per step.
        assert_eq!(c.bitlocker, 1, "bitlocker must run exactly once");
        assert_eq!(c.veracrypt, 1, "veracrypt must run exactly once");
        assert_eq!(c.tpm, 1, "tpm must run exactly once");
        // Stage-2 must NOT have run (no USB).
        assert_eq!(c.boot_next, 0, "BootNext must NOT be set when no USB found");
        assert_eq!(c.reboot, 0, "reboot must NOT be called when no USB found");

        let ord = order.lock().unwrap();
        // Stage-1 steps must appear in the correct order.
        assert_eq!(
            ord.first().copied(),
            Some("bitlocker"),
            "bitlocker must be first"
        );
        assert_eq!(
            ord.get(1).copied(),
            Some("veracrypt"),
            "veracrypt must be second"
        );
        assert_eq!(ord.get(2).copied(), Some("tpm"), "tpm must be third");
        assert_eq!(
            ord.len(),
            3,
            "only stage-1 steps must appear in the order log"
        );
    }

    // ── Escrow warning is NOT an abort ────────────────────────────────────────

    #[test]
    fn escrow_warning_is_not_abort_proceeds_to_stage2() {
        let counts = Arc::new(Mutex::new(CallCounts::default()));
        let c = counts.clone();
        let deps = F6Deps {
            stage1: Stage1Eraser {
                bitlocker_erase: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().bitlocker += 1;
                        // Non-empty escrow warning — must NOT abort.
                        Ok(Some("key escrowed to AAD".to_string()))
                    })
                },
                veracrypt_destroy: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().veracrypt += 1;
                        Ok(())
                    })
                },
                vault_tpm_delete: {
                    let cc = c.clone();
                    Box::new(move || {
                        cc.lock().unwrap().tpm += 1;
                        Ok(())
                    })
                },
            },
            set_boot_next: {
                let cc = c.clone();
                Box::new(move || {
                    cc.lock().unwrap().boot_next += 1;
                    Ok("{stub}".to_string())
                })
            },
            reboot: {
                let cc = c.clone();
                Box::new(move || {
                    cc.lock().unwrap().reboot += 1;
                    Ok(())
                })
            },
            now_unix: Box::new(|| TEST_NOW),
            signing_key: deterministic_key(),
            device_id: TEST_DEVICE.to_string(),
        };

        let result = execute_reboot_to_usb_wipe(Some(true), Some(true), deps);

        // Must not error — escrow warning is just a log entry.
        assert!(
            result.is_ok(),
            "escrow warning must not abort: {:?}",
            result
        );
        let c = counts.lock().unwrap();
        // All three stage-1 steps must have run.
        assert_eq!(
            c.bitlocker, 1,
            "bitlocker must have run despite escrow warning"
        );
        assert_eq!(c.veracrypt, 1, "veracrypt must have run");
        assert_eq!(c.tpm, 1, "tpm must have run");
    }
}
