// SPDX-License-Identifier: AGPL-3.0-or-later
//! Presentation of the local watchdog's observed preconditions.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Check {
    #[default]
    Never,
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
            Check::Failed => State::CheckFailed,
            Check::Succeeded => State::Monitoring,
        }
    }
}

#[cfg(test)]
#[path = "inactivity_status_tests.rs"]
mod tests;
