// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared, I/O-free content detection: validation, bounded scanning and local masking.
//!
//! Device and server rule tests use the same compiler and detector catalog.
//! Findings expose only detector metadata; matched values remain in the caller's
//! input. LocalSpans is deliberately not serializable and is omitted from report
//! serialization. Compilation errors and Debug output never echo patterns/content.
//!
//! Scanning caps input at 1 MiB and reports truncation for cooperative deadlines
//! and local span limits. Action execution and authorization remain the caller's
//! responsibility. This crate performs no clipboard, file or network I/O.

mod action;
mod base58;
mod builtin;
mod candidates;
mod catalog;
mod compile;
mod compile_errors;
mod content;
mod cooldown;
mod decoding;
mod iban;
mod ids;
mod legacy;
mod pii;
mod regex_compile;
mod rule;
mod rule_validation;
mod scan;
mod score;
mod severity;
mod structured;
mod truncate;
mod tuning;
mod verdict;

pub use action::Action;
pub use base58::base58_decode;
pub use builtin::BuiltinPattern;
pub use catalog::{
    builtin_rules, detector_catalog, detector_patterns, DetectorDescriptor, DetectorPattern,
};
pub use compile::{compile, CompileError, CompiledRuleSet};
pub use content::{
    mask_matches, mask_matches_for_rules, valid_content_class, Channel, DetectorCategory, Encoding,
    Finding, LocalSpans, Reason, RuleOptions, ScanReport,
};
pub use cooldown::{CooldownLedger, Emit};
pub use ids::{RuleId, RuleIdError};
pub use rule::{MatchKind, Rule, RuleSetLimits};
pub use scan::scan;
pub use score::shannon_entropy;
pub use severity::Severity;
pub use structured::StructuredKind;
pub use truncate::truncate_for_match;
pub use tuning::{
    compile_content_rules, content_rule_limits, resolve_content_rules, validate_content_rules,
    BuiltinTuning, ChannelActions, ContentRulesSubtree, ContentTuning, MAX_CONTENT_SUBTREE_BYTES,
};
pub use verdict::Verdict;
