// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rule configuration types: `RuleSetLimits`, `MatchKind`, `Rule`.

use serde::{Deserialize, Serialize};

use crate::{Action, BuiltinPattern, RuleId, Severity, StructuredKind};

/// Bounds `compile()` enforces on an input ruleset. Every field is a hard
/// cap, not a soft target — exceeding one is a `CompileError`, never a
/// silent clamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "camelCase")]
pub struct RuleSetLimits {
    /// Max number of rules with `enabled: true`. Disabled rules (kept
    /// around as drafts / toggled-off presets) don't count and aren't
    /// compiled at all — see `compile()`.
    pub max_enabled_rules: usize,
    /// Max byte length of a `Phrase.value` or `Regex.pattern` string.
    pub max_pattern_bytes: usize,
    /// The clipboard-text read cap. NOT enforced by this crate — it's a
    /// contract on the caller, who must truncate (via
    /// `truncate_for_match`) before calling `CompiledRuleSet::evaluate`.
    pub max_text_bytes: usize,
    /// Per-rule compiled-regex-program byte bound, passed to both
    /// `RegexBuilder::size_limit` (NFA build size) and
    /// `RegexBuilder::dfa_size_limit` (lazy-DFA search-time cache) — see
    /// `compile.rs` for why one number governs both.
    pub regex_size_limit: usize,
    /// Summed bound across every regex-backed rule in the set (custom
    /// `Regex` rules AND regex-backed `Builtin` rules alike). The `regex`
    /// crate doesn't expose a compiled program's actual byte size after a
    /// successful build, so this is enforced by charging each successful
    /// compile its full `regex_size_limit` allowance against a running
    /// total — conservative (never under-counts worst-case memory), not
    /// exact.
    pub regex_total_size_limit: usize,
}

impl Default for RuleSetLimits {
    fn default() -> Self {
        Self {
            max_enabled_rules: 100,
            max_pattern_bytes: 2048,
            max_text_bytes: 1_048_576, // 1 MiB
            // Well under `regex`'s own 10 MiB default NFA size limit —
            // deliberately tight per-rule so no single rule (custom or
            // builtin) can eat a large slice of a background watcher's
            // memory budget.
            regex_size_limit: 256 * 1024, // 256 KiB
            // Headroom for ~32 rules at full per-rule cost simultaneously,
            // which is generous against `max_enabled_rules`'s default of
            // 100 (real rulesets mix cheap builtins with a handful of
            // custom regexes, not 100 maximally-sized ones) while still
            // bounding the worst case to a single-digit-MiB footprint.
            regex_total_size_limit: 8 * 1024 * 1024, // 8 MiB
        }
    }
}

/// What a rule matches against clipboard text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
// Adjacently tagged (`kind` + `params`), not internally tagged: the
// `Structured`/`Builtin` variants wrap another enum that itself serializes
// to a plain string, and serde's internal tagging only supports
// variants whose content serializes to a JSON object/map — a newtype
// wrapping a string-enum would fail to serialize under `tag = "kind"`
// alone. Adjacent tagging has no such restriction.
#[serde(tag = "kind", content = "params", rename_all = "snake_case")]
pub enum MatchKind {
    /// A literal substring match.
    Phrase { value: String, case_sensitive: bool },
    /// A Rust `regex`-compiled pattern. No lookahead/lookbehind — see
    /// `CompileError::LookaroundUnsupported`.
    Regex {
        pattern: String,
        case_sensitive: bool,
    },
    /// A checksum-validated structural format (payment card, IBAN).
    Structured(StructuredKind),
    /// One of the migrated free-tier patterns.
    Builtin(BuiltinPattern),
}

/// A single clipboard-guard rule, either custom (fleet-authored) or a
/// wrapper around one `BuiltinPattern`.
// `PartialEq`/`Eq`: consumers diff rulesets to decide whether a body actually
// changed (Fleet bumps a rule's `revision` only on a real change; the endpoint
// compares an incoming policy against the active one before swapping). Both are
// value comparisons on a pure data type, so deriving is correct and cheap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: RuleId,
    /// Bumped by the author on every edit — carried into `Verdict` so a
    /// consumer can tell whether a match was produced under the rule
    /// version it expects (e.g. before/after an admin edit lands).
    pub revision: u32,
    pub name: String,
    pub enabled: bool,
    /// 0..=1000, higher wins. Not enforced as a hard range by `compile()`
    /// (no `CompileError` variant exists for it) — `u16` accepts any
    /// value, and `evaluate()`'s max/tie-break logic is correct regardless
    /// of whether authors respect the documented range. The range is a
    /// UI/console convention, not a wire-level invariant.
    pub priority: u16,
    pub matcher: MatchKind,
    pub severity: Severity,
    pub actions: Vec<Action>,
    pub cooldown_seconds: u32,
    /// Whether a user-facing snooze can suppress this rule. Not read by
    /// this crate (`compile()`/`evaluate()` don't gate on it) — it's a
    /// hint for the endpoint's snooze UI to filter by.
    pub snoozable: bool,
    /// Whether the console should allow editing this rule. Not read by
    /// this crate — same as `snoozable`, a hint for the authoring UI.
    pub locked: bool,
}
