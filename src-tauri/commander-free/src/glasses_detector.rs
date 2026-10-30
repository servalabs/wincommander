// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded BLE scan for camera-capable smart glasses near this PC.
//!
//! Rules use public Bluetooth SIG company IDs plus the service UUID and name
//! patterns documented by yjeanrenaud/yj_nearbyglasses (AGPL-3.0); this is an
//! independent implementation. A hit means "camera-capable wearable likely
//! nearby", never proof of recording. The summary is content-free: device
//! addresses and names never leave this module, and distinct devices are
//! counted through a per-scan salted hash.

use serde::Serialize;

pub const DEFAULT_NEAR_RSSI: i16 = -75;
const MIN_SECONDS: u64 = 5;
const MAX_SECONDS: u64 = 120;

/// How much a hit on its own says about smart glasses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Vendor ships many unrelated BLE products (chip vendors).
    Low,
    /// Vendor also ships headsets or other wearables.
    Medium,
    /// Identifier belongs to a glasses maker or product line.
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub vendor: &'static str,
    pub rule: String,
    pub confidence: Confidence,
}

struct CompanyRule {
    id: u16,
    vendor: &'static str,
    confidence: Confidence,
}

// Registrants checked against the Bluetooth SIG company_identifiers.yaml on
// 2026-10-30. Meta's IDs also cover Quest headsets; Jieli is a chip vendor
// used in many cheap earbuds and speakers.
const COMPANY_RULES: &[CompanyRule] = &[
    CompanyRule {
        id: 0x01AB,
        vendor: "meta",
        confidence: Confidence::Medium,
    },
    CompanyRule {
        id: 0x058E,
        vendor: "meta",
        confidence: Confidence::Medium,
    },
    CompanyRule {
        id: 0x0D53,
        vendor: "luxottica",
        confidence: Confidence::High,
    },
    CompanyRule {
        id: 0x03C2,
        vendor: "snap",
        confidence: Confidence::High,
    },
    CompanyRule {
        id: 0x060C,
        vendor: "vuzix",
        confidence: Confidence::Medium,
    },
    CompanyRule {
        id: 0x05D6,
        vendor: "jieli",
        confidence: Confidence::Low,
    },
];

const HEYCYAN_SERVICE_UUID: &str = "7905fff0-b5ce-4e99-a40f-4b1e122d00d0";

const NAME_RULES: &[(&str, &str)] = &[
    ("ray-ban", "meta"),
    ("rayban", "meta"),
    ("heycyan", "heycyan"),
];

pub(crate) fn classify(
    company_ids: &[u16],
    service_uuids: &[String],
    local_name: Option<&str>,
) -> Vec<Match> {
    let mut out = Vec::new();
    for id in company_ids {
        if let Some(rule) = COMPANY_RULES.iter().find(|r| r.id == *id) {
            out.push(Match {
                vendor: rule.vendor,
                rule: format!("company_id:0x{id:04X}"),
                confidence: rule.confidence,
            });
        }
    }
    if service_uuids
        .iter()
        .any(|u| u.eq_ignore_ascii_case(HEYCYAN_SERVICE_UUID))
    {
        out.push(Match {
            vendor: "heycyan",
            rule: format!("service_uuid:{HEYCYAN_SERVICE_UUID}"),
            confidence: Confidence::High,
        });
    }
    if let Some(name) = local_name {
        let lower = name.to_ascii_lowercase();
        if let Some((pat, vendor)) = NAME_RULES.iter().find(|(p, _)| lower.contains(p)) {
            out.push(Match {
                vendor,
                rule: format!("name:{pat}"),
                confidence: Confidence::High,
            });
        }
    }
    out
}

mod alerts;
pub(crate) mod monitor;
#[cfg(windows)]
mod radio;
mod tally;
#[cfg(test)]
mod tests;

pub use tally::ScanSummary;
#[cfg(not(windows))]
use tally::Tally;

fn scan_blocking(seconds: u64, near_rssi: i16, active: bool, include_payload: bool) -> ScanSummary {
    let seconds = seconds.clamp(MIN_SECONDS, MAX_SECONDS);
    #[cfg(windows)]
    {
        radio::scan(seconds, near_rssi, active, include_payload)
    }
    #[cfg(not(windows))]
    {
        let mode = if active { "active" } else { "passive" };
        Tally::new(near_rssi, include_payload).finish("unsupported", None, seconds, mode)
    }
}

/// One bounded scan. Read-only: it never changes Bluetooth state or policy.
#[tauri::command]
pub async fn glasses_detector_scan(
    seconds: Option<u64>,
    near_rssi: Option<i16>,
    active: Option<bool>,
    include_payload: Option<bool>,
) -> Result<ScanSummary, String> {
    let seconds = seconds.unwrap_or(20);
    let near_rssi = near_rssi.unwrap_or(DEFAULT_NEAR_RSSI);
    tauri::async_runtime::spawn_blocking(move || {
        scan_blocking(
            seconds,
            near_rssi,
            active.unwrap_or(true),
            include_payload.unwrap_or(false),
        )
    })
    .await
    .map_err(|e| format!("glasses scan task failed: {e}"))
}
