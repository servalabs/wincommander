// SPDX-License-Identifier: AGPL-3.0-or-later
//! Event and notification wire contracts. B3 owns this module after G-1.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::DeviceId;

/// Agent → server report of ONE local Windows monitor alert the agent already
/// showed the user (screen capture, CPU/RAM/network threshold, ransomware,
/// remote access, driver, Wi-Fi, network honeypot, VPN kill switch, USB
/// security, or a content-free clipboard class). The per-alert setting or Fleet's signed master policy gates the
/// forwarding. `detail` is a closed, Pro-normalized summary: class/severity,
/// bounded counters, or aggregate metric values only—never clipboard text,
/// process names, paths, peers, SSIDs, window titles, or other free text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct LocalAlertReport {
    /// Endpoint-minted UUID. It is stable across an outbox retry, so the
    /// server can retain a repeated check-in as one occurrence rather than
    /// creating a second alert or second external notification.
    pub event_id: String,
    /// Closed allowlist enforced by Pro's `normalize_local_alert`.
    pub alert_type: String,
    /// e.g. `{"detected":"OBS Studio","process":"obs64.exe"}` or
    /// `{"metric":"cpu","value_pct":94,"threshold_pct":85,"duration_s":300}`.
    pub detail: Value,
    /// RFC3339, set by the agent at the moment the local notification fired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

/// One admin-facing notification (P3). Currently emitted for config DRIFT — a
/// device running behind its resolved policy epoch (and, when the device reports
/// `toggle_states`, the specific toggles that diverged). `detail` is PII-free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct FleetNotification {
    pub id: i64,
    /// Notification class — "drift" (config drift) or "insider_risk" (a critical
    /// Argus DLP/tamper/tripwire signal); extensible (e.g. "device_dark").
    pub kind: String,
    /// "info" | "warn" | "critical".
    pub severity: String,
    /// One-line human summary (no PII).
    pub summary: String,
    /// Structured PII-free detail (e.g. drifted toggle paths + desired values).
    pub detail: Value,
    #[serde(default)]
    pub device_id: Option<DeviceId>,
    #[serde(default)]
    pub hostname: Option<String>,
    pub read: bool,
    pub created_at: String,
    /// Number of identical events folded into this notification during its
    /// cooldown window. A value above one means Fleet suppressed duplicate
    /// paging while retaining occurrence evidence.
    #[serde(default = "default_notification_occurrence_count")]
    pub occurrence_count: u32,
    /// Most recent occurrence, distinct from when this incident card started.
    #[serde(default)]
    pub latest_at: String,
    /// Bounded, safe correlation/request IDs for this incident. Never carries
    /// device content, camera data, clipboard data, or free-form text.
    #[serde(default)]
    pub correlation_ids: Vec<String>,
}

fn default_notification_occurrence_count() -> u32 {
    1
}
