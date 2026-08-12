// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/canary_tokens.rs
//
// Free-side dispatch wrappers for canary tokens.
// The beacon listener, artifact generation, and hit storage live in
// commander-pro/src/canary_tokens.rs under the feature_ids listed below.
// v1 scope: self-hosted HTTP beacon canaries only (DNS canaries deferred to v2).
// Token types: "docx" (ZIP with remote-image reference) and "url" (.url shortcut).
// Reads dispatch ungated; mutations (generate/delete/start/stop/clear) call require_paid.

use serde_json::Value;

#[tauri::command]
pub async fn generate_canary(args: Value) -> Result<Value, String> {
    crate::license::require_paid("canary tokens")?;
    crate::sidecar::dispatch_paid_command("generate_canary", args).await
}

#[tauri::command]
pub async fn list_canaries() -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command("list_canaries", Value::Null).await
}

#[tauri::command]
pub async fn delete_canary(args: Value) -> Result<Value, String> {
    crate::license::require_paid("canary tokens")?;
    crate::sidecar::dispatch_paid_command("delete_canary", args).await
}

#[tauri::command]
pub async fn start_canary_listener(args: Value) -> Result<Value, String> {
    crate::license::require_paid("canary tokens")?;
    crate::sidecar::dispatch_paid_command("start_canary_listener", args).await
}

#[tauri::command]
pub async fn stop_canary_listener() -> Result<Value, String> {
    crate::license::require_paid("canary tokens")?;
    crate::sidecar::dispatch_paid_command("stop_canary_listener", Value::Null).await
}

#[tauri::command]
pub async fn canary_listener_status() -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command("canary_listener_status", Value::Null).await
}

#[tauri::command]
pub async fn get_canary_recent() -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command("get_canary_recent", Value::Null).await
}

#[tauri::command]
pub async fn clear_canary_recent() -> Result<Value, String> {
    crate::license::require_paid("canary tokens")?;
    crate::sidecar::dispatch_paid_command("clear_canary_recent", Value::Null).await
}
