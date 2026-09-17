use serde_json::Value;
use wincmd_shared::svc::ApplyMachineSettingRequest;

#[path = "rdp_redirection.rs"]
mod rdp_redirection;

#[tauri::command]
pub async fn apply_machine_setting(request: Value) -> Result<Value, String> {
    // Windows Server-only RDP resource redirection is intentionally carried
    // through the existing privileged machine-setting command. The special
    // setting is handled locally because it is a WinCommander desktop/RDS
    // concern; the normal typed service contract remains unchanged.
    if request.get("setting").and_then(Value::as_str) == Some("rdp_redirection") {
        let value = request
            .get("value")
            .ok_or_else(|| "RDP redirection value is required".to_string())?;
        return rdp_redirection::handle(value);
    }

    let typed: ApplyMachineSettingRequest = serde_json::from_value(request)
        .map_err(|e| format!("invalid machine setting request: {e}"))?;
    let observed = crate::svc_client::apply_machine_setting(typed).await?;
    serde_json::to_value(observed).map_err(|e| format!("machine setting result encode failed: {e}"))
}
