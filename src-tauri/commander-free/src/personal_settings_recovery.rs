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

pub(super) fn temporary_session_needs_refresh() -> bool {
    status().mode == Mode::Temporary
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
pub(super) fn refresh_temporary_session_if_due() -> bool {
    let mut session = match SESSION.lock() {
        Ok(session) => session,
        Err(_) => return false,
    };
    let Some(current) = session.as_mut() else {
        return false;
    };
    if current.mode != Mode::Temporary || !claim_temporary_reconnect_attempt() {
        return false;
    }
    match recover_temporary_session(current, load_current_session) {
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
}
