// SPDX-License-Identifier: AGPL-3.0-or-later
//! Building the content-free wire report for one rule match.
//!
//! [`build_report`]'s signature is the enforcement mechanism for this
//! crate's content-free discipline (plan §8), not just a convention: it
//! takes no clipboard text, no match offset, no capture — there is
//! nothing in its parameter list a caller COULD pass that would leak
//! matched content, even by mistake. This mirrors
//! `wincmd_clip_rules::Verdict`'s own doc: "structurally content-free by
//! construction".
//!
//! # Call `build_report` BEFORE `mark_submission_actions`, not after
//!
//! A message can't truthfully assert its own successful delivery from
//! inside itself, before delivery happens. So the `outcome` passed in
//! here should be the result of `actions::execute_local_actions` alone —
//! `RecordLocalReceipt`/`ReportFleet`/`AlertAdmin` are folded into the
//! caller's `ActionOutcome` via `actions::mark_submission_actions` only
//! AFTER this report has actually been sent (successfully or not), and
//! that update is for the caller's OWN local bookkeeping/health tracking
//! — it is never retransmitted as an amended report. See `ipc`'s caller
//! (the helper's main loop) for the exact sequencing.

use wincmd_clip_rules::{RuleId, Severity};
use wincmd_shared::fleet::ClipboardEventReport;

use crate::actions::ActionOutcome;

/// Build the wire report for one rule match. Every field traces back to
/// either a scalar the caller already had (policy_version, rule_revision,
/// severity), a closed enum (the two action lists), a content-free metric
/// (`suppressed_count`), or a freshly minted id/timestamp — never
/// clipboard text.
pub fn build_report(
    policy_version: i64,
    rule_id: &RuleId,
    rule_revision: u32,
    severity: Severity,
    outcome: &ActionOutcome,
    suppressed_count: u32,
) -> ClipboardEventReport {
    ClipboardEventReport {
        event_id: crate::ids::mint_event_id(),
        occurred_at: crate::timestamp::now_rfc3339(),
        policy_version,
        rule_id: rule_id.as_str().to_string(),
        rule_revision,
        severity,
        actions_attempted: outcome.attempted.clone(),
        actions_succeeded: outcome.succeeded.clone(),
        suppressed_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{execute_local_actions, mark_submission_actions, NullNotifier};
    use crate::engine::{MatchEngine, MatchOutcome};
    use crate::policy::{ClipboardPolicyResponse, PolicyStore};
    use std::time::Instant;
    use wincmd_clip_rules::{Action, MatchKind, Rule};

    /// End-to-end sentinel-absence test (plan's test list: "Sentinel text
    /// never appears in a submission or a log"). Feeds a sentinel through
    /// the REAL match → local-action → submission-mark → report path —
    /// not just `build_report`'s signature in isolation — because the
    /// content-free guarantee has to hold for the whole pipeline, not
    /// just the last step.
    #[test]
    fn sentinel_never_appears_in_the_built_report() {
        const SENTINEL: &str = "SENTINEL_MARKER_zzz_do_not_leak_this_9F3C";

        let mut store = PolicyStore::new();
        let rule = Rule {
            id: RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").unwrap(),
            revision: 7,
            name: "sentinel rule".to_string(),
            enabled: true,
            priority: 100,
            matcher: MatchKind::Phrase {
                value: SENTINEL.to_string(),
                case_sensitive: true,
            },
            severity: Severity::High,
            actions: vec![
                Action::NotifyUser,
                Action::ClearClipboard,
                Action::RecordLocalReceipt,
                Action::ReportFleet,
            ],
            cooldown_seconds: 60,
            snoozable: true,
            locked: false,
        };
        store
            .install(&ClipboardPolicyResponse {
                policy_version: 42,
                rules: vec![rule],
            })
            .expect("sentinel rule compiles");

        let mut engine = MatchEngine::new();
        let clipboard_text = format!("here is a secret: {SENTINEL} — do not let this leak");
        let outcome = engine.observe(store.active(), &clipboard_text, Instant::now());

        let MatchOutcome::Emit {
            verdict,
            suppressed_since_last,
        } = outcome
        else {
            panic!("expected the sentinel rule to match and emit");
        };

        struct AlwaysSucceedsWriter;
        impl crate::actions::ClipboardWriter for AlwaysSucceedsWriter {
            fn clear(&mut self) -> bool {
                true
            }
            fn quarantine(&mut self, _placeholder: &str) -> bool {
                true
            }
        }

        let mut writer = AlwaysSucceedsWriter;
        let mut notifier = NullNotifier;
        // `execute_local_actions` handles NotifyUser/ClearClipboard/
        // QuarantineClipboard only — RecordLocalReceipt/ReportFleet are
        // deliberately NOT part of this outcome (see module doc): a
        // report can't truthfully assert its own successful delivery
        // before it's been delivered, so those two are folded in via
        // `mark_submission_actions` AFTER the transmission attempt, for
        // LOCAL bookkeeping only — never re-sent as an amended report.
        let mut action_outcome = execute_local_actions(
            &verdict.actions,
            &mut writer,
            &mut notifier,
            verdict.severity,
            "[clipboard content withheld]",
        );

        let report = build_report(
            store.active().policy_version,
            &verdict.rule_id,
            verdict.rule_revision,
            verdict.severity,
            &action_outcome,
            suppressed_since_last,
        );

        let serialized = serde_json::to_string(&report).expect("report serializes");
        assert!(
            !serialized.contains(SENTINEL),
            "clipboard text leaked into the serialized report: {serialized}"
        );
        assert!(
            !serialized.contains(&clipboard_text),
            "full clipboard text leaked into the serialized report: {serialized}"
        );

        // Sanity: the report DOES carry the content-free fields we expect,
        // so this test is actually exercising the real pipeline and not
        // trivially passing on an empty report.
        assert_eq!(report.policy_version, 42);
        assert_eq!(report.rule_revision, 7);
        assert!(report.actions_attempted.contains(&Action::ClearClipboard));
        assert!(report.actions_succeeded.contains(&Action::ClearClipboard));

        // Simulate the (already-sent) report having reached svc
        // successfully — this updates LOCAL bookkeeping only, never a
        // second transmission.
        mark_submission_actions(&verdict.actions, &mut action_outcome, true);
        assert!(action_outcome.succeeded.contains(&Action::ReportFleet));
    }

    #[test]
    fn build_report_signature_cannot_carry_clipboard_text() {
        // Documentation-as-test: this function's parameter list is the
        // enforcement mechanism (see module doc). This test just pins
        // that the produced report's fields are exactly the closed set
        // plan §4.4 specifies — a future edit that widens the struct
        // (e.g. adding a free-text field) would need to touch this
        // assertion, making the change reviewable rather than silent.
        let outcome = ActionOutcome {
            attempted: vec![Action::NotifyUser],
            succeeded: vec![Action::NotifyUser],
        };
        let rule_id = RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").unwrap();
        let report = build_report(1, &rule_id, 1, Severity::Info, &outcome, 0);
        let value = serde_json::to_value(&report).unwrap();
        let mut fields: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            vec![
                "actions_attempted",
                "actions_succeeded",
                "event_id",
                "occurred_at",
                "policy_version",
                "rule_id",
                "rule_revision",
                "severity",
                "suppressed_count",
            ]
        );
    }
}
