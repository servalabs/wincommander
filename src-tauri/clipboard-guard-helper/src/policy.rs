// SPDX-License-Identifier: AGPL-3.0-or-later
//! The active, compiled clipboard-guard ruleset, and the atomic-install
//! discipline around swapping it in.
//!
//! Policy arrives from `commander-svc` over `svc.clipboard.get_policy`
//! (see `ipc`) as a small, versioned bundle of raw `wincmd_clip_rules::Rule`
//! values. This module owns exactly one thing: turning that bundle into a
//! ready-to-match [`ActivePolicy`] via `wincmd_clip_rules::compile()`, and
//! never half-applying a bad one.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use wincmd_clip_rules::{
    compile, Action, CompileError, CompiledRuleSet, Rule, RuleId, RuleSetLimits,
};

/// Where a policy rule came from. Source stays attached to the rule's
/// execution path: a local rule never becomes an organisation report merely
/// because Fleet is also configured on the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicySource {
    Local,
    Fleet,
}

/// An install was refused without changing the last valid policy for either
/// source. The variants carry no matcher text or clipboard content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyInstallError {
    Compile(Vec<CompileError>),
    RuleIdCollision { rule_id: RuleId },
    LocalActionNotAllowed { rule_id: RuleId, action: Action },
}

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
    // Disabled rules still reserve their ID across policy sources.
    rule_ids: HashSet<RuleId>,
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
            rule_ids: HashSet::new(),
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
    local: ActivePolicy,
    fleet: ActivePolicy,
    local_rules_compiled: bool,
    fleet_rules_compiled: bool,
}

impl Default for PolicyStore {
    fn default() -> Self {
        Self {
            local: ActivePolicy::empty(),
            fleet: ActivePolicy::empty(),
            local_rules_compiled: true,
            fleet_rules_compiled: true,
        }
    }
}

impl PolicyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Legacy Fleet-only install API. It preserves the existing external
    /// result shape; source-aware callers must use [`Self::install_fleet`].
    /// A legacy caller must not install while local rules exist because it
    /// cannot represent a collision failure.
    pub fn install(&mut self, response: &ClipboardPolicyResponse) -> Result<(), Vec<CompileError>> {
        if self.has_rule_id_collision(&response.rules, &self.local) {
            self.fleet_rules_compiled = false;
            return Err(Vec::new());
        }
        self.install_compiled(response, PolicySource::Fleet)
            .map_err(|error| match error {
                PolicyInstallError::Compile(errors) => errors,
                PolicyInstallError::RuleIdCollision { .. }
                | PolicyInstallError::LocalActionNotAllowed { .. } => Vec::new(),
            })
    }

    /// Atomically installs a Fleet policy. A bad update leaves the last good
    /// Fleet policy active; local rules are independent and remain active.
    pub fn install_fleet(
        &mut self,
        response: &ClipboardPolicyResponse,
    ) -> Result<(), PolicyInstallError> {
        self.install_compiled(response, PolicySource::Fleet)
    }

    /// Atomically installs locally-owned rules. Local rules cannot request
    /// Fleet reporting or administrator paging, preventing local clipboard
    /// text/tests from creating an organisation-facing side channel.
    pub fn install_local(
        &mut self,
        response: &ClipboardPolicyResponse,
    ) -> Result<(), PolicyInstallError> {
        for rule in &response.rules {
            for action in &rule.actions {
                if matches!(action, Action::ReportFleet | Action::AlertAdmin) {
                    self.local_rules_compiled = false;
                    return Err(PolicyInstallError::LocalActionNotAllowed {
                        rule_id: rule.id.clone(),
                        action: *action,
                    });
                }
            }
        }
        self.install_compiled(response, PolicySource::Local)
    }

    /// Explicit unenrolment removes only the cached Fleet source. Personal
    /// rules stay in place, so disconnecting an organisation never weakens a
    /// user's local protection.
    pub fn clear_fleet_on_unenroll(&mut self) {
        self.fleet = ActivePolicy::empty();
        self.fleet_rules_compiled = true;
    }

    fn install_compiled(
        &mut self,
        response: &ClipboardPolicyResponse,
        source: PolicySource,
    ) -> Result<(), PolicyInstallError> {
        let other = match source {
            PolicySource::Local => &self.fleet,
            PolicySource::Fleet => &self.local,
        };
        if let Some(rule_id) = self.first_rule_id_collision(&response.rules, other) {
            self.set_compiled_health(source, false);
            return Err(PolicyInstallError::RuleIdCollision { rule_id });
        }
        let limits = RuleSetLimits::default();
        match compile(&response.rules, &limits) {
            Ok(compiled) => {
                let cooldowns = response
                    .rules
                    .iter()
                    .filter(|r| r.enabled)
                    .map(|r| (r.id.clone(), Duration::from_secs(r.cooldown_seconds as u64)))
                    .collect();
                let active = ActivePolicy {
                    policy_version: response.policy_version,
                    compiled,
                    cooldowns,
                    rule_ids: response.rules.iter().map(|rule| rule.id.clone()).collect(),
                };
                match source {
                    PolicySource::Local => self.local = active,
                    PolicySource::Fleet => self.fleet = active,
                }
                self.set_compiled_health(source, true);
                Ok(())
            }
            Err(errors) => {
                self.set_compiled_health(source, false);
                Err(PolicyInstallError::Compile(errors))
            }
        }
    }

    fn set_compiled_health(&mut self, source: PolicySource, compiled: bool) {
        match source {
            PolicySource::Local => self.local_rules_compiled = compiled,
            PolicySource::Fleet => self.fleet_rules_compiled = compiled,
        }
    }

    fn has_rule_id_collision(&self, rules: &[Rule], other: &ActivePolicy) -> bool {
        self.first_rule_id_collision(rules, other).is_some()
    }

    fn first_rule_id_collision(&self, rules: &[Rule], other: &ActivePolicy) -> Option<RuleId> {
        rules
            .iter()
            .find_map(|rule| other.rule_ids.contains(&rule.id).then(|| rule.id.clone()))
    }

    /// Legacy alias for the Fleet policy, kept for existing helper callers.
    pub fn active(&self) -> &ActivePolicy {
        self.fleet()
    }

    pub fn local(&self) -> &ActivePolicy {
        &self.local
    }

    pub fn fleet(&self) -> &ActivePolicy {
        &self.fleet
    }

    /// Legacy Fleet health alias, kept for existing helper callers.
    pub fn rules_compiled(&self) -> bool {
        self.fleet_rules_compiled
    }

    pub fn rules_compiled_for(&self, source: PolicySource) -> bool {
        match source {
            PolicySource::Local => self.local_rules_compiled,
            PolicySource::Fleet => self.fleet_rules_compiled,
        }
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
    fn fleet_install_failure_keeps_last_good_fleet_and_local_policies() {
        let mut store = PolicyStore::new();
        let local = ClipboardPolicyResponse {
            policy_version: 2,
            rules: vec![good_rule()],
        };
        store.install_local(&local).unwrap();

        let mut fleet_rule = good_rule();
        fleet_rule.id = rule_id("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let fleet = ClipboardPolicyResponse {
            policy_version: 3,
            rules: vec![fleet_rule],
        };
        store.install_fleet(&fleet).unwrap();

        let failure = ClipboardPolicyResponse {
            policy_version: 4,
            rules: vec![bad_rule()],
        };
        assert!(matches!(
            store.install_fleet(&failure),
            Err(PolicyInstallError::Compile(_))
        ));
        assert_eq!(store.local().policy_version, 2);
        assert_eq!(store.fleet().policy_version, 3);
        assert!(!store.rules_compiled_for(PolicySource::Fleet));
        assert!(store.rules_compiled_for(PolicySource::Local));
    }

    #[test]
    fn rejects_rule_id_collisions_between_sources_without_replacing_either() {
        let mut store = PolicyStore::new();
        let local = ClipboardPolicyResponse {
            policy_version: 1,
            rules: vec![good_rule()],
        };
        store.install_local(&local).unwrap();

        let collision = ClipboardPolicyResponse {
            policy_version: 9,
            rules: vec![good_rule()],
        };
        assert!(matches!(
            store.install_fleet(&collision),
            Err(PolicyInstallError::RuleIdCollision { .. })
        ));
        assert_eq!(store.local().policy_version, 1);
        assert_eq!(store.fleet().policy_version, 0);
    }

    #[test]
    fn rejects_cross_source_collisions_when_either_rule_is_disabled() {
        let mut disabled_local = good_rule();
        disabled_local.enabled = false;
        let mut store = PolicyStore::new();
        store
            .install_local(&ClipboardPolicyResponse {
                policy_version: 1,
                rules: vec![disabled_local],
            })
            .unwrap();
        assert!(matches!(
            store.install_fleet(&ClipboardPolicyResponse {
                policy_version: 2,
                rules: vec![good_rule()],
            }),
            Err(PolicyInstallError::RuleIdCollision { .. })
        ));

        let mut disabled_fleet = good_rule();
        disabled_fleet.enabled = false;
        let mut store = PolicyStore::new();
        store
            .install_local(&ClipboardPolicyResponse {
                policy_version: 1,
                rules: vec![good_rule()],
            })
            .unwrap();
        assert!(matches!(
            store.install_fleet(&ClipboardPolicyResponse {
                policy_version: 2,
                rules: vec![disabled_fleet],
            }),
            Err(PolicyInstallError::RuleIdCollision { .. })
        ));
    }

    #[test]
    fn local_rules_cannot_request_organisation_actions() {
        let mut store = PolicyStore::new();
        let mut rule = good_rule();
        rule.actions = vec![Action::NotifyUser, Action::ReportFleet];
        let response = ClipboardPolicyResponse {
            policy_version: 1,
            rules: vec![rule],
        };
        assert!(matches!(
            store.install_local(&response),
            Err(PolicyInstallError::LocalActionNotAllowed {
                action: Action::ReportFleet,
                ..
            })
        ));
        assert_eq!(store.local().policy_version, 0);
        assert!(!store.rules_compiled_for(PolicySource::Local));
    }

    #[test]
    fn explicit_unenrol_removes_fleet_only() {
        let mut store = PolicyStore::new();
        let local = ClipboardPolicyResponse {
            policy_version: 2,
            rules: vec![good_rule()],
        };
        store.install_local(&local).unwrap();
        let mut fleet_rule = good_rule();
        fleet_rule.id = rule_id("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        store
            .install_fleet(&ClipboardPolicyResponse {
                policy_version: 3,
                rules: vec![fleet_rule],
            })
            .unwrap();

        store.clear_fleet_on_unenroll();
        assert_eq!(store.local().policy_version, 2);
        assert_eq!(store.fleet().policy_version, 0);
        assert!(store.local().compiled.evaluate("SECRET").is_some());
        assert!(store.fleet().compiled.evaluate("SECRET").is_none());
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
