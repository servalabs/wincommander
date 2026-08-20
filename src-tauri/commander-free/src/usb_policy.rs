// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/usb_policy.rs
//
// USB U-D + U-E — Free-side thin dispatch wrappers.
//
// Pro sidecar holds every system-modifying action (Disable/Enable-PnpDevice,
// diskpart read-only).  Free wraps them exactly like canary_tokens.rs:
//   require_paid() gate → dispatch_paid_command(feature_id, args).
//
// feature_id mapping (must match handlers.rs::dispatch arms):
//   block_usb_device      → "Set-UsbDeviceBlock"
//   allow_usb_device      → "Set-UsbDeviceAllow"
//   set_usb_volume_readonly → "Set-UsbVolumeReadOnly"
//   quarantine_usb_device → "Invoke-UsbQuarantine"

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbTrustSignals {
    pub serial_stable: bool,
    pub is_hid: bool,
    pub is_mass_storage: bool,
    pub known_vendor: bool,
    pub hid_alerts: u32,
    pub quarantine_actions: u32,
    pub transfer_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbTrustScore {
    pub device_key: String,
    pub score: i32,
    pub signals: UsbTrustSignals,
}

pub fn score_usb_device(signals: &UsbTrustSignals) -> i32 {
    let mut score = 50i32;

    score += if signals.serial_stable { 20 } else { -10 };
    score += if signals.known_vendor { 15 } else { -5 };

    if signals.is_mass_storage {
        score += 5;
    }
    if signals.is_hid {
        score -= 25;
    }

    score -= (signals.hid_alerts.min(2) as i32) * 25;
    score -= (signals.quarantine_actions.min(2) as i32) * 20;

    if signals.transfer_bytes >= 4 * 1024 * 1024 * 1024 {
        score -= 20;
    } else if signals.transfer_bytes >= 1024 * 1024 * 1024 {
        score -= 10;
    }

    score.clamp(0, 100)
}

pub fn trust_signals_from_identity(
    identity: &crate::usb_monitor::DeviceIdentity,
    hid_alerts: u32,
    quarantine_actions: u32,
    transfer_bytes: u64,
) -> UsbTrustSignals {
    UsbTrustSignals {
        serial_stable: identity.serial_stable,
        is_hid: identity.is_hid,
        is_mass_storage: identity.is_mass_storage,
        known_vendor: known_vendor(&identity.manufacturer, &identity.vid),
        hid_alerts,
        quarantine_actions,
        transfer_bytes,
    }
}

fn known_vendor(manufacturer: &str, vid: &str) -> bool {
    let trimmed = manufacturer.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    let generic = [
        "standard",
        "generic",
        "microsoft",
        "usb input device",
        "compatible",
        "(standard usb host controller)",
    ];
    if generic.iter().any(|needle| lower.contains(needle)) {
        return false;
    }

    !vid.trim().is_empty() && vid != "0000"
}

/// Read-only numeric USB trust score for the UI.
#[tauri::command]
pub fn usb_device_trust_score(device_key: String) -> Result<UsbTrustScore, String> {
    let identity = crate::usb_monitor::identity_for_key(&device_key)
        .ok_or_else(|| format!("USB device not found: {device_key}"))?;
    let signals = trust_signals_from_identity(
        &identity,
        crate::usb_hid_guard::alert_count_for_device(&device_key),
        crate::usb_auto_sandbox::quarantine_action_count_for_device(&device_key),
        crate::usb_metering::total_transfer_bytes_for_device(&device_key),
    );

    Ok(UsbTrustScore {
        device_key,
        score: score_usb_device(&signals),
        signals,
    })
}

/// U-D: Disable a USB device via its InstanceId (Disable-PnpDevice).
/// Gated paid; reversible via allow_usb_device.
/// Args: { "instanceId": "<Windows PnP instance id>" }
#[tauri::command]
pub async fn block_usb_device(args: Value) -> Result<Value, String> {
    crate::license::require_paid("USB device trust policy")?;
    let result = crate::sidecar::dispatch_paid_command("Set-UsbDeviceBlock", args).await?;
    crate::fleet_agent::report_required_device_alert("usb_security", "policy_blocked", "warning");
    Ok(result)
}

/// U-D: Re-enable a previously blocked USB device (Enable-PnpDevice).
/// Gated paid; undoes block_usb_device.
/// Args: { "instanceId": "<Windows PnP instance id>" }
#[tauri::command]
pub async fn allow_usb_device(args: Value) -> Result<Value, String> {
    crate::license::require_paid("USB device trust policy")?;
    crate::sidecar::dispatch_paid_command("Set-UsbDeviceAllow", args).await
}

/// U-E: Force a volume read-only (or clear read-only) via diskpart.
/// Gated paid; reversible (readOnly: false restores write).
/// Args: { "driveLetter": "E" | "E:", "readOnly": true | false }
#[tauri::command]
pub async fn set_usb_volume_readonly(args: Value) -> Result<Value, String> {
    crate::license::require_paid("USB volume read-only")?;
    crate::sidecar::dispatch_paid_command("Set-UsbVolumeReadOnly", args).await
}

/// U-D/U-E: Quarantine a device — disables it via the same PnP path as block.
/// Gated paid; reversible via allow_usb_device.
/// Args: { "instanceId": "<Windows PnP instance id>" }
#[tauri::command]
pub async fn quarantine_usb_device(args: Value) -> Result<Value, String> {
    crate::license::require_paid("USB device quarantine")?;
    let result = crate::sidecar::dispatch_paid_command("Invoke-UsbQuarantine", args).await?;
    crate::fleet_agent::report_required_device_alert("usb_security", "quarantined", "warning");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_score_orders_known_storage_above_unknown_storage_above_badusb() {
        let trusted = UsbTrustSignals {
            serial_stable: true,
            is_hid: false,
            is_mass_storage: true,
            known_vendor: true,
            hid_alerts: 0,
            quarantine_actions: 0,
            transfer_bytes: 32 * 1024 * 1024,
        };
        let unknown = UsbTrustSignals {
            serial_stable: false,
            is_hid: false,
            is_mass_storage: true,
            known_vendor: false,
            hid_alerts: 0,
            quarantine_actions: 0,
            transfer_bytes: 32 * 1024 * 1024,
        };
        let badusb = UsbTrustSignals {
            serial_stable: false,
            is_hid: true,
            is_mass_storage: true,
            known_vendor: false,
            hid_alerts: 1,
            quarantine_actions: 1,
            transfer_bytes: 8 * 1024 * 1024 * 1024,
        };

        let trusted_score = score_usb_device(&trusted);
        let unknown_score = score_usb_device(&unknown);
        let badusb_score = score_usb_device(&badusb);

        assert!(trusted_score > unknown_score);
        assert!(unknown_score > badusb_score);
        assert!((0..=100).contains(&trusted_score));
        assert!((0..=100).contains(&unknown_score));
        assert!((0..=100).contains(&badusb_score));
    }

    #[test]
    fn trust_signals_from_identity_combine_vendor_hid_transfer_and_action_history() {
        let identity = crate::usb_monitor::DeviceIdentity {
            key: "USB:05AC:024F:ABCDEF".to_string(),
            vid: "05AC".to_string(),
            pid: "024F".to_string(),
            serial: "ABCDEF".to_string(),
            serial_stable: true,
            friendly_name: "Apple Keyboard".to_string(),
            manufacturer: "Apple Inc.".to_string(),
            class: "HIDClass".to_string(),
            is_hid: true,
            is_mass_storage: false,
            instance_id: r"USB\VID_05AC&PID_024F\ABCDEF".to_string(),
        };

        let signals = trust_signals_from_identity(&identity, 1, 2, 5 * 1024 * 1024 * 1024);

        assert!(signals.serial_stable);
        assert!(signals.is_hid);
        assert!(!signals.is_mass_storage);
        assert!(signals.known_vendor);
        assert_eq!(signals.hid_alerts, 1);
        assert_eq!(signals.quarantine_actions, 2);
        assert_eq!(signals.transfer_bytes, 5 * 1024 * 1024 * 1024);
        assert!(score_usb_device(&signals) < 50);
    }
}
