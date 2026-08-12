// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/remote_access.rs
//
// ═══════════════════════════════════════════════════════════════════════
// REMOTE-ACCESS MONITOR — Free-side dispatch wrappers (#4)
// ═══════════════════════════════════════════════════════════════════════
//
// The paid implementation (poll task, tool catalogue, TCP correlation,
// log parsers, confidence scoring, per-session debounce, recent-fires
// ring) lives in commander-pro/src/remote_access.rs. Free retains the
// seven Tauri commands invoked from RemoteAccessMonitorSection.tsx —
// bodies are thin dispatch wrappers via sidecar::dispatch_paid_command.
//
// This is the long-running-detector forwarding pattern (B2): dedicated
// #[tauri::command] wrappers registered in lib.rs that bypass
// run_backend_script / get_command_tier entirely. dispatch_paid_command
// itself enforces require_paid / PRO_NOT_INSTALLED, and the mutators
// additionally gate locally with license::require_paid so a clear error
// surfaces before the IPC round-trip. Mirrors network_honeypot.rs.
//
// Detection events arrive over IPC as Notifications, re-emitted by Free's
// sidecar reader:
//   - "remote-access-detected" → frontend listener via app.emit
//   - "wc-native-toast"        → sidecar reader opens the custom alert window
//
// RemoteAccessHit / RemoteAccessStatus / ToolEntry shapes stay in this
// file so serde can decode the dispatch returns.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolEntry {
    pub id: String,
    pub label: String,
    pub process_names: Vec<String>,
    pub ports: Vec<u16>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAccessHit {
    pub tool: String,
    /// "info" | "high"
    pub confidence: String,
    /// "processPresent" | "establishedTcp" | "logEntry" | "both"
    pub reason: String,
    pub port: Option<u16>,
    pub peer: Option<String>,
    pub log_hint: Option<String>,
    pub detected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAccessStatus {
    pub running: bool,
    pub watching_tools: u32,
    pub triggered: bool,
}

#[tauri::command]
pub async fn start_remote_access_monitor(
    _app: tauri::AppHandle,
) -> Result<RemoteAccessStatus, String> {
    crate::license::require_paid("remote-access monitor")?;
    let v = crate::sidecar::dispatch_paid_command(
        "start_remote_access_monitor",
        serde_json::Value::Null,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("remote-access start decode: {}", e))
}

#[tauri::command]
pub async fn stop_remote_access_monitor() -> Result<(), String> {
    let _ = crate::sidecar::dispatch_paid_command(
        "stop_remote_access_monitor",
        serde_json::Value::Null,
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn remote_access_monitor_status() -> Result<RemoteAccessStatus, String> {
    let v = crate::sidecar::dispatch_paid_command(
        "remote_access_monitor_status",
        serde_json::Value::Null,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("remote-access status decode: {}", e))
}

#[tauri::command]
pub async fn get_remote_access_recent() -> Result<Vec<RemoteAccessHit>, String> {
    let v =
        crate::sidecar::dispatch_paid_command("get_remote_access_recent", serde_json::Value::Null)
            .await?;
    serde_json::from_value(v).map_err(|e| format!("remote-access recent decode: {}", e))
}

#[tauri::command]
pub async fn clear_remote_access_recent() -> Result<(), String> {
    let _ = crate::sidecar::dispatch_paid_command(
        "clear_remote_access_recent",
        serde_json::Value::Null,
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn get_remote_access_tools() -> Result<Vec<ToolEntry>, String> {
    let v =
        crate::sidecar::dispatch_paid_command("get_remote_access_tools", serde_json::Value::Null)
            .await?;
    serde_json::from_value(v).map_err(|e| format!("remote-access tools decode: {}", e))
}

#[tauri::command]
pub async fn set_remote_access_tool_enabled(tool_id: String, enabled: bool) -> Result<(), String> {
    crate::license::require_paid("remote-access monitor")?;
    let _ = crate::sidecar::dispatch_paid_command(
        "set_remote_access_tool_enabled",
        serde_json::json!({ "toolId": tool_id, "enabled": enabled }),
    )
    .await?;
    Ok(())
}
