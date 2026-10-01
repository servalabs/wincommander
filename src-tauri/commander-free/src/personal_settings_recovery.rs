// SPDX-License-Identifier: AGPL-3.0-or-later
//! Recovery of a personal-settings session that began before the authenticated
//! local service was ready. This deliberately reloads native state and never
//! accepts renderer-provided preferences as recovery input.
use super::*;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TEMPORARY_RECONNECT_INTERVAL: Duration = Duration::from_secs(5);
const RECOVERED_SETTINGS_RELOAD_REQUIRED: &str =
    "Personal settings service recovered. Refresh settings before saving so newer preferences are preserved.";
static LAST_TEMPORARY_RECONNECT_ATTEMPT: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum TemporaryRecovery {
    ReloadRequired,
}

pub(super) fn session_needs_refresh(recheck_locked: bool) -> bool {
    let published = status();
    published.mode == Mode::Temporary || (recheck_locked && published.recovery_required)
}

fn reconnect_due(last_attempt: Option<Instant>, now: Instant) -> bool {
    last_attempt.is_none_or(|last| now.duration_since(last) >= TEMPORARY_RECONNECT_INTERVAL)
}

fn claim_temporary_reconnect_attempt() -> bool {
    let now = Instant::now();
    let Ok(mut last_attempt) = LAST_TEMPORARY_RECONNECT_ATTEMPT.lock() else {
        return false;
    };
    if !reconnect_due(*last_attempt, now) {
        return false;
    }
    *last_attempt = Some(now);
    true
}

/// Reconnect a session that started while the authenticated personal-settings
/// service was unavailable. Existing service data is adopted, then the caller
/// must reload its full settings snapshot rather than replaying stale values.
pub(super) fn refresh_session_if_due(recheck_locked: bool) -> bool {
    refresh_session_with(
        recheck_locked,
        load_current_session,
        claim_temporary_reconnect_attempt,
    )
}

fn refresh_session_with(
    recheck_locked: bool,
    reload: impl FnOnce() -> Result<(Session, Option<Value>), String>,
    claim_attempt: impl FnOnce() -> bool,
) -> bool {
    let mut session = match SESSION.lock() {
        Ok(session) => session,
        Err(_) => return false,
    };
    let Some(current) = session.as_mut() else {
        return false;
    };
    if !session_needs_refresh(recheck_locked) || !claim_attempt() {
        return false;
    }
    match recover_temporary_session(current, reload) {
        // settings.rs owns the transaction that invalidates the mixed cache.
        // Do not publish the recovered status yet: load() will do that only
        // after it has rebuilt one full authoritative settings snapshot.
        Ok(_) => true,
        Err(error) => {
            crate::log_message(
                "debug",
                &format!("[Settings] personal service reconnect deferred: {error}"),
            );
            false
        }
    }
}

pub(super) fn recover_temporary_session(
    current: &mut Session,
    reload: impl FnOnce() -> Result<(Session, Option<Value>), String>,
) -> Result<TemporaryRecovery, String> {
    let (recovered, _restored_value) = reload().map_err(|error| {
        if service_unavailable(&error) {
            "Personal settings service is still unavailable; no preferences were saved".to_string()
        } else {
            error
        }
    })?;
    if recovered.mode != Mode::Service {
        return Err(
            "Personal settings service is still unavailable; no preferences were saved".into(),
        );
    }
    // `safe_defaults` with a reachable service means old encrypted secrets are
    // still protected, while ordinary preferences may later be saved without
    // replacing those secrets. The full reload is required before that write.
    *current = recovered;
    Ok(TemporaryRecovery::ReloadRequired)
}

pub(super) fn recovered_settings_reload_required(error: &str) -> bool {
    error == RECOVERED_SETTINGS_RELOAD_REQUIRED
}

pub(super) fn recovered_settings_reload_error() -> String {
    RECOVERED_SETTINGS_RELOAD_REQUIRED.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_throttle_allows_first_and_spaced_attempts_only() {
        let now = Instant::now();
        assert!(reconnect_due(None, now));
        assert!(!reconnect_due(Some(now), now));
        assert!(reconnect_due(Some(now - TEMPORARY_RECONNECT_INTERVAL), now));
    }

    #[test]
    fn published_temporary_status_can_recover_a_session_still_in_service_mode() {
        let _global = super::super::super::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _fixture = replace_session_with_temporary_for_test().unwrap();
        let (current, _) = load_with(
            Ok(super::super::tests::record(Some(
                serde_json::json!({"app":{"theme":"dark"}}),
            ))),
            true,
            || panic!("obsolete legacy state"),
            || Ok(serde_json::json!({})),
        )
        .unwrap();
        *SESSION.lock().unwrap() = Some(current);
        assert!(
            refresh_session_with(
                false,
                || {
                    let mut record = super::super::tests::record(Some(
                        serde_json::json!({"app":{"theme":"light"}}),
                    ));
                    record.revision = 9;
                    load_with(
                        Ok(record),
                        true,
                        || panic!("obsolete legacy state"),
                        || Ok(serde_json::json!({})),
                    )
                },
                || true
            ),
            "published blocked status must not strand a service session"
        );
        assert_eq!(SESSION.lock().unwrap().as_ref().unwrap().revision, 9);
        assert_eq!(
            status(),
            UNAVAILABLE_STATUS,
            "cache rebuild must precede writable status"
        );
    }

    #[test]
    fn locked_service_data_can_be_rechecked_without_pretending_it_was_recovered() {
        let _global = super::super::super::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let original = status();
        publish_status(Status {
            mode: Mode::Service,
            recovery_required: true,
            can_save: true,
        });
        let retry = session_needs_refresh(true);
        let automatic_retry = session_needs_refresh(false);
        publish_status(original);
        assert!(retry);
        assert!(
            !automatic_retry,
            "routine reads must not retry locked Windows keys"
        );
    }

    #[test]
    fn explicit_recheck_adopts_restored_secrets_without_publishing_before_cache_reload() {
        let _global = super::super::super::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _fixture = replace_session_with_temporary_for_test().unwrap();
        let locked_record = super::super::tests::record(Some(json!({
            "app": {"theme":"light"}, "_personalSecrets":"original-ciphertext"
        })));
        let (locked, _) = load_with_open(
            Ok(locked_record.clone()),
            true,
            || panic!("obsolete legacy state"),
            || panic!("envelope exists"),
            |_| Err(super::super::tests::unavailable_key()),
        )
        .unwrap();
        let published = locked.status();
        *SESSION.lock().unwrap() = Some(locked);
        publish_status(published);
        assert!(refresh_session_with(
            true,
            || load_with_open(
                Ok(locked_record),
                true,
                || panic!("obsolete legacy state"),
                || panic!("envelope exists"),
                |_| Ok(json!({"app":{"flowSigningSeedB64":"original-test-seed"}})),
            ),
            || true
        ));
        let guard = SESSION.lock().unwrap();
        let restored = guard.as_ref().unwrap();
        assert!(!restored.secrets_locked);
        assert_eq!(
            restored.protected_secrets.as_deref(),
            Some("original-ciphertext")
        );
        assert_eq!(status(), published);
        assert!(
            !automation_available(),
            "cached locked snapshot still controls automation"
        );
    }

    #[test]
    fn failed_recheck_preserves_the_previous_revision_and_protected_envelope() {
        let (mut locked, _) = load_with_open(
            Ok(super::super::tests::record(Some(
                json!({"_personalSecrets":"original-ciphertext"}),
            ))),
            true,
            || panic!("obsolete legacy state"),
            || panic!("envelope exists"),
            |_| Err(super::super::tests::unavailable_key()),
        )
        .unwrap();
        let result = recover_temporary_session(&mut locked, || {
            Err("service rejected request: forbidden (denied)".into())
        });
        assert!(result.is_err());
        assert_eq!(locked.revision, 4);
        assert!(locked.secrets_locked);
        assert_eq!(
            locked.protected_secrets.as_deref(),
            Some("original-ciphertext")
        );
    }
}
