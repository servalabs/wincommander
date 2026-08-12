// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/network_defense.rs
//
// ═══════════════════════════════════════════════════════════════════════
// NETWORK DEFENSE — Free-side dispatch wrappers (A-4 module 5)
// ═══════════════════════════════════════════════════════════════════════
//
// The paid netsh-rule-management implementation lives in
// commander-pro/src/handlers.rs under feature_ids
// `get_ping_block_status` and `set_ping_block`. Free retains the two
// Tauri commands invoked from NetworkHoneypotSection.tsx so the
// frontend interface is unchanged; bodies now dispatch over IPC.
//
// Rule naming convention `WC_<rule>` is preserved across the move so
// any rule a user already has on disk carries forward after upgrade.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingBlockStatus {
    pub blocked: bool,
}

#[tauri::command]
pub async fn get_ping_block_status() -> Result<PingBlockStatus, String> {
    let v = crate::sidecar::dispatch_paid_command("get_ping_block_status", serde_json::Value::Null)
        .await?;
    serde_json::from_value(v).map_err(|e| format!("ping block status decode: {}", e))
}

#[tauri::command]
pub async fn set_ping_block(enabled: bool) -> Result<PingBlockStatus, String> {
    crate::license::require_paid("ping block")?;
    let v = crate::sidecar::dispatch_paid_command(
        "set_ping_block",
        serde_json::json!({ "enabled": enabled }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("ping block toggle decode: {}", e))
}
