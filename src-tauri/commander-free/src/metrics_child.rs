// SPDX-License-Identifier: AGPL-3.0-or-later
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};

const OUTPUT_LIMIT: u64 = 1024 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

async fn read_bounded(reader: impl AsyncRead + Unpin, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "Probe output read failed")?;
    if bytes.len() as u64 > limit {
        return Err("Probe output exceeded limit".into());
    }
    Ok(bytes)
}

pub(super) fn output(command: &mut Command) -> Result<Output, String> {
    tauri::async_runtime::block_on(run(command, PROBE_TIMEOUT, OUTPUT_LIMIT))
}

pub(super) fn output_until(command: &mut Command, deadline: Instant) -> Result<Output, String> {
    tauri::async_runtime::block_on(run_until_observed(
        command,
        deadline.min(Instant::now() + PROBE_TIMEOUT),
        OUTPUT_LIMIT,
        |_| {},
    ))
}

async fn run(command: &mut Command, budget: Duration, limit: u64) -> Result<Output, String> {
    run_observed(command, budget, limit, |_| {}).await
}

async fn run_observed(
    command: &mut Command,
    budget: Duration,
    limit: u64,
    spawned: impl FnOnce(u32),
) -> Result<Output, String> {
    run_until_observed(command, Instant::now() + budget, limit, spawned).await
}

async fn run_until_observed(
    command: &mut Command,
    deadline: Instant,
    limit: u64,
    spawned: impl FnOnce(u32),
) -> Result<Output, String> {
    if Instant::now() >= deadline {
        return Err("Hardware probe timed out".into());
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut command =
        tokio::process::Command::from(std::mem::replace(command, Command::new("unused")));
    command.kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| "Hardware probe could not start")?;
    if let Some(pid) = child.id() {
        spawned(pid);
    }
    let stdout = child.stdout.take().ok_or("Probe stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("Probe stderr unavailable")?;
    let result = tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), async {
        let (stdout, stderr, status) = tokio::try_join!(
            read_bounded(stdout, limit),
            read_bounded(stderr, limit),
            async {
                child
                    .wait()
                    .await
                    .map_err(|_| "Probe wait failed".to_string())
            },
        )?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    })
    .await;
    match result {
        Ok(Ok(output)) => Ok(output),
        failure => {
            // Kill and reap our own child; dropping pipes also cancels inherited-pipe waits.
            let _ = tokio::time::timeout(Duration::from_secs(2), child.kill()).await;
            match failure {
                Ok(Err(error)) => Err(error),
                _ => Err("Hardware probe timed out".into()),
            }
        }
    }
}

#[cfg(test)]
#[path = "metrics_child_tests.rs"]
mod tests;
