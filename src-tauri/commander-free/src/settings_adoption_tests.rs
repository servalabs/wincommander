// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use serde_json::json;

fn patch() -> Value {
    json!({"ideal":{"privacy":{"telemetry":{"windowsDisabled":true}}}})
}

fn settings(intent: Option<bool>, observed: Option<bool>) -> AppSettings {
    let mut settings = super::super::create_default_settings();
    settings.ideal.privacy.telemetry.windows_disabled = intent;
    settings.current.privacy.telemetry.windows_disabled = observed;
    settings
}

#[test]
fn ordinary_patch_reproduces_lost_intent_but_conditional_adoption_preserves_it() {
    let current = settings(Some(false), Some(true));
    let old = local_write::prepare(&current, local_write::Mutation::Patch(patch())).unwrap();
    assert_eq!(old.ideal.privacy.telemetry.windows_disabled, Some(true));
    let adopted = prepare(&current, &patch()).unwrap();
    assert_eq!(
        adopted.ideal.privacy.telemetry.windows_disabled,
        Some(false)
    );
}

#[test]
fn only_unset_positive_observations_are_adopted() {
    for (intent, observed, expected) in [
        (None, Some(true), Some(true)),
        (None, Some(false), None),
        (None, None, None),
        (Some(false), Some(true), Some(false)),
        (Some(true), Some(false), Some(true)),
    ] {
        let adopted = prepare(&settings(intent, observed), &patch()).unwrap();
        assert_eq!(adopted.ideal.privacy.telemetry.windows_disabled, expected);
    }
}

#[test]
fn adoption_cannot_change_app_policy_current_or_non_boolean_preferences() {
    for patch in [
        json!({"app":{"theme":"light"}}),
        json!({"policy":{"managed":false}}),
        json!({"current":{"privacy":true}}),
        json!({"ideal":{"privacy":{"telemetry":{"windowsDisabled":false}}}}),
        json!({"ideal":{"privacy":{"telemetry":{"windowsDisabled":[]}}}}),
        json!({"ideal":{"unknownPreference":true}}),
        json!({"ideal":{},"app":{}}),
    ] {
        assert!(prepare(&settings(None, Some(true)), &patch).is_err());
    }
}

#[test]
fn managed_locks_apply_to_adoption_before_persistence() {
    let mut current = settings(None, Some(true));
    current.policy.managed = true;
    current.policy.locked_paths = vec!["privacy.telemetry.windowsDisabled".into()];
    assert!(prepare(&current, &patch()).unwrap_err().contains("locked"));
}

struct Fixture {
    previous: Option<AppSettings>,
    decoy: bool,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    fn new() -> Self {
        let lock = super::super::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let previous = super::super::SETTINGS_CACHE
            .lock()
            .unwrap()
            .replace(settings(None, Some(true)));
        let decoy = super::super::DECOY_MODE.swap(false, std::sync::atomic::Ordering::Relaxed);
        Self {
            previous,
            decoy,
            _lock: lock,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        *super::super::SETTINGS_CACHE.lock().unwrap() = self.previous.take();
        super::super::DECOY_MODE.store(self.decoy, std::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn authoritative_load_wins_over_a_stale_in_process_snapshot() {
    let _fixture = Fixture::new();
    let result = apply_with(
        patch(),
        true,
        || Ok(settings(Some(false), Some(true))),
        |candidate| {
            assert_eq!(
                candidate.ideal.privacy.telemetry.windows_disabled,
                Some(false)
            );
            Ok(())
        },
        |before, after| assert_eq!(before["ideal"], after["ideal"]),
    )
    .unwrap();
    assert_eq!(result.ideal.privacy.telemetry.windows_disabled, Some(false));
}

#[test]
fn rejected_fresh_policy_cannot_leave_a_stale_cache_beside_its_new_revision() {
    let _fixture = Fixture::new();
    let mut fresh = settings(None, Some(true));
    fresh.policy.managed = true;
    fresh.policy.locked_paths = vec!["privacy.telemetry.windowsDisabled".into()];
    let result = apply_with(
        patch(),
        true,
        || Ok(fresh),
        |_| panic!("rejected adoption persisted"),
        |_, _| panic!("rejected adoption notified"),
    );
    assert!(result.unwrap_err().contains("locked"));
    assert!(super::super::SETTINGS_CACHE.lock().unwrap().is_none());
}

#[test]
fn uncertain_or_conflicted_adoption_is_never_replayed_or_published() {
    let _fixture = Fixture::new();
    let attempts = std::cell::Cell::new(0);
    let result = apply_with(
        patch(),
        true,
        || Ok(settings(None, Some(true))),
        |_| {
            attempts.set(attempts.get() + 1);
            Err("personal_settings_conflict".into())
        },
        |_, _| panic!("unconfirmed adoption notified observers"),
    );
    assert!(result.is_err());
    assert_eq!(attempts.get(), 1);
    assert!(super::super::SETTINGS_CACHE.lock().unwrap().is_none());
}

#[test]
fn decoy_rejection_precedes_authoritative_read_or_write() {
    let _fixture = Fixture::new();
    super::super::DECOY_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
    let result = apply_with(
        patch(),
        true,
        || panic!("decoy adoption read real settings"),
        |_| panic!("decoy adoption persisted"),
        |_, _| panic!("decoy adoption notified"),
    );
    assert!(result.unwrap_err().contains("decoy"));
}
