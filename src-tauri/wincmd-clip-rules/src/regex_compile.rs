// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{CompileError, RuleSetLimits};
use regex_automata::{meta::Regex, util::syntax};
pub(crate) fn build_regex(
    pattern: &str,
    case_sensitive: bool,
    fixed: bool,
    index: usize,
    limits: &RuleSetLimits,
    consumed: &mut usize,
) -> Result<Regex, CompileError> {
    if pattern.len() > limits.max_pattern_bytes {
        return Err(CompileError::PatternTooLong {
            index,
            limit: limits.max_pattern_bytes,
            actual: pattern.len(),
        });
    }
    if ["(?=", "(?!", "(?<=", "(?<!"]
        .iter()
        .any(|s| pattern.contains(s))
    {
        return Err(CompileError::LookaroundUnsupported { index });
    }
    if !fixed && consumed.saturating_add(limits.regex_size_limit) > limits.regex_total_size_limit {
        return Err(CompileError::RegexTotalProgramTooLarge {
            index,
            limit: limits.regex_total_size_limit,
            actual: consumed.saturating_add(limits.regex_size_limit),
        });
    }
    let re = Regex::builder()
        .syntax(
            syntax::Config::new()
                .case_insensitive(!case_sensitive)
                .unicode(!fixed)
                .utf8(!fixed),
        )
        .configure(
            Regex::config()
                .nfa_size_limit(Some(limits.regex_size_limit))
                .dfa_size_limit(Some(limits.regex_size_limit))
                .hybrid_cache_capacity(16 * 1024),
        )
        .build(pattern)
        .map_err(|error| {
            if error.size_limit().is_some() {
                CompileError::RegexProgramTooLarge {
                    index,
                    limit: limits.regex_size_limit,
                }
            } else {
                CompileError::RegexSyntax { index }
            }
        })?;
    *consumed = consumed.saturating_add(if fixed {
        re.memory_usage()
    } else {
        limits.regex_size_limit
    });
    if *consumed > limits.regex_total_size_limit {
        return Err(CompileError::RegexTotalProgramTooLarge {
            index,
            limit: limits.regex_total_size_limit,
            actual: *consumed,
        });
    }
    Ok(re)
}
