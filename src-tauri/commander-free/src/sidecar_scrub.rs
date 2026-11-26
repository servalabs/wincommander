// SPDX-License-Identifier: AGPL-3.0-or-later
//! A dedicated lane for metadata scrubs: a big folder can run for minutes, so it
//! must neither hold the one general Pro worker nor time out waiting behind it.
use std::{sync::OnceLock, time::Duration};
use tokio::sync::{Semaphore, SemaphorePermit};

pub(super) const BUSY_MESSAGE: &str =
    "WinCommander Pro is still finishing an earlier metadata scrub. Try again when it completes.";

pub(super) fn matches(feature: &str) -> bool {
    feature == "scrub_metadata_paths"
}

fn lane() -> &'static Semaphore {
    static SCRUB: OnceLock<Semaphore> = OnceLock::new();
    SCRUB.get_or_init(|| Semaphore::new(1))
}

pub(super) async fn admit(budget: Duration) -> Result<SemaphorePermit<'static>, String> {
    admit_on(lane(), budget).await
}

// Two scrubs may touch the same files, so a later one queues rather than runs alongside.
async fn admit_on(lane: &Semaphore, budget: Duration) -> Result<SemaphorePermit<'_>, String> {
    tokio::time::timeout(budget, lane.acquire())
        .await
        .map_err(|_| BUSY_MESSAGE.to_string())?
        .map_err(|_| "Pro session is closing".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queued_scrub_waits_for_an_earlier_scrub_instead_of_failing_busy() {
        let lane = Semaphore::new(1);
        let earlier = lane.acquire().await.unwrap();
        let finish = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            drop(earlier);
        };
        let (_, queued) = tokio::join!(finish, admit_on(&lane, Duration::from_secs(5)));
        assert!(queued.is_ok());
    }

    #[tokio::test]
    async fn scrub_blocked_past_its_budget_gets_a_plain_message_and_frees_the_lane() {
        let lane = Semaphore::new(1);
        let earlier = lane.acquire().await.unwrap();
        let error = admit_on(&lane, Duration::from_millis(20)).await.unwrap_err();
        assert_eq!(error, BUSY_MESSAGE);
        drop(earlier);
        assert!(lane.try_acquire().is_ok());
    }

    #[test]
    fn only_the_metadata_scrub_uses_the_scrub_lane() {
        assert!(matches("scrub_metadata_paths"));
        for name in [
            "get_metadata_scrubber_status",
            "scrub_metadata_paths/other",
            "backup_container_export",
            "",
        ] {
            assert!(!matches(name));
        }
    }
}
