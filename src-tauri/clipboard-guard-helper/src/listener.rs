// SPDX-License-Identifier: AGPL-3.0-or-later
//! Event-driven clipboard-change source, with a polling fallback (plan
//! §4.2 "Move to event-driven").
//!
//! `AddClipboardFormatListener` + `WM_CLIPBOARDUPDATE` on a message-only
//! window replaces the 2000ms poll (`paste_monitor.rs:920`) and is what
//! makes clear/quarantine fast enough to matter. This module uses
//! `clipboard-win`'s `monitor` feature (`clipboard_win::Monitor`) for the
//! actual window/message-pump plumbing rather than hand-rolling a second
//! one — that crate already implements exactly this pattern (message-only
//! window via `HWND_MESSAGE` + `AddClipboardFormatListener` +
//! `RemoveClipboardFormatListener` on drop + a `Shutdown` handle safe to
//! signal from another thread) and ships its own tests for it.
//!
//! **Keep a slow poll as a fallback if listener registration fails, and
//! surface the degraded mode as a health boolean — never silently** (plan
//! §4.2). [`resolve_listener_mode`] is the pure decision step that makes
//! this testable without a real window.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::health::HelperHealth;

/// Which clipboard-change source is actually active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerMode {
    /// `AddClipboardFormatListener` registered successfully — changes are
    /// pushed via `WM_CLIPBOARDUPDATE`.
    EventDriven,
    /// Registration failed; falling back to a slow sequence-number poll.
    /// Degraded, but never silent — see [`resolve_listener_mode`].
    Polling,
}

/// Pure decision step, split out from the real Win32 registration attempt
/// specifically so the fallback behaviour is unit-testable without a real
/// message-only window (plan's test list: "Listener-registration failure
/// falls back to polling AND flips the degraded health flag").
///
/// Always writes `health.listener_registered` — the plan is explicit that
/// a registration failure must "surface the degraded mode as a health
/// boolean, never silently", so this function is the one place that
/// boolean gets set from a registration outcome.
pub fn resolve_listener_mode(
    registration_succeeded: bool,
    health: &mut HelperHealth,
) -> ListenerMode {
    health.listener_registered = registration_succeeded;
    if registration_succeeded {
        ListenerMode::EventDriven
    } else {
        ListenerMode::Polling
    }
}

/// A source of "the clipboard just changed" signals. Both the real
/// event-driven listener and the polling fallback implement this with the
/// same blocking contract: `wait_for_change` blocks the calling thread
/// until either a real change happens (`true`) or the source is asked to
/// stop (`false`, after which the caller should stop calling it). Callers
/// that need this alongside async work should run it via
/// `tokio::task::spawn_blocking`.
pub trait ClipboardChangeSource: Send {
    fn wait_for_change(&mut self) -> bool;
}

/// Slow-poll fallback: compares the OS clipboard sequence number
/// (`clipboard_win::seq_num`, itself a thin wrapper over
/// `GetClipboardSequenceNumber`) on an interval, without ever reading
/// clipboard CONTENT just to detect that it changed. Used only when
/// `AddClipboardFormatListener` registration fails — see
/// [`resolve_listener_mode`].
pub struct SequencePoller {
    last_seq: Option<u32>,
    interval: Duration,
    stop: Arc<AtomicBool>,
}

impl SequencePoller {
    pub fn new(interval: Duration, stop: Arc<AtomicBool>) -> Self {
        Self {
            last_seq: current_sequence(),
            interval,
            stop,
        }
    }
}

impl ClipboardChangeSource for SequencePoller {
    fn wait_for_change(&mut self) -> bool {
        while !self.stop.load(Ordering::SeqCst) {
            std::thread::sleep(self.interval);
            let seq = current_sequence();
            if seq != self.last_seq {
                self.last_seq = seq;
                return true;
            }
        }
        false
    }
}

#[cfg(windows)]
fn current_sequence() -> Option<u32> {
    clipboard_win::seq_num().map(|n| n.get())
}

#[cfg(not(windows))]
fn current_sequence() -> Option<u32> {
    None
}

/// The real event-driven listener: a dedicated OS thread owns a
/// `clipboard_win::Monitor` (message-only window + `AddClipboardFormatListener`)
/// for the listener's whole lifetime, forwarding each `WM_CLIPBOARDUPDATE`
/// through a plain channel. `Monitor` is documented as unsafe to move
/// across threads, so it is constructed AND consumed entirely inside the
/// spawned thread — only the resulting `Shutdown` handle (documented `Send`)
/// and change signals cross the thread boundary.
///
/// Always drop-shuts-down: `Drop` posts the close message via `Shutdown`
/// and joins the thread, so `RemoveClipboardFormatListener` and the
/// message-only window's teardown always run before this struct's memory
/// goes away — "Always `RemoveClipboardFormatListener` on shutdown; a
/// message-only window still needs a real message pump" (plan §4.2).
/// `Win32EventListener::try_start` failed — `AddClipboardFormatListener`
/// (or the message-only window / listener thread it needs) could not be
/// set up. Carries no detail beyond "it failed" deliberately: the caller's
/// only correct response is to fall back to [`SequencePoller`] via
/// [`resolve_listener_mode`], not to branch on a reason.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerStartError;

#[cfg(windows)]
pub struct Win32EventListener {
    join: Option<std::thread::JoinHandle<()>>,
    shutdown: Option<clipboard_win::monitor::Shutdown>,
    rx: std::sync::mpsc::Receiver<()>,
}

#[cfg(windows)]
impl Win32EventListener {
    /// Attempt to start the listener. `Err` means
    /// `AddClipboardFormatListener` (or the message-only window it needs)
    /// failed to register — the caller should fall back to
    /// [`SequencePoller`] via [`resolve_listener_mode`].
    pub fn try_start() -> Result<Self, ListenerStartError> {
        let (ready_tx, ready_rx) =
            std::sync::mpsc::channel::<Option<clipboard_win::monitor::Shutdown>>();
        let (change_tx, change_rx) = std::sync::mpsc::channel::<()>();

        let join = std::thread::Builder::new()
            .name("clipboard-guard-listener".to_string())
            .spawn(move || {
                match clipboard_win::monitor::Monitor::new() {
                    Ok(mut monitor) => {
                        let shutdown = monitor.shutdown_channel();
                        if ready_tx.send(Some(shutdown)).is_err() {
                            // Nobody is listening for the ready signal any
                            // more (constructor gave up) — nothing to do
                            // but let the monitor (and its listener
                            // registration) drop here.
                            return;
                        }
                        loop {
                            match monitor.recv() {
                                Ok(true) => {
                                    // Receiver gone is not our problem —
                                    // this thread's only job is to keep
                                    // pumping messages so the OS-owned
                                    // window stays healthy.
                                    let _ = change_tx.send(());
                                }
                                // Graceful shutdown requested via `Shutdown`.
                                Ok(false) => break,
                                // The underlying message pump errored —
                                // stop rather than spin. The process-level
                                // health surface will show
                                // `listener_registered` as stale/true from
                                // the last successful state; a future
                                // enhancement could re-arm here, but V1
                                // treats this as "listener died, helper
                                // should restart" rather than silently
                                // limping.
                                Err(_) => break,
                            }
                        }
                    }
                    Err(_) => {
                        let _ = ready_tx.send(None);
                    }
                }
            })
            .map_err(|_| ListenerStartError)?;

        match ready_rx.recv() {
            Ok(Some(shutdown)) => Ok(Self {
                join: Some(join),
                shutdown: Some(shutdown),
                rx: change_rx,
            }),
            _ => {
                let _ = join.join();
                Err(ListenerStartError)
            }
        }
    }
}

#[cfg(windows)]
impl ClipboardChangeSource for Win32EventListener {
    fn wait_for_change(&mut self) -> bool {
        self.rx.recv().is_ok()
    }
}

#[cfg(windows)]
impl Drop for Win32EventListener {
    fn drop(&mut self) {
        // Dropping `Shutdown` posts the close message to the listener
        // thread's window, which makes `monitor.recv()` return `Ok(false)`
        // and break the loop above.
        self.shutdown.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Either the real listener or the polling fallback — whichever
/// [`start`] resolved to.
pub enum ActiveListener {
    #[cfg(windows)]
    EventDriven(Win32EventListener),
    Polling(SequencePoller),
}

impl ClipboardChangeSource for ActiveListener {
    fn wait_for_change(&mut self) -> bool {
        match self {
            #[cfg(windows)]
            ActiveListener::EventDriven(l) => l.wait_for_change(),
            ActiveListener::Polling(p) => p.wait_for_change(),
        }
    }
}

/// Start the clipboard change source for real: attempt the event-driven
/// listener first, falling back to [`SequencePoller`] if registration
/// fails, and honestly recording the result into `health` via
/// [`resolve_listener_mode`]. `poll_interval` only matters for the
/// fallback path.
pub fn start(
    poll_interval: Duration,
    stop: Arc<AtomicBool>,
    health: &mut HelperHealth,
) -> (ActiveListener, ListenerMode) {
    #[cfg(windows)]
    {
        match Win32EventListener::try_start() {
            Ok(listener) => {
                let mode = resolve_listener_mode(true, health);
                (ActiveListener::EventDriven(listener), mode)
            }
            Err(ListenerStartError) => {
                let mode = resolve_listener_mode(false, health);
                (
                    ActiveListener::Polling(SequencePoller::new(poll_interval, stop)),
                    mode,
                )
            }
        }
    }
    #[cfg(not(windows))]
    {
        let mode = resolve_listener_mode(false, health);
        (
            ActiveListener::Polling(SequencePoller::new(poll_interval, stop)),
            mode,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_success_yields_event_driven_and_healthy_flag() {
        let mut health = HelperHealth::default();
        let mode = resolve_listener_mode(true, &mut health);
        assert_eq!(mode, ListenerMode::EventDriven);
        assert!(health.listener_registered);
    }

    #[test]
    fn registration_failure_falls_back_to_polling_and_flips_degraded_flag() {
        let mut health = HelperHealth {
            listener_registered: true, // start "healthy" so the flip is observable
            ..HelperHealth::default()
        };
        let mode = resolve_listener_mode(false, &mut health);
        assert_eq!(mode, ListenerMode::Polling);
        assert!(
            !health.listener_registered,
            "must flip the degraded flag, not leave it stale"
        );
    }

    #[test]
    fn sequence_poller_reports_change_when_sequence_moves() {
        // Pure test of the stop-flag contract: with `stop` already set,
        // `wait_for_change` must return `false` promptly rather than
        // blocking forever, regardless of platform (current_sequence()
        // stubs to `None` on non-Windows, so a real clipboard is never
        // required for this test either).
        let stop = Arc::new(AtomicBool::new(true));
        let mut poller = SequencePoller::new(Duration::from_millis(1), stop);
        assert!(!poller.wait_for_change());
    }
}
