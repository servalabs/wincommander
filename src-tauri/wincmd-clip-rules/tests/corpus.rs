// SPDX-License-Identifier: AGPL-3.0-or-later
use serde::Deserialize;
use wincmd_clip_rules::*;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Example {
    detector_id: String,
    text: String,
}

#[test]
fn labelled_positives_and_validator_negatives() {
    let builtins = builtin_rules();
    for (source, expected) in [
        (include_str!("corpus/positives.json"), true),
        (include_str!("corpus/legacy-positives.json"), true),
        (include_str!("corpus/generated-secrets.json"), true),
        (include_str!("corpus/negatives.json"), false),
    ] {
        for example in serde_json::from_str::<Vec<Example>>(source).unwrap() {
            let mut rule = builtins
                .iter()
                .find(|r| match &r.matcher {
                    MatchKind::Detector(id) => *id == example.detector_id,
                    MatchKind::Structured(StructuredKind::PaymentCard) => {
                        example.detector_id == "payment_card"
                    }
                    MatchKind::Structured(StructuredKind::Iban) => example.detector_id == "iban",
                    MatchKind::Builtin(pattern) => {
                        serde_json::to_value(pattern).unwrap().as_str()
                            == Some(example.detector_id.as_str())
                    }
                    _ => false,
                })
                .unwrap_or_else(|| panic!("missing corpus detector {}", example.detector_id))
                .clone();
            rule.enabled = true;
            let compiled = compile(&[rule], &content_rule_limits()).unwrap();
            let report = compiled.scan(&example.text, Channel::Clipboard);
            assert!(
                !report.truncated,
                "{} corpus scan incomplete",
                example.detector_id
            );
            assert_eq!(
                !report.findings.is_empty(),
                expected,
                "{} corpus expectation",
                example.detector_id
            );
        }
    }
}

#[test]
fn ten_thousand_benign_samples_preserve_alert_only_baseline() {
    let compiled = compile_content_rules(&ContentRulesSubtree::default()).unwrap();
    let mut corpus = String::new();
    for i in 0..10_000u64 {
        let numeric = i.wrapping_mul(7919).wrapping_add(4321012345678901);
        let line = match i % 5 {
            0 => format!("order {numeric} items 2 total 49.99\n"),
            1 => format!("INFO request 0e8f1a2b-3c4d-5e6f-7a8b-{i:012x} completed\n"),
            2 => format!("const fixture_{i} = {{ enabled: true, count: 17 }};\n"),
            3 => format!("{i},sample,accepted,2026-01-02\n"),
            _ => format!("The report has {i} ordinary rows and no credentials.\n"),
        };
        corpus.push_str(&line);
    }
    assert!(corpus.len() < 1_048_576);
    let report = compiled.scan_with_budget(
        &corpus,
        Channel::Clipboard,
        std::time::Duration::from_secs(10),
    );
    assert!(!report.truncated, "benign corpus scan incomplete");
    for finding in &report.findings {
        assert!(
            finding.count <= 1,
            "{} exceeds one false positive per 10,000 benign samples: {}",
            finding.detector_id,
            finding.count
        );
    }
    assert!(
        report.findings.is_empty(),
        "synthetic benign corpus must have no matches"
    );
}
