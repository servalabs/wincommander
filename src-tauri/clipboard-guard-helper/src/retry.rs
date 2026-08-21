// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded retry-with-backoff — the shared primitive behind every clipboard
//! open in this crate (plan §4.2: "Clipboard opens fail transiently when
//! another process holds it: retry with bounded backoff, and treat
//! exhausted retries as a reported failure, not a silent skip.").
//!
//! `clipboard-win`'s own `Clipboard::new_attempts(10)` already retries
//! `OpenClipboard` internally, but via a bare `Sleep(0)` scheduler-yield
//! between attempts — not a real backoff, and not enough to ride out a
//! competing app that holds the clipboard open for tens of milliseconds.
//! This module adds a second, outer retry with an actual sleep between
//! attempts, on top of (not instead of) `clipboard-win`'s own.

use std::time::Duration;

/// Call `attempt` up to `max_attempts` times (at least once), sleeping
/// `backoff` between tries (never before the first, never after the
/// last). Returns the first `Some` result, or `None` if every attempt
/// failed.
///
/// Callers MUST treat a `None` return as a genuine, reportable failure —
/// never silently reinterpret it as "there was nothing to read/write".
/// See `read::ReadOutcome::Failed` and `actions::ClipboardWriter` for the
/// two call sites that this contract exists for.
pub fn retry_with_backoff<T>(
    max_attempts: u32,
    backoff: Duration,
    mut attempt: impl FnMut() -> Option<T>,
) -> Option<T> {
    let attempts = max_attempts.max(1);
    for i in 0..attempts {
        if let Some(value) = attempt() {
            return Some(value);
        }
        if i + 1 < attempts {
            std::thread::sleep(backoff);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn succeeds_immediately_without_sleeping() {
        let calls = AtomicU32::new(0);
        let result = retry_with_backoff(5, Duration::ZERO, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Some(42)
        });
        assert_eq!(result, Some(42));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn succeeds_on_a_later_attempt() {
        let calls = AtomicU32::new(0);
        let result = retry_with_backoff(5, Duration::ZERO, || {
            let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
            if n < 3 {
                None
            } else {
                Some(n)
            }
        });
        assert_eq!(result, Some(3));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn exhausts_every_attempt_and_reports_failure() {
        let calls = AtomicU32::new(0);
        let result: Option<()> = retry_with_backoff(4, Duration::ZERO, || {
            calls.fetch_add(1, Ordering::SeqCst);
            None
        });
        assert_eq!(result, None);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            4,
            "every attempt must actually run before reporting failure"
        );
    }

    #[test]
    fn zero_max_attempts_still_tries_once() {
        // A caller passing 0 by mistake must not silently skip the attempt
        // entirely (that would be indistinguishable from "never checked").
        let calls = AtomicU32::new(0);
        let _ = retry_with_backoff(0, Duration::ZERO, || {
            calls.fetch_add(1, Ordering::SeqCst);
            None::<()>
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
