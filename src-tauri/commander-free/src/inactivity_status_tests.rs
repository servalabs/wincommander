use super::*;

fn input() -> Inputs {
    Inputs {
        enabled: true,
        fired: false,
        task_running: true,
        personal_available: true,
        baseline_valid: true,
        lockdown_enabled: true,
        last_check: Check::Succeeded,
        check_recent: true,
    }
}

#[test]
fn reports_actual_watchdog_preconditions_without_asserting_action_completion() {
    assert_eq!(evaluate(input()), State::Monitoring);
    assert_eq!(
        evaluate(Inputs {
            enabled: false,
            ..input()
        }),
        State::Disabled
    );
    assert_eq!(
        evaluate(Inputs {
            fired: true,
            ..input()
        }),
        State::Triggered
    );
    assert_eq!(
        evaluate(Inputs {
            fired: true,
            enabled: false,
            ..input()
        }),
        State::Triggered
    );
    assert_eq!(
        evaluate(Inputs {
            task_running: false,
            ..input()
        }),
        State::WatchdogStopped
    );
    assert_eq!(
        evaluate(Inputs {
            personal_available: false,
            ..input()
        }),
        State::SettingsUnavailable
    );
    assert_eq!(
        evaluate(Inputs {
            baseline_valid: false,
            ..input()
        }),
        State::InvalidBaseline
    );
    assert_eq!(
        evaluate(Inputs {
            lockdown_enabled: false,
            ..input()
        }),
        State::LockdownDisabled
    );
    assert_eq!(
        evaluate(Inputs {
            last_check: Check::Never,
            ..input()
        }),
        State::Starting
    );
    assert_eq!(
        evaluate(Inputs {
            last_check: Check::Failed,
            ..input()
        }),
        State::CheckFailed
    );
    assert_eq!(
        evaluate(Inputs {
            last_check: Check::InProgress,
            ..input()
        }),
        State::Checking
    );
    assert_eq!(
        evaluate(Inputs {
            last_check: Check::InProgress,
            check_recent: false,
            ..input()
        }),
        State::CheckStale
    );
    assert_eq!(
        evaluate(Inputs {
            last_check: Check::Succeeded,
            check_recent: false,
            ..input()
        }),
        State::CheckStale
    );
}

#[test]
fn old_or_clock_rolled_back_iterations_are_not_current() {
    assert!(recent(Check::InProgress, Some(100), 400));
    assert!(!recent(Check::InProgress, Some(100), 401));
    assert!(recent(Check::Succeeded, Some(100), 7300));
    assert!(!recent(Check::Succeeded, Some(100), 7301));
    assert!(!recent(Check::Succeeded, Some(101), 100));
    assert!(!recent(Check::Succeeded, None, 100));
}
