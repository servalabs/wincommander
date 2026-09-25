// SPDX-License-Identifier: AGPL-3.0-or-later
use super::super::{create_default_settings, GLOBAL_STATE_TEST_LOCK, SETTINGS_CACHE};
use super::*;
use serde_json::json;
use std::cell::Cell;

struct Fixture {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<AppSettings>,
}
impl Fixture {
    fn managed() -> Self {
        let lock = GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut current = create_default_settings();
        current.policy.sync_mode = "managed".into();
        current.policy.managed = true;
        current.policy.fleet_signing_key = Some("test-public-key-not-a-credential".into());
        current.policy.master_config_version = Some(10);
        current.policy.locked_paths = vec!["privacy.telemetry.windowsDisabled".into()];
        let mut raw = serde_json::to_value(current).unwrap();
        raw["ideal"]["privacy"]["telemetry"]["windowsDisabled"] = json!(true);
        let previous = SETTINGS_CACHE
            .lock()
            .unwrap()
            .replace(serde_json::from_value(raw).unwrap());
        Self {
            _lock: lock,
            previous,
        }
    }
    fn current(&self) -> Value {
        serde_json::to_value(SETTINGS_CACHE.lock().unwrap().as_ref().unwrap()).unwrap()
    }
    fn deny(&self, mutation: Mutation) {
        let before = self.current();
        let attempts = Cell::new(0);
        let result = apply_with(
            mutation,
            false,
            |_| {
                attempts.set(attempts.get() + 1);
                Ok(())
            },
            |_, _| panic!("denied mutation notified observers"),
        );
        assert!(result.is_err(), "unauthorized mutation reached persistence");
        assert_eq!(attempts.get(), 0, "authorization must precede every write");
        assert_eq!(
            self.current(),
            before,
            "denial must not replace cached state"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        *SETTINGS_CACHE.lock().unwrap() = self.previous.take();
    }
}

#[test]
fn renderer_cannot_remove_management_or_the_pinned_key() {
    let fixture = Fixture::managed();
    fixture.deny(Mutation::Patch(json!({"policy": {
        "syncMode": "standalone", "managed": false, "lockedPaths": [], "fleetSigningKey": null
    }})));
}

#[test]
fn renderer_cannot_retarget_or_disable_managed_fleet() {
    let fixture = Fixture::managed();
    fixture.deny(Mutation::Patch(
        json!({"app": {"fleet": {"serverUrl": "http://example.invalid"}}}),
    ));
}

#[test]
fn full_replacement_cannot_bypass_a_locked_preference() {
    let fixture = Fixture::managed();
    let mut proposed = fixture.current();
    proposed["ideal"]["privacy"]["telemetry"]["windowsDisabled"] = json!(false);
    fixture.deny(Mutation::Replace(proposed));
}

#[test]
fn local_or_elevated_writes_cannot_disable_a_locked_monitor_reporter() {
    let fixture = Fixture::managed();
    {
        let mut state = SETTINGS_CACHE.lock().unwrap();
        let current = state.as_mut().unwrap();
        current.policy.locked_paths = vec!["security.monitorAlertReporting.remoteAccess".into()];
        current.ideal.security.monitor_alert_reporting.remote_access = Some(true);
    }
    fixture.deny(Mutation::Patch(json!({
        "ideal": { "security": { "monitorAlertReporting": { "remoteAccess": false } } }
    })));
}

#[test]
fn local_or_elevated_writes_cannot_disable_each_locked_extended_monitor_reporter() {
    for reporter in ["print", "usb", "dlp", "tamper"] {
        let fixture = Fixture::managed();
        {
            let mut state = SETTINGS_CACHE.lock().unwrap();
            let current = state.as_mut().unwrap();
            current.policy.locked_paths =
                vec![format!("security.monitorAlertReporting.{reporter}")];
            let reporting = &mut current.ideal.security.monitor_alert_reporting;
            match reporter {
                "print" => reporting.print = Some(true),
                "usb" => reporting.usb = Some(true),
                "dlp" => reporting.dlp = Some(true),
                "tamper" => reporting.tamper = Some(true),
                _ => unreachable!(),
            }
        }
        fixture.deny(Mutation::Patch(json!({
            "ideal": { "security": { "monitorAlertReporting": { (reporter): false } } }
        })));
    }
}

#[test]
fn imported_backup_cannot_bypass_a_locked_preference() {
    let fixture = Fixture::managed();
    let mut proposed = fixture.current();
    proposed["ideal"]["privacy"]["telemetry"]["windowsDisabled"] = json!(false);
    fixture.deny(Mutation::Import(serde_json::to_string(&proposed).unwrap()));
}

#[test]
fn a_pinned_key_still_enforces_locks_if_the_legacy_mode_is_standalone() {
    let fixture = Fixture::managed();
    SETTINGS_CACHE
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .policy
        .sync_mode = "standalone".into();
    fixture.deny(Mutation::Patch(
        json!({"ideal": {"privacy": {"telemetry": {"windowsDisabled": false}}}}),
    ));
}

#[test]
fn renderer_cannot_change_the_device_identity() {
    let fixture = Fixture::managed();
    fixture.deny(Mutation::Patch(
        json!({"deviceId": "44c166db-2071-4c12-8457-d8a0687b60c9"}),
    ));
}

#[test]
fn a_direct_json_import_has_the_same_size_limit_as_the_native_picker() {
    let fixture = Fixture::managed();
    let mut json = serde_json::to_string(&fixture.current()).unwrap();
    json.push_str(&" ".repeat(4 * 1024 * 1024));
    fixture.deny(Mutation::Import(json));
}

#[test]
fn the_existing_locked_patch_is_denied_before_writing() {
    let fixture = Fixture::managed();
    fixture.deny(Mutation::Patch(
        json!({"ideal": {"privacy": {"telemetry": {"windowsDisabled": false}}}}),
    ));
}

#[test]
fn a_managed_user_can_change_an_unlocked_preference() {
    let _fixture = Fixture::managed();
    let updated = apply_with(
        Mutation::Patch(json!({"app": {"theme": "light"}})),
        false,
        |_| Ok(()),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(updated).unwrap()["app"]["theme"],
        "light"
    );
}

#[test]
fn failed_persistence_leaves_the_existing_state_and_observers_untouched() {
    let fixture = Fixture::managed();
    let before = fixture.current();
    let result = apply_with(
        Mutation::Patch(json!({"app": {"theme": "light"}})),
        true,
        |_| Err("injected persistence failure".into()),
        |_, _| panic!("failed persistence notified observers"),
    );
    assert!(result.is_err());
    assert_eq!(fixture.current(), before);
}

#[test]
fn ancestor_replacement_cannot_reset_a_locked_subtree() {
    let fixture = Fixture::managed();
    SETTINGS_CACHE
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .policy
        .locked_paths = vec!["privacy.telemetry".into()];
    let mut proposed = fixture.current();
    proposed["ideal"]["privacy"]["telemetry"] = json!({});
    fixture.deny(Mutation::Replace(proposed));
}

#[test]
fn identical_locked_values_are_idempotent_but_changes_are_not() {
    let fixture = Fixture::managed();
    let before = fixture.current();
    let updated = apply_with(Mutation::Replace(before), false, |_| Ok(()), |_, _| {}).unwrap();
    assert_eq!(
        serde_json::to_value(updated).unwrap()["ideal"]["privacy"]["telemetry"]["windowsDisabled"],
        true
    );
}

#[test]
fn flows_alias_and_explicit_app_locks_cover_full_replacement() {
    let fixture = Fixture::managed();
    SETTINGS_CACHE
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .app
        .pro_flows = vec![json!({"name":"fixture-only"})];
    for lock in ["app.flows", "app.proFlows"] {
        SETTINGS_CACHE
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .policy
            .locked_paths = vec![lock.into()];
        let mut proposed = fixture.current();
        proposed["app"]["proFlows"] = json!([]);
        // A non-empty locked rule set cannot be replaced with an empty set.
        fixture.deny(Mutation::Replace(proposed));
    }
    SETTINGS_CACHE
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .policy
        .locked_paths = vec!["app.theme".into()];
    fixture.deny(Mutation::Patch(json!({"app": {"theme": "light"}})));
}

#[test]
fn runtime_session_observations_remain_writable_without_authority_changes() {
    let _fixture = Fixture::managed();
    let updated = apply_with(
        Mutation::Patch(json!({"app": {"fleet": {"privacyShieldSessionOwned": true}}})),
        false,
        |_| Ok(()),
        |_, _| {},
    )
    .unwrap();
    assert!(updated.app.fleet.privacy_shield_session_owned);
}

#[test]
fn standalone_settings_remain_editable_and_import_preserves_device_identity() {
    let fixture = Fixture::managed();
    *SETTINGS_CACHE.lock().unwrap() = Some(create_default_settings());
    let before = fixture.current();
    let mut imported = before.clone();
    imported["deviceId"] = json!("a-backup-from-another-device");
    imported["createdAt"] = json!("2000-01-01T00:00:00Z");
    imported["app"]["theme"] = json!("light");
    let updated = apply_with(
        Mutation::Import(imported.to_string()),
        false,
        |_| Ok(()),
        |_, _| {},
    )
    .unwrap();
    let updated = serde_json::to_value(updated).unwrap();
    assert_eq!(updated["deviceId"], before["deviceId"]);
    assert_eq!(updated["createdAt"], before["createdAt"]);
    assert_eq!(updated["app"]["theme"], "light");
}
