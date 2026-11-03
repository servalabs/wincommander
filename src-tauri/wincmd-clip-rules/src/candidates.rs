// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::compile::{CompiledMatcher, CompiledRule};
use crate::scan::MAX_SPANS;
use std::ops::Range;
pub(crate) struct Candidate {
    pub secret: Range<usize>,
    pub full: Range<usize>,
}
impl From<Range<usize>> for Candidate {
    fn from(range: Range<usize>) -> Self {
        Self {
            secret: range.clone(),
            full: range,
        }
    }
}
pub(crate) fn candidates(rule: &CompiledRule, text: &str) -> Vec<Candidate> {
    match &rule.matcher {
        CompiledMatcher::Phrase {
            needle,
            case_sensitive,
        } => {
            if needle.is_empty() {
                return vec![];
            }
            if *case_sensitive {
                return text
                    .match_indices(needle)
                    .take(MAX_SPANS)
                    .map(|(start, _)| Candidate::from(start..start + needle.len()))
                    .collect();
            }
            let mut lower = String::with_capacity(text.len());
            let mut mapping = Vec::new();
            for (index, c) in text.char_indices() {
                let start = lower.len();
                lower.extend(c.to_lowercase());
                for _ in start..lower.len() {
                    mapping.push((index, index + c.len_utf8()));
                }
            }
            lower
                .match_indices(needle)
                .take(MAX_SPANS)
                .filter_map(|(start, _)| {
                    Some(Candidate::from(
                        mapping.get(start)?.0..mapping.get(start + needle.len() - 1)?.1,
                    ))
                })
                .collect()
        }
        CompiledMatcher::Regex(regex) => regex
            .find_iter(text)
            .take(MAX_SPANS)
            .map(|m| Candidate::from(m.start()..m.end()))
            .collect(),
        CompiledMatcher::BuiltinProcedural(pattern) => {
            if crate::builtin::matches_procedural(*pattern, text) {
                procedural_spans(*pattern, text)
            } else {
                vec![]
            }
        }
        CompiledMatcher::Detector { regex, id } => {
            if id == "iban" {
                let mut spans = Vec::new();
                let mut cursor = 0;
                while cursor < text.len() && spans.len() < MAX_SPANS {
                    let Some(found) =
                        regex.find(regex_automata::Input::new(text).span(cursor..text.len()))
                    else {
                        break;
                    };
                    if let Some(span) = iban_span(text, found.start()..found.end()) {
                        spans.push(Candidate::from(span));
                    }
                    cursor = found.start() + 1;
                }
                return spans;
            }
            let secret = id.starts_with("gitleaks.");
            regex
                .captures_iter(text)
                .take(MAX_SPANS)
                .filter_map(|captures| {
                    let full = captures.get_group(0)?;
                    let span = if secret {
                        (1..regex.captures_len())
                            .find_map(|i| captures.get_group(i).filter(|g| g.start < g.end))
                            .unwrap_or(full)
                    } else {
                        full
                    };
                    if id == "iban" {
                        return iban_span(text, span.start..span.end).map(Candidate::from);
                    }
                    Some(Candidate {
                        secret: span.start..span.end,
                        full: full.start..full.end,
                    })
                })
                .collect()
        }
    }
}
fn iban_span(text: &str, span: Range<usize>) -> Option<Range<usize>> {
    let candidate = text.get(span.clone())?;
    let mut alnum = 0;
    for (index, b) in candidate.bytes().enumerate() {
        if b.is_ascii_alphanumeric() {
            alnum += 1;
        }
        if alnum >= 15
            && b.is_ascii_alphanumeric()
            && crate::pii::valid("iban", &candidate[..index + 1])
        {
            let end = span.start + index + 1;
            if text
                .as_bytes()
                .get(end)
                .is_none_or(|b| !b.is_ascii_alphanumeric())
            {
                return Some(span.start..end);
            }
        }
    }
    None
}
fn procedural_spans(pattern: crate::BuiltinPattern, text: &str) -> Vec<Candidate> {
    use crate::BuiltinPattern::*;
    match pattern {
        UnicodeBidiOverride => text
            .char_indices()
            .filter(|(_, c)| matches!(c, '\u{202D}' | '\u{202E}'))
            .map(|(i, c)| Candidate::from(i..i + c.len_utf8()))
            .collect(),
        UnicodeZeroWidthInCode => text
            .char_indices()
            .filter(|(i, c)| {
                matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')
                    && (text[..*i]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_ascii_alphanumeric())
                        || text[*i + c.len_utf8()..]
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_alphanumeric()))
            })
            .map(|(i, c)| Candidate::from(i..i + c.len_utf8()))
            .collect(),
        UnicodeConfusableUrlHost => {
            let position = text
                .find("http://")
                .into_iter()
                .chain(text.find("https://"))
                .min()
                .unwrap_or(0);
            let start = position
                + if text[position..].starts_with("https://") {
                    8
                } else {
                    7
                };
            let end = text[start..]
                .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace())
                .map(|i| start + i)
                .unwrap_or(text.len());
            vec![Candidate::from(start..end)]
        }
        _ => vec![],
    }
}
