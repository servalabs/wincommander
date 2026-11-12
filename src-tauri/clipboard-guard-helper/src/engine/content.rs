// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use wincmd_clip_rules::{Channel, Finding, LocalSpans};

/// Findings are safe for receipts; spans stay in the caller's process.
pub struct ContentObservation {
    pub outcome: CombinedMatchOutcome,
    pub findings: Vec<(PolicySource, Finding)>,
    pub local_spans: LocalSpans,
    pub truncated: bool,
}

impl MatchEngine {
    pub fn observe_content_sources(
        &mut self,
        policies: &PolicyStore,
        text: &str,
        now: Instant,
    ) -> ContentObservation {
        let mut findings = Vec::new();
        let mut spans = LocalSpans::default();
        let mut truncated = false;
        let mut matches = Vec::new();
        let mut suppressed = None;
        for (source, policy, state) in [
            (
                PolicySource::Local,
                policies.local(),
                &mut self.local_cooldown,
            ),
            (
                PolicySource::Fleet,
                policies.fleet(),
                &mut self.fleet_cooldown,
            ),
        ] {
            let report = policy.compiled.scan(text, Channel::Clipboard);
            truncated |= report.truncated;
            spans.extend(report.local_spans);
            for finding in report.findings {
                let rule_id = finding.rule_id.clone();
                let verdict = Verdict {
                    rule_id: rule_id.clone(),
                    rule_revision: finding.rule_revision,
                    severity: finding.severity,
                    actions: finding.actions.clone(),
                };
                match state.cooldown.should_emit(
                    rule_id.clone(),
                    now,
                    policy.cooldown_for(&rule_id),
                ) {
                    Emit::Now => {
                        matches.push(SourcedVerdict {
                            source,
                            verdict,
                            suppressed_since_last: state
                                .pending_suppressed
                                .remove(&rule_id)
                                .unwrap_or(0),
                        });
                    }
                    Emit::Suppressed { count } => {
                        state.pending_suppressed.insert(rule_id.clone(), count);
                        suppressed.get_or_insert((source, rule_id, count));
                    }
                }
                findings.push((source, finding));
            }
        }
        let outcome = if !matches.is_empty() {
            CombinedMatchOutcome::Emit {
                verdict: combine(matches),
            }
        } else if let Some((source, rule_id, count)) = suppressed {
            CombinedMatchOutcome::Suppressed {
                source,
                rule_id,
                count,
            }
        } else {
            CombinedMatchOutcome::NoMatch
        };
        ContentObservation {
            outcome,
            findings,
            local_spans: spans,
            truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ClipboardPolicyResponse;
    use wincmd_clip_rules::{MatchKind, Rule, RuleOptions};

    fn rule(id: &str, phrase: &str) -> Rule {
        Rule {
            id: RuleId::new(id).unwrap(),
            revision: 1,
            name: "Synthetic".into(),
            enabled: true,
            priority: 100,
            matcher: MatchKind::Phrase {
                value: phrase.into(),
                case_sensitive: true,
            },
            severity: Severity::Warn,
            actions: vec![Action::MaskMatches],
            cooldown_seconds: 30,
            snoozable: true,
            locked: false,
            options: RuleOptions::default(),
        }
    }

    #[test]
    fn scans_every_matching_rule_and_keeps_source_cooldowns() {
        let first = rule("a0000000000000000000000000000001", "SYNTHETIC_ALPHA");
        let second = rule("a0000000000000000000000000000002", "SYNTHETIC_BRAVO");
        let mut policies = PolicyStore::new();
        policies
            .install_local(&ClipboardPolicyResponse { reporting_binding:None,
                content_rules: None,
                policy_version: 0,
                rules: vec![first, second],
            })
            .unwrap();
        let mut engine = MatchEngine::new();
        let now = Instant::now();
        let result =
            engine.observe_content_sources(&policies, "SYNTHETIC_ALPHA SYNTHETIC_BRAVO", now);
        assert_eq!(result.findings.len(), 2);
        let CombinedMatchOutcome::Emit { verdict } = result.outcome else {
            panic!("expected emitted detections")
        };
        assert_eq!(verdict.matches.len(), 2);
        assert!(matches!(
            engine
                .observe_content_sources(&policies, "SYNTHETIC_ALPHA SYNTHETIC_BRAVO", now)
                .outcome,
            CombinedMatchOutcome::Suppressed { .. }
        ));
    }

    #[test]
    fn findings_and_masked_text_never_contain_matched_values() {
        const SENTINEL: &str = "SYNTHETIC_VALUE_NOT_FOR_REPORTING";
        let selected = rule("a0000000000000000000000000000001", SENTINEL);
        let mut policies = PolicyStore::new();
        policies
            .install_local(&ClipboardPolicyResponse { reporting_binding:None,
                content_rules: None,
                policy_version: 0,
                rules: vec![selected.clone()],
            })
            .unwrap();
        let result =
            MatchEngine::new().observe_content_sources(&policies, SENTINEL, Instant::now());
        let metadata = serde_json::to_string(
            &result
                .findings
                .iter()
                .map(|(_, finding)| finding)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(!metadata.contains(SENTINEL));
        assert!(!wincmd_clip_rules::mask_matches_for_rules(
            SENTINEL,
            &result.local_spans,
            &[selected.id]
        )
        .contains(SENTINEL));
        assert!(!format!("{:?}", result.local_spans).contains(SENTINEL));
    }

    #[test]
    fn compact_managed_subtree_preserves_global_allowlist_without_expanding_the_wire() {
        let mut subtree = wincmd_clip_rules::ContentRulesSubtree::default();
        subtree.tuning.allow_list = (0..60)
            .map(|index| format!("{index:04}{}", "x".repeat(1996)))
            .collect();
        subtree.tuning.allow_list.push("4111 1111 1111 1111".into());
        let response = ClipboardPolicyResponse { reporting_binding:None,
            policy_version: 1,
            rules: vec![],
            content_rules: Some(subtree),
        };
        assert!(serde_json::to_vec(&response).unwrap().len() < 256 * 1024);
        let mut policies = PolicyStore::new();
        policies.install_fleet(&response).unwrap();
        let scan = policies.fleet().compiled.scan(
            "credit card number: 4111 1111 1111 1111",
            Channel::Clipboard,
        );
        assert!(!scan
            .findings
            .iter()
            .any(|finding| finding.category == wincmd_clip_rules::DetectorCategory::PaymentCard));
    }

    #[test]
    fn local_policy_cannot_supply_a_managed_content_subtree() {
        let response = ClipboardPolicyResponse { reporting_binding:None,
            policy_version: 0,
            rules: vec![],
            content_rules: Some(Default::default()),
        };
        assert!(PolicyStore::new().install_local(&response).is_err());
    }
}
