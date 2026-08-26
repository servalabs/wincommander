// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::monitoring_rows::{local_row, locked_row};

pub(crate) async fn local_monitors(app: AppHandle, paid: bool) -> Vec<Value> {
    let mut rows = Vec::with_capacity(8);

    // Privacy Shield already has a detailed card, but its aggregate state
    // belongs in this inventory too. Keep camera names/messages in Rust and
    // publish only the boolean capability needed for operator triage.
    // This is a dashboard probe, not an install or remediation operation. A
    // stuck WMI/PowerShell check must become an unavailable row promptly and
    // the helper kills its process tree at this deadline.
    let shield_status = crate::backend::run_backend_script_with_timeout(
        app,
        "Get-PrivacyShieldStatus".to_string(),
        HashMap::new(),
        Some(Duration::from_secs(5)),
    )
    .await;
    let shield_issue = shield_status
        .as_ref()
        .is_ok_and(|status| status.get("cameraAvailable").and_then(Value::as_bool) == Some(false));
    rows.push(local_row(
        "privacy-shield",
        "Privacy Gaze Shield",
        "physical",
        false,
        false,
        Some(3),
        &["camera-observation", "look-away-alerts", "local-blur"],
        shield_status,
        None,
        None,
        shield_issue,
        if shield_issue {
            Some("camera_unavailable")
        } else {
            None
        },
    ));

    let paste_status = bool_status(crate::paste_monitor::paste_monitor_status().await);
    let paste_running = running(&paste_status);
    let paste_health = crate::paste_monitor::get_paste_monitor_health().await;
    let paste_issue = paste_health.as_ref().map_or(paste_running, |health| {
        paste_running
            && (!health.helper_running
                || !health.listener_registered
                || !health.policy_current
                || !health.rules_compiled
                || health.clear_failing)
    });
    rows.push(local_row(
        "clipboard-risk",
        "Clipboard risk monitor",
        "data",
        false,
        false,
        Some(2),
        &["credential-patterns", "malicious-paste", "auto-clear"],
        paste_status,
        crate::paste_monitor::get_paste_monitor_recent()
            .await
            .ok()
            .map(|items| items.len() as u64),
        None,
        paste_issue,
        if paste_issue {
            Some("listener_or_policy_degraded")
        } else {
            None
        },
    ));

    let ransomware_status =
        bool_status(crate::ransomware_monitor::ransomware_monitor_status().await);
    let ransomware_running = running(&ransomware_status);
    let ransomware_health = crate::ransomware_monitor::ransomware_monitor_health().await;
    let ransomware_issue = ransomware_health
        .as_ref()
        .is_ok_and(|health| ransomware_running && !health.process_attribution_ready);
    rows.push(local_row(
        "ransomware-behavior",
        "Ransomware behavior monitor",
        "files",
        false,
        false,
        Some(3),
        &["mass-modify-rate", "safe-exclusions", "response-guidance"],
        ransomware_status,
        crate::ransomware_monitor::get_ransomware_recent()
            .await
            .ok()
            .map(|items| items.len() as u64),
        None,
        ransomware_issue,
        if ransomware_issue {
            Some("attribution_unavailable")
        } else {
            None
        },
    ));

    let usb_status = crate::usb_guard::usb_monitor_status();
    let usb_active = usb_status
        .as_ref()
        .ok()
        .and_then(|value| value.get("connected").and_then(Value::as_u64));
    rows.push(local_row(
        "usb-activity-free",
        "USB activity timeline",
        "devices",
        false,
        false,
        Some(3),
        &["attach-detach", "connected-count", "machine-wide-history"],
        usb_status,
        None,
        usb_active,
        false,
        None,
    ));

    let metric_config = crate::net_traffic_alert::metric_alerts_get_config();
    let metric_enabled = [
        metric_config.cpu.enabled,
        metric_config.ram.enabled,
        metric_config.upload.enabled,
        metric_config.download.enabled,
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count() as u64;
    rows.push(local_row(
        "metric-alerts",
        "System and network metric alerts",
        "system",
        true,
        !paid,
        Some(metric_config.evaluation_interval_secs as u64),
        &[
            "cpu",
            "memory",
            "upload",
            "download",
            "sustained-thresholds",
        ],
        Ok(json!({ "running": metric_enabled > 0, "enabledCount": metric_enabled })),
        None,
        Some(metric_enabled),
        false,
        None,
    ));

    let (rdp_enabled, rdp_count) = crate::settings::read_settings()
        .ok()
        .map(|settings| {
            let tracking = &settings.ideal.privacy.tracking;
            let tweaks = &settings.ideal.tweaks.rdp;
            let count = [
                tracking.rdp_idle_disconnect_enabled.unwrap_or(false),
                tweaks.incoming_idle_timeout_enabled.unwrap_or(false),
            ]
            .into_iter()
            .filter(|enabled| *enabled)
            .count() as u64;
            (count > 0, count)
        })
        .unwrap_or((false, 0));
    rows.push(local_row(
        "rdp-idle-protection",
        "RDP idle protections",
        "sessions",
        true,
        !paid,
        None,
        &[
            "idle-disconnect",
            "session-end-cleanup",
            "incoming-session-policy",
        ],
        Ok(json!({ "running": rdp_enabled, "activeCount": rdp_count })),
        None,
        Some(rdp_count),
        false,
        None,
    ));

    if paid {
        let print_status = crate::print_log::get_print_audit_status()
            .await
            .map(|status| {
                json!({
                    "running": status.channel_enabled,
                    "channelPresent": status.channel_present,
                })
            });
        let print_issue = print_status.as_ref().is_ok_and(|status| {
            status.get("channelPresent").and_then(Value::as_bool) == Some(false)
        });
        rows.push(local_row(
            "print-audit",
            "Print audit",
            "data",
            true,
            false,
            None,
            &["print-service-channel", "job-history", "channel-health"],
            print_status,
            crate::print_log::get_print_audit_log(Some(50))
                .await
                .ok()
                .map(|items| items.len() as u64),
            None,
            print_issue,
            if print_issue {
                Some("channel_missing")
            } else {
                None
            },
        ));
    } else {
        rows.push(locked_row(
            "print-audit",
            "Print audit",
            "data",
            None,
            "print-service-channel,job-history,channel-health",
        ));
    }

    rows
}

fn bool_status(result: Result<bool, String>) -> Result<Value, String> {
    result.map(|running| json!({ "running": running }))
}

fn running(result: &Result<Value, String>) -> bool {
    result
        .as_ref()
        .ok()
        .and_then(|value| value.get("running").and_then(Value::as_bool))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_status_preserves_monitor_failure() {
        let result = bool_status(Ok(true)).unwrap();
        assert_eq!(result["running"], true);
        assert!(bool_status(Err("private error".to_string())).is_err());
    }
}
