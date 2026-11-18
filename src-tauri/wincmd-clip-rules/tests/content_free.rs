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
    let Verdict {
        rule_id: _,
        rule_revision: _,
        severity: _,
        actions: _,
    } = v;
}

#[test]
fn no_match_returns_none_not_an_empty_verdict() {
    let rule = common::phrase_rule(1, 1, "will-not-appear-anywhere");
    let compiled = compile(&[rule], &RuleSetLimits::default()).unwrap();
    assert!(compiled.evaluate("completely unrelated text").is_none());
}
