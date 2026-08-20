// SPDX-License-Identifier: AGPL-3.0-or-later
//! Limit-enforcement boundary tests: one pair (ok / rejected) per limit in
//! `RuleSetLimits`, plus lookaround rejection.

mod common;

use wincmd_clip_rules::{compile, CompileError, MatchKind, RuleSetLimits};

// ── max_enabled_rules ─────────────────────────────────────────────────

#[test]
fn one_hundred_enabled_rules_is_ok() {
    let rules: Vec<_> = (0..100u8).map(|i| common::phrase_rule(i, 1, "x")).collect();
    assert!(compile(&rules, &RuleSetLimits::default()).is_ok());
}

#[test]
fn one_hundred_and_one_enabled_rules_is_rejected() {
    let rules: Vec<_> = (0..101u8).map(|i| common::phrase_rule(i, 1, "x")).collect();
    let errors = common::expect_errors(compile(&rules, &RuleSetLimits::default()));
    assert_eq!(
        errors,
        vec![CompileError::TooManyEnabledRules {
            limit: 100,
            actual: 101
        }]
    );
}

#[test]
fn disabled_rules_do_not_count_toward_the_limit() {
    // 101 rules total, but only 100 enabled — must compile fine.
    let mut rules: Vec<_> = (0..100u8).map(|i| common::phrase_rule(i, 1, "x")).collect();
    let mut extra = common::phrase_rule(200, 1, "x");
    extra.enabled = false;
    rules.push(extra);
    assert!(compile(&rules, &RuleSetLimits::default()).is_ok());
}

// ── max_pattern_bytes ─────────────────────────────────────────────────

#[test]
fn pattern_at_exactly_the_byte_limit_is_ok() {
    let limits = RuleSetLimits::default();
    let value = "a".repeat(limits.max_pattern_bytes);
    let rule = common::rule(
        1,
        1,
        MatchKind::Phrase {
            value,
            case_sensitive: true,
        },
    );
    assert!(compile(&[rule], &limits).is_ok());
}

#[test]
fn pattern_one_byte_over_the_limit_is_rejected() {
    let limits = RuleSetLimits::default();
    let value = "a".repeat(limits.max_pattern_bytes + 1);
    let rule = common::rule(
        1,
        1,
        MatchKind::Phrase {
            value,
            case_sensitive: true,
        },
    );
    let errors = common::expect_errors(compile(&[rule], &limits));
    assert_eq!(
        errors,
        vec![CompileError::PatternTooLong {
            index: 0,
            limit: limits.max_pattern_bytes,
            actual: limits.max_pattern_bytes + 1,
        }]
    );
}

#[test]
fn regex_pattern_over_the_byte_limit_is_rejected_before_compiling() {
    // Same limit, but through the Regex matcher — the byte-length check
    // must apply uniformly to both pattern-bearing MatchKind variants.
    let limits = RuleSetLimits::default();
    let pattern = format!("{}$", "a".repeat(limits.max_pattern_bytes));
    let rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern,
            case_sensitive: true,
        },
    );
    let errors = common::expect_errors(compile(&[rule], &limits));
    assert!(matches!(
        errors[0],
        CompileError::PatternTooLong { index: 0, .. }
    ));
}

// ── regex_size_limit ──────────────────────────────────────────────────

#[test]
fn a_regex_whose_compiled_program_exceeds_the_per_rule_limit_is_rejected() {
    // `\w` alone needs a large NFA (full-Unicode word-char class) — the
    // `regex` crate's own docs use exactly this pattern to demonstrate
    // `size_limit` rejecting a "seemingly small" pattern. 1000 bytes is
    // nowhere near enough for it.
    let limits = RuleSetLimits {
        regex_size_limit: 1000,
        ..RuleSetLimits::default()
    };
    let rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: r"\w".to_string(),
            case_sensitive: true,
        },
    );
    let errors = common::expect_errors(compile(&[rule], &limits));
    assert_eq!(
        errors,
        vec![CompileError::RegexProgramTooLarge {
            index: 0,
            limit: 1000
        }]
    );
}

#[test]
fn a_small_regex_under_the_per_rule_limit_is_ok() {
    let limits = RuleSetLimits {
        regex_size_limit: 1000,
        ..RuleSetLimits::default()
    };
    let rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: "abc".to_string(),
            case_sensitive: true,
        },
    );
    assert!(compile(&[rule], &limits).is_ok());
}

// ── regex_total_size_limit ────────────────────────────────────────────
//
// `regex` doesn't expose a successful build's actual compiled size, so
// `compile()` charges each success the FULL per-rule `regex_size_limit`
// against a running total (see `compile.rs::compile_regex`) — meaning this
// boundary is exact and deterministic regardless of how small the actual
// patterns are.

#[test]
fn two_small_regexes_whose_charged_total_exceeds_the_ruleset_limit_is_rejected() {
    let limits = RuleSetLimits {
        regex_size_limit: 2000,
        regex_total_size_limit: 3000, // room for one rule's charge (2000), not two (4000)
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
    let second = common::rule(
        2,
        1,
        MatchKind::Regex {
            pattern: "def".to_string(),
            case_sensitive: true,
        },
    );

    let errors = common::expect_errors(compile(&[first, second], &limits));
    assert_eq!(
        errors,
        vec![CompileError::RegexTotalProgramTooLarge {
            index: 1,
            limit: 3000,
            actual: 4000
        }]
    );
}

#[test]
fn two_small_regexes_within_the_ruleset_total_is_ok() {
    let limits = RuleSetLimits {
        regex_size_limit: 1000,
        regex_total_size_limit: 3000, // room for both charges (2000 total)
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
    let second = common::rule(
        2,
        1,
        MatchKind::Regex {
            pattern: "def".to_string(),
            case_sensitive: true,
        },
    );
    assert!(compile(&[first, second], &limits).is_ok());
}

// ── lookaround rejection ──────────────────────────────────────────────

#[test]
fn positive_lookahead_is_rejected() {
    let rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: "abc(?=def)".to_string(),
            case_sensitive: true,
        },
    );
    let errors = common::expect_errors(compile(&[rule], &RuleSetLimits::default()));
    assert_eq!(
        errors,
        vec![CompileError::LookaroundUnsupported { index: 0 }]
    );
}

#[test]
fn negative_lookahead_is_rejected() {
    let rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: "abc(?!def)".to_string(),
            case_sensitive: true,
        },
    );
    let errors = common::expect_errors(compile(&[rule], &RuleSetLimits::default()));
    assert_eq!(
        errors,
        vec![CompileError::LookaroundUnsupported { index: 0 }]
    );
}

#[test]
fn positive_lookbehind_is_rejected() {
    let rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: "(?<=abc)def".to_string(),
            case_sensitive: true,
        },
    );
    let errors = common::expect_errors(compile(&[rule], &RuleSetLimits::default()));
    assert_eq!(
        errors,
        vec![CompileError::LookaroundUnsupported { index: 0 }]
    );
}

#[test]
fn negative_lookbehind_is_rejected() {
    let rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: "(?<!abc)def".to_string(),
            case_sensitive: true,
        },
    );
    let errors = common::expect_errors(compile(&[rule], &RuleSetLimits::default()));
    assert_eq!(
        errors,
        vec![CompileError::LookaroundUnsupported { index: 0 }]
    );
}

#[test]
fn named_capture_group_is_not_mistaken_for_lookbehind() {
    // `(?<name>` shares its opening bytes with `(?<=`/`(?<!` but is a
    // real, Rust-supported named capture group — must NOT be rejected.
    let rule = common::rule(
        1,
        1,
        MatchKind::Regex {
            pattern: "(?<year>[0-9]{4})-(?<month>[0-9]{2})".to_string(),
            case_sensitive: true,
        },
    );
    assert!(compile(&[rule], &RuleSetLimits::default()).is_ok());
}

// ── multiple errors collected in one pass ────────────────────────────

#[test]
fn compile_collects_every_error_not_just_the_first() {
    let limits = RuleSetLimits::default();
    let bad_pattern = common::rule(
        1,
        1,
        MatchKind::Phrase {
            value: "a".repeat(limits.max_pattern_bytes + 1),
            case_sensitive: true,
        },
    );
    let bad_lookaround = common::rule(
        2,
        1,
        MatchKind::Regex {
            pattern: "a(?=b)".to_string(),
            case_sensitive: true,
        },
    );

    let errors = common::expect_errors(compile(&[bad_pattern, bad_lookaround], &limits));
    assert_eq!(
        errors.len(),
        2,
        "both independent errors must be reported, not just the first"
    );
    assert!(matches!(
        errors[0],
        CompileError::PatternTooLong { index: 0, .. }
    ));
    assert!(matches!(
        errors[1],
        CompileError::LookaroundUnsupported { index: 1 }
    ));
}
