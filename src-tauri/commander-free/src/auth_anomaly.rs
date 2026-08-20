// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/auth_anomaly.rs
//
// Free-side dispatch wrappers for the access-session (auth-anomaly) monitor.
// The Security-log detector lives in commander-pro/src/auth_anomaly.rs under the
// feature_ids `start_auth_anomaly_monitor`, `stop_auth_anomaly_monitor`,
// `auth_anomaly_status`, `get_auth_anomaly_recent`, `clear_auth_anomaly_recent`,
// and `set_auth_anomaly_config`.
// Reads dispatch ungated (Pro's tier gate + the panel gating enforce
// entitlement, the driver_health precedent); start/stop/clear call require_paid.

use serde_json::{json, Value};

#[tauri::command]
pub async fn start_auth_anomaly_monitor() -> Result<Value, String> {
    crate::license::require_paid("access-session monitor")?;
    crate::sidecar::dispatch_paid_command("start_auth_anomaly_monitor", Value::Null).await
}

#[tauri::command]
pub async fn stop_auth_anomaly_monitor() -> Result<Value, String> {
    crate::license::require_paid("access-session monitor")?;
    crate::sidecar::dispatch_paid_command("stop_auth_anomaly_monitor", Value::Null).await
}

#[tauri::command]
pub async fn auth_anomaly_status() -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command("auth_anomaly_status", Value::Null).await
}

#[tauri::command]
pub async fn get_auth_anomaly_recent() -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command("get_auth_anomaly_recent", Value::Null).await
}

#[tauri::command]
pub async fn clear_auth_anomaly_recent() -> Result<Value, String> {
    crate::license::require_paid("access-session monitor")?;
    crate::sidecar::dispatch_paid_command("clear_auth_anomaly_recent", Value::Null).await
}

#[tauri::command]
pub async fn set_auth_anomaly_config(config: Value) -> Result<Value, String> {
    crate::license::require_paid("access-session monitor")?;
    let require_fleet_reporting = crate::settings::read_settings()?
        .ideal
        .security
        .require_all_device_alerts_in_fleet;
    crate::sidecar::dispatch_paid_command(
        "set_auth_anomaly_config",
        enforce_required_fleet_reporting(config, require_fleet_reporting),
    )
    .await
}

/// Signed device policy is authoritative over this monitor's local Fleet
/// preference. Keep this at the Free→Pro boundary so direct IPC cannot weaken
/// a managed requirement before the sidecar receives the policy.
fn enforce_required_fleet_reporting(mut config: Value, required: bool) -> Value {
    if required {
        if let Some(object) = config.as_object_mut() {
            object.insert("reportToFleet".into(), json!(true));
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_fleet_policy_overrides_a_local_auth_reporting_disable() {
        let config = enforce_required_fleet_reporting(json!({ "reportToFleet": false }), true);
        assert_eq!(config["reportToFleet"], json!(true));
    }
}
