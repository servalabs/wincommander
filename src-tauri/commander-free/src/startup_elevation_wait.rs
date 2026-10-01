use std::process::Child;
use std::time::{Duration, Instant};

pub(super) const NOT_STARTED_EXIT_CODE: i32 = 77;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SchedulerHandoff {
    Accepted,
    NotStarted,
    AcceptanceUnknown,
}

/// A timed-out client may already have asked Scheduler to launch the GUI.
pub(super) fn wait_for_handoff(child: &mut Child, budget: Duration) -> SchedulerHandoff {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(0) => SchedulerHandoff::Accepted,
                    Some(NOT_STARTED_EXIT_CODE) => SchedulerHandoff::NotStarted,
                    _ => SchedulerHandoff::AcceptanceUnknown,
                };
            }
            Ok(None) if started.elapsed() < budget => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                // Stop only our PowerShell client. Do not cancel Scheduler's
                // accepted task or wait indefinitely for process termination.
                let _ = child.kill();
                return SchedulerHandoff::AcceptanceUnknown;
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    fn hidden_powershell(script: &str) -> Child {
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                script,
            ])
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    #[test]
    fn accepted_rejected_and_uncertain_requests_have_distinct_outcomes() {
        for (script, expected) in [
            ("exit 0", SchedulerHandoff::Accepted),
            ("exit 77", SchedulerHandoff::NotStarted),
            ("exit 1", SchedulerHandoff::AcceptanceUnknown),
        ] {
            let mut child = hidden_powershell(script);
            assert_eq!(
                wait_for_handoff(&mut child, Duration::from_secs(5)),
                expected
            );
        }
    }

    #[test]
    fn stalled_client_is_bounded_and_never_classified_as_safe_to_replay() {
        let mut child = hidden_powershell("Start-Sleep -Seconds 30");
        let started = Instant::now();
        assert_eq!(
            wait_for_handoff(&mut child, Duration::from_millis(40)),
            SchedulerHandoff::AcceptanceUnknown
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        let deadline = Instant::now() + Duration::from_secs(2);
        while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            child.try_wait().unwrap().is_some(),
            "owned client must be terminated"
        );
    }
}
