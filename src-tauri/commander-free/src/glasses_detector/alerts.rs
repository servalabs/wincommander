// SPDX-License-Identifier: AGPL-3.0-or-later
//! Turns a stream of advertisements into rate-limited glasses alerts.

use super::tally::Observation;
use super::{classify, Confidence, DEFAULT_NEAR_RSSI};
use serde::Serialize;
use std::collections::HashMap;

/// One advert can be a reflection; a second near sighting confirms presence.
const CONFIRM_WINDOW_MS: u64 = 10_000;
/// One alert per vendor per window keeps a colleague's glasses from spamming.
pub(super) const ALERT_COOLDOWN_MS: u64 = 5 * 60_000;
const MAX_TRACKED_DEVICES: usize = 512;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GlassesDetection {
    pub vendor: &'static str,
    pub rule: String,
    pub confidence: Confidence,
    pub rssi: i16,
    pub detected_at: String,
}

#[derive(Default)]
pub(crate) struct AlertGate {
    last_near_ms: HashMap<u64, u64>,
    last_alert_ms: HashMap<&'static str, u64>,
}

impl AlertGate {
    /// `now_ms` is any monotonic clock; `detected_at` is the wall-clock label.
    pub(crate) fn observe(
        &mut self,
        obs: &Observation,
        now_ms: u64,
        detected_at: impl FnOnce() -> String,
    ) -> Option<GlassesDetection> {
        if obs.rssi < DEFAULT_NEAR_RSSI {
            return None;
        }
        // Low-confidence vendors (chip makers) are never alert-worthy alone.
        let best = classify(
            &obs.company_ids,
            &obs.service_uuids,
            obs.local_name.as_deref(),
        )
        .into_iter()
        .filter(|m| m.confidence > Confidence::Low)
        .max_by_key(|m| m.confidence)?;

        if self.last_near_ms.len() >= MAX_TRACKED_DEVICES {
            self.last_near_ms
                .retain(|_, seen| now_ms.saturating_sub(*seen) <= CONFIRM_WINDOW_MS);
        }
        let previous = self.last_near_ms.insert(obs.device, now_ms);
        if !previous.is_some_and(|seen| now_ms.saturating_sub(seen) <= CONFIRM_WINDOW_MS) {
            return None;
        }
        if let Some(last) = self.last_alert_ms.get(best.vendor) {
            if now_ms.saturating_sub(*last) < ALERT_COOLDOWN_MS {
                return None;
            }
        }
        self.last_alert_ms.insert(best.vendor, now_ms);
        Some(GlassesDetection {
            vendor: best.vendor,
            rule: best.rule,
            confidence: best.confidence,
            rssi: obs.rssi,
            detected_at: detected_at(),
        })
    }
}

pub(super) fn vendor_label(vendor: &str) -> &'static str {
    match vendor {
        "meta" => "Meta / Ray-Ban Meta",
        "luxottica" => "Ray-Ban / Oakley Meta",
        "snap" => "Snap Spectacles",
        "vuzix" => "Vuzix",
        "heycyan" => "HeyCyan-based camera glasses",
        _ => "camera glasses",
    }
}
