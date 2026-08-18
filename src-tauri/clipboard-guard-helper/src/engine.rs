// SPDX-License-Identifier: AGPL-3.0-or-later
//! Match + cooldown engine: truncate, evaluate, and fold repeated matches
//! into a content-free suppressed count instead of an alert storm (plan
//! §4.2/§4.4: "then `CooldownLedger::should_emit` per rule so a repeated
//! match becomes a content-free `suppressed_count` instead of an alert
//! storm").

use std::collections::HashMap;
use std::time::Instant;

use wincmd_clip_rules::{CooldownLedger, Emit, RuleId, Verdict};

use crate::policy::ActivePolicy;
use crate::read::MAX_CLIPBOARD_READ_BYTES;

/// What one `observe()` call decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchOutcome {
    /// No enabled rule matched.
    NoMatch,
    /// This rule should actually fire now — either its first-ever match,
    /// or its cooldown window has elapsed. `suppressed_since_last` folds
    /// in however many matches of THIS rule were suppressed since the
    /// previous emission (0 if there were none), so a burst of repeats
    /// inside one cooldown window shows up as one alert with a count,
    /// never as N separate alerts.
    Emit {
        verdict: Verdict,
        suppressed_since_last: u32,
    },
    /// Matched, but still inside this rule's cooldown window since its
    /// last emission — no action/report should be executed for this
    /// specific match. `count` is folded into the NEXT `Emit` for this
    /// rule (see `MatchEngine::observe`).
    Suppressed { rule_id: RuleId, count: u32 },
}

/// Owns the cooldown ledger and the "suppressed since last real emission"
/// bookkeeping. Stateful and per-process; a fresh `MatchEngine` treats
/// every rule as never having fired, matching `CooldownLedger`'s own
/// semantics.
#[derive(Default)]
pub struct MatchEngine {
    cooldown: CooldownLedger,
    pending_suppressed: HashMap<RuleId, u32>,
}

impl MatchEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Truncate `text` to [`MAX_CLIPBOARD_READ_BYTES`], evaluate it
    /// against `policy`'s compiled ruleset, and gate the result through
    /// this rule's cooldown. `now` is caller-supplied (not read from the
    /// system clock here) so cooldown behaviour is deterministically
    /// testable — mirrors `CooldownLedger::should_emit`'s own contract.
    pub fn observe(&mut self, policy: &ActivePolicy, text: &str, now: Instant) -> MatchOutcome {
        let truncated = wincmd_clip_rules::truncate_for_match(text, MAX_CLIPBOARD_READ_BYTES);
        let Some(verdict) = policy.compiled.evaluate(truncated) else {
            return MatchOutcome::NoMatch;
        };
        let cooldown = policy.cooldown_for(&verdict.rule_id);
        match self
            .cooldown
            .should_emit(verdict.rule_id.clone(), now, cooldown)
        {
            Emit::Now => {
                let suppressed_since_last = self
                    .pending_suppressed
                    .remove(&verdict.rule_id)
                    .unwrap_or(0);
                MatchOutcome::Emit {
                    verdict,
                    suppressed_since_last,
                }
            }
            Emit::Suppressed { count } => {
                self.pending_suppressed
                    .insert(verdict.rule_id.clone(), count);
                MatchOutcome::Suppressed {
                    rule_id: verdict.rule_id,
                    count,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{ClipboardPolicyResponse, PolicyStore};
    use std::time::Duration;
    use wincmd_clip_rules::{Action, MatchKind, Rule, RuleId, Severity};

    #[test]
    fn no_match_returns_no_match() {
        let mut store = PolicyStore::new();
        let rule = Rule {
            id: RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").unwrap(),
            revision: 1,
            name: "aws key".to_string(),
            enabled: true,
            priority: 100,
            matcher: MatchKind::Phrase {
                value: "SECRET".to_string(),
                case_sensitive: true,
            },
            severity: Severity::Warn,
            actions: vec![Action::NotifyUser],
            cooldown_seconds: 60,
            snoozable: true,
            locked: false,
        };
        store
            .install(&ClipboardPolicyResponse {
                policy_version: 1,
                rules: vec![rule],
            })
            .unwrap();

        let mut engine = MatchEngine::new();
        let outcome = engine.observe(store.active(), "nothing interesting here", Instant::now());
        assert_eq!(outcome, MatchOutcome::NoMatch);
    }

    #[test]
    fn first_match_emits_with_zero_suppressed() {
        let mut store = PolicyStore::new();
        let rule = Rule {
            id: RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").unwrap(),
            revision: 1,
            name: "aws key".to_string(),
            enabled: true,
            priority: 100,
            matcher: MatchKind::Phrase {
                value: "SECRET".to_string(),
                case_sensitive: true,
            },
            severity: Severity::Warn,
            actions: vec![Action::NotifyUser],
            cooldown_seconds: 60,
            snoozable: true,
            locked: false,
        };
        store
            .install(&ClipboardPolicyResponse {
                policy_version: 1,
                rules: vec![rule],
            })
            .unwrap();

        let mut engine = MatchEngine::new();
        let outcome = engine.observe(store.active(), "my SECRET value", Instant::now());
        match outcome {
            MatchOutcome::Emit {
                suppressed_since_last,
                ..
            } => assert_eq!(suppressed_since_last, 0),
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn repeated_matches_inside_cooldown_produce_one_count_not_n_events() {
        let mut store = PolicyStore::new();
        let rule = Rule {
            id: RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").unwrap(),
            revision: 1,
            name: "aws key".to_string(),
            enabled: true,
            priority: 100,
            matcher: MatchKind::Phrase {
                value: "SECRET".to_string(),
                case_sensitive: true,
            },
            severity: Severity::Warn,
            actions: vec![Action::NotifyUser],
            cooldown_seconds: 60,
            snoozable: true,
            locked: false,
        };
        store
            .install(&ClipboardPolicyResponse {
                policy_version: 1,
                rules: vec![rule],
            })
            .unwrap();

        let mut engine = MatchEngine::new();
        let t0 = Instant::now();

        // First match: emits.
        let first = engine.observe(store.active(), "SECRET one", t0);
        assert!(matches!(first, MatchOutcome::Emit { .. }));

        // Three more matches within the 60s cooldown: every one of them
        // must be Suppressed (never a second Emit), with an increasing
        // count — this IS "a repeated match becomes a count, not N
        // events".
        let second = engine.observe(store.active(), "SECRET two", t0 + Duration::from_secs(5));
        assert_eq!(
            second,
            MatchOutcome::Suppressed {
                rule_id: RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").unwrap(),
                count: 1
            }
        );
        let third = engine.observe(store.active(), "SECRET three", t0 + Duration::from_secs(10));
        assert_eq!(
            third,
            MatchOutcome::Suppressed {
                rule_id: RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").unwrap(),
                count: 2
            }
        );
        let fourth = engine.observe(store.active(), "SECRET four", t0 + Duration::from_secs(15));
        assert_eq!(
            fourth,
            MatchOutcome::Suppressed {
                rule_id: RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").unwrap(),
                count: 3
            }
        );

        // Once the cooldown elapses, the NEXT real emission folds in the
        // suppressed count from the window that just ended (3), not a
        // fresh burst of separate events.
        let after_cooldown =
            engine.observe(store.active(), "SECRET five", t0 + Duration::from_secs(61));
        match after_cooldown {
            MatchOutcome::Emit {
                suppressed_since_last,
                ..
            } => assert_eq!(suppressed_since_last, 3),
            other => panic!("expected Emit with folded suppressed count, got {other:?}"),
        }
    }

    #[test]
    fn truncation_applies_before_matching_at_a_multibyte_boundary() {
        // A phrase rule whose needle sits just past a 1-byte cap lands
        // inside a multi-byte codepoint if truncation isn't UTF-8-boundary
        // safe. Use a tiny effective cap via a text engineered so the
        // match only exists AFTER the MAX_CLIPBOARD_READ_BYTES cut point —
        // this proves `observe()` truncates before evaluating, using the
        // exact same boundary-safe helper the crate ships
        // (`wincmd_clip_rules::truncate_for_match`), not a naive slice.
        let mut store = PolicyStore::new();
        let rule = Rule {
            id: RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").unwrap(),
            revision: 1,
            name: "far needle".to_string(),
            enabled: true,
            priority: 100,
            matcher: MatchKind::Phrase {
                value: "NEEDLE".to_string(),
                case_sensitive: true,
            },
            severity: Severity::Warn,
            actions: vec![Action::NotifyUser],
            cooldown_seconds: 60,
            snoozable: true,
            locked: false,
        };
        store
            .install(&ClipboardPolicyResponse {
                policy_version: 1,
                rules: vec![rule],
            })
            .unwrap();

        // Build text where "NEEDLE" only appears after byte
        // MAX_CLIPBOARD_READ_BYTES, using multi-byte 'é' filler so the cut
        // point is guaranteed to land mid-codepoint if truncation isn't
        // boundary-safe (2-byte char means an even cap can still split an
        // odd-aligned run — the point is this must not panic either way).
        let filler: String = "é".repeat(MAX_CLIPBOARD_READ_BYTES);
        let text = format!("{filler}NEEDLE");

        let mut engine = MatchEngine::new();
        let outcome = engine.observe(store.active(), &text, Instant::now());
        assert_eq!(
            outcome,
            MatchOutcome::NoMatch,
            "NEEDLE lands past the read cap and must not be seen"
        );
    }
}
