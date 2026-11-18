// SPDX-License-Identifier: AGPL-3.0-or-later
//! Determinism: the same ruleset + text must resolve to the same winning
//! rule every time — across repeated `evaluate` calls, across independent
//! recompiles of an identical rule set (fleet console vs. endpoint each
//! compile their own copy), and regardless of declaration order.

mod common;

use wincmd_clip_rules::{compile, RuleSetLimits};

#[test]
fn declaration_order_does_not_affect_the_tie_break_winner() {
    let forward: Vec<_> = (0..10u8)
        .map(|i| common::phrase_rule(i, 50, "order-independent"))
        .collect();
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
