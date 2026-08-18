// SPDX-License-Identifier: AGPL-3.0-or-later
//! §8 "content-free by construction": exhaustive-destructure and runtime
//! checks that `Verdict` never carries clipboard content.

mod common;

use wincmd_clip_rules::{compile, RuleSetLimits, Verdict};

/// Compile-time enforcement of §8's structural half. Destructuring EVERY
/// field of `Verdict` with no `..` means this function fails to compile
/// the moment anyone adds a field to `Verdict` without updating this
/// test — so an offset/excerpt/capture-group field can't be added later
/// without a reviewer seeing this exact assertion fail to build. (Tied to
/// plan §8, "Enforcing content-free, structurally" — the type layer.)
#[allow(dead_code)]
fn assert_verdict_shape_is_exhaustively_known(v: Verdict) {
    let Verdict { rule_id: _, rule_revision: _, severity: _, actions: _ } = v;
}

#[test]
fn matched_clipboard_text_never_appears_in_the_verdict() {
    const SENTINEL: &str = "SENTINEL_zzz_clipboard_secret_should_never_leak_ffffff";
    let rule = common::phrase_rule(1, 1, SENTINEL);
    let compiled = compile(&[rule], &RuleSetLimits::default()).unwrap();

    let text = format!("some clipboard content around {SENTINEL} and more text after it");
    let verdict = compiled.evaluate(&text).expect("should match");

    let rendered = format!("{verdict:?}");
    assert!(
        !rendered.contains(SENTINEL),
        "Verdict Debug output leaked matched clipboard text: {rendered}"
    );
}

#[test]
fn no_match_returns_none_not_an_empty_verdict() {
    let rule = common::phrase_rule(1, 1, "will-not-appear-anywhere");
    let compiled = compile(&[rule], &RuleSetLimits::default()).unwrap();
    assert!(compiled.evaluate("completely unrelated text").is_none());
}

#[test]
fn structured_and_builtin_matches_are_equally_content_free() {
    use wincmd_clip_rules::{BuiltinPattern, MatchKind, StructuredKind};

    let card_rule = common::rule(1, 1, MatchKind::Structured(StructuredKind::PaymentCard));
    let builtin_rule = common::rule(2, 1, MatchKind::Builtin(BuiltinPattern::AwsAccessKey));

    let card_text = "my card is 4532015112830366 please don't share it";
    let key_text = "leaked: AKIAABCDEFGHIJKLMNOP in this paste";

    let card_verdict = compile(&[card_rule], &RuleSetLimits::default())
        .unwrap()
        .evaluate(card_text)
        .expect("card should match");
    let key_verdict = compile(&[builtin_rule], &RuleSetLimits::default())
        .unwrap()
        .evaluate(key_text)
        .expect("key should match");

    assert!(!format!("{card_verdict:?}").contains("4532015112830366"));
    assert!(!format!("{key_verdict:?}").contains("AKIAABCDEFGHIJKLMNOP"));
}
