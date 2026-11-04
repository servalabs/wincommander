// SPDX-License-Identifier: AGPL-3.0-or-later
//! Presentation of the local watchdog's observed preconditions.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Check {
    #[default]
    Never,
    InProgress,
    Succeeded,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum State {
    Disabled,
    Triggered,
    WatchdogStopped,
    SettingsUnavailable,
    InvalidBaseline,
    LockdownDisabled,
    Starting,
    Checking,
    CheckStale,
    CheckFailed,
    Monitoring,
}

pub(super) struct Inputs {
    pub enabled: bool,
    pub fired: bool,
    pub task_running: bool,
    pub personal_available: bool,
    pub baseline_valid: bool,
    pub lockdown_enabled: bool,
    pub last_check: Check,
    pub check_recent: bool,
}

/// This only limits what the UI may infer; it does not affect the watchdog.
pub(super) fn recent(check: Check, changed_at: Option<i64>, now: i64) -> bool {
    let Some(age) = changed_at.and_then(|then| now.checked_sub(then)) else {
        return false;
    };
    let limit = match check {
        Check::InProgress => 5 * 60,
        Check::Succeeded => 2 * 60 * 60,
        _ => return false,
    };
    (0..=limit).contains(&age)
}

pub(super) fn evaluate(input: Inputs) -> State {
    if input.fired {
        State::Triggered
    } else if !input.enabled {
        State::Disabled
    } else if !input.task_running {
        State::WatchdogStopped
    } else if !input.personal_available {
        State::SettingsUnavailable
    } else if !input.baseline_valid {
        State::InvalidBaseline
    } else if !input.lockdown_enabled {
        State::LockdownDisabled
    } else {
        match input.last_check {
            Check::Never => State::Starting,
            Check::InProgress if input.check_recent => State::Checking,
            Check::InProgress => State::CheckStale,
            Check::Failed => State::CheckFailed,
            Check::Succeeded if input.check_recent => State::Monitoring,
            Check::Succeeded => State::CheckStale,
        }
    }
}

#[cfg(test)]
#[path = "inactivity_status_tests.rs"]
mod tests;
