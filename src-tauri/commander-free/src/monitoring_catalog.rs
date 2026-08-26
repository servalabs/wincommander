// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::Value;

use crate::monitoring_rows::{locked_row, unavailable_row};

pub(crate) type MonitorSpec = (
    &'static str,
    &'static str,
    &'static str,
    Option<u64>,
    &'static str,
);

// This is the public label/capability catalogue only. Detection and
// enforcement remain in the Pro sidecar; keeping the catalogue here lets a
// free user see what is available without executing a paid command.
pub(crate) const PRO_MONITORS: &[MonitorSpec] = &[
    (
        "network-honeypot",
        "Network honeypot",
        "network",
        Some(5),
        "start-stop,port-rules,recent-alerts",
    ),
    (
        "wifi-guard",
        "Wi-Fi Guard",
        "network",
        Some(5),
        "trusted-networks,learning-mode,recent-alerts",
    ),
    (
        "usb-device",
        "USB device monitor",
        "devices",
        Some(3),
        "attach-detach,connected-count,timeline",
    ),
    (
        "usb-transfer",
        "USB transfer metering",
        "devices",
        None,
        "read-write-counters,large-transfer-alerts,history",
    ),
    (
        "usb-hid",
        "USB keyboard and HID guard",
        "devices",
        Some(3),
        "timing-anomaly,sensitivity,alert-history",
    ),
    (
        "usb-approval",
        "USB HID approval gate",
        "devices",
        None,
        "pending-approvals,user-presence-challenge,containment-status",
    ),
    (
        "usb-autosandbox",
        "USB auto-isolation",
        "devices",
        None,
        "observe-mode,enforce-mode,action-history",
    ),
    (
        "ransomware-etw",
        "Ransomware process attribution",
        "files",
        None,
        "process-attribution,response-ready,cooperative-feed",
    ),
    (
        "decoy-files",
        "Decoy file tripwire",
        "files",
        None,
        "file-access,read-audit,recent-alerts",
    ),
    (
        "remote-access",
        "Remote-access detector",
        "sessions",
        Some(5),
        "tool-catalogue,port-presence,recent-alerts",
    ),
    (
        "screen-capture",
        "Screen-capture detector",
        "sessions",
        Some(10),
        "tool-catalogue,debounce,recent-alerts",
    ),
    (
        "driver-health",
        "Driver health watcher",
        "system",
        Some(60),
        "device-problems,severity,watcher",
    ),
    (
        "auth-anomaly",
        "Access and session anomaly detector",
        "sessions",
        Some(15),
        "failed-bursts,off-hours,new-accounts,recent-alerts",
    ),
    (
        "session-assurance",
        "Session Assurance",
        "sessions",
        Some(5),
        "attention-episodes,health,session-score",
    ),
    (
        "argus-app-usage",
        "App-usage monitor",
        "productivity",
        Some(5),
        "window-usage,idle-windows,recent-samples",
    ),
    (
        "clipboard-guard",
        "Fleet clipboard guard",
        "data",
        Some(30),
        "fleet-policy,pending-signals,content-free-reports",
    ),
    (
        "argus-dlp",
        "DLP signal monitor",
        "data",
        Some(30),
        "clipboard-risk,cloud-upload,removable-copy,recent-signals",
    ),
    (
        "argus-tamper",
        "Tamper and evasion detector",
        "integrity",
        None,
        "service-events,log-events,recent-signals",
    ),
    (
        "argus-print-usb",
        "Print and removable-media signals",
        "data",
        Some(30),
        "print-jobs,removable-media,watermarks,recent-signals",
    ),
    (
        "canary-listener",
        "Canary token listener",
        "deception",
        None,
        "http-beacons,token-count,recent-alerts",
    ),
    (
        "ink-receipt",
        "Print workflow receipt bridge",
        "integrity",
        None,
        "signed-receipts,accepted-denied-calls,history",
    ),
];

fn capabilities(value: &str) -> Vec<&str> {
    value.split(',').filter(|item| !item.is_empty()).collect()
}

pub(crate) fn append_locked_pro(monitors: &mut Vec<Value>) {
    for (id, label, group, cadence_secs, capability_string) in PRO_MONITORS {
        monitors.push(locked_row(
            id,
            label,
            group,
            *cadence_secs,
            capability_string,
        ));
    }
}

pub(crate) fn append_unavailable_pro(monitors: &mut Vec<Value>, error_code: &str) {
    for (id, label, group, cadence_secs, capability_string) in PRO_MONITORS {
        monitors.push(unavailable_row(
            id,
            label,
            group,
            true,
            *cadence_secs,
            &capabilities(capability_string),
            error_code,
        ));
    }
}
