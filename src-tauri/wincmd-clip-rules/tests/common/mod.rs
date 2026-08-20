// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared helpers for `wincmd-clip-rules` integration tests.
//!
//! Not every test binary in `tests/` uses every helper here — each
//! integration-test file compiles `mod common;` as its own crate, so an
//! unused helper in one file is expected, not a mistake.
#![allow(dead_code)]

use wincmd_clip_rules::{Action, CompileError, MatchKind, Rule, RuleId, Severity};

/// A deterministic, distinct, valid 32-hex-char `RuleId` for test fixtures.
/// Only the last byte varies, so ids stay visually recognizable in test
/// failure output while remaining valid (`RuleId::new` requires exactly
/// 32 lowercase hex chars for the hyphenless form).
pub fn id(seed: u8) -> RuleId {
    RuleId::new(format!("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a{seed:02x}")).expect("valid 32-hex-char id")
}

/// A minimal, otherwise-default `Rule` wrapping `matcher` at `priority`,
/// enabled, with the given id seed. Callers override individual fields on
/// the returned struct when a test needs to (e.g. `cooldown_seconds`).
pub fn rule(seed: u8, priority: u16, matcher: MatchKind) -> Rule {
    Rule {
        id: id(seed),
        revision: 1,
        name: format!("test rule {seed}"),
        enabled: true,
        priority,
        matcher,
        severity: Severity::Warn,
        actions: vec![Action::NotifyUser],
        cooldown_seconds: 0,
        snoozable: true,
        locked: false,
    }
}

/// A case-sensitive phrase rule — the most common fixture shape across the
/// test suite (cheap to reason about, no regex-limit interactions).
pub fn phrase_rule(seed: u8, priority: u16, value: &str) -> Rule {
    rule(
        seed,
        priority,
        MatchKind::Phrase {
            value: value.to_string(),
            case_sensitive: true,
        },
    )
}

/// Unwrap the `Err` side of a `compile()` result without requiring the
/// `Ok` type (`CompiledRuleSet`) to implement `Debug` — it deliberately
/// doesn't (it's opaque by design), so a plain `.unwrap_err()` won't
/// compile. Panics with the caller's own message if `compile()` actually
/// succeeded.
pub fn expect_errors<T>(result: Result<T, Vec<CompileError>>) -> Vec<CompileError> {
    match result {
        Ok(_) => panic!("expected compile() to return errors, but it succeeded"),
        Err(errors) => errors,
    }
}
