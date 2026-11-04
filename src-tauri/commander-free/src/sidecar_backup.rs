// SPDX-License-Identifier: AGPL-3.0-or-later
//! A bounded, temporary lane for explicit backup work; never consumes protection admission.
use std::sync::OnceLock;
use tokio::sync::{Semaphore, SemaphorePermit};

pub(super) fn matches(feature: &str) -> bool {
    matches!(
        feature,
        "backup_container_export" | "backup_container_verify" | "backup_generation_restore"
    )
}

pub(super) fn admit() -> Result<SemaphorePermit<'static>, String> {
    static BACKUP: OnceLock<Semaphore> = OnceLock::new();
    BACKUP
        .get_or_init(|| Semaphore::new(1))
        .try_acquire()
        .map_err(|_| "backup_already_running".into())
}

pub(super) async fn run<T>(
    work: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let _permit = admit()?;
    work.await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn blocked_backup_launch_does_not_consume_protection_admission_and_cancels_cleanly() {
        let (started, ready) = tokio::sync::oneshot::channel();
        let backup = tokio::spawn(run(async move {
            started.send(()).unwrap();
            std::future::pending::<Result<(), String>>().await
        }));
        ready.await.unwrap();
        assert!(
            admit().is_err(),
            "a second backup must not queue or spawn another child"
        );
        let protection = super::super::pool_semaphore().try_acquire().unwrap();
        let launch = super::super::pro_spawn_gate().try_lock().unwrap();
        drop(launch);
        drop(protection);
        backup.abort();
        assert!(backup.await.unwrap_err().is_cancelled());
        assert!(admit().is_ok());
    }
    #[test]
    fn only_fixed_backup_features_get_an_independent_worker() {
        for name in [
            "backup_container_export",
            "backup_container_verify",
            "backup_generation_restore",
        ] {
            assert!(matches(name));
        }
        for name in [
            "backup_anything",
            "backup_container_export/other",
            "fleet_agent_status",
            "run_destruct_step",
            "Attach-StegoContainer",
            "",
        ] {
            assert!(!matches(name));
        }
    }
}
