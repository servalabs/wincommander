// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/remote_support.rs
//
// Remote Support — Free-side thin dispatch (paid implementation in
// commander-pro/src/remote_support.rs). The three ids are agent-affine in
// `sidecar::is_agent_affine`: the tunnel and its state live in one Pro process,
// so start/stop/status must reach the same session.
//
// No argument is accepted from the frontend: the 59-minute limit, the
// authorized user and the tunnel host are fixed in trusted code.

#[tauri::command]
pub async fn start_remote_support() -> Result<serde_json::Value, String> {
    crate::license::require_paid("remote support")?;
    crate::sidecar::dispatch_paid_command("start_remote_support", serde_json::Value::Null).await
}

#[tauri::command]
pub async fn stop_remote_support() -> Result<serde_json::Value, String> {
    crate::license::require_paid("remote support")?;
    crate::sidecar::dispatch_paid_command("stop_remote_support", serde_json::Value::Null).await
}

#[tauri::command]
pub async fn remote_support_status() -> Result<serde_json::Value, String> {
    crate::sidecar::dispatch_paid_command("remote_support_status", serde_json::Value::Null).await
}
