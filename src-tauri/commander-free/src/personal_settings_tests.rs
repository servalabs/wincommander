// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

pub(super) fn unavailable_key() -> String {
    "SETTINGS_KEY_UNAVAILABLE: original data preserved".into()
}

pub(super) fn record(value: Option<Value>) -> PersonalSettingsRecord {
    PersonalSettingsRecord {
        revision: if value.is_some() { 4 } else { 0 },
        value,
        legacy_recovery_required: false,
    }
}

#[test]
fn migrated_preferences_survive_a_password_reset_while_secrets_remain_locked() {
    let (session, value) = load_with(
        Ok(record(Some(
            json!({"app": {"theme": "light", "lockedPanelIds": ["vault"]}}),
        ))),
        true,
        || panic!("must not reload obsolete preferences"),
        || Err(unavailable_key()),
    )
    .unwrap();
    assert_eq!(value.unwrap()["app"]["theme"], "light");
    assert!(session.status().can_save);
    assert!(session.status().recovery_required);
    assert!(!session.safe_defaults);
    assert!(session.secrets_locked);
}

#[test]
fn migration_keeps_readable_preferences_and_exact_signing_identity() {
    let source = json!({"app": {"theme": "dark", "flowSigningSeedB64": "original-seed"}});
    let (state, value) = load_with(
        Ok(record(None)),
        false,
        || Ok(Some(source.clone())),
        || panic!("not used before migration"),
    )
    .unwrap();
    assert_eq!(value, Some(source));
    assert_eq!(state.secrets["app"]["flowSigningSeedB64"], "original-seed");
    assert!(!state.status().recovery_required);
    assert_eq!(state.revision, 0);
    assert!(state.legacy_overlay_pending_migration);
}

#[test]
fn reset_before_migration_uses_separate_service_defaults_without_replacing_old_data() {
    let (state, value) = load_with(
        Ok(record(None)),
        false,
        || Err(unavailable_key()),
        || panic!("not used before migration"),
    )
    .unwrap();
    assert!(value.is_none());
    assert!(state.safe_defaults);
    assert!(state.legacy_recovery_required);
    assert!(state.secrets_locked);
    assert!(state.status().can_save);
}

#[test]
fn unavailable_service_never_reverts_a_migrated_account_to_obsolete_preferences() {
    let (state, value) = load_with(
        Err("service connection failed".into()),
        true,
        || panic!("must not resurrect obsolete settings"),
        || panic!("must not touch secrets"),
    )
    .unwrap();
    assert_eq!(state.mode, Mode::Temporary);
    assert!(state.safe_defaults);
    assert!(!state.status().can_save);
    assert!(value.is_none());
}

#[test]
fn absent_service_keeps_a_readable_existing_profile_usable() {
    let (state, value) = load_with(
        Err("service rejected request: unknown_verb (unsupported)".into()),
        false,
        || Ok(Some(json!({"app":{"theme":"light"}}))),
        || panic!("unused"),
    )
    .unwrap();
    assert_eq!(state.mode, Mode::Legacy);
    assert_eq!(value.unwrap()["app"]["theme"], "light");
    assert!(state.status().can_save);
    assert!(!state.legacy_overlay_pending_migration);
}

#[test]
fn unavailable_legacy_key_without_service_allows_only_temporary_preferences() {
    let (state, value) = load_with(
        Err("service connection failed".into()),
        false,
        || Err(unavailable_key()),
        || panic!("unused"),
    )
    .unwrap();
    assert_eq!(state.mode, Mode::Temporary);
    assert!(!state.status().can_save);
    assert!(state.status().recovery_required);
    assert!(value.is_none());
}

#[test]
fn temporary_session_adopts_recovered_service_data_but_requires_a_full_reload() {
    let (mut temporary, _) = load_with(
        Err("service connection failed".into()),
        true,
        || panic!("migrated data must not fall back"),
        || panic!("unused"),
    )
    .unwrap();
    let result = super::recovery::recover_temporary_session(&mut temporary, || {
        load_with(
            Ok(record(Some(json!({"app":{"theme":"restored"}})))),
            true,
            || panic!("unused"),
            || Ok(json!({})),
        )
    })
    .unwrap();
    assert_eq!(result, super::recovery::TemporaryRecovery::ReloadRequired);
    assert_eq!(temporary.mode, Mode::Service);
    assert_eq!(temporary.revision, 4);
}

#[test]
fn temporary_session_with_locked_legacy_secrets_recovers_for_ordinary_preference_saves() {
    let (mut temporary, _) = load_with(
        Err("service connection failed".into()),
        false,
        || Err(unavailable_key()),
        || panic!("unused"),
    )
    .unwrap();
    let result = super::recovery::recover_temporary_session(&mut temporary, || {
        load_with(
            Ok(record(None)),
            false,
            || Err(unavailable_key()),
            || panic!("unused"),
        )
    })
    .unwrap();
    assert_eq!(result, super::recovery::TemporaryRecovery::ReloadRequired);
    assert_eq!(temporary.mode, Mode::Service);
    assert!(temporary.safe_defaults && temporary.secrets_locked);
    save_service_with(
        &mut temporary,
        &json!({"app":{"theme":"light"}}),
        |_, _| panic!("ordinary settings must not replace locked secrets"),
        |request| {
            assert!(request.value[SECRET_ENVELOPE].is_null());
            assert!(request.legacy_recovery_required);
            Ok(PersonalSettingsRecord {
                revision: 1,
                value: Some(request.value),
                legacy_recovery_required: true,
            })
        },
    )
    .unwrap();
    assert_eq!(temporary.revision, 1);
}

#[test]
fn temporary_session_does_not_recreate_a_missing_migrated_service_record() {
    let (mut temporary, _) = load_with(
        Err("service connection failed".into()),
        true,
        || panic!("migrated data must not fall back"),
        || panic!("unused"),
    )
    .unwrap();
    let result = super::recovery::recover_temporary_session(&mut temporary, || {
        load_with(
            Ok(record(None)),
            true,
            || panic!("migrated data must not fall back"),
            || panic!("unused"),
        )
    });
    assert!(result.is_err());
    assert_eq!(temporary.mode, Mode::Temporary);
}

#[test]
fn service_and_legacy_integrity_errors_never_trigger_a_reset() {
    for error in [
        "service rejected request: personal_settings_corrupt (invalid)",
        "service rejected request: forbidden (denied)",
        "Invalid personal settings service response",
        "Service identity could not be verified",
        "service reply signature check failed: invalid",
        "service reply did not match the request",
    ] {
        assert!(load_with(
            Err(error.into()),
            false,
            || panic!("must not fallback"),
            || panic!("unused")
        )
        .is_err());
    }
    for error in [
        "Access denied",
        "per-user data could not be decoded",
        "Invalid JSON",
    ] {
        assert!(load_with(
            Ok(record(None)),
            false,
            || Err(error.into()),
            || panic!("unused")
        )
        .is_err());
    }
}

#[test]
fn a_missing_migrated_record_is_not_first_run() {
    assert!(load_with(
        Ok(record(None)),
        true,
        || panic!("must preserve"),
        || panic!("unused")
    )
    .is_err());
}

#[test]
fn restoring_old_key_access_unlocks_features_without_reverting_new_preferences() {
    let mut recovered = record(Some(json!({"app":{"theme":"light"}})));
    recovered.legacy_recovery_required = true;
    let (state, value) = load_with(
        Ok(recovered),
        true,
        || Ok(Some(json!({"app":{"theme":"dark"}}))),
        || Ok(json!({})),
    )
    .unwrap();
    assert!(!state.status().recovery_required);
    assert_eq!(value.unwrap()["app"]["theme"], "light");
}

#[test]
fn deleting_legacy_files_does_not_count_as_recovering_the_old_key() {
    let mut missing = record(Some(json!({"app":{"theme":"light"}})));
    missing.legacy_recovery_required = true;
    let (state, _) = load_with(Ok(missing), true, || Ok(None), || Ok(json!({}))).unwrap();
    assert!(state.status().recovery_required);
}

#[test]
fn a_personal_record_cannot_override_machine_policy_or_inject_a_signing_secret() {
    let malicious = json!({"policy":{"managed":false}, "deviceId":"other",
        "ideal":{"privacy":{"startupPin":{"enabled":false,"realHash":"wrong"}}},
        "app":{"theme":"light","flowSigningSeedB64":"injected"}});
    let (_, value) = load_with(
        Ok(record(Some(malicious))),
        true,
        || panic!("unused"),
        || Ok(json!({"app":{"flowSigningSeedB64":"original"}})),
    )
    .unwrap();
    let value = value.unwrap();
    assert!(value.get("policy").is_none());
    assert!(value.get("deviceId").is_none());
    assert!(value.pointer("/ideal/privacy/startupPin").is_none());
    assert_eq!(value["app"]["flowSigningSeedB64"], "original");
}

#[test]
fn recovery_defaults_preserve_machine_pin_and_policy_and_disable_automation() {
    let mut settings = super::super::create_default_settings();
    settings.ideal.privacy.startup_pin.enabled = Some(true);
    settings.ideal.privacy.startup_pin.real_hash = Some("existing-real-hash".into());
    settings.ideal.privacy.startup_pin.decoy_hash = Some("existing-decoy-hash".into());
    settings.ideal.privacy.startup_pin.destroy_hash = Some("existing-destroy-hash".into());
    settings.policy.managed = true;
    let before = serde_json::to_value(&settings).unwrap();
    super::super::apply_personal_recovery_defaults(&mut settings);
    let after = serde_json::to_value(&settings).unwrap();
    assert_eq!(before["ideal"], after["ideal"]);
    assert_eq!(before["policy"], after["policy"]);
    assert!(!settings.app.auto_heal);
    assert!(!settings.app.auto_fix_all);
    assert!(!settings.app.auto_update);
    assert!(settings.app.flows.is_empty() && settings.app.pro_flows.is_empty());
    assert!(settings.app.dead_mans_switch.is_none());
}

#[test]
fn a_client_version_change_does_not_require_machine_write_permission_for_personal_preferences() {
    let settings = super::super::create_default_settings();
    let (mut old, _) =
        super::super::split_settings_value(serde_json::to_value(&settings).unwrap()).unwrap();
    old["appVersion"] = json!("older-version");
    let (current, _) =
        super::super::split_settings_value(serde_json::to_value(&settings).unwrap()).unwrap();
    assert!(!super::super::machine_settings_changed(&old, &current).unwrap());
    let mut changed_policy = current;
    changed_policy["policy"]["managed"] = json!(true);
    assert!(super::super::machine_settings_changed(&old, &changed_policy).unwrap());
}
