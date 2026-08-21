// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/print_audit.rs
//
// ═══════════════════════════════════════════════════════════════════════
// PRINT AUDIT — Free-side dispatch wrappers (A-4 module 1)
// ═══════════════════════════════════════════════════════════════════════
//
// The paid implementation lives in commander-pro/src/handlers.rs under
// the feature_ids `get_print_audit_status`, `set_print_audit_enabled`,
// and `get_print_audit_log`. Free retains the three Tauri commands
// invoked by [PrintAuditSection.tsx](../../../src/panels/intelligence/PrintAuditSection.tsx)
// so the frontend interface is unchanged; bodies now thin-dispatch via
// `sidecar::dispatch_paid_command` over the IPC channel established
// by A-1.
//
// What used to live here (full Get-WinEvent + wevtutil PowerShell
// invocations) is now Pro-owned. The struct shapes stay in this file
// so Free can deserialize Pro's JSON replies without redefining types
// in two crates.
//
// License model: set_print_audit_enabled flips a Windows event channel
// state — admin-required, paid feature. The two read commands are
// dispatched without an explicit require_paid call because the panel's
// own gating + Pro's dispatcher already enforce the entitlement; an
// unlicensed user can't trigger the panel mount in the first place.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintAuditEntry {
    /// ISO-8601 UTC.
    pub time_created: String,
    /// Document name as the application submitted it.
    pub document: String,
    /// Total page count (Windows reports `Size` in bytes too but page
    /// count is the operator-relevant number).
    pub pages: u32,
    /// Printer queue name.
    pub printer: String,
    /// User who submitted the job (`DOMAIN\user`).
    pub user: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintAuditStatus {
    pub channel_enabled: bool,
    /// Whether the channel exists at all — false on Win versions without
    /// the Print Service feature (rare).
    pub channel_present: bool,
}

#[tauri::command]
pub async fn get_print_audit_status() -> Result<PrintAuditStatus, String> {
    let v =
        crate::sidecar::dispatch_paid_command("get_print_audit_status", serde_json::Value::Null)
            .await?;
    serde_json::from_value(v).map_err(|e| format!("print audit status decode: {}", e))
}

#[tauri::command]
pub async fn set_print_audit_enabled(enabled: bool) -> Result<(), String> {
    crate::license::require_paid("print audit")?;
    let _ = crate::sidecar::dispatch_paid_command(
        "set_print_audit_enabled",
        serde_json::json!({ "enabled": enabled }),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn get_print_audit_log(limit: Option<u32>) -> Result<Vec<PrintAuditEntry>, String> {
    let n = limit.unwrap_or(50).min(500);
    let v = crate::sidecar::dispatch_paid_command(
        "get_print_audit_log",
        serde_json::json!({ "limit": n }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("print audit log decode: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_round_trips_through_json() {
        let e = PrintAuditEntry {
            time_created: "2026-05-12T10:00:00Z".to_string(),
            document: "Report.docx".to_string(),
            pages: 7,
            printer: "HP LaserJet".to_string(),
            user: "DOMAIN\\alice".to_string(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: PrintAuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.document, "Report.docx");
        assert_eq!(back.pages, 7);
    }

    #[test]
    fn status_serialises_camel_case() {
        let s = PrintAuditStatus {
            channel_enabled: true,
            channel_present: true,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("channelEnabled"));
        assert!(json.contains("channelPresent"));
    }
}
