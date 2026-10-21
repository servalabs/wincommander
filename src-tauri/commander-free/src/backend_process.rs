// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io::Write;
use std::process::{Child, Output};
use std::time::Duration;

pub(super) async fn wait_for_script(
    mut child: Child,
    script: String,
    command: &str,
    deadline: Duration,
) -> Result<Output, String> {
    // Keep this process identity alive until cleanup ends; Windows must not recycle its PID.
    #[cfg(windows)]
    let _identity_guard = {
        use std::os::windows::io::AsHandle;
        child.as_handle().try_clone_to_owned().ok()
    };
    #[cfg(windows)]
    let can_target_pid = _identity_guard.is_some();
    #[cfg(not(windows))]
    let can_target_pid = false;
    let child_pid = child.id();
    let mut owner = tokio::task::spawn_blocking(move || {
        // Script writes can block, so they share the execution deadline.
        let write_result = match child.stdin.take() {
            Some(mut stdin) => stdin.write_all(script.as_bytes()),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "PowerShell stdin unavailable",
            )),
        };
        let output = child.wait_with_output();
        write_result?;
        output
    });
    match tokio::time::timeout(deadline, &mut owner).await {
        Ok(joined) => joined
            .map_err(|error| format!("Process owner failed: {error}"))?
            .map_err(|error| format!("PowerShell input/output failed: {error}")),
        Err(_) => {
            // Retain the owner task: dispatching a kill isn't proof of cleanup.
            let accepted = if can_target_pid {
                let mut kill = tokio::process::Command::new("taskkill");
                kill.args(["/F", "/T", "/PID", &child_pid.to_string()]);
                kill.kill_on_drop(true);
                #[cfg(windows)]
                kill.creation_flags(0x08000000);
                let termination =
                    tokio::time::timeout(Duration::from_secs(10), kill.output()).await;
                matches!(termination, Ok(Ok(ref result)) if result.status.success())
            } else {
                false
            };
            let reaped = matches!(
                tokio::time::timeout(Duration::from_secs(5), &mut owner).await,
                Ok(Ok(Ok(_)))
            );
            Err(format!(
                "Command '{}' timed out after {} seconds. {} Operation outcome is unknown; verify its state before retrying.",
                command, deadline.as_secs(),
                if accepted && reaped {
                    "Windows accepted tree termination and the launcher exited; detached work was not verified."
                } else {
                    "Process-tree termination could not be confirmed."
                }
            ))
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    fn fixture(command: &str) -> Child {
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocked_stdin_is_inside_the_operation_deadline() {
        let child = fixture("Start-Sleep -Seconds 30");
        let started = std::time::Instant::now();
        let error = wait_for_script(
            child,
            "x".repeat(4 * 1024 * 1024),
            "fixture",
            Duration::from_millis(200),
        )
        .await
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(20));
        assert!(error.contains("timed out"));
        assert!(error.contains("outcome is unknown"));
        assert!(!error.contains("was terminated"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn preserves_stdout_stderr_and_exit_status() {
        let child = fixture("$s = [Console]::In.ReadToEnd(); [Console]::Out.Write($s); [Console]::Error.Write('diagnostic'); exit 7");
        let output = wait_for_script(
            child,
            "fixture text".into(),
            "fixture",
            Duration::from_secs(15),
        )
        .await
        .unwrap();
        assert_eq!(output.stdout, b"fixture text");
        assert_eq!(output.stderr, b"diagnostic");
        assert_eq!(output.status.code(), Some(7));
    }
}
