// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/monitor_util.rs
//
// ═══════════════════════════════════════════════════════════════════════
// MONITOR UTIL — shared start/stop-epoch skeleton for background monitors
// ═══════════════════════════════════════════════════════════════════════
//
// Factors out the piece of the "spawn a background watcher, gate it with an
// idempotent start/stop, and let live tasks notice a stop via a monotonic
// epoch counter" skeleton that is genuinely identical across the monitor
// modules (see usb_monitor.rs and usb_metering.rs). Each monitor still owns
// its own `static RUNNING` / `static RUN_EPOCH` — this only holds the guard
// logic and the couple of tiny pure helpers duplicated verbatim everywhere.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Result of attempting to start a monitor via [`try_start`].
pub enum StartOutcome {
    /// Wasn't running; now is. Carries the new epoch for the caller's
    /// spawned tasks to compare against on every loop iteration.
    Started(u64),
    /// Already running — the caller's start command should be a no-op.
    AlreadyRunning,
}

/// Guarded, idempotent "start": flips `running` false→true and, only on
/// success, bumps `epoch` and returns the new value. Mirrors the identical
/// `compare_exchange` + `fetch_add` pairing in usb_monitor/usb_metering.
pub fn try_start(running: &'static AtomicBool, epoch: &'static AtomicU64) -> StartOutcome {
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return StartOutcome::AlreadyRunning;
    }
    let new_epoch = epoch.fetch_add(1, Ordering::SeqCst) + 1;
    StartOutcome::Started(new_epoch)
}

/// Guarded, idempotent "stop": flips `running` true→false and, only on
/// success, bumps `epoch` so live watcher loops see a mismatch and exit.
/// Returns `true` iff this call actually stopped a running monitor.
pub fn try_stop(running: &'static AtomicBool, epoch: &'static AtomicU64) -> bool {
    if running
        .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return false;
    }
    epoch.fetch_add(1, Ordering::SeqCst);
    true
}

/// Whether a still-running watcher/sampler loop should keep going, given the
/// epoch it was spawned with. Callers re-check this on every line/event so a
/// stop (which bumps the epoch) is noticed without a dedicated cancel channel.
/// Not yet wired into usb_monitor/usb_metering (they still inline the same
/// check) — kept + tested as the shared helper new call sites should use.
#[allow(dead_code)]
pub fn epoch_current(epoch: &'static AtomicU64, spawned_epoch: u64) -> bool {
    epoch.load(Ordering::Relaxed) == spawned_epoch
}

/// Unix epoch seconds — duplicated verbatim across monitor modules.
pub fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Escape a string for embedding inside a PowerShell single-quoted literal
/// by doubling any apostrophes — duplicated verbatim across monitor modules.
pub fn ps_escape(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_RUNNING: AtomicBool = AtomicBool::new(false);
    static TEST_EPOCH: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn start_from_stopped_bumps_epoch_and_reports_started() {
        TEST_RUNNING.store(false, Ordering::SeqCst);
        TEST_EPOCH.store(0, Ordering::SeqCst);

        match try_start(&TEST_RUNNING, &TEST_EPOCH) {
            StartOutcome::Started(epoch) => assert_eq!(epoch, 1),
            StartOutcome::AlreadyRunning => panic!("expected Started"),
        }
        assert!(TEST_RUNNING.load(Ordering::SeqCst));
    }

    #[test]
    fn start_while_running_reports_already_running() {
        TEST_RUNNING.store(true, Ordering::SeqCst);
        let before = TEST_EPOCH.load(Ordering::SeqCst);

        match try_start(&TEST_RUNNING, &TEST_EPOCH) {
            StartOutcome::AlreadyRunning => {}
            StartOutcome::Started(_) => panic!("expected AlreadyRunning"),
        }
        // Epoch must not move on a no-op start.
        assert_eq!(TEST_EPOCH.load(Ordering::SeqCst), before);

        TEST_RUNNING.store(false, Ordering::SeqCst);
    }

    #[test]
    fn stop_while_running_bumps_epoch_and_returns_true() {
        TEST_RUNNING.store(true, Ordering::SeqCst);
        let before = TEST_EPOCH.load(Ordering::SeqCst);

        assert!(try_stop(&TEST_RUNNING, &TEST_EPOCH));
        assert!(!TEST_RUNNING.load(Ordering::SeqCst));
        assert_eq!(TEST_EPOCH.load(Ordering::SeqCst), before + 1);
    }

    #[test]
    fn stop_while_stopped_is_noop() {
        TEST_RUNNING.store(false, Ordering::SeqCst);
        let before = TEST_EPOCH.load(Ordering::SeqCst);

        assert!(!try_stop(&TEST_RUNNING, &TEST_EPOCH));
        assert_eq!(TEST_EPOCH.load(Ordering::SeqCst), before);
    }

    #[test]
    fn epoch_current_detects_stale_epoch_after_stop() {
        TEST_RUNNING.store(false, Ordering::SeqCst);
        TEST_EPOCH.store(5, Ordering::SeqCst);

        assert!(epoch_current(&TEST_EPOCH, 5));
        TEST_EPOCH.fetch_add(1, Ordering::SeqCst);
        assert!(!epoch_current(&TEST_EPOCH, 5));
    }

    #[test]
    fn apostrophes_are_doubled() {
        assert_eq!(ps_escape("it's"), "it''s");
        assert_eq!(ps_escape("a'b'c"), "a''b''c");
    }
}
