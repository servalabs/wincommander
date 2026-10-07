// SPDX-License-Identifier: AGPL-3.0-or-later
use super::tests::{record, unavailable_key};
use super::*;

#[test]
fn committed_status_remains_readable_during_a_session_transaction() {
    let _global = super::super::GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let original = status();
    let committed = Status {
        mode: Mode::Service,
        recovery_required: false,
        can_save: true,
    };
    publish_status(committed);
    let session = SESSION.lock().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        send.send(status()).unwrap();
    });
    let observed = receive.recv_timeout(std::time::Duration::from_secs(1));
    drop(session);
    reader.join().unwrap();
    publish_status(original);
    assert_eq!(observed.unwrap(), committed);
}

#[test]
fn failed_save_disables_automation_without_waiting_for_another_status_read() {
    let _global = super::super::GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let original_status = status();
    let original_session = SESSION.lock().unwrap().take();
    publish_status(Status {
        mode: Mode::Service,
        recovery_required: false,
        can_save: true,
    });
    let result = save(&json!({}));
    let available = automation_available();
    *SESSION.lock().unwrap() = original_session;
    publish_status(original_status);
    assert!(result.is_err());
    assert!(!available);
}

#[test]
fn failed_save_does_not_forget_that_original_protected_data_is_still_locked() {
    let _global = super::super::GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _fixture = replace_session_with_temporary_for_test().unwrap();
    *SESSION.lock().unwrap() = None;
    publish_status(Status {
        mode: Mode::Service,
        recovery_required: true,
        can_save: true,
    });
    assert!(save(&json!({})).is_err());
    assert_eq!(status().mode, Mode::Temporary);
    assert!(status().recovery_required);
    assert!(!automation_available());
}

#[test]
fn password_reset_defaults_round_trip_ordinary_preferences_without_replacing_lost_secrets() {
    let (mut state, _) = load_with(
        Ok(record(None)),
        false,
        || Err(unavailable_key()),
        || panic!("unused"),
    )
    .unwrap();
    let mut settings = super::super::create_default_settings();
    super::super::apply_personal_recovery_defaults(&mut settings);
    let (_, mut user) =
        super::super::split_settings_value(serde_json::to_value(settings).unwrap()).unwrap();
    user["app"]["theme"] = json!("light");
    let mut committed = None;
    save_service_with(
        &mut state,
        &user,
        |_, _| panic!("must not create or replace the lost key"),
        |request| {
            assert!(request.legacy_recovery_required);
            assert!(request.value[SECRET_ENVELOPE].is_null());
            let result = PersonalSettingsRecord {
                revision: 1,
                value: Some(request.value),
                legacy_recovery_required: true,
            };
            committed = Some(result.clone());
            Ok(result)
        },
    )
    .unwrap();
    let (restored, value) = load_with(
        Ok(committed.unwrap()),
        true,
        || Err(unavailable_key()),
        || panic!("explicit empty envelope must not read old secrets"),
    )
    .unwrap();
    assert_eq!(value.unwrap()["app"]["theme"], "light");
    assert!(restored.status().can_save && restored.status().recovery_required);
    assert!(restored.secrets_locked);
}

#[test]
fn removing_a_key_during_an_open_session_cannot_replace_or_clear_protected_secrets() {
    for changed in [json!({"app":{"flowSigningSeedB64":"new-seed"}}), json!({})] {
        let (mut state, _) = load_with_open(
            Ok(record(Some(
                json!({"_personalSecrets":"existing-envelope"}),
            ))),
            true,
            || panic!("unused"),
            || panic!("unused"),
            |_| Ok(json!({"app":{"flowSigningSeedB64":"original-seed"}})),
        )
        .unwrap();
        let result = save_service_with(
            &mut state,
            &changed,
            |_, require_existing| {
                assert!(require_existing);
                Err(unavailable_key())
            },
            |_| panic!("must not commit after key loss"),
        );
        assert!(result.is_err());
        assert_eq!(
            state.protected_secrets.as_deref(),
            Some("existing-envelope")
        );
        assert_eq!(state.secrets["app"]["flowSigningSeedB64"], "original-seed");
    }
}

#[test]
fn locked_envelope_survives_an_ordinary_preference_save_unchanged() {
    let (mut state, value) = load_with_open(
        Ok(record(Some(
            json!({"app":{"theme":"dark"},"_personalSecrets":"original-ciphertext"}),
        ))),
        true,
        || panic!("obsolete"),
        || panic!("obsolete"),
        |_| Err(unavailable_key()),
    )
    .unwrap();
    let mut value = value.unwrap();
    value["app"]["theme"] = json!("light");
    save_service_with(
        &mut state,
        &value,
        |_, _| panic!("must not replace key"),
        |request| {
            assert_eq!(request.value[SECRET_ENVELOPE], "original-ciphertext");
            assert_eq!(request.value["app"]["theme"], "light");
            Ok(PersonalSettingsRecord {
                revision: 5,
                value: Some(request.value),
                legacy_recovery_required: false,
            })
        },
    )
    .unwrap();
    assert_eq!(state.revision, 5);
    assert!(state.secrets_locked);
}

#[test]
fn failed_or_unacknowledged_writes_do_not_advance_cached_secret_generation() {
    for error in [
        "service rejected request: personal_settings_conflict (changed)",
        "service reply timed out",
    ] {
        let (mut state, _) = load_with_open(
            Ok(record(Some(
                json!({"app":{"theme":"dark"},"_personalSecrets":"old-ciphertext"}),
            ))),
            true,
            || panic!("obsolete"),
            || panic!("obsolete"),
            |_| Ok(json!({"app":{"flowSigningSeedB64":"old-seed"}})),
        )
        .unwrap();
        let changed = json!({"app":{"theme":"light","flowSigningSeedB64":"new-seed"}});
        assert!(save_service_with(
            &mut state,
            &changed,
            |_, require_existing| {
                assert!(require_existing);
                Ok("new-ciphertext".into())
            },
            |request| {
                assert_eq!(request.expected_revision, 4);
                assert_eq!(request.value[SECRET_ENVELOPE], "new-ciphertext");
                assert!(request.value["app"].get("flowSigningSeedB64").is_none());
                Err(error.into())
            }
        )
        .is_err());
        assert_eq!(state.revision, 4);
        assert_eq!(state.protected_secrets.as_deref(), Some("old-ciphertext"));
        assert_eq!(state.secrets["app"]["flowSigningSeedB64"], "old-seed");
    }
}

#[test]
fn first_migration_seals_unchanged_legacy_secrets_in_the_same_record() {
    let original = json!({"app":{"theme":"dark","flowSigningSeedB64":"old-seed"}});
    let (mut state, value) = load_with(
        Ok(record(None)),
        false,
        || Ok(Some(original)),
        || panic!("unused"),
    )
    .unwrap();
    save_service_with(
        &mut state,
        &value.unwrap(),
        |secret, require_existing| {
            assert!(!require_existing);
            assert_eq!(secret["app"]["flowSigningSeedB64"], "old-seed");
            Ok("preserved-seed-ciphertext".into())
        },
        |request| {
            assert_eq!(request.value[SECRET_ENVELOPE], "preserved-seed-ciphertext");
            Ok(PersonalSettingsRecord {
                revision: 1,
                value: Some(request.value),
                legacy_recovery_required: false,
            })
        },
    )
    .unwrap();
}

#[test]
fn explicit_empty_envelope_does_not_depend_on_an_old_profile_key() {
    let (mut state, value) = load_with_open(
        Ok(record(Some(
            json!({"app":{"theme":"light"},"_personalSecrets":null}),
        ))),
        true,
        || panic!("obsolete"),
        || panic!("must not read old key"),
        |_| panic!("no secret"),
    )
    .unwrap();
    assert!(!state.status().recovery_required);
    save_service_with(
        &mut state,
        &value.unwrap(),
        |_, _| panic!("must not create key"),
        |request| {
            assert!(request.value[SECRET_ENVELOPE].is_null());
            Ok(PersonalSettingsRecord {
                revision: 5,
                value: Some(request.value),
                legacy_recovery_required: false,
            })
        },
    )
    .unwrap();
}
