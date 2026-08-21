// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/network_honeypot.rs
//
// ═══════════════════════════════════════════════════════════════════════
// NETWORK HONEYPOT MONITOR — Free-side dispatch wrappers (A-4 module 7)
// ═══════════════════════════════════════════════════════════════════════
//
// The paid implementation (TCP listeners on user-selected ports, hit
// logging, per-port debounce, recent-fires ring buffer) lives in
// commander-pro/src/honeypot.rs. Free retains the ten Tauri commands
// invoked from NetworkHoneypotSection.tsx — bodies are thin dispatch
// wrappers via sidecar::dispatch_paid_command.
//
// Detection events arrive over IPC as Notifications:
//   - "network-honeypot-detected" → frontend listener via app.emit
//   - "wc-native-toast"           → sidecar reader opens the custom alert window
//
// PortEntry / HoneypotHit / HoneypotStatus shapes stay in this file so
// serde can decode the dispatch returns + the Notification payloads.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortEntry {
    pub port: u16,
    pub label: String,
    pub enabled: bool,
    /// Backward-compat with frontend; always `true` for post-2026-05 entries.
    pub custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoneypotHit {
    pub port: u16,
    pub service: String,
    pub peer: String,
    pub peek_hex: String,
    pub detected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoneypotStatus {
    pub running: bool,
    pub armed_ports: Vec<u16>,
    pub conflicting_ports: Vec<u16>,
    pub bind_all_interfaces: bool,
}

#[tauri::command]
pub async fn start_network_honeypot(_app: tauri::AppHandle) -> Result<HoneypotStatus, String> {
    crate::license::require_paid("network honeypot")?;
    let v =
        crate::sidecar::dispatch_paid_command("start_network_honeypot", serde_json::Value::Null)
            .await?;
    serde_json::from_value(v).map_err(|e| format!("honeypot start decode: {}", e))
}

#[tauri::command]
pub async fn stop_network_honeypot() -> Result<(), String> {
    let _ = crate::sidecar::dispatch_paid_command("stop_network_honeypot", serde_json::Value::Null)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn network_honeypot_status() -> Result<HoneypotStatus, String> {
    let v =
        crate::sidecar::dispatch_paid_command("network_honeypot_status", serde_json::Value::Null)
            .await?;
    serde_json::from_value(v).map_err(|e| format!("honeypot status decode: {}", e))
}

#[tauri::command]
pub async fn get_network_honeypot_recent() -> Result<Vec<HoneypotHit>, String> {
    let v = crate::sidecar::dispatch_paid_command(
        "get_network_honeypot_recent",
        serde_json::Value::Null,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("honeypot recent decode: {}", e))
}

#[tauri::command]
pub async fn clear_network_honeypot_recent() -> Result<(), String> {
    let _ = crate::sidecar::dispatch_paid_command(
        "clear_network_honeypot_recent",
        serde_json::Value::Null,
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn set_network_honeypot_bind_all_interfaces(value: bool) -> Result<(), String> {
    crate::license::require_paid("network honeypot")?;
    let _ = crate::sidecar::dispatch_paid_command(
        "set_network_honeypot_bind_all_interfaces",
        serde_json::json!({ "value": value }),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn get_network_honeypot_bind_all_interfaces() -> Result<bool, String> {
    let v = crate::sidecar::dispatch_paid_command(
        "get_network_honeypot_bind_all_interfaces",
        serde_json::Value::Null,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("honeypot bind-all decode: {}", e))
}

#[tauri::command]
pub async fn get_network_honeypot_ports() -> Result<Vec<PortEntry>, String> {
    let v = crate::sidecar::dispatch_paid_command(
        "get_network_honeypot_ports",
        serde_json::Value::Null,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("honeypot ports decode: {}", e))
}

#[tauri::command]
pub async fn set_network_honeypot_port_enabled(port: u16, enabled: bool) -> Result<(), String> {
    crate::license::require_paid("network honeypot")?;
    let _ = crate::sidecar::dispatch_paid_command(
        "set_network_honeypot_port_enabled",
        serde_json::json!({ "port": port, "enabled": enabled }),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn add_network_honeypot_custom_port(port: u16, label: String) -> Result<(), String> {
    crate::license::require_paid("network honeypot")?;
    let _ = crate::sidecar::dispatch_paid_command(
        "add_network_honeypot_custom_port",
        serde_json::json!({ "port": port, "label": label }),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn remove_network_honeypot_custom_port(port: u16) -> Result<(), String> {
    crate::license::require_paid("network honeypot")?;
    let _ = crate::sidecar::dispatch_paid_command(
        "remove_network_honeypot_custom_port",
        serde_json::json!({ "port": port }),
    )
    .await?;
    Ok(())
}
