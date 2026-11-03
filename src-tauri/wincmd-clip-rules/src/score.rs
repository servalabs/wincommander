// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::compile::{CompiledMatcher, CompiledRule};
use crate::Reason;
use regex_automata::meta::Regex;
use std::ops::Range;
pub(crate) fn score_candidate(
    rule: &CompiledRule,
    text: &str,
    span: &Range<usize>,
    full_span: &Range<usize>,
    reasons: &mut Vec<Reason>,
    global_allows: &[Regex],
) -> u8 {
    let value = &text[span.clone()];
    let mut score = 60u8;
    let mut context: Vec<&str> = rule.options.context.iter().map(String::as_str).collect();
    if let CompiledMatcher::Detector { id, .. } = &rule.matcher {
        if let Some(p) = crate::pii::PII.iter().find(|p| p.id == id) {
            if !crate::pii::valid(id, value) {
                return 0;
            }
            score = p.base_score;
            context.extend(p.context.iter().copied());
            if crate::pii::checksum(id) {
                score = score.saturating_add(20);
                reasons.push(Reason::ChecksumValid);
            }
        } else if let Some(p) = crate::catalog::SECRET_RULES
            .iter()
            .find(|p| format!("gitleaks.{}", p.id) == *id)
        {
            let full_match = &text[full_span.clone()];
            let line_start = text[..span.start].rfind('\n').map(|n| n + 1).unwrap_or(0);
            let line_end = text[span.end..]
                .find('\n')
                .map(|n| span.end + n)
                .unwrap_or(text.len());
            let lower = value.to_ascii_lowercase();
            if global_allows.iter().any(|allow| allow.is_match(value))
                || crate::catalog::GLOBAL_STOPWORDS
                    .iter()
                    .any(|stop| lower.contains(stop))
                || rule.allows.iter().any(|(target, regex, stops)| {
                    regex.is_match(match target.as_str() {
                        "match" => full_match,
                        "line" => &text[line_start..line_end],
                        _ => value,
                    }) || stops.iter().any(|stop| lower.contains(stop))
                })
            {
                return 0;
            }
            if p.entropy > 0.0 {
                if shannon_entropy(value) <= p.entropy {
                    return 0;
                }
                score = score.saturating_add(20);
                reasons.push(Reason::EntropyAbove);
            }
        }
    }
    if context_near(text, span, &context) {
        score = score.saturating_add(20);
        reasons.push(Reason::ContextWord);
    }
    score.min(100)
}
fn context_near(text: &str, span: &Range<usize>, context: &[&str]) -> bool {
    let left: Vec<_> = text[..span.start]
        .rsplit(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .take(5)
        .collect();
    let right = text[span.end..]
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .take(5);
    left.into_iter().chain(right).any(|word| {
        context
            .iter()
            .any(|context| word.eq_ignore_ascii_case(context))
    })
}
pub fn shannon_entropy(value: &str) -> f64 {
    if value.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for b in value.bytes() {
        counts[usize::from(b)] += 1;
    }
    counts
        .iter()
        .filter(|n| **n > 0)
        .map(|n| {
            let p = *n as f64 / value.len() as f64;
            -p * p.log2()
        })
        .sum()
}
