// SPDX-License-Identifier: AGPL-3.0-or-later
//! Content-free aggregation of matched advertisements for one scan.

use super::{classify, Confidence};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

pub(super) const MAX_PAYLOAD_SAMPLES: usize = 5;

/// One received advertisement, already reduced to what the rules need.
pub(crate) struct Observation {
    pub device: u64,
    pub rssi: i16,
    pub elapsed_ms: u64,
    pub company_ids: Vec<u16>,
    /// Manufacturer payloads, index-aligned with `company_ids`.
    pub payloads: Vec<Vec<u8>>,
    pub service_uuids: Vec<String>,
    pub local_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuleSummary {
    pub vendor: &'static str,
    pub rule: String,
    pub confidence: Confidence,
    pub adverts: u32,
    pub devices: u32,
    pub near_devices: u32,
    pub strongest_rssi: i16,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    /// Hex manufacturer payloads for rule tuning; empty unless requested.
    pub payload_samples: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    /// completed | radio_unavailable | stopped_early | start_failed | unsupported
    pub status: &'static str,
    pub stop_reason: Option<String>,
    pub seconds: u64,
    pub scanning_mode: &'static str,
    pub near_rssi: i16,
    pub adverts_total: u32,
    pub devices_total: u32,
    /// Highest confidence among rules seen at or above `near_rssi`.
    pub near_confidence: Option<Confidence>,
    pub matches: Vec<RuleSummary>,
}

struct RuleTally {
    summary: RuleSummary,
    devices: HashSet<u64>,
    near: HashSet<u64>,
}

pub(crate) struct Tally {
    near_rssi: i16,
    include_payload: bool,
    adverts: u32,
    devices: HashSet<u64>,
    rules: BTreeMap<String, RuleTally>,
}

impl Tally {
    pub(crate) fn new(near_rssi: i16, include_payload: bool) -> Self {
        Self {
            near_rssi,
            include_payload,
            adverts: 0,
            devices: HashSet::new(),
            rules: BTreeMap::new(),
        }
    }

    pub(crate) fn observe(&mut self, obs: &Observation) {
        self.adverts = self.adverts.saturating_add(1);
        self.devices.insert(obs.device);
        for m in classify(
            &obs.company_ids,
            &obs.service_uuids,
            obs.local_name.as_deref(),
        ) {
            let entry = self
                .rules
                .entry(m.rule.clone())
                .or_insert_with(|| RuleTally {
                    summary: RuleSummary {
                        vendor: m.vendor,
                        rule: m.rule.clone(),
                        confidence: m.confidence,
                        adverts: 0,
                        devices: 0,
                        near_devices: 0,
                        strongest_rssi: i16::MIN,
                        first_seen_ms: obs.elapsed_ms,
                        last_seen_ms: obs.elapsed_ms,
                        payload_samples: Vec::new(),
                    },
                    devices: HashSet::new(),
                    near: HashSet::new(),
                });
            let s = &mut entry.summary;
            s.adverts = s.adverts.saturating_add(1);
            s.strongest_rssi = s.strongest_rssi.max(obs.rssi);
            s.last_seen_ms = s.last_seen_ms.max(obs.elapsed_ms);
            entry.devices.insert(obs.device);
            if obs.rssi >= self.near_rssi {
                entry.near.insert(obs.device);
            }
            if self.include_payload && s.payload_samples.len() < MAX_PAYLOAD_SAMPLES {
                let payload = obs
                    .company_ids
                    .iter()
                    .position(|id| m.rule == format!("company_id:0x{id:04X}"))
                    .and_then(|i| obs.payloads.get(i));
                if let Some(bytes) = payload {
                    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                    if !s.payload_samples.contains(&hex) {
                        s.payload_samples.push(hex);
                    }
                }
            }
        }
    }

    pub(crate) fn finish(
        self,
        status: &'static str,
        stop_reason: Option<String>,
        seconds: u64,
        scanning_mode: &'static str,
    ) -> ScanSummary {
        let matches: Vec<RuleSummary> = self
            .rules
            .into_values()
            .map(|t| RuleSummary {
                devices: t.devices.len() as u32,
                near_devices: t.near.len() as u32,
                ..t.summary
            })
            .collect();
        let near_confidence = matches
            .iter()
            .filter(|m| m.near_devices > 0)
            .map(|m| m.confidence)
            .max();
        ScanSummary {
            status,
            stop_reason,
            seconds,
            scanning_mode,
            near_rssi: self.near_rssi,
            adverts_total: self.adverts,
            devices_total: self.devices.len() as u32,
            near_confidence,
            matches,
        }
    }
}
