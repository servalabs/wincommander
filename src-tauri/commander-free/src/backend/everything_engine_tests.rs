use super::*;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

struct Fake {
    replies: RefCell<VecDeque<Result<Vec<u8>, Failure>>>,
    calls: RefCell<Vec<Vec<String>>>,
    starts: Cell<usize>,
    can_start: bool,
}

impl Fake {
    fn new(replies: Vec<Result<Vec<u8>, Failure>>, can_start: bool) -> Self {
        Self {
            replies: RefCell::new(replies.into()),
            calls: RefCell::new(Vec::new()),
            starts: Cell::new(0),
            can_start,
        }
    }
}

impl Executor for Fake {
    async fn run(&self, args: &[String], _timeout: Duration) -> Result<Vec<u8>, Failure> {
        self.calls.borrow_mut().push(args.to_vec());
        self.replies
            .borrow_mut()
            .pop_front()
            .expect("unexpected engine call")
    }
    async fn recover(&self) -> bool {
        self.starts.set(self.starts.get() + 1);
        self.can_start
    }
    async fn pause(&self) {}
}

fn healthy() -> Result<Vec<u8>, Failure> {
    Ok(b"1\r\n".to_vec())
}
fn query() -> Vec<String> {
    vec!["-timeout".into(), "4000".into(), "invoice".into()]
}

#[tokio::test]
async fn healthy_zero_results_are_successful() {
    let engine = Fake::new(vec![Ok(Vec::new())], false);
    assert_eq!(
        search_with(&engine, &query(), Duration::from_secs(6)).await,
        Ok(Vec::new())
    );
    assert_eq!(engine.starts.get(), 0);
    assert_eq!(*engine.calls.borrow(), vec![query()]);
}

#[tokio::test]
async fn healthy_query_runs_once_without_a_version_probe() {
    let engine = Fake::new(vec![Ok(b"row".to_vec())], false);
    assert_eq!(
        search_with(&engine, &query(), Duration::from_secs(6)).await,
        Ok(b"row".to_vec())
    );
    assert_eq!(*engine.calls.borrow(), vec![query()]);
    assert_eq!(engine.starts.get(), 0);
}

#[tokio::test]
async fn healthy_query_is_not_gated_by_a_slow_version_probe_under_pressure() {
    struct Pressure;
    impl Executor for Pressure {
        async fn run(&self, args: &[String], _: Duration) -> Result<Vec<u8>, Failure> {
            if args
                .iter()
                .any(|arg| arg == "-get-result-count")
            {
                Err(Failure::Timeout)
            } else {
                Ok(b"row".to_vec())
            }
        }
        async fn recover(&self) -> bool {
            false
        }
        async fn pause(&self) {}
    }
    assert_eq!(
        search_with(&Pressure, &query(), Duration::from_secs(6)).await,
        Ok(b"row".to_vec())
    );
}

#[tokio::test]
async fn missing_ipc_starts_and_waits_for_readiness_before_query() {
    let engine = Fake::new(
        vec![
            Err(Failure::Unavailable),
            Err(Failure::Unavailable),
            Err(Failure::Unavailable),
            healthy(),
            Ok(b"row".to_vec()),
        ],
        true,
    );
    assert_eq!(
        search_with(&engine, &query(), Duration::from_secs(6)).await,
        Ok(b"row".to_vec())
    );
    assert_eq!(engine.starts.get(), 1);
    let calls = engine.calls.borrow();
    assert_eq!(calls[0], query());
    assert!(calls[1..4]
        .iter()
        .all(|args| args == &DATABASE_PROBE));
    assert_eq!(calls[4], query());
}

#[tokio::test]
async fn stopped_service_and_denied_start_never_become_empty_results() {
    let engine = Fake::new(
        vec![Err(Failure::Unavailable), Err(Failure::Unavailable)],
        false,
    );
    assert_eq!(
        search_with(&engine, &query(), Duration::from_secs(6)).await,
        Err(UNAVAILABLE.into())
    );
    assert_eq!(engine.starts.get(), 1);
    assert_eq!(engine.calls.borrow().len(), 2);
}

#[tokio::test]
async fn persistently_unresponsive_engine_has_bounded_readiness_attempts() {
    let engine = Fake::new(
        std::iter::once(Err(Failure::Unavailable))
            .chain((0..=READINESS_ATTEMPTS).map(|_| Err(Failure::Timeout)))
            .collect(),
        true,
    );
    assert_eq!(
        search_with(&engine, &query(), Duration::from_secs(6)).await,
        Err(UNAVAILABLE.into())
    );
    assert_eq!(engine.starts.get(), 1);
    assert_eq!(engine.calls.borrow().len(), READINESS_ATTEMPTS + 2);
    assert!(engine.replies.borrow().is_empty());
}

#[tokio::test]
async fn version_reply_does_not_prove_database_readiness() {
    let engine = Fake::new(
        vec![Err(Failure::Unavailable), Ok(b"1.4.1.1032\r\n".to_vec())],
        false,
    );
    assert_eq!(
        search_with(&engine, &query(), Duration::from_secs(6)).await,
        Err(UNAVAILABLE.into())
    );
    assert_eq!(engine.calls.borrow().len(), 2);
}

#[tokio::test]
async fn expensive_query_timeout_is_not_retried_or_misreported_as_engine_failure() {
    let engine = Fake::new(vec![Err(Failure::Timeout), healthy()], true);
    assert_eq!(
        search_with(&engine, &query(), Duration::from_secs(6)).await,
        Err(QUERY_TIMEOUT.into())
    );
    assert_eq!(engine.starts.get(), 0);
    assert_eq!(
        engine
            .calls
            .borrow()
            .iter()
            .filter(|args| **args == query())
            .count(),
        1
    );
}

#[tokio::test]
async fn stalled_engine_after_query_reports_engine_failure() {
    let engine = Fake::new(vec![Err(Failure::Timeout), Err(Failure::Timeout)], false);
    assert_eq!(
        search_with(&engine, &query(), Duration::from_secs(6)).await,
        Err(UNAVAILABLE.into())
    );
    assert_eq!(engine.starts.get(), 0);
}

#[tokio::test]
async fn disconnected_query_is_recovered_once() {
    let engine = Fake::new(
        vec![
            Err(Failure::Unavailable),
            Err(Failure::Unavailable),
            healthy(),
            Ok(b"row".to_vec()),
        ],
        true,
    );
    assert_eq!(
        search_with(&engine, &query(), Duration::from_secs(6)).await,
        Ok(b"row".to_vec())
    );
    assert_eq!(engine.starts.get(), 1);
}

#[tokio::test]
async fn invalid_cli_is_not_retried_as_missing_engine() {
    let engine = Fake::new(vec![Err(Failure::Other("invalid switch".into()))], true);
    assert_eq!(
        search_with(&engine, &query(), Duration::from_secs(6)).await,
        Err("invalid switch".into())
    );
    assert_eq!(engine.starts.get(), 0);
}

#[cfg(windows)]
#[tokio::test]
async fn pending_launch_keeps_database_readiness_polling() {
    let (finish, pending) = tokio::sync::oneshot::channel();
    let launch = tokio::spawn(async { pending.await.unwrap_or(false) });
    assert!(launch_can_be_pending(launch, Duration::from_millis(5)).await);
    assert!(finish.send(true).is_ok());

    let rejected = tokio::spawn(async { false });
    assert!(!launch_can_be_pending(rejected, Duration::from_secs(1)).await);
}

#[cfg(windows)]
#[tokio::test]
async fn actual_cli_exit_codes_identify_ipc_failure_without_english_error_text() {
    let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let executor = ProcessExecutor(shell.to_str().unwrap());
    for code in [7, 8] {
        assert_eq!(
            executor
                .run(
                    &[
                        "-NoProfile".into(),
                        "-Command".into(),
                        format!("exit {code}")
                    ],
                    Duration::from_secs(20)
                )
                .await,
            Err(Failure::Unavailable)
        );
    }
}

#[cfg(windows)]
#[tokio::test]
async fn actual_hung_cli_is_bounded() {
    let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let executor = ProcessExecutor(shell.to_str().unwrap());
    let started = std::time::Instant::now();
    assert_eq!(
        executor
            .run(
                &[
                    "-NoProfile".into(),
                    "-Command".into(),
                    "Start-Sleep -Seconds 10".into()
                ],
                Duration::from_millis(50)
            )
            .await,
        Err(Failure::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}
