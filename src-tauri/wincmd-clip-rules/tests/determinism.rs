// SPDX-License-Identifier: AGPL-3.0-or-later
//! Determinism: the same ruleset + text must resolve to the same winning
//! rule every time — across repeated `evaluate` calls, across independent
//! recompiles of an identical rule set (fleet console vs. endpoint each
//! compile their own copy), and regardless of declaration order.

mod common;

use wincmd_clip_rules::{compile, RuleSetLimits};

#[test]
fn equal_priority_rules_resolve_identically_across_repeated_evaluations() {
    let rules: Vec<_> = (0..20u8).map(|i| common::phrase_rule(i, 100, "same-priority-match")).collect();
    let compiled = compile(&rules, &RuleSetLimits::default()).unwrap();

    let text = "this text contains same-priority-match somewhere";
    let first = compiled.evaluate(text).unwrap();
    for _ in 0..200 {
        let again = compiled.evaluate(text).unwrap();
        assert_eq!(again.rule_id, first.rule_id, "tie-break winner must be stable across repeated calls");
    }
}

#[test]
fn equal_priority_rules_resolve_identically_across_independent_compiles() {
    // Same logical rule set, recompiled from scratch twice — simulating
    // the fleet console's validation compile and the endpoint's own
    // compile of the same policy. The winner must match.
    let build_rules = || -> Vec<_> {
        (0..20u8).map(|i| common::phrase_rule(i, 100, "same-priority-match")).collect()
    };

    let compiled_a = compile(&build_rules(), &RuleSetLimits::default()).unwrap();
    let compiled_b = compile(&build_rules(), &RuleSetLimits::default()).unwrap();

    let verdict_a = compiled_a.evaluate("same-priority-match").unwrap();
    let verdict_b = compiled_b.evaluate("same-priority-match").unwrap();
    assert_eq!(verdict_a.rule_id, verdict_b.rule_id);
}

#[test]
fn declaration_order_does_not_affect_the_tie_break_winner() {
    let forward: Vec<_> = (0..10u8).map(|i| common::phrase_rule(i, 50, "order-independent")).collect();
    let mut reversed = forward.clone();
    reversed.reverse();

    let winner_forward = compile(&forward, &RuleSetLimits::default())
        .unwrap()
        .evaluate("order-independent")
        .unwrap()
        .rule_id;
    let winner_reversed = compile(&reversed, &RuleSetLimits::default())
        .unwrap()
        .evaluate("order-independent")
        .unwrap()
        .rule_id;

    assert_eq!(winner_forward, winner_reversed);
}
