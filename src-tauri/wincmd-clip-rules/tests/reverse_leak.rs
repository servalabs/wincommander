// SPDX-License-Identifier: AGPL-3.0-or-later
//! The reverse-leak rule (plan §8): a `CompileError`'s `Display`/`Debug`
//! must never echo the pattern text, the rule name, or any clipboard text
//! — regardless of which rule triggered it. Every check below embeds a
//! distinctive sentinel in the field that WOULD leak if this rule were
//! ever violated, then asserts the sentinel is absent from both
//! renderings of every error `compile()` returns.

mod common;

use wincmd_clip_rules::{compile, CompileError, MatchKind, RuleSetLimits};

const SENTINEL: &str = "SENTINEL_zzz_should_never_appear_in_any_error_ffffff";

fn assert_no_leak(errors: &[CompileError]) {
    assert!(
        !errors.is_empty(),
        "test setup bug: expected at least one CompileError"
    );
    for e in errors {
        let display = e.to_string();
        let debug = format!("{e:?}");
        assert!(
            !display.contains(SENTINEL),
            "Display leaked the sentinel: {display}"
        );
        assert!(
            !debug.contains(SENTINEL),
            "Debug leaked the sentinel: {debug}"
        );
    }
}

#[test]
fn pattern_too_long_error_never_echoes_the_pattern_or_name() {
    let limits = RuleSetLimits::default();
    let value = format!("{SENTINEL}{}", "a".repeat(limits.max_pattern_bytes));
    let mut rule = common::rule(
        1,
        1,
        MatchKind::Phrase {
            value,
            case_sensitive: true,
        },
    );
    rule.name = format!("{SENTINEL} rule name");
    let errors = common::expect_errors(compile(&[rule], &limits));
    assert!(matches!(
        errors[0],
        CompileError::PatternTooLong { index: 0, .. }
    ));
    assert_no_leak(&errors);
}

#[test]
fn lookaround_error_never_echoes_the_pattern_or_name() {
    let mut rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: format!("{SENTINEL}(?=x)"),
            case_sensitive: true,
        },
    );
    rule.name = format!("{SENTINEL} rule name");
    let errors = common::expect_errors(compile(&[rule], &RuleSetLimits::default()));
    assert!(matches!(
        errors[0],
        CompileError::LookaroundUnsupported { index: 0 }
    ));
    assert_no_leak(&errors);
}

#[test]
fn regex_syntax_error_never_echoes_the_pattern_or_name() {
    // Unbalanced group — a genuine syntax error, not lookaround, not
    // size. `regex::Error::Syntax`'s own message normally quotes the
    // offending pattern text; this crate must drop that message entirely.
    let mut rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: format!("{SENTINEL}("),
            case_sensitive: true,
        },
    );
    rule.name = format!("{SENTINEL} rule name");
    let errors = common::expect_errors(compile(&[rule], &RuleSetLimits::default()));
    assert!(matches!(errors[0], CompileError::RegexSyntax { index: 0 }));
    assert_no_leak(&errors);
}

#[test]
fn regex_too_large_error_never_echoes_the_pattern_or_name() {
    let limits = RuleSetLimits {
        regex_size_limit: 1000,
        ..RuleSetLimits::default()
    };
    // Alternation, not concatenation, so the sentinel's plain text can't
    // be mistaken for a `{m,n}` repetition bound next to `\w`.
    let mut rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: format!(r"\w|{SENTINEL}"),
            case_sensitive: true,
        },
    );
    rule.name = format!("{SENTINEL} rule name");
    let errors = common::expect_errors(compile(&[rule], &limits));
    assert!(matches!(
        errors[0],
        CompileError::RegexProgramTooLarge { index: 0, .. }
    ));
    assert_no_leak(&errors);
}

#[test]
fn regex_total_too_large_error_never_echoes_the_pattern_or_name() {
    let limits = RuleSetLimits {
        regex_size_limit: 2000,
        regex_total_size_limit: 3000,
        ..RuleSetLimits::default()
    };
    let first = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: "abc".to_string(),
            case_sensitive: true,
        },
    );
    let mut second = common::rule(
        2,
        1,
        MatchKind::Regex {
            pattern: format!("{SENTINEL}def"),
            case_sensitive: true,
        },
    );
    second.name = format!("{SENTINEL} rule name");
    let errors = common::expect_errors(compile(&[first, second], &limits));
    assert!(matches!(
        errors[0],
        CompileError::RegexTotalProgramTooLarge { index: 1, .. }
    ));
    assert_no_leak(&errors);
}

#[test]
fn too_many_enabled_rules_error_never_echoes_any_rule_name() {
    let mut rules: Vec<_> = (0..101u8).map(|i| common::phrase_rule(i, 1, "x")).collect();
    for r in &mut rules {
        r.name = format!("{SENTINEL} {}", r.name);
    }
    let errors = common::expect_errors(compile(&rules, &RuleSetLimits::default()));
    assert!(matches!(
        errors[0],
        CompileError::TooManyEnabledRules { .. }
    ));
    assert_no_leak(&errors);
}

// ── CompiledRuleSet's Debug is hand-written for this reason ──────────────

/// `CompiledMatcher::Regex` wraps a `regex::Regex`, and `regex::Regex`'s own
/// `Debug` prints the **pattern source**. So a derived `Debug` on
/// `CompiledRuleSet` would make `{:?}` emit every compiled pattern — the same
/// reverse leak the errors above are guarded against, but through the one
/// formatting call reviewers habitually treat as harmless (a test's
/// `unwrap_err()` panic message, a stray tracing field).
///
/// This pins the hand-written impl. If someone later replaces it with
/// `#[derive(Debug)]`, this test fails instead of silently leaking.
#[test]
fn compiled_ruleset_debug_never_echoes_a_pattern_or_rule_name() {
    let mut rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: format!("{SENTINEL}[0-9]+"),
            case_sensitive: true,
        },
    );
    rule.name = format!("{SENTINEL} rule name");

    let compiled = compile(&[rule], &RuleSetLimits::default())
        .expect("test setup bug: this ruleset should compile cleanly");

    let debug = format!("{compiled:?}");
    assert!(
        !debug.contains(SENTINEL),
        "CompiledRuleSet Debug leaked a pattern or rule name: {debug}"
    );
}
