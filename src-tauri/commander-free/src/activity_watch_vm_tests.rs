// SPDX-License-Identifier: AGPL-3.0-or-later
use super::request;
use serde_json::Value;
use std::path::Path;

fn require_guest(phase: &str) {
    assert_eq!(std::env::var("WINCMD_RUN_AW_VM_E2E").as_deref(), Ok("1"));
    assert_eq!(std::env::var("COMPUTERNAME").as_deref(), Ok("WCVCUPGRADE"));
    assert!(Path::new(r"C:\WcVcLab\DISPOSABLE-VM").is_file());
    let bios = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(r"HARDWARE\DESCRIPTION\System\BIOS")
        .expect("guest BIOS identity");
    let manufacturer: String = bios
        .get_value("SystemManufacturer")
        .expect("guest manufacturer");
    assert!(manufacturer.to_ascii_lowercase().contains("qemu"));
    let proof: Value = serde_json::from_slice(
        &std::fs::read(r"C:\WcVcLab\aw-guest-proof.json").expect("guest preparation proof"),
    )
    .expect("guest preparation JSON");
    assert_eq!(proof["uuid"], "caf4e022-2f77-4f97-af5a-0c8c57f00c66");
    assert_eq!(proof["phase"], phase);
    assert!(proof["session"].as_u64().is_some_and(|session| session > 0));
}

#[tokio::test]
#[ignore = "real ActivityWatch in the marked interactive disposable Windows guest only"]
async fn reads_real_activitywatch_events_in_disposable_windows_guest() {
    require_guest("transport");
    let info = request("/api/0/info")
        .await
        .expect("real local ActivityWatch info");
    assert_eq!(info["version"], "v0.14.3");
    let events = request("/api/0/buckets/wincmd-vm-fixture/events?start=2026-11-15T00%3A00%3A00Z&end=2026-11-16T00%3A00%3A00Z&limit=-1")
        .await.expect("production transport must read the synthetic guest bucket");
    let events = events.as_array().expect("real event array");
    assert_eq!(events.len(), 2);
    assert_eq!(
        events
            .iter()
            .map(|event| event["duration"].as_f64().expect("duration"))
            .sum::<f64>(),
        180.0
    );
    assert!(events
        .iter()
        .all(|event| event["data"]["app"] == "wincmd-vm-fixture"));
    println!("AW_VM_TRANSPORT_OK: real production loopback transport read two synthetic events totaling 180 seconds");
}

#[tokio::test]
#[ignore = "real API-key protected Tauri server in the marked disposable Windows guest only"]
async fn reports_real_activitywatch_authorization_requirement_in_disposable_guest() {
    require_guest("tauri-auth");
    assert_eq!(
        request("/api/0/info").await.expect("public server info")["version"],
        "v0.14.3"
    );
    let error = request("/api/0/buckets/")
        .await
        .expect_err("protected server must refuse unauthenticated reads");
    assert!(error.starts_with("ActivityWatch returned HTTP 401"));
    assert!(!error.contains("Bearer"));
    println!("AW_VM_AUTH_DIAGNOSTIC_OK: real protected server remains protected and production transport reports HTTP 401");
}
