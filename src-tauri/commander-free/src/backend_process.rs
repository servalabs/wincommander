// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io::Write;
use std::process::{Child, Output};
use std::time::Duration;

pub(super) fn ui_script_timeout(command: &str) -> Option<Duration> {
    match command {
        // These are read-only status checks polled by the dashboard/Shield.
        // A stalled check must release the shared request tracker promptly.
        "Get-AppCapabilityAccessStatus" | "Get-PrivacyShieldStatus" => {
            Some(Duration::from_secs(15))
        }
        _ => None,
    }
}

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
    let command_is_shield_start = command == "Start-PrivacyShield";
    let mut owner = tokio::task::spawn_blocking(move || {
        if command_is_shield_start {
            return wait_for_shield_start(child, script);
        }
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

// A detector can inherit the runner's pipes despite redirected standard
// streams. Drain available bytes while the runner lives and stop at its exit,
// rather than waiting for EOF from the long-lived detector.
#[cfg(windows)]
fn wait_for_shield_start(mut child: Child, script: String) -> std::io::Result<Output> {
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("PowerShell stdout unavailable"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("PowerShell stderr unavailable"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("PowerShell stdin unavailable"))?;
    stdin.write_all(script.as_bytes())?;
    drop(stdin);
    let mut output = Vec::new();
    let mut errors = Vec::new();
    loop {
        drain_available(&mut stdout, &mut output)?;
        drain_available(&mut stderr, &mut errors)?;
        if let Some(status) = child.try_wait()? {
            drain_available(&mut stdout, &mut output)?;
            drain_available(&mut stderr, &mut errors)?;
            return Ok(Output {
                status,
                stdout: output,
                stderr: errors,
            });
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

#[cfg(windows)]
fn drain_available(
    stream: &mut (impl std::io::Read + std::os::windows::io::AsRawHandle),
    output: &mut Vec<u8>,
) -> std::io::Result<()> {
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;
    loop {
        let mut available = 0u32;
        let ok = unsafe {
            PeekNamedPipe(
                stream.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(109) {
                return Ok(());
            }
            return Err(error);
        }
        if available == 0 {
            return Ok(());
        }
        let mut bytes = vec![0; available.min(8192) as usize];
        let count = stream.read(&mut bytes)?;
        if count == 0 {
            return Ok(());
        }
        output.extend_from_slice(&bytes[..count]);
    }
}

#[cfg(not(windows))]
fn wait_for_shield_start(mut child: Child, script: String) -> std::io::Result<Output> {
    child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("PowerShell stdin unavailable"))?
        .write_all(script.as_bytes())?;
    child.wait_with_output()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    #[test]
    fn camera_status_has_a_bounded_deadline_without_shortening_other_commands() {
        assert_eq!(
            ui_script_timeout("Get-AppCapabilityAccessStatus"),
            Some(Duration::from_secs(15))
        );
        assert_eq!(
            ui_script_timeout("Get-PrivacyShieldStatus"),
            Some(Duration::from_secs(15))
        );
        assert_eq!(ui_script_timeout("Start-PrivacyShield"), None);
    }

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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shield_start_receipt_does_not_wait_for_detector_pipe_eof() {
        use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
            PROCESS_TERMINATE,
        };

        let child = fixture("$script = [Console]::In.ReadToEnd(); Invoke-Expression $script");
        // Keep a second read handle so the test can prove that a writer is
        // still holding the pipe open after the launcher receipt returns.
        let pipe = child
            .stdout
            .as_ref()
            .unwrap()
            .as_handle()
            .try_clone_to_owned()
            .unwrap();
        let script = r#"
$info = New-Object System.Diagnostics.ProcessStartInfo
$info.FileName = 'powershell.exe'
$info.Arguments = '-NoProfile -NonInteractive -Command "Start-Sleep -Seconds 90"'
$info.UseShellExecute = $false
$info.CreateNoWindow = $true
$detector = [System.Diagnostics.Process]::Start($info)
[Console]::Out.Write('{"success":true,"pid":' + $detector.Id + '}')
"#;
        let output = wait_for_script(
            child,
            script.into(),
            "Start-PrivacyShield",
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["success"], true);
        let pid = receipt["pid"].as_u64().unwrap() as u32;
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid) };
        assert!(!raw.is_null(), "detector process must still exist");
        let process = unsafe { OwnedHandle::from_raw_handle(raw) };
        let alive = unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } == WAIT_TIMEOUT;
        let pipe_open = unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } != 0;
        // Stop the fixture even when an assertion below fails.
        let terminated = unsafe { TerminateProcess(process.as_raw_handle(), 0) } != 0;
        let stopped =
            unsafe { WaitForSingleObject(process.as_raw_handle(), 5_000) } == WAIT_OBJECT_0;
        assert!(
            alive,
            "receipt must return while the detector is still running"
        );
        assert!(pipe_open, "detector must still hold the output pipe open");
        assert!(terminated && stopped, "detector fixture must be cleaned up");
        assert!(output.status.success());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "run in isolation; the CI workspace suite saturates process launch"]
    async fn failed_shield_runner_returns_without_waiting_for_descendant() {
        use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        };
        // The job owns the complete fixture tree even if parsing or an assertion
        // fails. Assign before sending stdin, so no descendant can escape it.
        let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        assert!(!raw_job.is_null(), "could not create fixture job");
        let job = unsafe { OwnedHandle::from_raw_handle(raw_job) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        assert_ne!(
            unsafe {
                SetInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            },
            0,
            "could not set fixture job cleanup"
        );
        let mut child = fixture("$script = [Console]::In.ReadToEnd(); Invoke-Expression $script");
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0 {
            let _ = child.kill();
            let _ = child.wait();
            panic!("could not assign fixture to cleanup job");
        }
        let stdout_pipe = child
            .stdout
            .as_ref()
            .unwrap()
            .as_handle()
            .try_clone_to_owned()
            .unwrap();
        let stderr_pipe = child
            .stderr
            .as_ref()
            .unwrap()
            .as_handle()
            .try_clone_to_owned()
            .unwrap();
        let script = r#"
$info = New-Object System.Diagnostics.ProcessStartInfo
$info.FileName = 'powershell.exe'
$info.Arguments = '-NoProfile -NonInteractive -Command "Start-Sleep -Seconds 120"'
$info.UseShellExecute = $false
$info.CreateNoWindow = $true
$info.RedirectStandardInput = $true
$info.RedirectStandardOutput = $true
$info.RedirectStandardError = $true
$detector = [System.Diagnostics.Process]::Start($info)
$detector.add_OutputDataReceived({ param($sender, $eventArgs) })
$detector.add_ErrorDataReceived({ param($sender, $eventArgs) })
$detector.BeginOutputReadLine()
$detector.BeginErrorReadLine()
$detector.StandardInput.Close()
[Console]::Error.Write('fixture failure:' + $detector.Id)
exit 7
"#;
        let output = wait_for_script(
            child,
            script.into(),
            "Start-PrivacyShield",
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        let error = std::str::from_utf8(&output.stderr).unwrap();
        let pid: u32 = error
            .strip_prefix("fixture failure:")
            .unwrap()
            .parse()
            .unwrap();
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
                0,
                pid,
            )
        };
        assert!(!raw.is_null(), "detector process must still exist");
        let process = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut in_fixture_job = 0;
        assert_ne!(
            unsafe {
                IsProcessInJob(
                    process.as_raw_handle(),
                    job.as_raw_handle(),
                    &mut in_fixture_job,
                )
            },
            0,
            "could not verify detector ownership"
        );
        assert_ne!(in_fixture_job, 0, "detector must belong to the fixture job");
        let alive = unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } == WAIT_TIMEOUT;
        let is_open = |pipe: &OwnedHandle| unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
        };
        let pipe_open = is_open(&stdout_pipe) || is_open(&stderr_pipe);
        // Clean up even if an assertion below fails.
        let terminated = unsafe { TerminateProcess(process.as_raw_handle(), 0) } != 0;
        let stopped =
            unsafe { WaitForSingleObject(process.as_raw_handle(), 5_000) } == WAIT_OBJECT_0;
        assert!(
            alive,
            "failure must return while the detector is still running"
        );
        assert!(terminated && stopped, "detector fixture must be cleaned up");
        assert!(pipe_open, "detector must keep a launcher output pipe open");
        assert_eq!(output.status.code(), Some(7));
        assert!(output.stdout.is_empty());
    }
}
