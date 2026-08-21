// SPDX-License-Identifier: AGPL-3.0-or-later
//! Match + cooldown engine: truncate, evaluate, and fold repeated matches
//! into a content-free suppressed count instead of an alert storm (plan
//! §4.2/§4.4: "then `CooldownLedger::should_emit` per rule so a repeated
//! match becomes a content-free `suppressed_count` instead of an alert
//! storm").

use std::collections::HashMap;
use std::time::Instant;

use wincmd_clip_rules::{Action, CooldownLedger, Emit, RuleId, Severity, Verdict};

use crate::policy::{ActivePolicy, PolicySource, PolicyStore};
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

/// An emitted verdict together with its policy source. The source is
/// required downstream so locally-authored matches stay local while Fleet
/// matches can use the existing content-free reporting path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcedVerdict {
    pub source: PolicySource,
    pub verdict: Verdict,
    pub suppressed_since_last: u32,
}

/// The protection decision across independently-evaluated local and Fleet
/// policies. `actions` is the de-duplicated union; `severity` is the maximum
/// severity. This makes a local rule additive only — it cannot reduce a
/// Fleet rule's protection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedVerdict {
    pub matches: Vec<SourcedVerdict>,
    pub severity: Severity,
    pub actions: Vec<Action>,
}

/// Outcome of source-aware evaluation. Suppression is retained per source so
/// two rules with the same id can never share a cooldown ledger (installs
/// reject collisions as a second defence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombinedMatchOutcome {
    NoMatch,
    Suppressed {
        source: PolicySource,
        rule_id: RuleId,
        count: u32,
    },
    Emit {
        verdict: CombinedVerdict,
    },
}

/// Owns the cooldown ledger and the "suppressed since last real emission"
/// bookkeeping. Stateful and per-process; a fresh `MatchEngine` treats
/// every rule as never having fired, matching `CooldownLedger`'s own
/// semantics.
#[derive(Default)]
pub struct MatchEngine {
    cooldown: CooldownLedger,
    pending_suppressed: HashMap<RuleId, u32>,
    local_cooldown: CooldownState,
    fleet_cooldown: CooldownState,
}

#[derive(Default)]
struct CooldownState {
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
        Self::observe_one(
            &mut self.cooldown,
            &mut self.pending_suppressed,
            policy,
            truncated,
            now,
        )
    }

    /// Evaluates the local and Fleet rule sets independently, then combines
    /// only the matches that are ready to emit. Callers can route each
    /// `SourcedVerdict` independently: local entries must never produce a
    /// Fleet report.
    pub fn observe_sources(
        &mut self,
        policies: &PolicyStore,
        text: &str,
        now: Instant,
    ) -> CombinedMatchOutcome {
        let truncated = wincmd_clip_rules::truncate_for_match(text, MAX_CLIPBOARD_READ_BYTES);
        let local = Self::observe_one(
            &mut self.local_cooldown.cooldown,
            &mut self.local_cooldown.pending_suppressed,
            policies.local(),
            truncated,
            now,
        );
        let fleet = Self::observe_one(
            &mut self.fleet_cooldown.cooldown,
            &mut self.fleet_cooldown.pending_suppressed,
            policies.fleet(),
            truncated,
            now,
        );

        let mut matches = Vec::new();
        let mut suppressed = None;
        for (source, outcome) in [(PolicySource::Local, local), (PolicySource::Fleet, fleet)] {
            match outcome {
                MatchOutcome::Emit {
                    verdict,
                    suppressed_since_last,
                } => matches.push(SourcedVerdict {
                    source,
                    verdict,
                    suppressed_since_last,
                }),
                MatchOutcome::Suppressed { rule_id, count } => {
                    suppressed.get_or_insert((source, rule_id, count));
                }
                MatchOutcome::NoMatch => {}
            }
        }
        if !matches.is_empty() {
            return CombinedMatchOutcome::Emit {
                verdict: combine(matches),
            };
        }
        match suppressed {
            Some((source, rule_id, count)) => CombinedMatchOutcome::Suppressed {
                source,
                rule_id,
                count,
            },
            None => CombinedMatchOutcome::NoMatch,
        }
    }

    fn observe_one(
        cooldown_ledger: &mut CooldownLedger,
        pending_suppressed: &mut HashMap<RuleId, u32>,
        policy: &ActivePolicy,
        text: &str,
        now: Instant,
    ) -> MatchOutcome {
        let Some(verdict) = policy.compiled.evaluate(text) else {
            return MatchOutcome::NoMatch;
        };
        let cooldown = policy.cooldown_for(&verdict.rule_id);
        match cooldown_ledger.should_emit(verdict.rule_id.clone(), now, cooldown) {
            Emit::Now => {
                let suppressed_since_last =
                    pending_suppressed.remove(&verdict.rule_id).unwrap_or(0);
                MatchOutcome::Emit {
                    verdict,
                    suppressed_since_last,
                }
            }
            Emit::Suppressed { count } => {
                pending_suppressed.insert(verdict.rule_id.clone(), count);
                MatchOutcome::Suppressed {
                    rule_id: verdict.rule_id,
                    count,
                }
            }
        }
    }
}

fn combine(matches: Vec<SourcedVerdict>) -> CombinedVerdict {
    let severity = matches
        .iter()
        .map(|matched| matched.verdict.severity)
        .max_by_key(|severity| severity_rank(*severity))
        .expect("combine is only called with at least one match");
    let mut actions = Vec::new();
    for matched in &matches {
        for action in &matched.verdict.actions {
            if !actions.contains(action) {
                actions.push(*action);
            }
        }
    }
    CombinedVerdict {
        matches,
        severity,
        actions,
    }
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Warn => 1,
        Severity::High => 2,
        Severity::Critical => 3,
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

    #[test]
    fn local_and_fleet_matches_union_actions_and_keep_fleet_severity() {
        let mut store = PolicyStore::new();
        let local = Rule {
            id: RuleId::new("11111111111111111111111111111111").unwrap(),
            revision: 1,
            name: "local secret".to_string(),
            enabled: true,
            priority: 1,
            matcher: MatchKind::Phrase {
                value: "SECRET".to_string(),
                case_sensitive: true,
            },
            severity: Severity::Warn,
            actions: vec![Action::NotifyUser, Action::ClearClipboard],
            cooldown_seconds: 60,
            snoozable: true,
            locked: false,
        };
        let fleet = Rule {
            id: RuleId::new("22222222222222222222222222222222").unwrap(),
            revision: 4,
            name: "fleet secret".to_string(),
            enabled: true,
            priority: 1,
            matcher: MatchKind::Phrase {
                value: "SECRET".to_string(),
                case_sensitive: true,
            },
            severity: Severity::Critical,
            actions: vec![Action::QuarantineClipboard, Action::ReportFleet],
            cooldown_seconds: 60,
            snoozable: false,
            locked: true,
        };
        store
            .install_local(&ClipboardPolicyResponse {
                policy_version: 1,
                rules: vec![local],
            })
            .unwrap();
        store
            .install_fleet(&ClipboardPolicyResponse {
                policy_version: 9,
                rules: vec![fleet],
            })
            .unwrap();

        let outcome = MatchEngine::new().observe_sources(&store, "SECRET", Instant::now());
        let CombinedMatchOutcome::Emit { verdict } = outcome else {
            panic!("both sources must protect a shared match");
        };
        assert_eq!(verdict.severity, Severity::Critical);
        assert_eq!(verdict.matches.len(), 2);
        assert_eq!(
            verdict.actions,
            vec![
                Action::NotifyUser,
                Action::ClearClipboard,
                Action::QuarantineClipboard,
                Action::ReportFleet,
            ]
        );
        assert_eq!(verdict.matches[0].source, PolicySource::Local);
        assert_eq!(verdict.matches[1].source, PolicySource::Fleet);
    }

    #[test]
    fn source_cooldowns_are_independent() {
        let mut store = PolicyStore::new();
        let local = Rule {
            id: RuleId::new("11111111111111111111111111111111").unwrap(),
            revision: 1,
            name: "local".to_string(),
            enabled: true,
            priority: 1,
            matcher: MatchKind::Phrase {
                value: "LOCAL".to_string(),
                case_sensitive: true,
            },
            severity: Severity::Warn,
            actions: vec![Action::NotifyUser],
            cooldown_seconds: 60,
            snoozable: true,
            locked: false,
        };
        let mut fleet = local.clone();
        fleet.id = RuleId::new("22222222222222222222222222222222").unwrap();
        fleet.matcher = MatchKind::Phrase {
            value: "FLEET".to_string(),
            case_sensitive: true,
        };
        fleet.actions = vec![Action::ReportFleet];
        fleet.severity = Severity::High;
        store
            .install_local(&ClipboardPolicyResponse {
                policy_version: 1,
                rules: vec![local],
            })
            .unwrap();
        store
            .install_fleet(&ClipboardPolicyResponse {
                policy_version: 1,
                rules: vec![fleet],
            })
            .unwrap();

        let mut engine = MatchEngine::new();
        let now = Instant::now();
        assert!(matches!(
            engine.observe_sources(&store, "LOCAL", now),
            CombinedMatchOutcome::Emit { .. }
        ));
        assert!(matches!(
            engine.observe_sources(&store, "FLEET", now + Duration::from_secs(1)),
            CombinedMatchOutcome::Emit { .. }
        ));
        assert!(matches!(
            engine.observe_sources(&store, "LOCAL", now + Duration::from_secs(2)),
            CombinedMatchOutcome::Suppressed {
                source: PolicySource::Local,
                ..
            }
        ));
    }
}
