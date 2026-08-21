// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/screen_capture_detect.rs
//
// ═══════════════════════════════════════════════════════════════════════
// SCREEN-CAPTURE DETECTOR — Free-side dispatch wrappers (#5, detection half)
// ═══════════════════════════════════════════════════════════════════════
//
// The paid detector implementation (Get-Process polling, the tool
// catalogue, per-tool debounce, hit storage, fire-event emission) lives in
// commander-pro/src/screen_capture.rs. Free retains these five thin Tauri
// commands so the frontend interface is unchanged; bodies thin-dispatch
// via sidecar::dispatch_paid_command (which itself enforces
// require_paid / PRO_NOT_INSTALLED — so these bypass run_backend_script
// and need NO get_command_tier arm, exactly like wifi_check.rs).
//
// When Pro detects a catalogued tool it fires two notifications over the
// IPC channel:
//   - "screen-capture-detected" → frontend listener (ScreenCaptureSection
//     + BackgroundPollers both listen for it)
//   - "wc-native-toast" → Free's sidecar reader opens the custom alert window
//     (Pro has no Tauri context)
//
// ScreenCaptureHit struct shape stays in this file so serde can decode the
// dispatch return values (the recent list) on Free's side.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenCaptureHit {
    pub tool: String,
    pub process_name: String,
    /// "high" | "low".
    pub confidence: String,
    pub detected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenCaptureStatus {
    pub running: bool,
    pub last_tick: Option<String>,
}

#[tauri::command]
pub async fn start_screen_capture_watch(
    _app: tauri::AppHandle,
) -> Result<ScreenCaptureStatus, String> {
    crate::license::require_paid("screen-capture detector")?;
    let v = crate::sidecar::dispatch_paid_command(
        "start_screen_capture_watch",
        serde_json::Value::Null,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("screen_capture start decode: {}", e))
}

#[tauri::command]
pub async fn stop_screen_capture_watch() -> Result<(), String> {
    let _ =
        crate::sidecar::dispatch_paid_command("stop_screen_capture_watch", serde_json::Value::Null)
            .await?;
    Ok(())
}

#[tauri::command]
pub async fn screen_capture_watch_status() -> Result<ScreenCaptureStatus, String> {
    let v = crate::sidecar::dispatch_paid_command(
        "screen_capture_watch_status",
        serde_json::Value::Null,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("screen_capture status decode: {}", e))
}

#[tauri::command]
pub async fn get_recent_screen_capture() -> Result<Vec<ScreenCaptureHit>, String> {
    let v =
        crate::sidecar::dispatch_paid_command("get_recent_screen_capture", serde_json::Value::Null)
            .await?;
    serde_json::from_value(v).map_err(|e| format!("screen_capture recent decode: {}", e))
}

#[tauri::command]
pub async fn clear_recent_screen_capture() -> Result<(), String> {
    let _ = crate::sidecar::dispatch_paid_command(
        "clear_recent_screen_capture",
        serde_json::Value::Null,
    )
    .await?;
    Ok(())
}
