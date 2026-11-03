// SPDX-License-Identifier: AGPL-3.0-or-later
use clipboard_guard_helper::{
    actions::{ActionOutcome, ClipboardWriter},
    policy::PolicySource,
};
use wincmd_clip_rules::{Action, Finding, LocalSpans};

pub(super) fn apply(
    findings: &[(PolicySource, Finding)],
    text: &str,
    spans: &LocalSpans,
    writer: &mut dyn ClipboardWriter,
    paid: bool,
) -> (ActionOutcome, Option<[u8; 32]>) {
    let mut outcome = ActionOutcome::new();
    if !paid {
        return (outcome, None);
    }
    // Cooldowns silence notifications, never protection of a newly copied value.
    let actions: Vec<_> = findings
        .iter()
        .flat_map(|(_, finding)| finding.actions.iter())
        .collect();
    let selected = if actions.contains(&&Action::ClearClipboard) {
        Some(Action::ClearClipboard)
    } else if actions.contains(&&Action::QuarantineClipboard) {
        Some(Action::QuarantineClipboard)
    } else if actions.contains(&&Action::MaskMatches) {
        Some(Action::MaskMatches)
    } else {
        None
    };
    let Some(action) = selected else {
        return (outcome, None);
    };
    let replacement = match action {
        Action::ClearClipboard => String::new(),
        Action::QuarantineClipboard => "[Clipboard Guard quarantined this content]".to_owned(),
        Action::MaskMatches => {
            let ids: Vec<_> = findings
                .iter()
                .filter(|(_, finding)| finding.actions.contains(&Action::MaskMatches))
                .map(|(_, finding)| finding.rule_id.clone())
                .collect();
            wincmd_clip_rules::mask_matches_for_rules(text, spans, &ids)
        }
        _ => return (outcome, None),
    };
    outcome.attempted.push(action);
    if writer.replace_if_unchanged(text, &replacement) {
        outcome.succeeded.push(action);
        return (outcome, Some(super::hash_text(&replacement)));
    }
    (outcome, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipboard_guard_helper::{
        engine::{CombinedMatchOutcome, MatchEngine},
        policy::{ClipboardPolicyResponse, PolicyStore},
    };
    use std::time::Instant;
    struct Writer(String);
    impl ClipboardWriter for Writer {
        fn clear(&mut self) -> bool {
            panic!("must compare before clearing")
        }
        fn quarantine(&mut self, _: &str) -> bool {
            panic!("must compare before replacing")
        }
        fn replace_if_unchanged(&mut self, expected: &str, replacement: &str) -> bool {
            if self.0 != expected {
                return false;
            }
            self.0 = replacement.into();
            true
        }
    }
    fn policy(action: Action) -> PolicyStore {
        let rule = serde_json::from_value(serde_json::json!({
            "id":"a0000000000000000000000000000001","revision":1,"name":"Synthetic",
            "enabled":true,"priority":100,"matcher":{"kind":"regex","params":{"pattern":"SYNTHETIC_[A-Z]+","case_sensitive":true}},
            "severity":"warn","actions":[action],"cooldownSeconds":30,"snoozable":true,"locked":false
        })).unwrap();
        let mut policy = PolicyStore::new();
        policy
            .install_local(&ClipboardPolicyResponse {
                reporting_binding: None,
                policy_version: 0,
                rules: vec![rule],
                content_rules: None,
            })
            .unwrap();
        policy
    }
    #[test]
    fn a_second_value_is_protected_while_notifications_are_cooled_down() {
        for action in [
            Action::MaskMatches,
            Action::ClearClipboard,
            Action::QuarantineClipboard,
        ] {
            let policies = policy(action);
            let mut engine = MatchEngine::new();
            let now = Instant::now();
            let first = engine.observe_content_sources(&policies, "SYNTHETIC_ALPHA", now);
            assert!(matches!(first.outcome, CombinedMatchOutcome::Emit { .. }));
            let second = engine.observe_content_sources(&policies, "SYNTHETIC_BRAVO", now);
            assert!(matches!(
                second.outcome,
                CombinedMatchOutcome::Suppressed { .. }
            ));
            let mut writer = Writer("SYNTHETIC_BRAVO".into());
            let (result, replacement_hash) = apply(
                &second.findings,
                "SYNTHETIC_BRAVO",
                &second.local_spans,
                &mut writer,
                true,
            );
            assert_eq!(result.succeeded, vec![action]);
            assert!(!writer.0.contains("SYNTHETIC_BRAVO"));
            assert_eq!(replacement_hash, Some(super::super::hash_text(&writer.0)));
        }
    }
    #[test]
    fn paid_authority_and_atomic_comparison_are_required_for_every_mutation() {
        let policies = policy(Action::ClearClipboard);
        let scan = MatchEngine::new().observe_content_sources(
            &policies,
            "SYNTHETIC_ALPHA",
            Instant::now(),
        );
        let mut writer = Writer("newer legitimate clipboard".into());
        assert!(apply(
            &scan.findings,
            "SYNTHETIC_ALPHA",
            &scan.local_spans,
            &mut writer,
            true
        )
        .0
        .succeeded
        .is_empty());
        assert_eq!(writer.0, "newer legitimate clipboard");
        assert!(apply(
            &scan.findings,
            "SYNTHETIC_ALPHA",
            &scan.local_spans,
            &mut writer,
            false
        )
        .0
        .attempted
        .is_empty());
    }
}
