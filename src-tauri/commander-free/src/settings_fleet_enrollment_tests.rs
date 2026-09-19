// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::SigningKey;

fn key(seed: u8) -> String {
    STANDARD.encode(
        SigningKey::from_bytes(&[seed; 32])
            .verifying_key()
            .to_bytes(),
    )
}
fn managed() -> AppSettings {
    let mut current = super::super::create_default_settings();
    current.policy.fleet_signing_key = Some(key(7));
    current.policy.sync_mode = "managed".into();
    current.policy.managed = true;
    current.policy.master_config_version = Some(12);
    current.policy.locked_paths = vec!["privacy.telemetry".into()];
    current.app.fleet.server_url = "https://fleet.example.invalid".into();
    current.app.fleet.signing_key_pub = key(7);
    current.app.fleet.enabled = true;
    current.app.fleet.dispatch = true;
    current.app.fleet.privacy_shield_session_owned = true;
    let mut value = serde_json::to_value(current).unwrap();
    value["ideal"]["privacy"]["privacyShield"]["fleetManaged"] = json!(true);
    value["ideal"]["privacy"]["privacyShield"]["fleetMonitoringEnabled"] = json!(true);
    serde_json::from_value(value).unwrap()
}
fn plan(current: &AppSettings) -> ConnectionPlan {
    ConnectionPlan::new(current, current.app.fleet.server_url.clone(), true, key(7)).unwrap()
}

#[test]
fn reconnect_keeps_committed_policy_and_personal_session_state() {
    let current = managed();
    let p = plan(&current);
    assert!(!p.needs_confirmation());
    let updated = p.prepare(&current, false).unwrap();
    let before = serde_json::to_value(current).unwrap();
    let after = serde_json::to_value(updated).unwrap();
    for field in ["/ideal", "/policy", "/app/fleet/privacyShieldSessionOwned"] {
        assert_eq!(before.pointer(field), after.pointer(field), "{field}");
    }
}

#[test]
fn native_policy_pin_requires_confirmation_when_the_ui_pin_is_missing() {
    let mut current = managed();
    current.app.fleet.signing_key_pub.clear();
    let p =
        ConnectionPlan::new(&current, current.app.fleet.server_url.clone(), true, key(8)).unwrap();
    assert!(p.needs_confirmation());
    assert!(p.prepare(&current, false).is_err());
}

#[test]
fn pinned_authority_cannot_be_removed_by_a_blank_connect_key() {
    let current = managed();
    assert!(ConnectionPlan::new(
        &current,
        current.app.fleet.server_url.clone(),
        true,
        String::new()
    )
    .is_err());
}

#[test]
fn reducing_managed_dispatch_requires_native_confirmation() {
    let current = managed();
    let p = ConnectionPlan::new(
        &current,
        current.app.fleet.server_url.clone(),
        false,
        key(7),
    )
    .unwrap();
    assert!(p.needs_confirmation());
    assert!(p.prepare(&current, false).is_err());
}

#[test]
fn approval_binds_the_key_dispatch_and_observed_state_not_only_the_url() {
    let current = managed();
    let a =
        ConnectionPlan::new(&current, current.app.fleet.server_url.clone(), true, key(8)).unwrap();
    let b =
        ConnectionPlan::new(&current, current.app.fleet.server_url.clone(), true, key(9)).unwrap();
    let c = ConnectionPlan::new(
        &current,
        current.app.fleet.server_url.clone(),
        false,
        key(8),
    )
    .unwrap();
    assert_ne!(a.confirmation_binding(), b.confirmation_binding());
    assert_ne!(a.confirmation_binding(), c.confirmation_binding());
}

#[test]
fn a_policy_change_while_the_dialog_is_open_invalidates_the_plan() {
    let mut current = managed();
    let p = plan(&current);
    current.policy.master_config_version = Some(13);
    assert!(p.prepare(&current, true).is_err());
}

#[test]
fn a_device_change_while_the_dialog_is_open_invalidates_the_plan() {
    let mut current = managed();
    let p = plan(&current);
    current.device_id = "other-device".into();
    assert!(p.prepare(&current, true).is_err());
}

#[test]
fn malformed_nonempty_verification_keys_are_rejected_before_mutation() {
    let current = super::super::create_default_settings();
    for key in [
        "not-a-key".to_string(),
        STANDARD.encode([1u8; 31]),
        STANDARD.encode([1u8; 33]),
    ] {
        assert!(
            ConnectionPlan::new(&current, "https://fleet.example.invalid".into(), true, key)
                .is_err()
        );
    }
}

#[test]
fn first_enrollment_remains_available_without_starting_privacy_shield() {
    let current = super::super::create_default_settings();
    let p = ConnectionPlan::new(
        &current,
        "https://fleet.example.invalid".into(),
        true,
        key(7),
    )
    .unwrap();
    assert!(!p.needs_confirmation());
    let updated = p.prepare(&current, false).unwrap();
    assert!(updated.app.fleet.enabled);
    assert_eq!(updated.policy.fleet_signing_key, Some(key(7)));
    assert_eq!(
        serde_json::to_value(updated).unwrap()["ideal"]["privacy"]["privacyShield"]
            ["fleetMonitoringEnabled"],
        false
    );
}

#[test]
fn rekeying_is_still_available_after_native_approval() {
    let current = managed();
    let p =
        ConnectionPlan::new(&current, current.app.fleet.server_url.clone(), true, key(8)).unwrap();
    assert!(p.prepare(&current, false).is_err());
    let updated = p.prepare(&current, true).unwrap();
    assert_eq!(updated.policy.fleet_signing_key, Some(key(8)));
    assert_eq!(
        updated.policy.master_config_version,
        current.policy.master_config_version
    );
}

#[test]
fn authorization_and_persistence_share_one_cache_transaction() {
    use super::super::{GLOBAL_STATE_TEST_LOCK, SETTINGS_CACHE};
    let _lock = GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    struct Restore(Option<AppSettings>);
    impl Drop for Restore {
        fn drop(&mut self) {
            *SETTINGS_CACHE.lock().unwrap() = self.0.take();
        }
    }
    let current = managed();
    let p = plan(&current);
    let _restore = Restore(SETTINGS_CACHE.lock().unwrap().replace(current));
    SETTINGS_CACHE
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .policy
        .master_config_version = Some(13);
    let before = serde_json::to_value(SETTINGS_CACHE.lock().unwrap().as_ref().unwrap()).unwrap();
    assert!(p
        .commit_with(
            true,
            false,
            |_| panic!("stale approval reached disk"),
            |_, _| panic!("stale approval notified observers")
        )
        .is_err());
    assert_eq!(
        serde_json::to_value(SETTINGS_CACHE.lock().unwrap().as_ref().unwrap()).unwrap(),
        before
    );
    let current = SETTINGS_CACHE.lock().unwrap().as_ref().unwrap().clone();
    let p = plan(&current);
    assert!(p
        .commit_with(
            false,
            true,
            |_| Err("injected write failure".into()),
            |_, _| panic!("failed write notified observers")
        )
        .is_err());
    assert_eq!(
        serde_json::to_value(SETTINGS_CACHE.lock().unwrap().as_ref().unwrap()).unwrap(),
        before
    );
    let updated = p.commit_with(false, false, |_| Ok(()), |_, _| {}).unwrap();
    assert_eq!(updated.policy.master_config_version, Some(13));
}
