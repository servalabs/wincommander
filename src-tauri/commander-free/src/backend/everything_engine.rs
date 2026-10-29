//! Bounded health checks keep an unavailable provider distinct from zero matches.
use std::process::Stdio;
use std::time::Duration;

const HEALTH_TIMEOUT: Duration = Duration::from_secs(1);
const UNAVAILABLE: &str = "Filename search engine is still loading or not responding. Wait for Everything to finish indexing, then retry the search. If this continues, start the Everything service and reopen Everything under this Windows account.";
const QUERY_TIMEOUT: &str =
    "Filename search timed out. The engine is responding; try a narrower query or fewer filters.";
static START_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Unavailable,
    Timeout,
    Other(String),
}

trait Executor {
    async fn run(&self, args: &[String], timeout: Duration) -> Result<Vec<u8>, Failure>;
    async fn recover(&self) -> bool;
    async fn pause(&self);
}

async fn probe(executor: &impl Executor) -> Result<(), Failure> {
    let bytes = executor
        .run(&["-get-everything-version".into()], HEALTH_TIMEOUT)
        .await?;
    // A blocked cross-integrity Windows message can print 0.0.0.0 with exit 0.
    let version = String::from_utf8_lossy(&bytes);
    if version
        .trim()
        .split('.')
        .next()
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0)
        == 0
    {
        return Err(Failure::Unavailable);
    }
    Ok(())
}

async fn ready(executor: &impl Executor) -> Result<(), String> {
    match probe(executor).await {
        Ok(()) => return Ok(()),
        Err(Failure::Other(message)) => return Err(message),
        Err(_) => {}
    }
    if executor.recover().await {
        // Poll readiness, not a blind delay followed by a successful empty query.
        for _ in 0..4 {
            executor.pause().await;
            match probe(executor).await {
                Ok(()) => return Ok(()),
                Err(Failure::Other(message)) => return Err(message),
                Err(_) => {}
            }
        }
    }
    Err(UNAVAILABLE.into())
}

async fn search_with(
    executor: &impl Executor,
    args: &[String],
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    // Every internal query includes ES's database-load timeout. Its successful
    // reply proves readiness; a separate one-second version gate can reject
    // a working engine under load before the real query gets its budget.
    match executor.run(args, timeout).await {
        Ok(bytes) => Ok(bytes),
        Err(Failure::Other(message)) => Err(message),
        // Never repeat an expensive query after its timeout. A cheap health
        // probe distinguishes a hung engine from a slow query/filter.
        Err(Failure::Timeout) => match probe(executor).await {
            Ok(()) => Err(QUERY_TIMEOUT.into()),
            Err(_) => Err(UNAVAILABLE.into()),
        },
        // Retry a disconnected query only after bounded same-session recovery.
        Err(Failure::Unavailable) => {
            ready(executor).await?;
            executor
                .run(args, timeout)
                .await
                .map_err(|error| match error {
                    Failure::Other(message) => message,
                    _ => UNAVAILABLE.into(),
                })
        }
    }
}

pub(super) async fn search(
    executable: &str,
    args: &[String],
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    search_with(&ProcessExecutor(executable), args, timeout).await
}

struct ProcessExecutor<'a>(&'a str);

impl Executor for ProcessExecutor<'_> {
    async fn run(&self, args: &[String], timeout: Duration) -> Result<Vec<u8>, Failure> {
        let mut command = tokio::process::Command::new(self.0);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Dropping a timed-out wait kills this ES client, never Everything.
        command.kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let child = command
            .spawn()
            .map_err(|e| Failure::Other(format!("Failed to run filename search: {e}")))?;
        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| Failure::Timeout)?
            .map_err(|e| Failure::Other(format!("Failed to run filename search: {e}")))?;
        if output.status.success() {
            return Ok(output.stdout);
        }
        let message = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        let message = String::from_utf8_lossy(message);
        if matches!(output.status.code(), Some(7 | 8))
            || message.to_lowercase().contains("ipc not found")
        {
            return Err(Failure::Unavailable);
        }
        Err(Failure::Other(format!(
            "Filename search failed (exit {}): {}",
            output.status,
            message.trim()
        )))
    }

    async fn recover(&self) -> bool {
        let Ok(_guard) = tokio::time::timeout(HEALTH_TIMEOUT, START_LOCK.lock()).await else {
            return false;
        };
        // Another search may have started the same-session client while we waited.
        if probe(self).await.is_ok() {
            return true;
        }
        // Existing token only: no elevation, service reconfiguration or restarts.
        // A denied start still allows an already-running service/client to recover.
        let service = tokio::task::spawn_blocking(start_existing_service);
        let _ = tokio::time::timeout(HEALTH_TIMEOUT, service).await;
        let startup = tokio::task::spawn_blocking(super::try_start_everything_daemon);
        tokio::time::timeout(HEALTH_TIMEOUT, startup)
            .await
            .is_ok_and(|r| r.unwrap_or(false))
    }

    async fn pause(&self) {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(windows)]
fn start_existing_service() {
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, StartServiceW, SC_MANAGER_CONNECT,
        SERVICE_START,
    };
    let name: Vec<u16> = "Everything\0".encode_utf16().collect();
    unsafe {
        let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if manager.is_null() {
            return;
        }
        let service = OpenServiceW(manager, name.as_ptr(), SERVICE_START);
        if !service.is_null() {
            StartServiceW(service, 0, std::ptr::null());
            CloseServiceHandle(service);
        }
        CloseServiceHandle(manager);
    }
}

#[cfg(not(windows))]
fn start_existing_service() {}

#[cfg(test)]
#[path = "everything_engine_tests.rs"]
mod tests;
