// SPDX-License-Identifier: AGPL-3.0-or-later

use super::tally::{Observation, Tally, MAX_PAYLOAD_SAMPLES};
use super::*;

fn obs(device: u64, rssi: i16, ids: &[u16]) -> Observation {
    Observation {
        device,
        rssi,
        elapsed_ms: 0,
        company_ids: ids.to_vec(),
        payloads: ids.iter().map(|id| id.to_le_bytes().to_vec()).collect(),
        service_uuids: Vec::new(),
        local_name: None,
    }
}

#[test]
fn unrelated_vendors_do_not_match() {
    // 0x004C Apple, 0x0006 Microsoft
    assert!(classify(&[0x004C, 0x0006], &[], None).is_empty());
}

#[test]
fn company_rules_carry_registrant_and_confidence() {
    let m = classify(&[0x0D53], &[], None);
    assert_eq!(
        (m[0].vendor, m[0].rule.as_str(), m[0].confidence),
        ("luxottica", "company_id:0x0D53", Confidence::High)
    );
    assert_eq!(classify(&[0x03C2], &[], None)[0].vendor, "snap");
    assert_eq!(
        classify(&[0x01AB], &[], None)[0].confidence,
        Confidence::Medium
    );
    assert_eq!(
        classify(&[0x05D6], &[], None)[0].confidence,
        Confidence::Low
    );
}

#[test]
fn service_uuid_and_names_match_case_insensitively() {
    let uuids = vec!["7905FFF0-B5CE-4E99-A40F-4B1E122D00D0".to_string()];
    assert_eq!(classify(&[], &uuids, None)[0].vendor, "heycyan");
    assert_eq!(
        classify(&[], &[], Some("Ray-Ban Meta 0A1B"))[0].rule,
        "name:ray-ban"
    );
    assert_eq!(
        classify(&[], &[], Some("HeyCyan-WXYZ"))[0].vendor,
        "heycyan"
    );
    assert!(classify(&[], &[], Some("Hydra 10 5.3")).is_empty());
}

#[test]
fn tally_counts_distinct_devices_and_near_devices() {
    let mut t = Tally::new(-75, false);
    t.observe(&obs(1, -60, &[0x0D53]));
    t.observe(&obs(1, -62, &[0x0D53]));
    t.observe(&obs(2, -90, &[0x0D53]));
    t.observe(&obs(3, -50, &[0x004C]));
    let s = t.finish("completed", None, 20, "active");
    assert_eq!((s.adverts_total, s.devices_total), (4, 3));
    assert_eq!(s.matches.len(), 1);
    let m = &s.matches[0];
    assert_eq!(
        (m.adverts, m.devices, m.near_devices, m.strongest_rssi),
        (3, 2, 1, -60)
    );
    assert_eq!(s.near_confidence, Some(Confidence::High));
    assert!(
        m.payload_samples.is_empty(),
        "payloads stay out unless requested"
    );
}

#[test]
fn far_only_matches_do_not_raise_near_confidence() {
    let mut t = Tally::new(-75, false);
    t.observe(&obs(1, -88, &[0x0D53]));
    t.observe(&obs(2, -50, &[0x05D6]));
    let s = t.finish("completed", None, 20, "active");
    assert_eq!(s.near_confidence, Some(Confidence::Low));
}

#[test]
fn payload_samples_are_per_rule_deduplicated_and_capped() {
    let mut t = Tally::new(-75, true);
    for device in 0..10u64 {
        let mut o = obs(device, -60, &[0x01AB, 0x004C]);
        o.payloads[0] = vec![device as u8 % 7, 0xAA];
        t.observe(&o);
    }
    let s = t.finish("completed", None, 20, "active");
    let m = &s.matches[0];
    assert_eq!(m.payload_samples.len(), MAX_PAYLOAD_SAMPLES);
    assert_eq!(m.payload_samples[0], "00aa");
}

#[test]
fn summary_never_contains_names_or_addresses() {
    let mut t = Tally::new(-75, false);
    let mut o = obs(0xC47C8D1E2B3F, -60, &[]);
    o.local_name = Some("Ray-Ban Meta Alice".into());
    t.observe(&o);
    let json = serde_json::to_string(&t.finish("completed", None, 20, "active")).unwrap();
    assert!(!json.contains("Alice"));
    assert!(!json.contains(&0xC47C8D1E2B3Fu64.to_string()));
}

/// Opt-in field capture on a real radio:
/// `WINCMD_GLASSES_SCAN_SECONDS=60 cargo test -p commander-free --lib live_glasses_scan -- --ignored --nocapture`
#[cfg(windows)]
#[test]
#[ignore = "needs a Bluetooth LE radio and real surroundings"]
fn live_glasses_scan() {
    let seconds = std::env::var("WINCMD_GLASSES_SCAN_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let summary = scan_blocking(seconds, DEFAULT_NEAR_RSSI, true, true);
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    assert_ne!(summary.status, "start_failed", "{:?}", summary.stop_reason);
}
