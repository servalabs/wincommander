// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/session_assurance.rs
//
// Free-side dispatch wrappers for Session Assurance (Pro insider-risk /
// attention monitoring). The webcam-backed attention collector lives in
// commander-pro/src/attention_collector.rs under the feature IDs below.
//
// Gating:
//   mutations (start, stop) → require_paid
//   reads (status, score, alerts) → ungated
//
// Follows the auth_anomaly.rs shape exactly.

use serde_json::{json, Value};

// ── Lifecycle ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_session_monitor(
    subject_id: String,
    org_id: String,
    device_id: String,
    model_level: Option<String>,
    check_gaze: Option<bool>,
    check_faces: Option<bool>,
    check_secondary_device: Option<bool>,
) -> Result<Value, String> {
    crate::license::require_paid("session assurance monitor")?;
    crate::sidecar::dispatch_paid_command(
        "start_session_monitor",
        json!({
            "subject_id": subject_id,
            "org_id": org_id,
            "device_id": device_id,
            "model_level": model_level,
            "check_gaze": check_gaze,
            "check_faces": check_faces,
            "check_secondary_device": check_secondary_device,
        }),
    )
    .await
}

#[tauri::command]
pub async fn stop_session_monitor(subject_id: String, org_id: String) -> Result<Value, String> {
    crate::license::require_paid("session assurance monitor")?;
    crate::sidecar::dispatch_paid_command(
        "stop_session_monitor",
        json!({
            "subject_id": subject_id,
            "org_id": org_id,
        }),
    )
    .await
}

#[tauri::command]
pub async fn session_monitor_status(subject_id: String, org_id: String) -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command(
        "session_monitor_status",
        json!({
            "subject_id": subject_id,
            "org_id": org_id,
        }),
    )
    .await
}

// ── Scoring + Alerts ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_session_score(
    org_id: String,
    subject_id: String,
    window_minutes: Option<u32>,
    on_policy_apps: Option<bool>,
) -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command(
        "get_session_score",
        json!({
            "org_id": org_id,
            "subject_id": subject_id,
            "window_minutes": window_minutes,
            "on_policy_apps": on_policy_apps,
        }),
    )
    .await
}

#[tauri::command]
pub async fn get_active_alerts(org_id: String) -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command("get_active_alerts", json!({ "org_id": org_id })).await
}
