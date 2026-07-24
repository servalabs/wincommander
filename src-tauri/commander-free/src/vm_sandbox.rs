// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/vm_sandbox.rs
//
// ═══════════════════════════════════════════════════════════════════════
// DISPOSABLE ISOLATION — Free-side dispatch wrappers
// ═══════════════════════════════════════════════════════════════════════
//
// The system-altering engine (Hyper-V VM + Windows Sandbox lifecycle) lives in
// commander-pro/src/vm_sandbox.rs under the feature_ids `vm_capabilities`,
// `vm_list`, `vm_create`, `vm_start`, `vm_stop`, `vm_destroy`, `sandbox_launch`,
// `sandbox_close`. These thin Tauri commands forward over the Pro IPC channel
// (the driver_health / print_audit precedent) — no VM/sandbox command strings
// live in the AV-clean Free binary.
//
// License model: the read-only `vm_capabilities` + `vm_list` dispatch WITHOUT an
// explicit require_paid (the panel's gating + Pro's own tier gate enforce
// entitlement, same reasoning driver_health documents). Every mutating op
// (create/start/stop/destroy a VM, launch/close a sandbox) calls require_paid.

use serde_json::Value;

#[tauri::command]
pub async fn vm_capabilities() -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command("vm_capabilities", Value::Null).await
}

#[tauri::command]
pub async fn vm_list() -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command("vm_list", Value::Null).await
}

#[tauri::command]
pub async fn vm_enable_feature(args: Value) -> Result<Value, String> {
    crate::license::require_paid("enable disposable isolation")?;
    crate::sidecar::dispatch_paid_command("vm_enable_feature", args).await
}

#[tauri::command]
pub async fn vm_create(args: Value) -> Result<Value, String> {
    crate::license::require_paid("create VM")?;
    crate::sidecar::dispatch_paid_command("vm_create", args).await
}

#[tauri::command]
pub async fn vm_start(args: Value) -> Result<Value, String> {
    crate::license::require_paid("start VM")?;
    crate::sidecar::dispatch_paid_command("vm_start", args).await
}

#[tauri::command]
pub async fn vm_stop(args: Value) -> Result<Value, String> {
    crate::license::require_paid("stop VM")?;
    crate::sidecar::dispatch_paid_command("vm_stop", args).await
}

#[tauri::command]
pub async fn vm_destroy(args: Value) -> Result<Value, String> {
    crate::license::require_paid("destroy VM")?;
    crate::sidecar::dispatch_paid_command("vm_destroy", args).await
}

#[tauri::command]
pub async fn sandbox_launch(args: Value) -> Result<Value, String> {
    crate::license::require_paid("launch Windows Sandbox")?;
    crate::sidecar::dispatch_paid_command("sandbox_launch", args).await
}

#[tauri::command]
pub async fn sandbox_close() -> Result<Value, String> {
    crate::license::require_paid("close Windows Sandbox")?;
    crate::sidecar::dispatch_paid_command("sandbox_close", Value::Null).await
}
