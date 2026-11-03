use serde_json::{Value, json};

pub(super) fn verified_transition(command: &str, receipt: &Value) -> Option<Value> {
    if receipt.get("success").and_then(Value::as_bool) != Some(true)
        || super::receipt::is_explicit_failure(receipt)
    {
        return None;
    }
    match command {
        "Start-PrivacyShield"
            if receipt.get("ready").and_then(Value::as_bool) == Some(true)
                && receipt.get("shieldLifecycle").and_then(Value::as_str) == Some("running") =>
        {
            let mut event = json!({ "running": true, "phase": "running" });
            if let Some(mode @ ("blur_notify" | "notify_only")) =
                receipt.get("activeMode").and_then(Value::as_str)
            {
                event["activeMode"] = json!(mode);
            }
            Some(event)
        }
        "Stop-PrivacyShield"
            if receipt.get("running").and_then(Value::as_bool) == Some(false)
                && receipt.get("shieldLifecycle").and_then(Value::as_str) == Some("stopped") =>
            Some(json!({ "running": false, "phase": "stopped" })),
        _ => None,
    }
}

pub(super) fn emit_verified_transition(app: &tauri::AppHandle, command: &str, receipt: &Value) {
    use tauri::Emitter;
    if let Some(event) = verified_transition(command, receipt) {
        let _ = app.emit("privacy-shield-status-changed", event);
    }
}

pub(crate) async fn stop_privacy_shield_gracefully(app: tauri::AppHandle) -> Result<(), String> {
    let receipt = super::run_backend_script(app, "Stop-PrivacyShield".into(), std::collections::HashMap::new()).await?;
    if verified_transition("Stop-PrivacyShield", &receipt).is_some() {
        Ok(())
    } else {
        Err("Privacy Shield stop could not be confirmed; check its status and try again".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_pid_or_success_alone_is_not_verified_protection() {
        for receipt in [json!({"success": true, "processId": 7}),
            json!({"success": true, "ready": false, "shieldLifecycle": "running"}),
            json!({"success": true, "ready": true, "shieldLifecycle": "starting"}),
            json!({"success": true, "ready": true, "shieldLifecycle": "running", "error": true})]
        {
            assert!(verified_transition("Start-PrivacyShield", &receipt).is_none());
        }
    }

    #[test]
    fn only_confirmed_transitions_emit_bounded_applied_state() {
        let start = verified_transition("Start-PrivacyShield", &json!({
            "success": true, "ready": true, "shieldLifecycle": "running",
            "activeMode": "notify_only", "processId": 8, "private": "excluded"
        })).unwrap();
        assert_eq!(start, json!({"running": true, "phase": "running", "activeMode": "notify_only"}));
        assert_eq!(verified_transition("Stop-PrivacyShield", &json!({"success": true, "running": false, "shieldLifecycle": "stopped"})),
            Some(json!({"running": false, "phase": "stopped"})));
        assert!(verified_transition("Stop-PrivacyShield", &json!({"success": true})).is_none());
        assert!(verified_transition("Stop-PrivacyShield", &json!({"success": false})).is_none());
        assert!(verified_transition("Get-PrivacyShieldStatus", &json!({"success": true})).is_none());
    }
}
