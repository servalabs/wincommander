// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/wifi_check.rs
//
// ═══════════════════════════════════════════════════════════════════════
// WI-FI GUARD — Free-side dispatch wrappers (A-4 module 6)
// ═══════════════════════════════════════════════════════════════════════
//
// The paid detector implementation (netsh polling, learning window,
// per-BSSID debounce, hit storage, fire-event emission) lives in
// commander-pro/src/wifi_guard.rs. Free retains the seven Tauri commands
// invoked by WifiGuardSection.tsx so the frontend interface is
// unchanged; bodies now thin-dispatch via sidecar::dispatch_paid_command.
//
// When Pro detects a rogue AP it fires two Notifications over the IPC
// channel:
//   - "wifi-guard-detected" → frontend listener (WifiGuardSection mounts
//     listen("wifi-guard-detected", ...))
//   - "wc-native-toast" → Free's sidecar reader opens the custom alert window
//     (Pro has no Tauri context)
//
// WifiGuardHit struct shape stays in this file so serde can decode the
// dispatch return values + the Notification payloads on Free's side.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiGuardBaselineEntry {
    pub ssid: String,
    pub bssids: Vec<String>,
    pub best_auth_strength: u8,
    #[serde(default)]
    pub ssid_hex: Option<String>,
    #[serde(default)]
    pub provenance: Option<String>,
    #[serde(default)]
    pub auth_algorithm: Option<u32>,
    #[serde(default)]
    pub cipher_algorithm: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiGuardConfig {
    pub learning_window_secs: u64,
    pub learning_until: Option<String>,
    pub poll_interval_secs: u64,
    pub alert_debounce_secs: u64,
    pub baseline: Option<Vec<WifiGuardBaselineEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiGuardHit {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub interface_id: String,
    pub ssid: String,
    pub bssid: String,
    pub auth: String,
    pub signal: String,
    /// "newBssid" | "authDowngrade" | "both"
    pub reason: String,
    pub detected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiGuardStatus {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub instance_id: String,
    #[serde(default)]
    pub baseline_revision: u64,
    #[serde(default)]
    pub health: String,
    #[serde(default)]
    pub collector_healthy: bool,
    #[serde(default)]
    pub observed_at: String,
    #[serde(default)]
    pub error_code: Option<u32>,
    #[serde(default)]
    pub interfaces: Vec<WifiGuardAssociation>,
    #[serde(default)]
    pub monitoring_scope: String,
    pub running: bool,
    pub learning: bool,
    pub learning_until: Option<String>,
    pub known_ssid_count: usize,
    pub current_ssid: Option<String>,
    pub current_bssid: Option<String>,
    pub learning_window_secs: u64,
    pub poll_interval_secs: u64,
    pub alert_debounce_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiGuardAssociation {
    pub interface_id: String,
    pub name: String,
    pub state: String,
    pub ssid: String,
    pub ssid_hex: String,
    pub bssid: String,
    pub auth_algorithm: Option<u32>,
    pub cipher_algorithm: Option<u32>,
    pub auth_strength: Option<u8>,
    pub signal: u32,
    pub trust: String,
    pub error_code: Option<u32>,
}

#[tauri::command]
pub async fn trust_wifi_guard_current(
    interface_id: String,
    expected_ssid_hex: String,
    expected_bssid: String,
    expected_auth_algorithm: u32,
    expected_cipher_algorithm: u32,
) -> Result<WifiGuardStatus, String> {
    crate::license::require_paid("wifi guard")?;
    let value = crate::sidecar::dispatch_paid_command("trust_wifi_guard_current", serde_json::json!({
        "interfaceId": interface_id,
        "expectedSsidHex": expected_ssid_hex,
        "expectedBssid": expected_bssid,
        "expectedAuthAlgorithm": expected_auth_algorithm,
        "expectedCipherAlgorithm": expected_cipher_algorithm,
    })).await?;
    serde_json::from_value(value).map_err(|error| format!("wifi guard approval decode: {error}"))
}

#[tauri::command]
pub async fn start_wifi_guard(_app: tauri::AppHandle) -> Result<WifiGuardStatus, String> {
    crate::license::require_paid("wifi guard")?;
    let v =
        crate::sidecar::dispatch_paid_command("start_wifi_guard", serde_json::Value::Null).await?;
    serde_json::from_value(v).map_err(|e| format!("wifi_guard start decode: {}", e))
}

#[tauri::command]
pub async fn stop_wifi_guard() -> Result<(), String> {
    let _ =
        crate::sidecar::dispatch_paid_command("stop_wifi_guard", serde_json::Value::Null).await?;
    Ok(())
}

#[tauri::command]
pub async fn wifi_guard_status() -> Result<WifiGuardStatus, String> {
    let v =
        crate::sidecar::dispatch_paid_command("wifi_guard_status", serde_json::Value::Null).await?;
    serde_json::from_value(v).map_err(|e| format!("wifi_guard status decode: {}", e))
}

#[tauri::command]
pub async fn get_wifi_guard_recent() -> Result<Vec<WifiGuardHit>, String> {
    let v = crate::sidecar::dispatch_paid_command("get_wifi_guard_recent", serde_json::Value::Null)
        .await?;
    serde_json::from_value(v).map_err(|e| format!("wifi_guard recent decode: {}", e))
}

#[tauri::command]
pub async fn clear_wifi_guard_recent() -> Result<(), String> {
    let _ =
        crate::sidecar::dispatch_paid_command("clear_wifi_guard_recent", serde_json::Value::Null)
            .await?;
    Ok(())
}

#[tauri::command]
pub async fn get_wifi_guard_known() -> Result<Vec<(String, Vec<String>)>, String> {
    let v = crate::sidecar::dispatch_paid_command("get_wifi_guard_known", serde_json::Value::Null)
        .await?;
    serde_json::from_value(v).map_err(|e| format!("wifi_guard known decode: {}", e))
}

/// Return the complete device-local baseline so Free can persist it across a
/// sidecar/app restart. This is never used for Fleet reporting.
#[tauri::command]
pub async fn get_wifi_guard_baseline() -> Result<Vec<WifiGuardBaselineEntry>, String> {
    let v =
        crate::sidecar::dispatch_paid_command("get_wifi_guard_baseline", serde_json::Value::Null)
            .await?;
    serde_json::from_value(v).map_err(|e| format!("wifi_guard baseline decode: {}", e))
}

/// Apply the operator policy before the detector is armed. The Pro sidecar
/// clamps the cadence, learning window, and alert cooldown to safe bounds.
#[tauri::command]
pub async fn configure_wifi_guard(config: WifiGuardConfig) -> Result<WifiGuardStatus, String> {
    crate::license::require_paid("wifi guard")?;
    let v = crate::sidecar::dispatch_paid_command(
        "configure_wifi_guard",
        serde_json::json!({ "config": config }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("wifi_guard config decode: {}", e))
}

#[tauri::command]
pub async fn clear_wifi_guard_known() -> Result<(), String> {
    let _ =
        crate::sidecar::dispatch_paid_command("clear_wifi_guard_known", serde_json::Value::Null)
            .await?;
    Ok(())
}

/// Manually trust an SSID (with an optional BSSID). When the user
/// already knows the legitimate MAC of their AP they can pre-seed it
/// here instead of waiting for the 24h learning window. Bssid is
/// optional — passing only the SSID just acknowledges the network
/// name so future associations to it are still verified normally.
#[tauri::command]
pub async fn add_wifi_guard_ssid(
    ssid: String,
    bssid: Option<String>,
) -> Result<WifiGuardStatus, String> {
    crate::license::require_paid("wifi guard")?;
    let payload = serde_json::json!({
        "ssid": ssid,
        "bssid": bssid,
    });
    let v = crate::sidecar::dispatch_paid_command("add_wifi_guard_ssid", payload).await?;
    serde_json::from_value(v).map_err(|e| format!("wifi_guard add decode: {}", e))
}
