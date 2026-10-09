// SPDX-License-Identifier: AGPL-3.0-or-later
// Public command projections; monitoring remains in the paid provider.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortEntry {
    pub port: u16, pub end_port: u16, pub protocol: String,
    pub label: String, pub enabled: bool, pub custom: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoneypotHit {
    pub id: u64, pub port: u16, pub protocol: String, pub service: String,
    pub peer: String, pub local_address: String, pub outcome: String,
    pub loopback: bool, pub detected_at: String, pub peek_hex: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoneypotStatus {
    pub schema_version: u32, pub running: bool, pub desired_enabled: bool,
    pub provider: String, pub health: String, pub coverage: String,
    pub coverage_complete: bool, pub coverage_note: String,
    pub collection_settings_retained: bool, pub collector_healthy: bool,
    pub last_error: Option<String>, pub observed_at: String, pub last_poll_at: String,
    pub generation: u64, pub observed_events: u64, pub dropped_events: u64,
    pub history_evicted: u64,
    pub history_error: Option<String>, pub history_retention: String,
}
async fn dispatch<T: serde::de::DeserializeOwned>(command: &str, args: Value) -> Result<T, String> {
    let value = crate::sidecar::dispatch_paid_command(command,args).await?;
    serde_json::from_value(value).map_err(|error|format!("Port Guard response could not be read: {error}. Update the app and Pro together."))
}
#[tauri::command]
pub async fn start_network_honeypot(_app: tauri::AppHandle) -> Result<HoneypotStatus,String> {
    crate::license::require_paid("Port Guard")?;
    dispatch("start_network_honeypot",Value::Null).await
}
#[tauri::command]
pub async fn reconcile_network_honeypot() -> Result<HoneypotStatus,String> {
    crate::license::require_paid("Port Guard")?;
    dispatch("reconcile_network_honeypot",Value::Null).await
}
#[tauri::command]
pub async fn stop_network_honeypot() -> Result<(),String> {
    let _: Value = dispatch("stop_network_honeypot",Value::Null).await?; Ok(())
}
#[tauri::command]
pub async fn network_honeypot_status() -> Result<HoneypotStatus,String> { dispatch("network_honeypot_status",Value::Null).await }
#[tauri::command]
pub async fn get_network_honeypot_recent() -> Result<Vec<HoneypotHit>,String> { dispatch("get_network_honeypot_recent",Value::Null).await }
#[tauri::command]
pub async fn clear_network_honeypot_recent() -> Result<(),String> {
    let _: Value = dispatch("clear_network_honeypot_recent",Value::Null).await?; Ok(())
}
#[tauri::command]
pub async fn get_network_honeypot_ports() -> Result<Vec<PortEntry>,String> { dispatch("get_network_honeypot_ports",Value::Null).await }
#[tauri::command]
pub async fn get_network_honeypot_bind_all_interfaces() -> Result<bool,String> { Ok(false) }
#[tauri::command]
pub async fn set_network_honeypot_bind_all_interfaces(_value: bool) -> Result<(),String> {
    Err("Port Guard observes existing network activity automatically; exposing a listener is no longer needed.".into())
}
#[tauri::command]
pub async fn set_network_honeypot_port_enabled(port:u16,enabled:bool,end_port:Option<u16>,protocol:Option<String>) -> Result<(),String> {
    crate::license::require_paid("Port Guard")?;
    let _:Value = dispatch("set_network_honeypot_port_enabled",json!({"port":port,"endPort":end_port,"protocol":protocol,"enabled":enabled})).await?; Ok(())
}
#[tauri::command]
pub async fn add_network_honeypot_custom_port(port:u16,label:String,end_port:Option<u16>,protocol:Option<String>,ports_spec:Option<String>) -> Result<(),String> {
    crate::license::require_paid("Port Guard")?;
    let _:Value = dispatch("add_network_honeypot_custom_port",json!({"port":port,"endPort":end_port,"protocol":protocol,"label":label,"portsSpec":ports_spec})).await?; Ok(())
}
#[tauri::command]
pub async fn remove_network_honeypot_custom_port(port:u16,end_port:Option<u16>,protocol:Option<String>) -> Result<(),String> {
    crate::license::require_paid("Port Guard")?;
    let _:Value = dispatch("remove_network_honeypot_custom_port",json!({"port":port,"endPort":end_port,"protocol":protocol})).await?; Ok(())
}
