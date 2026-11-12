use std::collections::HashMap;

use serde_json::{json, Value};

use super::catalog::Scenario;
use super::parser::Request;
use super::result::{failed, passed, sha256, ResultEnvelope};

pub(super) async fn execute(app: tauri::AppHandle, request: &Request) -> ResultEnvelope {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(request.deadline_ms);
    let facts = if request.scenario == Scenario::PrivacyShieldStartStop {
        privacy_start_stop().await
    } else {
        tokio::time::timeout_at(deadline, async {
            match request.scenario {
        Scenario::FleetPreflight => fleet_preflight(),
        Scenario::FleetCheckinReadback => fleet_checkin_readback().await,
        Scenario::ClipboardGuardSyntheticMarker => clipboard_marker(app, request).await,
        Scenario::PrivacyShieldStatus => privacy_status(app).await,
        Scenario::PrivacyShieldStartStop => unreachable!(),
            }
        }).await.unwrap_or(Err("deadline_exceeded_outcome_unknown"))
    };
    match facts {
        Ok(facts) => passed(request, facts),
        Err(code) => failed(request, code),
    }
}

fn fleet_preflight() -> Result<Value, &'static str> {
    let settings = crate::settings::read_settings().map_err(|_| "settings_unavailable")?;
    Ok(json!({
        "fleetConfigured": settings.app.fleet.enabled && !settings.app.fleet.server_url.is_empty(),
        "pinnedSigningKey": settings.policy.fleet_signing_key.is_some(),
        "deviceIdHash": sha256(&settings.device_id),
    }))
}

async fn fleet_checkin_readback() -> Result<Value, &'static str> {
    let status = crate::fleet_agent::fleet_status()
        .await
        .map_err(|_| "fleet_status_unavailable")?;
    Ok(json!({
        "connected": status.get("connected").and_then(Value::as_bool).unwrap_or(false),
        "pendingApproval": status.get("pendingApproval").and_then(Value::as_bool).unwrap_or(false),
        "hasEnrollmentTimestamp": status.get("lastEnrollAt").and_then(Value::as_str).is_some(),
        "hasTerminalError": status.get("lastError").and_then(Value::as_str).is_some(),
    }))
}

async fn clipboard_marker(app: tauri::AppHandle, request: &Request) -> Result<Value, &'static str> {
    let before = crate::paste_monitor::get_paste_monitor_recent()
        .await
        .map_err(|_| "clipboard_observe_failed")?
        .len();
    crate::paste_monitor::start_paste_monitor(app)
        .await
        .map_err(|_| "clipboard_start_failed")?;
    let marker = format!(
        "AKIA{}",
        sha256(&format!("{}:{}", request.run_id, request.fixture_id))[7..23].to_ascii_uppercase()
    );
    #[cfg(windows)]
    clipboard_win::set_clipboard_string(&marker).map_err(|_| "clipboard_fixture_write_failed")?;
    #[cfg(not(windows))]
    return Err("platform_unsupported");
    tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;
    let after = crate::paste_monitor::get_paste_monitor_recent()
        .await
        .map_err(|_| "clipboard_observe_failed")?
        .len();
    let health = crate::paste_monitor::get_paste_monitor_health()
        .await
        .map_err(|_| "clipboard_observe_failed")?;
    if after <= before {
        return Err("clipboard_marker_not_observed");
    }
    Ok(json!({
        "markerDigest": sha256(&marker),
        "detectionCountDelta": after - before,
        "listenerRegistered": health.listener_registered,
        "rulesCompiled": health.rules_compiled,
        "clearFailing": health.clear_failing,
    }))
}

async fn privacy_status(app: tauri::AppHandle) -> Result<Value, &'static str> {
    let result = run_privacy(app, "Get-PrivacyShieldStatus").await?;
    let running = privacy_running(&result)?;
    Ok(json!({ "statusObserved": true, "running": running,
        "protectionReady": result.get("protectionReady").and_then(Value::as_bool).unwrap_or(false),
        "statusHash": sha256(&result.to_string()) }))
}

async fn privacy_start_stop() -> Result<Value, &'static str> {
    // The generic backend cannot yet prove a cancelled Start has settled before Stop.
    Err("privacy_lifecycle_settlement_unavailable")
}
fn privacy_running(value: &Value) -> Result<bool, &'static str> {
    value.get("running").and_then(Value::as_bool).ok_or("privacy_status_schema_invalid")
}

fn validate_privacy_result(value: Value) -> Result<Value, &'static str> {
    if value.get("error").is_some_and(|error| error != &Value::Null && error != &Value::Bool(false))
        || value.get("ok").and_then(Value::as_bool) == Some(false)
        || value.get("success").and_then(Value::as_bool) == Some(false)
        || matches!(value.get("status").and_then(Value::as_str), Some("error" | "failed"))
    { return Err("privacy_semantic_failure"); }
    if !value.is_object() { return Err("privacy_status_schema_invalid"); }
    Ok(value)
}

async fn run_privacy(app: tauri::AppHandle, command: &str) -> Result<Value, &'static str> {
    crate::backend::run_backend_script(app, command.into(), HashMap::new())
        .await
        .map_err(|_| "privacy_handler_failed")
        .and_then(validate_privacy_result)
}

#[cfg(test)]
mod privacy_result_tests {
    use super::*;
    #[test]
    fn error_payload_is_never_an_observation() {
        for value in [json!({"error":true}),json!({"error":"denied"}),json!({"ok":false}),json!({"success":false}),json!({"status":"failed"}),json!(null)] {
            assert!(validate_privacy_result(value).is_err());
        }
        assert!(validate_privacy_result(json!({"running":false})).is_ok());
        assert!(privacy_running(&json!({})).is_err());
        assert_eq!(privacy_running(&json!({"running":false})), Ok(false));
    }
}
