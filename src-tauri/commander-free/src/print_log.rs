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
// invoked by the Privacy Monitor print surfaces so the frontend interface
// stays stable; bodies thin-dispatch via `sidecar::dispatch_paid_command`.
//
// Detailed Event 307 metadata is local-only. Fleet-safe print telemetry
// uses the separate Argus aggregate path and must never be populated from
// these fields.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintAuditEntry {
    /// ISO-8601 UTC from the Windows Print Service event.
    pub time_created: String,
    /// Document label only when Windows supplied one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    /// Total page count reported by Event 307.
    pub pages: u32,
    /// Printer queue name only when Windows supplied one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<String>,
    /// Submitting user only when Windows supplied one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Optional status for a future/alternate Windows source that actually
    /// supplies it. Event 307 does not fabricate a status value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintAuditStatus {
    pub channel_enabled: bool,
    /// Whether the channel exists at all — false on Windows installations
    /// without the Print Service Operational channel.
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
            document: Some("Report.docx".to_string()),
            pages: 7,
            printer: Some("HP LaserJet".to_string()),
            user: Some("DOMAIN\\alice".to_string()),
            job_status: None,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: PrintAuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.document.as_deref(), Some("Report.docx"));
        assert_eq!(back.pages, 7);
        assert!(!json.contains("jobStatus"));
    }

    #[test]
    fn missing_windows_optional_fields_decode_without_fabrication() {
        let json = r#"{"timeCreated":"2026-09-17T10:00:00Z","pages":2}"#;
        let entry: PrintAuditEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.pages, 2);
        assert!(entry.document.is_none());
        assert!(entry.printer.is_none());
        assert!(entry.user.is_none());
        assert!(entry.job_status.is_none());
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
