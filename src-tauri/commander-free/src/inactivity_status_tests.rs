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
}
