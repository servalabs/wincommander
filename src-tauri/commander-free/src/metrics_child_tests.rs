// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[tokio::test]
async fn output_limit_rejects_excess_before_allocating_unbounded_memory() {
    let result = read_bounded(&b"123456"[..], 5).await;
    assert_eq!(result.unwrap_err(), "Probe output exceeded limit");
    assert_eq!(read_bounded(&b"12345"[..], 5).await.unwrap(), b"12345");
}

#[cfg(windows)]
fn fixture(script: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new("powershell.exe");
    command.creation_flags(0x08000000);
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    command
}

#[cfg(windows)]
fn is_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // Query only the test-owned child; the handle is closed on every successful open.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut code = 0;
        let queried = GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
        queried != 0 && code == 259
    }
}

#[cfg(windows)]
#[tokio::test]
async fn deadline_terminates_and_reaps_the_owned_hung_child() {
    let mut command = fixture("Start-Sleep -Seconds 60");
    let mut pid = 0;
    let result = run_observed(&mut command, Duration::from_millis(100), 1024, |child| {
        pid = child
    })
    .await;
    assert_eq!(result.unwrap_err(), "Hardware probe timed out");
    assert_ne!(pid, 0);
    assert!(!is_running(pid));
}

#[cfg(windows)]
#[tokio::test]
async fn excessive_output_terminates_the_owned_child() {
    let mut command = fixture("[Console]::Out.Write(('x' * 8192)); Start-Sleep -Seconds 60");
    let mut pid = 0;
    let result = run_observed(&mut command, Duration::from_secs(10), 1024, |child| {
        pid = child
    })
    .await;
    assert_eq!(result.unwrap_err(), "Probe output exceeded limit");
    assert!(!is_running(pid));
}

#[cfg(windows)]
#[tokio::test]
async fn multiple_slow_steps_share_one_deadline_and_cannot_launch_after_it() {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(5);
    for _ in 0..2 {
        let output = run_until_observed(
            &mut fixture("Start-Sleep -Milliseconds 100; [Console]::Out.Write('ok')"),
            deadline,
            1024,
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(output.stdout, b"ok");
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    assert!(remaining < Duration::from_millis(4800));
    let mut last_pid = 0;
    let result = run_until_observed(
        &mut fixture("Start-Sleep -Seconds 60"),
        deadline,
        1024,
        |pid| last_pid = pid,
    )
    .await;
    assert_eq!(result.unwrap_err(), "Hardware probe timed out");
    assert_ne!(last_pid, 0);
    assert!(!is_running(last_pid));
    assert!(started.elapsed() < Duration::from_secs(8));
    let mut launched = false;
    let result = run_until_observed(
        &mut fixture("Start-Sleep -Seconds 60"),
        deadline,
        1024,
        |_| launched = true,
    )
    .await;
    assert_eq!(result.unwrap_err(), "Hardware probe timed out");
    assert!(!launched);
}
