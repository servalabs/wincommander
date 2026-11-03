// SPDX-License-Identifier: AGPL-3.0-or-later
use serde_json::{json, Value};
use wincmd_shared::personal_settings::{PersonalSettingsRecord, READ_PERSONAL_SETTINGS_VERB};

const MIGRATION_MARKER: &str = "personal-settings-service.migrated";

pub(super) fn service_call(verb: &'static str, args: Value) -> Result<Value, String> {
    service_call_with_deadline(verb, args, None)
}

fn service_call_with_deadline(
    verb: &'static str,
    args: Value,
    deadline: Option<std::time::Duration>,
) -> Result<Value, String> {
    // Settings reads also run inside async jobs; don't nest a runtime's block_on.
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Personal settings transport could not start".to_string())?;
        runtime.block_on(async move {
            let call = crate::svc_client::call(verb, args);
            match deadline {
                Some(deadline) => bound_read(call, deadline).await,
                None => call.await,
            }
        })
    })
    .join()
    .map_err(|_| "Personal settings transport failed".to_string())?
}

async fn bound_read<T>(
    call: impl std::future::Future<Output = Result<T, String>>,
    deadline: std::time::Duration,
) -> Result<T, String> {
    tokio::time::timeout(deadline, call)
        .await
        .map_err(|_| "Personal settings read deadline exceeded".to_string())?
}

pub(super) fn read_service() -> Result<PersonalSettingsRecord, String> {
    read_service_with(
        || {
            // Bound the whole attempt, including handshake retries, so three
            // attempts cannot individually consume the 60-second preload limit.
            serde_json::from_value(service_call_with_deadline(
                READ_PERSONAL_SETTINGS_VERB,
                json!({}),
                Some(std::time::Duration::from_millis(5_500)),
            )?)
            .map_err(|_| "Invalid personal settings service response".to_string())
        },
        std::thread::sleep,
    )
}

// A service restart or another session's bounded settings transaction can
// interrupt a read. Reissue only this read, with a fresh authenticated pipe;
// writes have revision semantics and must never be replayed here. A failure
// after the retry is returned unchanged, including identity and integrity
// failures, so it cannot turn into a default settings snapshot.
fn read_service_with(
    mut read: impl FnMut() -> Result<PersonalSettingsRecord, String>,
    mut sleep: impl FnMut(std::time::Duration),
) -> Result<PersonalSettingsRecord, String> {
    const DELAYS: [std::time::Duration; 2] = [
        std::time::Duration::from_millis(150),
        std::time::Duration::from_millis(500),
    ];
    for delay in DELAYS {
        match read() {
            Err(error) if retryable_read_failure(&error) => sleep(delay),
            result => return result,
        }
    }
    read()
}

fn retryable_read_failure(error: &str) -> bool {
    error.starts_with("service rejected request: personal_settings_busy (")
        || error.starts_with("service connect failed:")
        || error.starts_with("service reply read failed:")
        || error.starts_with("service request write failed:")
        || error.starts_with("service Hello write failed:")
        || error.starts_with("service Hello acknowledgement failed:")
        || error == "service reply timed out"
        || error == "service request write timed out"
        || error == "service Hello write timed out"
        || error == "service Hello acknowledgement timed out"
        || error == "Personal settings read deadline exceeded"
        || error == "Service identity could not be verified"
}

pub(super) fn has_migration_marker() -> Result<bool, String> {
    match std::fs::symlink_metadata(crate::paths::user_data_dir()?.join(MIGRATION_MARKER)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("Could not inspect personal settings migration state".to_string()),
    }
}

pub(super) fn mark_migrated() -> Result<(), String> {
    ensure_migration_marker(&crate::paths::user_data_dir()?.join(MIGRATION_MARKER))
}

fn ensure_migration_marker(path: &std::path::Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Could not inspect personal settings migration state".to_string()),
    }
    // Concurrent instances can create this marker at the same time. Publish
    // once and keep the winner, rather than replacing it on every startup.
    crate::datastore_io::publish_new(path, b"service-v1\n").map(|_| ())
}

pub(super) fn service_unavailable(error: &str) -> bool {
    // Authentication, integrity and denied records must never look like an absent service.
    error.starts_with("service connect failed:")
        || error == "service connection failed"
        || error == "Vault access service is available only on Windows"
        || error == "service reply timed out"
        || error == "service Hello acknowledgement timed out"
        || error.starts_with("service rejected request: unknown_verb ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stalled_read_is_bounded_and_remains_fail_closed() {
        let error = bound_read(
            std::future::pending::<Result<Value, String>>(),
            std::time::Duration::from_millis(20),
        ).await.unwrap_err();
        assert_eq!(error, "Personal settings read deadline exceeded");
        assert!(!service_unavailable(&error));
    }

    #[test]
    fn transient_read_failures_retry_and_return_the_authenticated_record() {
        let mut calls = 0;
        let mut delays = Vec::new();
        let result = read_service_with(
            || {
                calls += 1;
                match calls {
                    1 => Err("service rejected request: personal_settings_busy (personal_settings_busy) [operation 1]".into()),
                    2 => Err("Service identity could not be verified".into()),
                    _ => Ok(PersonalSettingsRecord::default()),
                }
            },
            |delay| delays.push(delay),
        );
        assert!(result.is_ok());
        assert_eq!(calls, 3);
        assert_eq!(delays.len(), 2);
    }

    #[test]
    fn integrity_failures_are_never_retried_or_treated_as_unavailable() {
        for error in [
            "service rejected request: personal_settings_corrupt (personal_settings_corrupt)",
            "service rejected request: forbidden (denied)",
            "service reply signature check failed: invalid",
            "Invalid personal settings service response",
        ] {
            let mut calls = 0;
            let result = read_service_with(
                || {
                    calls += 1;
                    Err(error.into())
                },
                |_| panic!("integrity failure must not retry"),
            );
            assert_eq!(result.unwrap_err(), error);
            assert_eq!(calls, 1);
            assert!(!service_unavailable(error));
        }
        let error = "Service identity could not be verified";
        let result = read_service_with(|| Err(error.into()), |_| {});
        assert_eq!(result.unwrap_err(), error);
        assert!(!service_unavailable(error));
    }

    #[test]
    fn concurrent_marker_creation_preserves_the_single_completed_marker() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(MIGRATION_MARKER);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    ensure_migration_marker(&path)
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"service-v1\n");
        std::fs::write(&path, b"existing marker").unwrap();
        ensure_migration_marker(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"existing marker");
    }
}
