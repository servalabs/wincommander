// SPDX-License-Identifier: AGPL-3.0-or-later
//! Action execution — honestly separating "we tried" from "we verified it
//! worked" (plan §9 Phase 2 exit criterion: "clear/quarantine outcome is
//! verified and reported as success/failure only").
//!
//! Six actions exist (`wincmd_clip_rules::Action`): `NotifyUser`,
//! `ClearClipboard`, `QuarantineClipboard`, `RecordLocalReceipt`,
//! `ReportFleet`, `AlertAdmin`. This module splits their execution into
//! two passes, because they have genuinely different failure semantics:
//!
//! - [`execute_local_actions`] runs the three actions this device can
//!   directly attempt and verify: notifying the user, and — the load-
//!   bearing case — clearing or quarantining the clipboard. Emptying the
//!   clipboard CAN fail (another process holding it, a write silently
//!   stomped by something else), so [`ClipboardWriter::clear`] and
//!   [`ClipboardWriter::quarantine`] don't just report the write call's
//!   own success — they read the clipboard back afterward and only report
//!   success if the content actually is what was asked for. Reporting an
//!   attempted-but-unverified action as succeeded would be a false audit
//!   record.
//! - [`mark_submission_actions`] folds in `RecordLocalReceipt`,
//!   `ReportFleet`, and `AlertAdmin` — all three of which, from this
//!   device's own perspective, are fulfilled by ONE thing: whether the
//!   `svc.clipboard.report_event` submission itself succeeded (svc is what
//!   actually persists the receipt and forwards it toward Fleet/an admin
//!   alert — see `ipc`). This module can't independently verify svc's
//!   downstream persistence any more than it could invent a way to
//!   ask "did the message I haven't sent yet arrive" — so it marks these
//!   three attempted/succeeded together, once, after the submission
//!   attempt actually happens.
//!
//!   Call `mark_submission_actions` AFTER building and sending the wire
//!   report (see `report::build_report`'s doc), and treat its effect on
//!   `ActionOutcome` as LOCAL bookkeeping/health tracking only — it is
//!   never used to retransmit an amended report. A report can't assert
//!   its own successful delivery from inside itself before delivery
//!   happens, so the payload that actually reaches svc only ever reflects
//!   the local actions; whether svc has the report AT ALL is the true
//!   answer to "did `ReportFleet`/`RecordLocalReceipt` succeed".

use wincmd_clip_rules::{Action, Severity};

/// Which of a matched rule's requested actions were attempted, and which
/// of those were verified to have actually succeeded. `succeeded` is
/// always a subset of `attempted` — never claim success for something
/// never attempted, and never grow `succeeded` without also recording the
/// attempt.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActionOutcome {
    pub attempted: Vec<Action>,
    pub succeeded: Vec<Action>,
}

impl ActionOutcome {
    pub fn new() -> Self {
        Self::default()
    }

    fn mark(&mut self, action: Action, succeeded: bool) {
        self.attempted.push(action);
        if succeeded {
            self.succeeded.push(action);
        }
    }
}

/// Clipboard write/verify surface. Both methods report success only after
/// verifying the clipboard actually holds the intended content afterward
/// — see the module doc for why that verification step is load-bearing,
/// not optional.
pub trait ClipboardWriter: Send {
    /// Overwrite the clipboard with an empty string; return `true` only
    /// if a bounded read-back confirms the clipboard is now empty.
    fn clear(&mut self) -> bool;
    /// Overwrite the clipboard with `placeholder`; return `true` only if
    /// a bounded read-back confirms the clipboard holds exactly that
    /// placeholder.
    fn quarantine(&mut self, placeholder: &str) -> bool;
}

/// Show the user a notification about the match. Kept as its own small
/// trait (rather than folded into `ClipboardWriter`) because it has
/// nothing to do with clipboard content and a caller embedding this crate
/// (e.g. a future `paste_monitor.rs` integration) will likely want to
/// route it through its own existing native-notification plumbing instead
/// of this crate's.
pub trait UserNotifier: Send {
    fn notify(&mut self, severity: Severity) -> bool;
}

/// A notifier that does nothing and always reports success. Useful for a
/// caller that doesn't want this crate to own notification UI, or for
/// tests that don't care about `NotifyUser`.
pub struct NullNotifier;

impl UserNotifier for NullNotifier {
    fn notify(&mut self, _severity: Severity) -> bool {
        true
    }
}

#[cfg(windows)]
pub struct Win32Writer {
    pub max_attempts: u32,
    pub backoff: std::time::Duration,
}

#[cfg(windows)]
impl Default for Win32Writer {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff: std::time::Duration::from_millis(50),
        }
    }
}

#[cfg(windows)]
impl Win32Writer {
    /// Write `value` to the clipboard (bounded retry), then read it back
    /// (bounded retry) and only report success if the read-back is
    /// EXACTLY `value`. A competing process overwriting the clipboard
    /// between our write and the read-back is exactly the case this
    /// verification is meant to catch — reporting "cleared" on a
    /// clipboard some other app just wrote a NEW secret into would be a
    /// dangerously false audit record.
    fn write_and_verify(&mut self, value: &str) -> bool {
        let wrote = crate::retry::retry_with_backoff(self.max_attempts, self.backoff, || {
            clipboard_win::set_clipboard_string(value).ok()
        });
        if wrote.is_none() {
            return false;
        }
        let read_back = crate::retry::retry_with_backoff(self.max_attempts, self.backoff, || {
            clipboard_win::get_clipboard::<String, _>(clipboard_win::formats::Unicode).ok()
        });
        matches!(read_back, Some(actual) if actual == value)
    }
}

#[cfg(windows)]
impl ClipboardWriter for Win32Writer {
    fn clear(&mut self) -> bool {
        self.write_and_verify("")
    }

    fn quarantine(&mut self, placeholder: &str) -> bool {
        self.write_and_verify(placeholder)
    }
}

#[cfg(not(windows))]
#[derive(Default)]
pub struct Win32Writer;

#[cfg(not(windows))]
impl ClipboardWriter for Win32Writer {
    fn clear(&mut self) -> bool {
        false
    }
    fn quarantine(&mut self, _placeholder: &str) -> bool {
        false
    }
}

/// Execute the LOCAL, synchronously-verifiable actions a matched rule
/// requested (`NotifyUser`, `ClearClipboard`, `QuarantineClipboard`).
/// `RecordLocalReceipt`/`ReportFleet`/`AlertAdmin` are deliberately NOT
/// touched here — see [`mark_submission_actions`] and the module doc.
pub fn execute_local_actions(
    actions: &[Action],
    writer: &mut dyn ClipboardWriter,
    notifier: &mut dyn UserNotifier,
    severity: Severity,
    quarantine_placeholder: &str,
) -> ActionOutcome {
    let mut outcome = ActionOutcome::new();
    for &action in actions {
        match action {
            Action::NotifyUser => outcome.mark(action, notifier.notify(severity)),
            Action::ClearClipboard => outcome.mark(action, writer.clear()),
            Action::QuarantineClipboard => {
                outcome.mark(action, writer.quarantine(quarantine_placeholder))
            }
            Action::RecordLocalReceipt | Action::ReportFleet | Action::AlertAdmin => {
                // Handled by `mark_submission_actions` after the svc
                // submission attempt — see module doc.
            }
        }
    }
    outcome
}

/// Fold the outcome of the `svc.clipboard.report_event` submission into
/// `outcome` for whichever of `RecordLocalReceipt`/`ReportFleet`/
/// `AlertAdmin` the rule actually requested. Call this AFTER the
/// submission attempt has actually happened (whether it succeeded or
/// not) — an attempted-but-failed submission must still show up in
/// `attempted`, never be silently dropped.
pub fn mark_submission_actions(
    actions: &[Action],
    outcome: &mut ActionOutcome,
    submitted_ok: bool,
) {
    for &action in actions {
        if matches!(
            action,
            Action::RecordLocalReceipt | Action::ReportFleet | Action::AlertAdmin
        ) {
            outcome.mark(action, submitted_ok);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeWriter {
        clear_result: bool,
        quarantine_result: bool,
    }
    impl ClipboardWriter for FakeWriter {
        fn clear(&mut self) -> bool {
            self.clear_result
        }
        fn quarantine(&mut self, _placeholder: &str) -> bool {
            self.quarantine_result
        }
    }

    struct FakeNotifier(bool);
    impl UserNotifier for FakeNotifier {
        fn notify(&mut self, _severity: Severity) -> bool {
            self.0
        }
    }

    #[test]
    fn a_failed_clear_reports_attempted_without_succeeded() {
        let mut writer = FakeWriter {
            clear_result: false,
            quarantine_result: true,
        };
        let mut notifier = NullNotifier;
        let outcome = execute_local_actions(
            &[Action::ClearClipboard],
            &mut writer,
            &mut notifier,
            Severity::High,
            "[redacted]",
        );
        assert_eq!(outcome.attempted, vec![Action::ClearClipboard]);
        assert!(
            outcome.succeeded.is_empty(),
            "a verified-failed clear must never appear in succeeded"
        );
    }

    #[test]
    fn a_verified_clear_reports_both_attempted_and_succeeded() {
        let mut writer = FakeWriter {
            clear_result: true,
            quarantine_result: false,
        };
        let mut notifier = NullNotifier;
        let outcome = execute_local_actions(
            &[Action::ClearClipboard],
            &mut writer,
            &mut notifier,
            Severity::Warn,
            "[redacted]",
        );
        assert_eq!(outcome.attempted, vec![Action::ClearClipboard]);
        assert_eq!(outcome.succeeded, vec![Action::ClearClipboard]);
    }

    #[test]
    fn a_failed_quarantine_reports_attempted_without_succeeded() {
        let mut writer = FakeWriter {
            clear_result: true,
            quarantine_result: false,
        };
        let mut notifier = NullNotifier;
        let outcome = execute_local_actions(
            &[Action::QuarantineClipboard],
            &mut writer,
            &mut notifier,
            Severity::High,
            "[redacted]",
        );
        assert_eq!(outcome.attempted, vec![Action::QuarantineClipboard]);
        assert!(outcome.succeeded.is_empty());
    }

    #[test]
    fn notify_user_outcome_follows_the_notifier() {
        let mut writer = FakeWriter {
            clear_result: true,
            quarantine_result: true,
        };
        let mut failing_notifier = FakeNotifier(false);
        let outcome = execute_local_actions(
            &[Action::NotifyUser],
            &mut writer,
            &mut failing_notifier,
            Severity::Warn,
            "[redacted]",
        );
        assert_eq!(outcome.attempted, vec![Action::NotifyUser]);
        assert!(outcome.succeeded.is_empty());
    }

    #[test]
    fn remote_actions_are_untouched_by_execute_local_actions() {
        let mut writer = FakeWriter {
            clear_result: true,
            quarantine_result: true,
        };
        let mut notifier = NullNotifier;
        let outcome = execute_local_actions(
            &[
                Action::RecordLocalReceipt,
                Action::ReportFleet,
                Action::AlertAdmin,
            ],
            &mut writer,
            &mut notifier,
            Severity::Critical,
            "[redacted]",
        );
        assert!(outcome.attempted.is_empty());
        assert!(outcome.succeeded.is_empty());
    }

    #[test]
    fn mark_submission_actions_marks_only_the_remote_actions_requested() {
        let mut outcome = ActionOutcome::new();
        mark_submission_actions(
            &[
                Action::RecordLocalReceipt,
                Action::ReportFleet,
                Action::NotifyUser,
            ],
            &mut outcome,
            true,
        );
        // NotifyUser is a LOCAL action — mark_submission_actions must not
        // touch it, even though it appeared in the rule's action list.
        assert_eq!(
            outcome.attempted,
            vec![Action::RecordLocalReceipt, Action::ReportFleet]
        );
        assert_eq!(
            outcome.succeeded,
            vec![Action::RecordLocalReceipt, Action::ReportFleet]
        );
    }

    #[test]
    fn mark_submission_actions_records_attempted_without_succeeded_on_failure() {
        let mut outcome = ActionOutcome::new();
        mark_submission_actions(&[Action::AlertAdmin], &mut outcome, false);
        assert_eq!(outcome.attempted, vec![Action::AlertAdmin]);
        assert!(outcome.succeeded.is_empty());
    }
}
