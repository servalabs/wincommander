// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, read-only PnP driver inventory and Windows Update handoff.

use serde::{Deserialize, Serialize};

const DRIVER_LIMIT: usize = 2_000;
const OPTIONAL_UPDATES_SETTINGS_URI: &str = "ms-settings:windowsupdate-optionalupdates";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const STDERR_SUMMARY_LIMIT: usize = 512;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverMaintenanceInventory {
    pub drivers: Vec<DriverInventoryEntry>,
    pub truncated: bool,
    pub cleanup_available: bool,
    pub cleanup_limitation: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverInventoryEntry {
    #[serde(alias = "DeviceName")]
    pub device_name: Option<String>,
    #[serde(alias = "DeviceClass")]
    pub device_class: Option<String>,
    #[serde(alias = "DeviceID")]
    pub device_id: Option<String>,
    #[serde(alias = "InfName")]
    pub inf_name: Option<String>,
    #[serde(alias = "Manufacturer")]
    pub manufacturer: Option<String>,
    #[serde(alias = "DriverVersion")]
    pub driver_version: Option<String>,
    #[serde(alias = "DriverDate")]
    pub driver_date: Option<String>,
    #[serde(alias = "IsSigned")]
    pub is_signed: Option<bool>,
    #[serde(alias = "Signer")]
    pub signer: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverUpdateSeam {
    pub provider: String,
    pub opened: bool,
    pub limitations: Vec<String>,
}

#[tauri::command]
pub async fn driver_maintenance_inventory() -> Result<DriverMaintenanceInventory, String> {
    tokio::task::spawn_blocking(read_driver_inventory)
        .await
        .map_err(|error| format!("driver inventory task failed: {error}"))?
}

#[tauri::command]
pub fn driver_update_seam() -> Result<DriverUpdateSeam, String> {
    #[cfg(windows)]
    let opened = std::process::Command::new("explorer.exe")
        .arg(OPTIONAL_UPDATES_SETTINGS_URI)
        .spawn()
        .map(|_| true)
        .map_err(|error| format!("could not open Windows Update: {error}"))?;
    #[cfg(not(windows))]
    let opened = false;
    Ok(DriverUpdateSeam {
        provider: "Windows Update".into(),
        opened,
        limitations: vec![
            "Windows Update decides applicability and installation; this command does not install drivers.".into(),
            "Driver-package cleanup is intentionally unavailable because active-package and rollback safety cannot be proven from PnP inventory alone.".into(),
        ],
    })
}

#[cfg(windows)]
fn read_driver_inventory() -> Result<DriverMaintenanceInventory, String> {
    use std::os::windows::process::CommandExt;

    const SCRIPT: &str = "$ErrorActionPreference='Stop';@(Get-CimInstance Win32_PnPSignedDriver|Select-Object -First 2001 DeviceName,DeviceClass,DeviceID,InfName,Manufacturer,DriverVersion,DriverDate,IsSigned,Signer)|ConvertTo-Json -Compress -Depth 3";
    let mut command = std::process::Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        // KT: PowerShell is an implementation detail; never flash a console
        // window while the desktop app performs a read-only inventory.
        .creation_flags(CREATE_NO_WINDOW);
    let output = command
        .output()
        .map_err(|error| format!("could not query PnP drivers: {error}"))?;
    if !output.status.success() {
        return Err(format_process_failure(output.status.code(), &output.stderr));
    }
    let mut drivers = parse_driver_records(&output.stdout)
        .map_err(|error| format!("PnP driver inventory response was invalid: {error}"))?;
    drivers.sort_by(|a, b| {
        a.device_name
            .cmp(&b.device_name)
            .then_with(|| a.inf_name.cmp(&b.inf_name))
    });
    let truncated = drivers.len() > DRIVER_LIMIT;
    drivers.truncate(DRIVER_LIMIT);
    Ok(inventory(drivers, truncated))
}

#[cfg(not(windows))]
fn read_driver_inventory() -> Result<DriverMaintenanceInventory, String> {
    Err("PnP driver inventory is available only on Windows".into())
}

fn inventory(drivers: Vec<DriverInventoryEntry>, truncated: bool) -> DriverMaintenanceInventory {
    DriverMaintenanceInventory {
        drivers,
        truncated,
        cleanup_available: false,
        cleanup_limitation: "No stale-driver cleanup is exposed: PnP inventory cannot safely establish that a package is inactive, rollback-safe, or not needed by an offline device.".into(),
    }
}

fn parse_driver_records(bytes: &[u8]) -> Result<Vec<DriverInventoryEntry>, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    match value {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(_) => serde_json::from_value(value),
        value => serde_json::from_value(value).map(|record| vec![record]),
    }
}

fn format_process_failure(exit_code: Option<i32>, stderr: &[u8]) -> String {
    let normalized: String = String::from_utf8_lossy(stderr)
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let summary: String = normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(STDERR_SUMMARY_LIMIT)
        .collect();
    let status = exit_code
        .map(|code| format!("exit code {code}"))
        .unwrap_or_else(|| "no exit code".into());
    if summary.is_empty() {
        format!("PnP driver inventory query failed ({status})")
    } else {
        format!("PnP driver inventory query failed ({status}): {summary}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_always_refuses_cleanup_without_package_proof() {
        let report = inventory(Vec::new(), false);
        assert!(!report.cleanup_available);
        assert!(report.cleanup_limitation.contains("cannot safely"));
    }
    #[test]
    fn driver_update_handoff_targets_optional_updates() {
        assert_eq!(
            OPTIONAL_UPDATES_SETTINGS_URI,
            "ms-settings:windowsupdate-optionalupdates"
        );
    }
    #[test]
    fn pnp_payload_uses_camel_case_fields() {
        let entry: DriverInventoryEntry =
            serde_json::from_str(r#"{"deviceName":"Audio","infName":"oem1.inf","isSigned":true}"#)
                .unwrap();
        assert_eq!(entry.inf_name.as_deref(), Some("oem1.inf"));
        assert_eq!(entry.is_signed, Some(true));
    }
    #[test]
    fn accepts_a_single_native_powershell_record() {
        let records = parse_driver_records(
            br#"{"DeviceName":"Realtek Audio","DeviceClass":"MEDIA","DeviceID":"HDAUDIO\\FUNC_01","InfName":"oem12.inf","Manufacturer":"Realtek","DriverVersion":"6.0.1.9999","DriverDate":"\/Date(1711929600000-0000)\/","IsSigned":true,"Signer":"Microsoft Windows Hardware Compatibility Publisher"}"#,
        )
        .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].device_name.as_deref(), Some("Realtek Audio"));
        assert_eq!(records[0].device_class.as_deref(), Some("MEDIA"));
        assert_eq!(records[0].inf_name.as_deref(), Some("oem12.inf"));
        assert_eq!(records[0].manufacturer.as_deref(), Some("Realtek"));
        assert_eq!(records[0].driver_version.as_deref(), Some("6.0.1.9999"));
        assert_eq!(records[0].is_signed, Some(true));
        assert_eq!(
            records[0].signer.as_deref(),
            Some("Microsoft Windows Hardware Compatibility Publisher")
        );
        let frontend_payload = serde_json::to_value(&records[0]).unwrap();
        assert_eq!(frontend_payload["deviceName"], "Realtek Audio");
        assert!(frontend_payload.get("DeviceName").is_none());
    }
    #[test]
    fn accepts_an_array_of_native_powershell_records() {
        let records = parse_driver_records(
            br#"[{"DeviceName":"Audio","InfName":"oem1.inf","IsSigned":true},{"DeviceName":"Display","InfName":"oem2.inf","IsSigned":false}]"#,
        )
        .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].device_name.as_deref(), Some("Display"));
        assert_eq!(records[1].inf_name.as_deref(), Some("oem2.inf"));
        assert_eq!(records[1].is_signed, Some(false));
    }
    #[test]
    fn accepts_null_for_an_empty_inventory() {
        let records = parse_driver_records(b"null").unwrap();
        assert!(records.is_empty());
    }
    #[test]
    fn process_failure_includes_bounded_normalized_stderr() {
        let error = format_process_failure(
            Some(5),
            format!("  Access\r\n denied {}\0", "x".repeat(600)).as_bytes(),
        );
        assert!(error.starts_with("PnP driver inventory query failed (exit code 5): Access denied"));
        assert!(error.chars().count() <= 584);
        assert!(!error.contains('\n'));
    }
}
