//! Destructive-Action Capability — a single-use, args-bound token that a
//! compromised WebView cannot forge. Catastrophic commands require one before
//! running any effect. Minting requires an un-forgeable human/trigger factor:
//! a Rust-verified destroy/real PIN, a Rust-initiated native OS dialog, or a
//! legitimate Rust-side duress trigger (dead-man / distress-phrase / hotkey).
//!
//! A frontend confirm() dialog is NOT a control against the assumed-compromised
//! WebView (SECURITY.md threat model) — only these Rust-minted capabilities are.
//! See docs/superpowers/specs/2026-07-07-security-hardening-design.md §4.1.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const TTL: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DestructiveAction {
    SelfDestruct,
    RemoveUsers,
    CryptoErase,
    FleetReenroll,
    DiskDelete,
    DecoyDelete,
    KillSwitch,
}

struct Minted {
    action: DestructiveAction,
    args_hash: [u8; 32],
    at: Instant,
}

static STORE: Lazy<Mutex<HashMap<String, Minted>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn hash_args(args_canonical: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(args_canonical.as_bytes());
    h.finalize().into()
}

/// Mint a capability for (action, args). Returns the opaque token id.
pub fn mint(action: DestructiveAction, args_canonical: &str) -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let token = raw.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    let mut store = STORE.lock().expect("authz store poisoned");
    // Opportunistic GC of expired tokens.
    store.retain(|_, m| m.at.elapsed() < TTL);
    store.insert(
        token.clone(),
        Minted {
            action,
            args_hash: hash_args(args_canonical),
            at: Instant::now(),
        },
    );
    token
}

/// Consume a capability. Fails closed on any mismatch, expiry, or reuse.
pub fn consume(token: &str, action: DestructiveAction, args_canonical: &str) -> Result<(), String> {
    let mut store = STORE.lock().expect("authz store poisoned");
    let Some(m) = store.remove(token) else {
        return Err("confirmation required (no valid capability)".into());
    };
    if m.at.elapsed() >= TTL {
        return Err("confirmation expired — please re-confirm".into());
    }
    if m.action != action {
        return Err("confirmation does not match this action".into());
    }
    if m.args_hash != hash_args(args_canonical) {
        return Err("confirmation does not match this request".into());
    }
    Ok(())
}

/// Registry of every catastrophic Tauri command and the action it performs.
/// A CI gate (tools/ci/check-destructive-authz.sh) and the unit test
/// `registry_covers_known_catastrophic_commands` enforce that every destructive
/// command appears here, so a new one cannot silently ship without an
/// authorization review. The runtime `consume`-on-fire wiring for the
/// self-destruct family lands with the C2 fire-gate follow-up; until then the
/// registry is the CI-/test-enforced source of truth.
#[allow(dead_code)]
pub const DESTRUCTIVE_COMMANDS: &[(&str, DestructiveAction)] = &[
    ("lockdown", DestructiveAction::SelfDestruct),
    ("full_lockdown", DestructiveAction::SelfDestruct),
    ("run_destruct_step", DestructiveAction::SelfDestruct),
    ("fleet_connect", DestructiveAction::FleetReenroll),
    ("disk_delete_item", DestructiveAction::DiskDelete),
    ("delete_decoy", DestructiveAction::DecoyDelete),
    ("internet_kill_switch_set", DestructiveAction::KillSwitch),
];

#[allow(dead_code)]
pub fn action_for(command: &str) -> Option<DestructiveAction> {
    DESTRUCTIVE_COMMANDS
        .iter()
        .find(|(n, _)| *n == command)
        .map(|(_, a)| *a)
}

/// Verify a PIN against the configured startup-PIN hashes, returning the mode
/// ("real" | "decoy" | "destroy" | "open" | "wrong"). Mirrors
/// startup_auth::verify_startup_pin but is callable synchronously from Rust.
fn verify_pin_local(pin: &str) -> Result<&'static str, String> {
    let s = crate::settings::read_settings().map_err(|e| format!("read settings: {e}"))?;
    let sp = &s.ideal.privacy.startup_pin;
    Ok(crate::startup_auth::verify_pin_mode(
        pin,
        sp.real_hash.as_deref(),
        sp.decoy_hash.as_deref(),
        sp.destroy_hash.as_deref(),
    ))
}

/// Mint a capability after verifying an un-forgeable human factor.
/// - If a PIN is supplied, it must resolve to the mode required for `action`
///   (`destroy` for the self-destruct/wipe family; `real`/`destroy` otherwise).
/// - If no PIN is supplied, fall back to a Rust-initiated native confirm dialog
///   that a WebView script can display but cannot answer.
#[tauri::command]
pub async fn request_destructive_confirmation(
    app: tauri::AppHandle,
    action: DestructiveAction,
    args_canonical: String,
    pin: Option<String>,
) -> Result<String, String> {
    let needs_destroy = matches!(
        action,
        DestructiveAction::SelfDestruct
            | DestructiveAction::RemoveUsers
            | DestructiveAction::CryptoErase
    );
    let authorized = match pin {
        Some(p) if !p.trim().is_empty() => {
            let mode = verify_pin_local(&p)?;
            if needs_destroy {
                mode == "destroy"
            } else {
                mode == "real" || mode == "destroy"
            }
        }
        _ => native_confirm(&app, action).await,
    };
    if !authorized {
        return Err("confirmation failed".into());
    }
    Ok(mint(action, &args_canonical))
}

/// Rust-initiated native confirm for a destructive action, exposed for
/// command-level fallback when no capability token is supplied. A WebView
/// script can trigger the dialog but cannot answer it, so it remains a valid
/// control against the assumed-compromised WebView.
pub async fn native_confirm_action(app: &tauri::AppHandle, action: DestructiveAction) -> bool {
    native_confirm(app, action).await
}

async fn native_confirm(app: &tauri::AppHandle, action: DestructiveAction) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(format!(
            "Confirm {:?}? This action cannot be undone.",
            action
        ))
        .title("WinCommander — confirm destructive action")
        .buttons(MessageDialogButtons::OkCancel)
        .show(move |ok| {
            let _ = tx.send(ok);
        });
    rx.await.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consume_rejects_wrong_action() {
        let tok = mint(DestructiveAction::SelfDestruct, "hash-A");
        assert!(consume(&tok, DestructiveAction::RemoveUsers, "hash-A").is_err());
    }

    #[test]
    fn consume_rejects_wrong_args() {
        let tok = mint(DestructiveAction::DiskDelete, "path=/a");
        assert!(consume(&tok, DestructiveAction::DiskDelete, "path=/b").is_err());
    }

    #[test]
    fn token_is_single_use() {
        let tok = mint(DestructiveAction::KillSwitch, "on");
        assert!(consume(&tok, DestructiveAction::KillSwitch, "on").is_ok());
        assert!(consume(&tok, DestructiveAction::KillSwitch, "on").is_err());
    }

    #[test]
    fn unknown_token_refused() {
        assert!(consume("not-a-real-token", DestructiveAction::SelfDestruct, "x").is_err());
    }

    #[test]
    fn registry_covers_known_catastrophic_commands() {
        for name in [
            "lockdown",
            "full_lockdown",
            "run_destruct_step",
            "fleet_connect",
        ] {
            assert!(
                action_for(name).is_some(),
                "missing registry entry for {name}"
            );
        }
    }
}
