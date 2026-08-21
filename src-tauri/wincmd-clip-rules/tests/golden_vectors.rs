// SPDX-License-Identifier: AGPL-3.0-or-later
//! Golden-vector tests for priority resolution and cooldown semantics.
//!
//! Mirrors the intent of `fleet-proto`'s `golden_epoch_preimage`: expected
//! results are FIXED LITERALS written once and frozen, not values
//! recomputed from the same inputs the code under test also reads. A
//! regression that changes priority-resolution or cooldown behaviour must
//! show up as a literal mismatch here, in BOTH the fleet-server console
//! build and the endpoint build of this crate (it's the same crate, so
//! there is exactly one place semantics can drift: this crate itself).

mod common;

use std::time::{Duration, Instant};

use wincmd_clip_rules::{
    compile, Action, CooldownLedger, Emit, MatchKind, RuleId, RuleSetLimits, Severity,
};

/// FROZEN golden vector — computed 2026-08-17. Three phrase rules all
/// match the same clipboard text ("leak-me"); the highest-`priority` rule
/// (500) must win regardless of declaration order, id, or the fact that a
/// lower-priority rule was compiled first.
#[test]
fn golden_priority_resolution_highest_wins() {
    let low = common::phrase_rule(0x01, 10, "leak-me");
    let high = common::phrase_rule(0x02, 500, "leak-me");
    let mid = common::phrase_rule(0x03, 200, "leak-me");

    let compiled = compile(&[low, high, mid], &RuleSetLimits::default()).expect("all rules valid");
    let verdict = compiled
        .evaluate("this text contains leak-me right here")
        .expect("should match");

    // FROZEN: id seed 0x02 ("...3a02"), revision 1, Warn, [NotifyUser] —
    // see `tests/common/mod.rs::rule` for the fixed defaults these seeds
    // expand to.
    assert_eq!(
        verdict.rule_id,
        RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a02").unwrap()
    );
    assert_eq!(verdict.rule_revision, 1);
    assert_eq!(verdict.severity, Severity::Warn);
    assert_eq!(verdict.actions, vec![Action::NotifyUser]);
}

/// FROZEN golden vector — computed 2026-08-17. Two rules at the SAME
/// priority both match; the tie is broken by the lexicographically
/// greater `RuleId` — seed 0x0b ("...3a0b") sorts after seed 0x0a
/// ("...3a0a") as hex text, so 0x0b must win.
#[test]
fn golden_priority_tie_break_by_rule_id() {
    let a = common::phrase_rule(0x0a, 100, "tie-break-me");
    let b = common::phrase_rule(0x0b, 100, "tie-break-me");

    let compiled = compile(&[a, b], &RuleSetLimits::default()).expect("all rules valid");
    let verdict = compiled.evaluate("tie-break-me").expect("should match");

    assert_eq!(
        verdict.rule_id,
        RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a0b").unwrap()
    );
}

/// FROZEN golden vector — computed 2026-08-17. A fixed sequence of
/// `should_emit` calls at fixed instants, asserted against fixed `Emit`
/// literals at each step: emit, suppress×2, emit-after-elapse,
/// suppress-restarts-at-1. This is the exact suppression-metric contract
/// the endpoint's health reporting depends on (plan §8.1's
/// `clipboard_guard.*` booleans are downstream of this never silently
/// changing shape).
#[test]
fn golden_cooldown_sequence() {
    let mut ledger = CooldownLedger::new();
    let rule_id = RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a01").unwrap();
    let cooldown = Duration::from_secs(60);
    let t0 = Instant::now();

    assert_eq!(ledger.should_emit(rule_id.clone(), t0, cooldown), Emit::Now);
    assert_eq!(
        ledger.should_emit(rule_id.clone(), t0 + Duration::from_secs(5), cooldown),
        Emit::Suppressed { count: 1 }
    );
    assert_eq!(
        ledger.should_emit(rule_id.clone(), t0 + Duration::from_secs(30), cooldown),
        Emit::Suppressed { count: 2 }
    );
    assert_eq!(
        ledger.should_emit(rule_id.clone(), t0 + Duration::from_secs(61), cooldown),
        Emit::Now
    );
    assert_eq!(
        ledger.should_emit(rule_id, t0 + Duration::from_secs(70), cooldown),
        Emit::Suppressed { count: 1 }
    );
}

/// Sanity check that the golden-vector fixtures above actually exercise
/// `MatchKind::Phrase` case-sensitively (not incidentally passing via some
/// other matcher) — a non-matching text must produce no verdict.
#[test]
fn golden_fixture_sanity_no_match_is_none() {
    let only = common::phrase_rule(0x01, 10, "leak-me");
    let compiled = compile(&[only], &RuleSetLimits::default()).expect("valid");
    assert!(compiled.evaluate("nothing interesting here").is_none());
}

/// Not a golden vector, but pinned alongside them: a `Regex`-matcher rule
/// and a `Builtin`-matcher rule that fires on the same underlying pattern
/// text must be indistinguishable at the `Verdict` level — proving "one
/// engine" (both code paths land in `CompiledMatcher::Regex`, see
/// `compile.rs`).
#[test]
fn custom_regex_and_builtin_share_the_same_matching_path() {
    use wincmd_clip_rules::BuiltinPattern;

    let custom = common::rule(
        0x21,
        10,
        MatchKind::Regex {
            pattern: r"\bAKIA[0-9A-Z]{16}\b".to_string(),
            case_sensitive: true,
        },
    );
    let builtin = common::rule(0x22, 10, MatchKind::Builtin(BuiltinPattern::AwsAccessKey));

    let matching_text = "aws key: AKIAABCDEFGHIJKLMNOP";
    let non_matching_text = "no secret here";

    let custom_compiled = compile(&[custom], &RuleSetLimits::default()).unwrap();
    let builtin_compiled = compile(&[builtin], &RuleSetLimits::default()).unwrap();

    assert!(
        custom_compiled.evaluate(matching_text).is_some(),
        "custom regex should match"
    );
    assert!(
        builtin_compiled.evaluate(matching_text).is_some(),
        "builtin should match identically"
    );
    assert!(custom_compiled.evaluate(non_matching_text).is_none());
    assert!(builtin_compiled.evaluate(non_matching_text).is_none());
}
