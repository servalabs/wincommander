// SPDX-License-Identifier: AGPL-3.0-or-later
//! `wincmd-clip-rules` — the clipboard-guard rule engine.
//!
//! Pure, `no-I/O` crate: rule types, compilation, matching, action
//! selection, and cooldown bookkeeping. NO clipboard access, NO
//! filesystem, NO network, NO logging, NO platform-specific dependencies.
//!
//! **Why this crate is shared, and why that constrains it.** It is
//! compiled into BOTH `fleet-server` (the console's rule-validation / test
//! panel, plan §4.5) and the endpoint (`commander-svc`/`commander-pro`, the
//! actual clipboard matcher). A rule that validates in the console MUST
//! behave identically on-device — so this crate has exactly one matching
//! implementation, not a Rust copy and a parallel JS copy that could drift.
//! Every regex a rule can use goes through the SAME `compile()` path
//! whether it's a fleet-authored custom rule or one of the migrated
//! free-tier builtins (see `builtin.rs`) — "one engine", not two.
//!
//! **Content-free by construction (plan §8).** `Verdict` — the only thing
//! `CompiledRuleSet::evaluate` returns — carries no offset, no excerpt, no
//! capture group, and no matched text (see `verdict.rs`). `CompileError`
//! identifies a bad rule by index only and never echoes pattern/rule-name/
//! clipboard text (see `compile.rs`). This is the crate's load-bearing
//! privacy property, not a convenience.
//!
//! **Fallibility boundary.** [`compile`] is the ONLY fallible entry point.
//! Everything downstream of a successful compile — [`CompiledRuleSet::
//! evaluate`], [`CooldownLedger::should_emit`], [`truncate_for_match`] — is
//! infallible.
//!
//! **What this crate does NOT do:** read the clipboard, write anywhere,
//! make network calls, decide WHEN to poll, or execute an `Action` (that's
//! the endpoint's job — this crate only decides WHICH actions a match
//! requests).

mod action;
mod builtin;
mod compile;
mod cooldown;
mod ids;
mod rule;
mod severity;
mod structured;
mod truncate;
mod verdict;

pub use action::Action;
pub use builtin::BuiltinPattern;
pub use compile::{compile, CompileError, CompiledRuleSet};
pub use cooldown::{CooldownLedger, Emit};
pub use ids::{RuleId, RuleIdError};
pub use rule::{MatchKind, Rule, RuleSetLimits};
pub use severity::Severity;
pub use structured::StructuredKind;
pub use truncate::truncate_for_match;
pub use verdict::Verdict;
