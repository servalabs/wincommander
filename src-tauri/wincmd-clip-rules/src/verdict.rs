// SPDX-License-Identifier: AGPL-3.0-or-later
//! The content-free result of a match.

use crate::{Action, RuleId, Severity};

/// The result of a match. Structurally content-free by construction: every
/// field is a scalar, an id, or a closed enum — there is no offset, no
/// excerpt, no capture group, and no matched text anywhere in this type.
/// This is the structural half of plan §8 ("enforcing content-free,
/// structurally") — the type layer, not a doc-comment promise: adding a
/// field here that could carry clipboard content would have to be a
/// deliberate, reviewable change to this exact struct.
///
/// (Compile-time note tying this to §8: every field below is `Copy` or a
/// bounded id/enum; there is no `String`, `Vec<u8>`, or `serde_json::Value`
/// field, so nothing here can hold arbitrary text.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub rule_id: RuleId,
    pub rule_revision: u32,
    pub severity: Severity,
    pub actions: Vec<Action>,
}
