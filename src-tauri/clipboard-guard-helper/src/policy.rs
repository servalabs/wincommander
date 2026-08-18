// SPDX-License-Identifier: AGPL-3.0-or-later
//! The active, compiled clipboard-guard ruleset, and the atomic-install
//! discipline around swapping it in.
//!
//! Policy arrives from `commander-svc` over `svc.clipboard.get_policy`
//! (see `ipc`) as a small, versioned bundle of raw `wincmd_clip_rules::Rule`
//! values. This module owns exactly one thing: turning that bundle into a
//! ready-to-match [`ActivePolicy`] via `wincmd_clip_rules::compile()`, and
//! never half-applying a bad one.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use wincmd_clip_rules::{compile, CompileError, CompiledRuleSet, Rule, RuleId, RuleSetLimits};

/// Wire shape of a successful `svc.clipboard.get_policy` response.
///
/// This is a **local UI/helper ↔ `commander-svc` contract**, not the Fleet
/// wire — `commander-svc` resolves/verifies the signed epoch itself and
/// exposes only the resolved ruleset over the pipe, so this type
/// deliberately carries no signature or epoch metadata. `limits` is NOT
/// part of the wire: this crate always enforces its own
/// `RuleSetLimits::default()` regardless of what the server validated
/// against, so a compromised or out-of-date svc response can't loosen the
/// on-device resource bound (see `PolicyStore::install`).
///
/// NOTE for the agent that wires `commander-svc/src/pipe.rs`'s
/// `svc.clipboard.get_policy` handler (out of this crate's file ownership
/// — see the handoff note): the `Response.result` JSON must deserialize
/// into exactly this shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardPolicyResponse {
    pub policy_version: i64,
    pub rules: Vec<Rule>,
}

/// A successfully compiled, ready-to-match ruleset plus the per-rule
/// cooldown durations `engine::MatchEngine` needs. `wincmd_clip_rules::
/// Verdict` deliberately carries no `cooldown_seconds` (content-free
/// design — see that crate's doc), so the cooldown has to be looked up
/// from the original `Rule` list at policy-install time, not recovered
/// from a match result.
pub struct ActivePolicy {
    pub policy_version: i64,
    pub compiled: CompiledRuleSet,
    cooldowns: HashMap<RuleId, Duration>,
}

impl ActivePolicy {
    /// The empty, always-compilable starting policy — the honest default
    /// before the first successful `get_policy` round trip, and the value
    /// `PolicyStore` retains if compilation never succeeds even once.
    fn empty() -> Self {
        let compiled = compile(&[], &RuleSetLimits::default())
            .expect("an empty ruleset has nothing to fail to compile");
        Self {
            policy_version: 0,
            compiled,
            cooldowns: HashMap::new(),
        }
    }

    /// Cooldown for `rule_id`, or `Duration::ZERO` if unknown (matched a
    /// rule this policy doesn't recognise — defensive only; `evaluate()`
    /// can't return a `Verdict` for a rule that isn't in `compiled`, so
    /// this branch is unreachable in practice, but zero-cooldown is the
    /// safe fallback rather than panicking).
    pub fn cooldown_for(&self, rule_id: &RuleId) -> Duration {
        self.cooldowns
            .get(rule_id)
            .copied()
            .unwrap_or(Duration::ZERO)
    }
}

/// Holds the currently active policy plus the "did the last install
/// actually compile" health bit (plan §8.1's `clipboard_guard.rules_compiled`
/// capability-status key). Atomic install with last-valid retention: a
/// bad ruleset from svc leaves the previous good one live and flips
/// `rules_compiled` false — it never half-applies (plan §4.4).
pub struct PolicyStore {
    active: ActivePolicy,
    rules_compiled: bool,
}

impl Default for PolicyStore {
    fn default() -> Self {
        Self {
            active: ActivePolicy::empty(),
            rules_compiled: true,
        }
    }
}

impl PolicyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attempt to install `response` as the active policy. On success,
    /// swaps `active` and sets `rules_compiled` true. On failure, `active`
    /// is left completely untouched (the old ruleset — possibly still the
    /// empty starting one — keeps matching) and `rules_compiled` is set
    /// false so the health surface can report it honestly. Always enforces
    /// this crate's own `RuleSetLimits::default()`, independent of
    /// whatever `commander-svc`/Fleet validated against.
    pub fn install(&mut self, response: &ClipboardPolicyResponse) -> Result<(), Vec<CompileError>> {
        let limits = RuleSetLimits::default();
        match compile(&response.rules, &limits) {
            Ok(compiled) => {
                let cooldowns = response
                    .rules
                    .iter()
                    .filter(|r| r.enabled)
                    .map(|r| (r.id.clone(), Duration::from_secs(r.cooldown_seconds as u64)))
                    .collect();
                self.active = ActivePolicy {
                    policy_version: response.policy_version,
                    compiled,
                    cooldowns,
                };
                self.rules_compiled = true;
                Ok(())
            }
            Err(errors) => {
                self.rules_compiled = false;
                Err(errors)
            }
        }
    }

    pub fn active(&self) -> &ActivePolicy {
        &self.active
    }

    pub fn rules_compiled(&self) -> bool {
        self.rules_compiled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wincmd_clip_rules::{Action, MatchKind, Severity};

    fn rule_id(s: &str) -> RuleId {
        RuleId::new(s).unwrap()
    }

    fn good_rule() -> Rule {
        Rule {
            id: rule_id("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b"),
            revision: 1,
            name: "test rule".to_string(),
            enabled: true,
            priority: 100,
            matcher: MatchKind::Phrase {
                value: "SECRET".to_string(),
                case_sensitive: true,
            },
            severity: Severity::Warn,
            actions: vec![Action::NotifyUser],
            cooldown_seconds: 30,
            snoozable: true,
            locked: false,
        }
    }

    fn bad_rule() -> Rule {
        // Lookaround is rejected by wincmd_clip_rules::compile — a clean,
        // deterministic way to force a CompileError without depending on
        // limit-boundary details.
        Rule {
            id: rule_id("1a2b3c4d5e6f7a8b9c0d1e2f3a4b0e8f"),
            revision: 1,
            name: "bad rule".to_string(),
            enabled: true,
            priority: 100,
            matcher: MatchKind::Regex {
                pattern: r"(?=bad)".to_string(),
                case_sensitive: true,
            },
            severity: Severity::Warn,
            actions: vec![Action::NotifyUser],
            cooldown_seconds: 30,
            snoozable: true,
            locked: false,
        }
    }

    #[test]
    fn starts_with_an_empty_but_valid_policy() {
        let store = PolicyStore::new();
        assert!(store.rules_compiled());
        assert_eq!(store.active().policy_version, 0);
        assert!(store.active().compiled.evaluate("anything").is_none());
    }

    #[test]
    fn good_policy_installs_and_flips_health_true() {
        let mut store = PolicyStore::new();
        let response = ClipboardPolicyResponse {
            policy_version: 5,
            rules: vec![good_rule()],
        };
        assert!(store.install(&response).is_ok());
        assert!(store.rules_compiled());
        assert_eq!(store.active().policy_version, 5);
        assert!(store
            .active()
            .compiled
            .evaluate("my SECRET value")
            .is_some());
    }

    #[test]
    fn bad_policy_leaves_previous_active_untouched_and_flips_health_false() {
        let mut store = PolicyStore::new();
        let good = ClipboardPolicyResponse {
            policy_version: 5,
            rules: vec![good_rule()],
        };
        store.install(&good).expect("good policy installs");

        let bad = ClipboardPolicyResponse {
            policy_version: 6,
            rules: vec![bad_rule()],
        };
        let result = store.install(&bad);
        assert!(result.is_err());
        assert!(!store.rules_compiled());
        // The OLD policy (version 5, still matching SECRET) must still be
        // live — never half-applied.
        assert_eq!(store.active().policy_version, 5);
        assert!(store
            .active()
            .compiled
            .evaluate("my SECRET value")
            .is_some());
    }

    #[test]
    fn cooldown_for_unknown_rule_id_is_zero() {
        let store = PolicyStore::new();
        let unknown = RuleId::new("ffffffffffffffffffffffffffffffff").unwrap();
        assert_eq!(store.active().cooldown_for(&unknown), Duration::ZERO);
    }

    #[test]
    fn cooldown_for_installed_rule_matches_its_declared_seconds() {
        let mut store = PolicyStore::new();
        let response = ClipboardPolicyResponse {
            policy_version: 1,
            rules: vec![good_rule()],
        };
        store.install(&response).unwrap();
        let id = rule_id("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b");
        assert_eq!(store.active().cooldown_for(&id), Duration::from_secs(30));
    }
}
