// SPDX-License-Identifier: AGPL-3.0-or-later
//! `compile()` — the only fallible entry point — and the opaque
//! `CompiledRuleSet` it produces.

use std::fmt;

use regex::{Regex, RegexBuilder};

use crate::{
    Action, BuiltinPattern, MatchKind, Rule, RuleId, RuleSetLimits, Severity, StructuredKind,
};

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
    TooManyEnabledRules { limit: usize, actual: usize },
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
    LookaroundUnsupported { index: usize },
    /// The rule at `index`'s regex failed to compile for a reason other
    /// than lookaround or exceeding the size limit (malformed syntax). The
    /// underlying `regex`-crate message is deliberately dropped — its
    /// syntax-error text can quote fragments of the pattern, which §8
    /// forbids regardless of the pattern's provenance.
    RegexSyntax { index: usize },
    /// The rule at `index`'s regex compiled to a program bigger than
    /// `RuleSetLimits::regex_size_limit` allows, even considered alone.
    RegexProgramTooLarge { index: usize, limit: usize },
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

/// What a compiled rule's matcher actually runs at evaluate-time. Every
/// real regex — whether authored as `MatchKind::Regex` or sourced from a
/// `BuiltinPattern` — ends up here, compiled through the exact same
/// size-limited path (see `compile_regex`), so a custom rule and a builtin
/// rule behave identically once compiled.
enum CompiledMatcher {
    Phrase {
        needle: String,
        case_sensitive: bool,
    },
    Regex(Regex),
    Structured(StructuredKind),
    /// Only ever holds one of the three procedural `Unicode*` variants —
    /// regex-backed builtins are compiled into `Regex` above instead.
    BuiltinProcedural(BuiltinPattern),
}

struct CompiledRule {
    id: RuleId,
    revision: u32,
    priority: u16,
    severity: Severity,
    actions: Vec<Action>,
    matcher: CompiledMatcher,
}

/// A validated, ready-to-match ruleset. Opaque by design — the only way to
/// get one is `compile()`, and the only thing you can do with one is
/// `evaluate()`.
///
/// See the hand-written [`std::fmt::Debug`] impl below: it is deliberately
/// pattern-free.
pub struct CompiledRuleSet {
    rules: Vec<CompiledRule>,
    /// `true` if any compiled `Phrase` rule is case-insensitive. Lets
    /// `evaluate()` skip lowercasing the input text entirely for rulesets
    /// that don't need it (regex case-insensitivity is handled by the
    /// regex engine itself, not by lowercasing text — see `compile_regex`).
    needs_lowercase: bool,
}

/// Hand-written rather than derived, and that is load-bearing.
///
/// `CompiledMatcher::Regex` wraps a [`regex::Regex`], whose own `Debug` impl
/// prints the **pattern source**. Deriving `Debug` here would therefore make
/// `{:?}` on a ruleset emit every rule's pattern — precisely the reverse leak
/// §8 exists to prevent, and it would do so through the one formatting call
/// that reviewers habitually treat as harmless. A panic message from
/// `unwrap_err()` in a test, or a stray `tracing` field, would be enough.
///
/// So this prints shape only: how many rules compiled and whether the
/// lowercase fast path is engaged. Never a pattern, a rule name, or an id.
impl std::fmt::Debug for CompiledRuleSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledRuleSet")
            .field("rules", &self.rules.len())
            .field("needs_lowercase", &self.needs_lowercase)
            .finish_non_exhaustive()
    }
}

/// The ONLY fallible entry point. Validates every limit in `limits` and
/// compiles every regex (custom and builtin alike), collecting every
/// violation found rather than stopping at the first one — so a console
/// "validate this ruleset" panel can show every problem in one pass, not
/// one-error-per-save-attempt.
///
/// Only rules with `enabled: true` are compiled or counted against any
/// limit; disabled rules are ignored entirely (kept in the input purely so
/// callers can persist drafts / toggled-off presets without losing them).
pub fn compile(
    rules: &[Rule],
    limits: &RuleSetLimits,
) -> Result<CompiledRuleSet, Vec<CompileError>> {
    let mut errors = Vec::new();

    let enabled_count = rules.iter().filter(|r| r.enabled).count();
    if enabled_count > limits.max_enabled_rules {
        errors.push(CompileError::TooManyEnabledRules {
            limit: limits.max_enabled_rules,
            actual: enabled_count,
        });
    }

    let mut compiled: Vec<CompiledRule> = Vec::with_capacity(enabled_count);
    let mut consumed_total: usize = 0;
    let mut needs_lowercase = false;

    for (index, rule) in rules.iter().enumerate() {
        if !rule.enabled {
            continue;
        }

        let matcher = match &rule.matcher {
            MatchKind::Phrase {
                value,
                case_sensitive,
            } => {
                if value.len() > limits.max_pattern_bytes {
                    errors.push(CompileError::PatternTooLong {
                        index,
                        limit: limits.max_pattern_bytes,
                        actual: value.len(),
                    });
                    continue;
                }
                let (needle, case_sensitive) = if *case_sensitive {
                    (value.clone(), true)
                } else {
                    needs_lowercase = true;
                    (value.to_lowercase(), false)
                };
                CompiledMatcher::Phrase {
                    needle,
                    case_sensitive,
                }
            }
            MatchKind::Regex {
                pattern,
                case_sensitive,
            } => {
                match compile_regex(pattern, *case_sensitive, index, limits, &mut consumed_total) {
                    Ok(re) => CompiledMatcher::Regex(re),
                    Err(e) => {
                        errors.push(e);
                        continue;
                    }
                }
            }
            MatchKind::Structured(kind) => CompiledMatcher::Structured(*kind),
            MatchKind::Builtin(bp) => match bp.regex_source() {
                Some(src) => {
                    // `case_sensitive: true` — no builder-level override;
                    // several builtins embed an inline `(?i)` in the
                    // pattern text itself, and that inline flag must be the
                    // only source of truth (see `BuiltinPattern::
                    // regex_source`'s doc comment).
                    match compile_regex(src, true, index, limits, &mut consumed_total) {
                        Ok(re) => CompiledMatcher::Regex(re),
                        Err(e) => {
                            errors.push(e);
                            continue;
                        }
                    }
                }
                None => CompiledMatcher::BuiltinProcedural(*bp),
            },
        };

        compiled.push(CompiledRule {
            id: rule.id.clone(),
            revision: rule.revision,
            priority: rule.priority,
            severity: rule.severity,
            actions: rule.actions.clone(),
            matcher,
        });
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(CompiledRuleSet {
        rules: compiled,
        needs_lowercase,
    })
}

/// `true` if `pattern` contains one of the four PCRE-style lookaround
/// tokens. A plain substring scan, not a full parse — deliberately, so it
/// runs BEFORE `RegexBuilder::build()` and its syntax-error path can ever
/// see (and potentially echo, via `regex::Error::Syntax`'s message) the
/// pattern text. Matching `(?<name>` (a real, Rust-supported named capture
/// group) is avoided by requiring the exact 4-byte `(?<=`/`(?<!` tokens,
/// not just `(?<`.
fn has_lookaround(pattern: &str) -> bool {
    pattern.contains("(?=")
        || pattern.contains("(?!")
        || pattern.contains("(?<=")
        || pattern.contains("(?<!")
}

/// Compile one regex through the shared, size-limited path every real
/// regex in this crate goes through (custom `MatchKind::Regex` rules and
/// regex-backed `BuiltinPattern`s alike).
///
/// `consumed_total` is charged the rule's FULL `regex_size_limit`
/// allowance on success, never the program's actual size — `regex` doesn't
/// expose that number for a successful build, so this conservatively
/// assumes worst case rather than under-counting (see `RuleSetLimits::
/// regex_total_size_limit`).
fn compile_regex(
    pattern: &str,
    case_sensitive: bool,
    index: usize,
    limits: &RuleSetLimits,
    consumed_total: &mut usize,
) -> Result<Regex, CompileError> {
    if pattern.len() > limits.max_pattern_bytes {
        return Err(CompileError::PatternTooLong {
            index,
            limit: limits.max_pattern_bytes,
            actual: pattern.len(),
        });
    }
    if has_lookaround(pattern) {
        return Err(CompileError::LookaroundUnsupported { index });
    }

    // Check the set-wide budget BEFORE attempting the (potentially
    // expensive) build — fail fast rather than doing compile work whose
    // result we'd discard anyway.
    let projected_total = *consumed_total + limits.regex_size_limit;
    if projected_total > limits.regex_total_size_limit {
        return Err(CompileError::RegexTotalProgramTooLarge {
            index,
            limit: limits.regex_total_size_limit,
            actual: projected_total,
        });
    }

    let built = RegexBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .size_limit(limits.regex_size_limit)
        .dfa_size_limit(limits.regex_size_limit)
        .build();

    match built {
        Ok(re) => {
            *consumed_total = projected_total;
            Ok(re)
        }
        Err(regex::Error::CompiledTooBig(_)) => Err(CompileError::RegexProgramTooLarge {
            index,
            limit: limits.regex_size_limit,
        }),
        Err(_) => Err(CompileError::RegexSyntax { index }),
    }
}

impl CompiledRuleSet {
    /// Highest-`priority` match wins; ties broken by the lexicographically
    /// greater `RuleId` (an arbitrary but deterministic and stable total
    /// order — any consistent tiebreak works, this just needs to be the
    /// same one every time). `text` MUST already be truncated to
    /// `RuleSetLimits::max_text_bytes` by the caller (e.g. via
    /// `truncate_for_match`) — `evaluate` trusts that precondition and does
    /// not re-check it.
    pub fn evaluate(&self, text: &str) -> Option<crate::Verdict> {
        let lower = if self.needs_lowercase {
            Some(text.to_lowercase())
        } else {
            None
        };

        let mut best: Option<&CompiledRule> = None;
        for rule in &self.rules {
            let is_match = match &rule.matcher {
                CompiledMatcher::Phrase {
                    needle,
                    case_sensitive,
                } => {
                    if *case_sensitive {
                        text.contains(needle.as_str())
                    } else {
                        // `needs_lowercase` is set at compile-time whenever
                        // a case-insensitive Phrase rule exists, so `lower`
                        // is normally `Some` here already. Falling back to
                        // an inline `to_lowercase()` (rather than
                        // `.expect()`) keeps this branch correct even if
                        // that invariant were ever violated, instead of
                        // turning an internal bookkeeping slip into a
                        // clipboard-watcher panic.
                        match &lower {
                            Some(l) => l.contains(needle.as_str()),
                            None => text.to_lowercase().contains(needle.as_str()),
                        }
                    }
                }
                CompiledMatcher::Regex(re) => re.is_match(text),
                CompiledMatcher::Structured(kind) => crate::structured::matches(*kind, text),
                CompiledMatcher::BuiltinProcedural(bp) => {
                    crate::builtin::matches_procedural(*bp, text)
                }
            };
            if !is_match {
                continue;
            }
            best = Some(match best {
                None => rule,
                Some(current) => {
                    if rule.priority > current.priority
                        || (rule.priority == current.priority && rule.id > current.id)
                    {
                        rule
                    } else {
                        current
                    }
                }
            });
        }

        best.map(|r| crate::Verdict {
            rule_id: r.id.clone(),
            rule_revision: r.revision,
            severity: r.severity,
            actions: r.actions.clone(),
        })
    }
}
