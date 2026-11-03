// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reconcile the current account's shared-shortcut claim with committed state.

use serde_json::{json, Value};

static CLAIM_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn active_for_kind(kind: &str) -> Result<bool, String> {
    match kind {
        "calculator" => {
            let settings = crate::settings::read_settings()?;
            Ok(crate::startup_auth::gate_enabled(&settings.ideal.privacy.startup_pin))
        }
        "hide" => crate::paths::hide_flag_path()?
            .try_exists()
            .map_err(|_| "Could not read WinCommander visibility state.".to_string()),
        _ => Err("Unknown shared shortcut claim.".into()),
    }
}

#[tauri::command]
pub(crate) async fn reconcile_shared_shortcut_claim(
    kind: String,
    expected_active: Option<bool>,
) -> Result<Value, String> {
    if crate::settings::is_decoy_mode() {
        return Err("Shared shortcut changes are unavailable in decoy mode.".into());
    }
    let _guard = CLAIM_GATE.lock().await;
    let active = active_for_kind(&kind)?;
    if expected_active.is_some_and(|expected| expected != active) {
        return Err("Shortcut setting changed before shared links were updated.".into());
    }
    crate::svc_client::call(
        "svc.shared_shortcuts.set_claim",
        json!({ "kind": kind, "active": active }),
    )
    .await
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_unknown_claim_without_contacting_service() {
        assert!(super::active_for_kind("other").is_err());
    }
}
