// SPDX-License-Identifier: AGPL-3.0-or-later
use std::fmt;
/// Why a rule (or the ruleset as a whole) failed to compile. Every variant
/// identifies the offending rule BY INDEX ONLY (into the caller's `rules`
/// slice) and never carries the pattern text, the rule name, or any
/// clipboard text — the plan §8 reverse-leak rule. A `CompileError`'s
/// `Display`/`Debug` is safe to put directly into a `BadRequest` body or a
/// log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// More rules have `enabled: true` than `RuleSetLimits::
    /// max_enabled_rules` allows. Set-wide — no single rule is at fault,
    /// so there's no index.
    TooManyEnabledRules {
        limit: usize,
        actual: usize,
    },
    /// The rule at `index`'s `Phrase.value` or `Regex.pattern` exceeds
    /// `RuleSetLimits::max_pattern_bytes`.
    PatternTooLong {
        index: usize,
        limit: usize,
        actual: usize,
    },
    /// The rule at `index`'s regex uses lookahead/lookbehind
    /// (`(?=`, `(?!`, `(?<=`, `(?<!`) — syntax Rust `regex` does not
    /// support, though the fleet console's JS `RegExp` accepts it. Named
    /// separately from `RegexSyntax` so this specific, expected divergence
    /// reads as "unsupported construct", not "your regex is broken".
    LookaroundUnsupported {
        index: usize,
    },
    /// The rule at `index`'s regex failed to compile for a reason other
    /// than lookaround or exceeding the size limit (malformed syntax). The
    /// underlying `regex`-crate message is deliberately dropped — its
    /// syntax-error text can quote fragments of the pattern, which §8
    /// forbids regardless of the pattern's provenance.
    RegexSyntax {
        index: usize,
    },
    InvalidRuleOptions {
        index: usize,
    },
    UnknownDetector {
        index: usize,
    },
    /// The rule at `index`'s regex compiled to a program bigger than
    /// `RuleSetLimits::regex_size_limit` allows, even considered alone.
    RegexProgramTooLarge {
        index: usize,
        limit: usize,
    },
    /// Charging the rule at `index`'s full `regex_size_limit` allowance
    /// against the running total would exceed `RuleSetLimits::
    /// regex_total_size_limit`. `actual` is the projected total had this
    /// rule been admitted (conservative worst-case accounting — see
    /// `RuleSetLimits::regex_total_size_limit`'s doc comment).
    RegexTotalProgramTooLarge {
        index: usize,
        limit: usize,
        actual: usize,
    },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRuleOptions { index } => write!(f,"rule[{index}]: invalid content options"),
            Self::UnknownDetector { index } => write!(f,"rule[{index}]: unknown detector"),
            CompileError::TooManyEnabledRules { limit, actual } => write!(
                f,
                "too many enabled rules: {actual} enabled, limit is {limit}"
            ),
            CompileError::PatternTooLong { index, limit, actual } => write!(
                f,
                "rule[{index}]: pattern is {actual} bytes, limit is {limit}"
            ),
            CompileError::LookaroundUnsupported { index } => write!(
                f,
                "rule[{index}]: regex uses lookahead/lookbehind, which is unsupported"
            ),
            CompileError::RegexSyntax { index } => {
                write!(f, "rule[{index}]: regex failed to compile")
            }
            CompileError::RegexProgramTooLarge { index, limit } => write!(
                f,
                "rule[{index}]: compiled regex program exceeds the {limit}-byte per-rule limit"
            ),
            CompileError::RegexTotalProgramTooLarge { index, limit, actual } => write!(
                f,
                "rule[{index}]: would bring the ruleset's total compiled regex size to {actual} bytes, exceeding the {limit}-byte limit"
            ),
        }
    }
}

impl std::error::Error for CompileError {}
