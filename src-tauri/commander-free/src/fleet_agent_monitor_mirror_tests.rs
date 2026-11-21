use super::{merge_remote_patch, remote_command_settings_patch, RemoteCommandStateUpdate};
use crate::settings::{create_default_settings, AppSettings};
use serde_json::{json, Value};

fn update(catalog_id: &str) -> RemoteCommandStateUpdate {
    RemoteCommandStateUpdate {
        command_id: "cmd-test".to_string(),
        catalog_id: catalog_id.to_string(),
        payload: json!({}),
    }
}

const MONITORS: [(&str, &str, [&str; 2]); 5] = [
    ("monitoring.ransomware", "privacy", ["ransomwareMonitor", "enabled"]),
    ("monitoring.auth_anomaly", "privacy", ["authAnomalyMonitor", "enabled"]),
    ("monitoring.remote_access", "privacy", ["remoteAccessMonitor", "enabled"]),
    ("monitoring.wifi_guard", "network", ["wifiGuard", "enabled"]),
    ("monitoring.driver_watch", "security", ["drivers", "watchEnabled"]),
];

#[test]
fn fleet_monitor_commands_move_only_their_on_off_preference() {
    for (prefix, section, path) in MONITORS {
        for (verb, expected) in [("start", true), ("stop", false)] {
            let catalog = format!("{prefix}.{verb}");
            let patch = remote_command_settings_patch(&update(&catalog))
                .unwrap_or_else(|| panic!("{catalog} must be mirrored into settings"));
            for side in ["ideal", "current"] {
                let group = &patch[side][section][path[0]];
                assert_eq!(group[path[1]], expected, "{catalog}");
                assert_eq!(group.as_object().unwrap().len(), 1, "{catalog} must not touch tuning");
            }
        }
    }
}

#[test]
fn status_reads_and_unknown_monitors_are_not_mirrored() {
    for catalog in [
        "driver.watch.status",
        "monitoring.ransomware.status",
        "monitoring.unknown.start",
        "monitoring.glasses.start",
    ] {
        assert!(remote_command_settings_patch(&update(catalog)).is_none(), "{catalog}");
    }
}

#[test]
fn mirrored_monitor_state_lands_in_the_fields_the_ui_reads() {
    let mut value: Value = serde_json::to_value(create_default_settings()).unwrap();
    for verb in ["start", "stop"] {
        for (prefix, ..) in MONITORS {
            let patch = remote_command_settings_patch(&update(&format!("{prefix}.{verb}"))).unwrap();
            merge_remote_patch(&mut value, &patch);
        }
        let settings: AppSettings = serde_json::from_value(value.clone()).unwrap();
        let on = verb == "start";
        for state in [&settings.ideal, &settings.current] {
            assert_eq!(state.privacy.ransomware_monitor.enabled, Some(on));
            assert_eq!(state.privacy.auth_anomaly_monitor.enabled, Some(on));
            assert_eq!(state.privacy.remote_access_monitor.enabled, Some(on));
            assert_eq!(state.network.wifi_guard.enabled, Some(on));
            assert_eq!(state.security.drivers.watch_enabled, on);
        }
    }
}

#[test]
fn a_later_stop_wins_over_an_earlier_start_in_one_batch() {
    let mut patch = json!({});
    for catalog in ["monitoring.wifi_guard.start", "monitoring.wifi_guard.stop"] {
        merge_remote_patch(&mut patch, &remote_command_settings_patch(&update(catalog)).unwrap());
    }
    assert_eq!(patch["ideal"]["network"]["wifiGuard"]["enabled"], false);
}

#[test]
fn enrolling_after_launch_starts_the_apply_loop() {
    let source = include_str!("fleet_agent.rs");
    let start = source.find("async fn fleet_connect_inner_locked(").expect("connect fn");
    let end = start + source[start..].find("struct ConnectOutcome").expect("connect fn end");
    let body = &source[start..end];
    let commit = body.find("plan.commit(confirmed)").expect("commit");
    let ensure = body.find("crate::ensure_fleet_apply_loop()").expect("loop start after enrolling");
    assert!(commit < ensure, "the loop starts only after enrollment was persisted");
}
