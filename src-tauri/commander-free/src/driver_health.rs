// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/driver_health.rs
//
// ═══════════════════════════════════════════════════════════════════════
// DRIVER HEALTH — Free-side dispatch wrappers (#6)
// ═══════════════════════════════════════════════════════════════════════
//
// The paid implementation lives in commander-pro/src/driver_health.rs
// under the feature_ids `Get-DriverHealth`, `start_driver_watch`,
// `stop_driver_watch`, and `driver_watch_status`. Free retains the typed
// Tauri commands invoked by the startup watcher in
// `src/components/BackgroundPollers.tsx`; bodies thin-dispatch via
// `sidecar::dispatch_paid_command` over the IPC channel (the print_audit
// pattern). Keep this out of the Home dashboard unless that surface is
// explicitly re-approved.
//
// The struct shapes live here so Free can deserialize Pro's JSON replies
// without redefining types in two crates. `#[serde(rename_all =
// "camelCase")]` so the camelCase JSON Pro emits round-trips unchanged.
//
// License model: `get_driver_health` and `driver_watch_status` are
// read-only and dispatch WITHOUT an explicit require_paid — the panel's
// own gating + Pro's tier gate already enforce entitlement (same
// reasoning print_audit documents for its two read commands).
// `start_/stop_driver_watch` additionally call `require_paid`.
//
// `open_device_manager` is the one intentionally-FREE action: a benign
// shell-out to `devmgmt.msc` so the guidance button isn't dead on the
// Free tier. It is NOT routed through Pro and NOT in `get_command_tier`.
// The target is pinned literally in Rust; it accepts no argument from the
// frontend (never accept a path/host from the frontend).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverProblem {
    pub name: String,
    pub class: String,
    pub status: String,
    pub problem_code: Option<i64>,
    pub problem_text: String,
    /// "critical" | "warning" | "info"
    pub severity: String,
    pub instance_id: String,
    pub manufacturer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverHealthSummary {
    pub total: u32,
    pub critical: u32,
    pub warning: u32,
    pub info: u32,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverHealthReport {
    pub devices: Vec<DriverProblem>,
    pub summary: DriverHealthSummary,
}

#[tauri::command]
pub async fn get_driver_health() -> Result<DriverHealthReport, String> {
    let v =
        crate::sidecar::dispatch_paid_command("Get-DriverHealth", serde_json::Value::Null).await?;
    serde_json::from_value(v).map_err(|e| format!("driver health decode: {}", e))
}

#[tauri::command]
pub async fn start_driver_watch(interval_secs: Option<u64>) -> Result<(), String> {
    crate::license::require_paid("driver watch")?;
    let args = match interval_secs {
        Some(n) => serde_json::json!({ "intervalSecs": n }),
        None => serde_json::Value::Null,
    };
    let _ = crate::sidecar::dispatch_paid_command("start_driver_watch", args).await?;
    Ok(())
}

#[tauri::command]
pub async fn stop_driver_watch() -> Result<(), String> {
    crate::license::require_paid("driver watch")?;
    let _ =
        crate::sidecar::dispatch_paid_command("stop_driver_watch", serde_json::Value::Null).await?;
    Ok(())
}

#[tauri::command]
pub async fn driver_watch_status() -> Result<serde_json::Value, String> {
    crate::sidecar::dispatch_paid_command("driver_watch_status", serde_json::Value::Null).await
}

// ── BYOVD (A3) — mirror types + dispatch ─────────────────────────────
//
// VulnerableDriver mirrors the camelCase JSON shape emitted by Pro's
// `scan_vulnerable_drivers()`. Dispatched via `Get-VulnerableDrivers`
// exactly like `Get-DriverHealth` — ungated read, no `require_paid`
// (same reasoning: Pro's own tier gate + panel gating covers this).

// Wire-shape documentation for Pro's scan_vulnerable_drivers() output; not deserialized into yet.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VulnerableDriver {
    pub filename: String,
    pub path: String,
    pub state: String,
    pub reason: String,
    pub matched_by: String,
}

// Wire-shape documentation for Pro's scan_vulnerable_drivers() output; not deserialized into yet.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VulnerableDriversReport {
    pub vulnerable: Vec<VulnerableDriver>,
    pub scanned: u32,
    pub ok: bool,
}

/// Dispatch `Get-VulnerableDrivers` to the Pro sidecar and return the
/// structured report. Read-only; ungated (identical reasoning to
/// `get_driver_health`).
#[tauri::command]
pub async fn get_vulnerable_drivers() -> Result<serde_json::Value, String> {
    crate::sidecar::dispatch_paid_command("Get-VulnerableDrivers", serde_json::Value::Null).await
}

/// Launch the native Windows Device Manager (`devmgmt.msc`). The one
/// non-paid action in this feature — works on the Free tier so the
/// "Open Device Manager" guidance button is never dead. Target is pinned
/// in Rust; no frontend argument is accepted.
#[tauri::command]
pub fn open_device_manager() -> Result<(), String> {
    // `mmc.exe devmgmt.msc` launches the Device Manager MMC snap-in. We
    // intentionally do NOT set CREATE_NO_WINDOW — the user must see the
    // console window. This is a benign, ungated shell-out.
    std::process::Command::new("mmc.exe")
        .arg("devmgmt.msc")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open Device Manager: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_round_trips_through_json() {
        let report = DriverHealthReport {
            devices: vec![DriverProblem {
                name: "Realtek USB Audio".to_string(),
                class: "MEDIA".to_string(),
                status: "Error".to_string(),
                problem_code: Some(28),
                problem_text: "No driver is installed for this device.".to_string(),
                severity: "critical".to_string(),
                instance_id: "USB\\VID_0BDA&PID_4014\\6&abc".to_string(),
                manufacturer: "Realtek".to_string(),
            }],
            summary: DriverHealthSummary {
                total: 1,
                critical: 1,
                warning: 0,
                info: 0,
                ok: false,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        // camelCase keys present.
        assert!(json.contains("problemCode"));
        assert!(json.contains("problemText"));
        assert!(json.contains("instanceId"));
        let back: DriverHealthReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.devices[0].problem_code, Some(28));
        assert_eq!(back.summary.critical, 1);
        assert!(!back.summary.ok);
    }

    #[test]
    fn deserializes_pro_camel_case_payload() {
        // Mirrors exactly what Pro's `scan()` emits.
        let pro = r#"{
            "devices": [
                {"name":"X","class":"USB","status":"Error","problemCode":43,
                 "problemText":"Windows stopped this device because it reported problems.",
                 "severity":"critical","instanceId":"USB\\X","manufacturer":"y"}
            ],
            "summary": {"total":1,"critical":1,"warning":0,"info":0,"ok":false}
        }"#;
        let report: DriverHealthReport = serde_json::from_str(pro).unwrap();
        assert_eq!(report.devices[0].severity, "critical");
        assert_eq!(report.devices[0].problem_code, Some(43));
    }
}
