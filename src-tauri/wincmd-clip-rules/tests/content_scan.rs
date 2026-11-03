// SPDX-License-Identifier: AGPL-3.0-or-later
mod common;
use wincmd_clip_rules::*;

#[test]
fn curated_catalog_compiles_with_the_eight_mib_budget() {
    let catalog = detector_catalog();
    assert_eq!(catalog.len(), 251);
    assert!(
        catalog
            .iter()
            .filter(|p| p.id.starts_with("gitleaks."))
            .count()
            >= 200
    );
    let compiled = compile_content_rules(&ContentRulesSubtree::default()).unwrap();
    assert!(
        !compiled
            .scan("plain prose without sensitive material", Channel::Clipboard)
            .truncated
    );
}

#[test]
fn reports_all_counts_without_returning_text_spans_or_rule_names() {
    let marker = "SENTINEL_private_value_for_reverse_leak";
    let mut rule = common::phrase_rule(1, 100, marker);
    rule.name = marker.into();
    let compiled = compile(&[rule], &RuleSetLimits::default()).unwrap();
    let report = compiled.scan(&format!("{marker} {marker}"), Channel::Clipboard);
    assert_eq!(report.findings[0].count, 2);
    assert_eq!(report.local_spans.len(), 2);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains(marker));
    assert!(!json.contains("localSpans"));
    assert!(!format!("{report:?}").contains(marker));
    assert!(!format!("{compiled:?}").contains(marker));
}

#[test]
fn masks_each_card_locally_and_preserves_other_unicode_text() {
    let rule = common::rule(1, 100, MatchKind::Structured(StructuredKind::PaymentCard));
    let compiled = compile(&[rule], &RuleSetLimits::default()).unwrap();
    let text = "前 card 4532 0151 1283 0366; card 4111111111111111 後";
    let report = compiled.scan(text, Channel::Clipboard);
    assert_eq!(report.findings[0].count, 2);
    let masked = mask_matches(text, &report.local_spans);
    assert_eq!(
        masked,
        "前 card •••• •••• •••• 0366; card •••• •••• •••• 1111 後"
    );
    assert!(!serde_json::to_string(&report).unwrap().contains("0366"));
}

#[test]
fn threshold_context_allowlist_and_channel_are_enforced() {
    let mut rule = common::rule(1, 100, MatchKind::Detector("us_ssn".into()));
    let rules = compile(&[rule.clone()], &RuleSetLimits::default()).unwrap();
    assert!(rules
        .scan("123-45-6789", Channel::Clipboard)
        .findings
        .is_empty());
    let report = rules.scan("SSN: 123-45-6789", Channel::Clipboard);
    assert_eq!(report.findings[0].category, DetectorCategory::NationalId);
    assert!(report.findings[0].reasons.contains(&Reason::ContextWord));
    assert!(rules
        .scan("SSN: 123-45-6789", Channel::Removable)
        .findings
        .is_empty());
    rule.options.allow_list.push("123-45-6789".into());
    rule.options.threshold = 0;
    let rules = compile(&[rule], &RuleSetLimits::default()).unwrap();
    assert!(rules
        .scan("SSN: 123-45-6789; SSN: 000-12-3456", Channel::Clipboard)
        .findings
        .is_empty());
}

#[test]
fn lowercasing_expansions_keep_original_byte_ranges() {
    let rule = common::rule(
        1,
        100,
        MatchKind::Phrase {
            value: "i\u{307}secret".into(),
            case_sensitive: false,
        },
    );
    let rules = compile(&[rule], &RuleSetLimits::default()).unwrap();
    let text = "prefix İsecret suffix";
    let report = rules.scan(text, Channel::Clipboard);
    assert_eq!(report.findings[0].count, 1);
    assert_eq!(
        mask_matches(text, &report.local_spans),
        "prefix [secret removed] suffix"
    );
}

#[test]
fn deadline_and_utf8_text_cap_return_truthful_partial_reports() {
    let rule = common::phrase_rule(1, 100, "secret");
    let limits = RuleSetLimits {
        max_text_bytes: 7,
        ..RuleSetLimits::default()
    };
    let rules = compile(&[rule], &limits).unwrap();
    let report = rules.scan("secret€more", Channel::Clipboard);
    assert!(report.truncated);
    assert_eq!(report.findings[0].count, 1);
    let deadline = rules.scan_with_budget("secret", Channel::Clipboard, std::time::Duration::ZERO);
    assert!(deadline.truncated);
    assert!(deadline.findings.is_empty());
}

#[test]
fn one_level_decoding_masks_the_outer_token_without_returning_decoded_content() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let secret = "AKIAABCDEFGHIJKLMNOP";
    let encoded = STANDARD.encode(secret);
    let rule = common::rule(1, 100, MatchKind::Builtin(BuiltinPattern::AwsAccessKey));
    let rules = compile(&[rule], &RuleSetLimits::default()).unwrap();
    let text = format!("before '{encoded}' after");
    let report = rules.scan(&text, Channel::Clipboard);
    assert_eq!(report.findings[0].count, 1);
    assert!(report.findings[0]
        .reasons
        .contains(&Reason::Decoded(Encoding::Base64)));
    assert_eq!(
        mask_matches(&text, &report.local_spans),
        "before '[secret removed]' after"
    );
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains(secret));
    assert!(!json.contains(&encoded));
    assert!(rules
        .scan(&STANDARD.encode(encoded), Channel::Clipboard)
        .findings
        .is_empty());
}

#[test]
fn overlapping_rules_cannot_reveal_digits_from_a_secret() {
    let card = "4532015112830366";
    let rules = compile(
        &[
            common::rule(1, 100, MatchKind::Structured(StructuredKind::PaymentCard)),
            common::phrase_rule(2, 100, card),
        ],
        &RuleSetLimits::default(),
    )
    .unwrap();
    let report = rules.scan(card, Channel::Clipboard);
    assert_eq!(mask_matches(card, &report.local_spans), "[secret removed]");
    assert_eq!(
        mask_matches_for_rules(card, &report.local_spans, &[common::id(1)]),
        "•••• •••• •••• 0366"
    );
}

#[test]
fn stale_or_unknown_tuning_and_unproven_default_masking_are_rejected() {
    let mut subtree = ContentRulesSubtree::default();
    subtree.tuning.builtins.push(BuiltinTuning {
        detector_id: "untrusted sentinel".into(),
        enabled: true,
        threshold: 50,
    });
    assert!(!validate_content_rules(&subtree)
        .unwrap_err()
        .contains("sentinel"));
    subtree.tuning.builtins.clear();
    subtree.tuning.channel_actions.push(ChannelActions {
        channel: Channel::Clipboard,
        actions: vec![Action::MaskMatches],
    });
    assert!(validate_content_rules(&subtree).is_err());
    subtree.tuning.channel_actions.clear();
    subtree.enabled = false;
    assert!(compile_content_rules(&subtree)
        .unwrap()
        .scan("SSN: 123-45-6789", Channel::Clipboard)
        .findings
        .is_empty());
}

#[test]
fn managed_defaults_report_and_explicit_channel_actions_override() {
    let mut subtree = ContentRulesSubtree::default();
    let compiled = compile_content_rules(&subtree).unwrap();
    let report = compiled.scan("SSN: 123-45-6789", Channel::Clipboard);
    assert!(report
        .findings
        .iter()
        .any(|p| p.detector_id == "us_ssn" && p.actions.contains(&Action::ReportFleet)));
    subtree.tuning.channel_actions.push(ChannelActions {
        channel: Channel::Clipboard,
        actions: vec![],
    });
    let report = compile_content_rules(&subtree)
        .unwrap()
        .scan("SSN: 123-45-6789", Channel::Clipboard);
    assert!(report.findings.iter().all(|p| p.actions.is_empty()));
    assert!(builtin_rules()
        .iter()
        .all(|p| p.actions == vec![Action::NotifyUser]));
}

#[test]
fn global_allowlists_are_applied_once_without_expanding_each_builtin() {
    let mut subtree = ContentRulesSubtree::default();
    subtree.tuning.allow_list = vec!["123-45-6789".into()];
    for i in 0..120 {
        subtree
            .tuning
            .allow_list
            .push(format!("{i}{}", "x".repeat(1024)));
    }
    let resolved = resolve_content_rules(&subtree).unwrap();
    assert!(resolved
        .iter()
        .all(|rule| rule.options.allow_list.is_empty()));
    assert!(serde_json::to_vec(&resolved).unwrap().len() < 128 * 1024);
    let report = compile_content_rules(&subtree)
        .unwrap()
        .scan("SSN: 123-45-6789", Channel::Clipboard);
    assert!(report
        .findings
        .iter()
        .all(|finding| finding.detector_id != "us_ssn"));
}

#[test]
fn spanish_id_checksums_and_context_score_are_distinct_from_pattern_hits() {
    let rule = builtin_rules()
        .into_iter()
        .find(|r| matches!(&r.matcher,MatchKind::Detector(id) if id=="es_dni"))
        .unwrap();
    let compiled = compile(&[rule], &content_rule_limits()).unwrap();
    let bare = compiled.scan("12345678Z", Channel::Clipboard);
    let contextual = compiled.scan("DNI: 12345678Z", Channel::Clipboard);
    assert_eq!(bare.findings[0].score, 60);
    assert_eq!(contextual.findings[0].score, 80);
    assert!(contextual.findings[0]
        .reasons
        .contains(&Reason::ChecksumValid));
    assert!(contextual.findings[0]
        .reasons
        .contains(&Reason::ContextWord));
    assert!(compiled
        .scan("DNI: 12345678A", Channel::Clipboard)
        .findings
        .is_empty());
}

#[test]
fn adjacent_printed_ibans_each_count_and_mask() {
    let rule = common::rule(1, 100, MatchKind::Structured(StructuredKind::Iban));
    let compiled = compile(&[rule], &RuleSetLimits::default()).unwrap();
    let text = "GB82 WEST 1234 5698 7654 32 GB82 WEST 1234 5698 7654 32";
    let report = compiled.scan(text, Channel::Clipboard);
    assert_eq!(report.findings[0].count, 2);
    assert_eq!(
        mask_matches(text, &report.local_spans),
        "[secret removed] [secret removed]"
    );
}

#[test]
fn confusable_host_mask_uses_the_same_http_url_as_the_detector() {
    let rule = common::rule(
        1,
        100,
        MatchKind::Builtin(BuiltinPattern::UnicodeConfusableUrlHost),
    );
    let compiled = compile(&[rule], &RuleSetLimits::default()).unwrap();
    let text = "ftp://example.invalid https://p\u{0430}ypal.example.invalid/login";
    let report = compiled.scan(text, Channel::Clipboard);
    assert_eq!(
        mask_matches(text, &report.local_spans),
        "ftp://example.invalid https://[secret removed]/login"
    );
}
