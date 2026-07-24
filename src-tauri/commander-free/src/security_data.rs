// SPDX-License-Identifier: AGPL-3.0-or-later
//! Trusted Free-side wrappers for read-only Pro security posture data.

use serde_json::{json, Value};

async fn dispatch(feature_id: &str, args: Value) -> Result<Value, String> {
    crate::license::require_paid("security posture data")?;
    if crate::settings::is_decoy_mode() {
        return Err("Refused: security posture data is unavailable in Decoy mode.".into());
    }
    crate::sidecar::dispatch_paid_command(feature_id, args).await
}

#[tauri::command]
pub async fn security_threat_snapshot() -> Result<Value, String> {
    dispatch("security_threat_snapshot", json!({})).await
}

#[tauri::command]
pub async fn security_cve_snapshot() -> Result<Value, String> {
    dispatch("security_cve_snapshot", json!({})).await
}

#[tauri::command]
pub async fn security_breach_monitor_status(opt_in: bool) -> Result<Value, String> {
    dispatch("security_breach_monitor_status", json!({ "optIn": opt_in })).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn breach_wrapper_exposes_only_boolean_opt_in() {
        let value = serde_json::json!({ "optIn": true });
        assert_eq!(value.as_object().map(|object| object.len()), Some(1));
    }
}
