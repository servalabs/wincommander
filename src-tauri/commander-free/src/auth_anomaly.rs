// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/auth_anomaly.rs
//
// Free-side dispatch wrappers for the access-session (auth-anomaly) monitor.
// The Security-log detector lives in commander-pro/src/auth_anomaly.rs under the
// feature_ids `start_auth_anomaly_monitor`, `stop_auth_anomaly_monitor`,
// `auth_anomaly_status`, `get_auth_anomaly_recent`, `clear_auth_anomaly_recent`.
// Reads dispatch ungated (Pro's tier gate + the panel gating enforce
// entitlement, the driver_health precedent); start/stop/clear call require_paid.

use serde_json::Value;

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
