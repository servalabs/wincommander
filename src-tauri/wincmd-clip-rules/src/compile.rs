// SPDX-License-Identifier: AGPL-3.0-or-later
//! `compile()` — the only fallible entry point — and the opaque
//! `CompiledRuleSet` it produces.

use std::fmt;

use crate::regex_compile::build_regex;
use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use regex_automata::meta::Regex;

use crate::{
    Action, BuiltinPattern, MatchKind, Rule, RuleId, RuleSetLimits, Severity, StructuredKind,
};

pub use crate::compile_errors::CompileError;
pub(crate) enum CompiledMatcher {
    Phrase {
        needle: String,
        case_sensitive: bool,
    },
    Regex(Regex),
    Detector {
        regex: Regex,
        id: String,
    },
    BuiltinProcedural(BuiltinPattern),
}
pub(crate) struct CompiledRule {
    pub id: RuleId,
    pub revision: u32,
    pub priority: u16,
    pub severity: Severity,
    pub actions: Vec<Action>,
    pub matcher: CompiledMatcher,
    pub options: crate::RuleOptions,
    pub detector_id: String,
    pub category: crate::DetectorCategory,
    pub keyword_ids: Vec<usize>,
    pub allows: Vec<(String, Regex, Vec<String>)>,
}
pub struct CompiledRuleSet {
    pub(crate) rules: Vec<CompiledRule>,
    pub(crate) max_text_bytes: usize,
    pub(crate) global_allows: Vec<Regex>,
    pub(crate) prefilter: Option<AhoCorasick>,
    pub(crate) keyword_count: usize,
    pub(crate) global_allow_list: Vec<String>,
}
impl fmt::Debug for CompiledRuleSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompiledRuleSet")
            .field("rules", &self.rules.len())
            .finish_non_exhaustive()
    }
}

pub fn compile(
    rules: &[Rule],
    limits: &RuleSetLimits,
) -> Result<CompiledRuleSet, Vec<CompileError>> {
    let mut errors = Vec::new();
    let enabled = rules.iter().filter(|r| r.enabled).count();
    if enabled > limits.max_enabled_rules {
        errors.push(CompileError::TooManyEnabledRules {
            limit: limits.max_enabled_rules,
            actual: enabled,
        });
    }
    let mut compiled = Vec::new();
    let mut consumed = 0usize;
    let mut global_allows = Vec::new();
    let mut keywords_by_id = Vec::<String>::new();
    let mut keyword_lookup = std::collections::HashMap::<String, usize>::new();
    if rules.iter().any(|r| {
        r.enabled && matches!(&r.matcher,MatchKind::Detector(id) if id.starts_with("gitleaks."))
    }) {
        for pattern in crate::catalog::GLOBAL_ALLOW_REGEXES {
            match build_regex(pattern, true, true, 0, limits, &mut consumed) {
                Ok(re) => global_allows.push(re),
                Err(error) => errors.push(error),
            }
        }
    }
    for (index, rule) in rules.iter().enumerate().filter(|(_, r)| r.enabled) {
        let options = &rule.options;
        if !crate::rule_validation::valid_options(rule) {
            errors.push(CompileError::InvalidRuleOptions { index });
            continue;
        }
        let mut keywords = options.keywords.clone();
        if let MatchKind::Builtin(pattern) = rule.matcher {
            keywords.extend(
                crate::catalog::legacy_keywords(pattern)
                    .iter()
                    .map(|s| s.to_string()),
            );
        }
        let mut allows = Vec::new();
        let matcher_result = (|| {
            Ok(match &rule.matcher {
                MatchKind::Phrase {
                    value,
                    case_sensitive,
                } => {
                    if value.len() > limits.max_pattern_bytes {
                        return Err(CompileError::PatternTooLong {
                            index,
                            limit: limits.max_pattern_bytes,
                            actual: value.len(),
                        });
                    }
                    CompiledMatcher::Phrase {
                        needle: if *case_sensitive {
                            value.clone()
                        } else {
                            value.to_lowercase()
                        },
                        case_sensitive: *case_sensitive,
                    }
                }
                MatchKind::Regex {
                    pattern,
                    case_sensitive,
                } => CompiledMatcher::Regex(build_regex(
                    pattern,
                    *case_sensitive,
                    false,
                    index,
                    limits,
                    &mut consumed,
                )?),
                MatchKind::Builtin(pattern) => match pattern.regex_source() {
                    Some(source) => CompiledMatcher::Regex(build_regex(
                        source,
                        true,
                        true,
                        index,
                        limits,
                        &mut consumed,
                    )?),
                    None => CompiledMatcher::BuiltinProcedural(*pattern),
                },
                MatchKind::Structured(kind) => {
                    let id = match kind {
                        StructuredKind::PaymentCard => "payment_card",
                        StructuredKind::Iban => "iban",
                    };
                    let definition = crate::pii::PII
                        .iter()
                        .find(|p| p.id == id)
                        .ok_or(CompileError::UnknownDetector { index })?;
                    CompiledMatcher::Detector {
                        regex: build_regex(
                            definition.pattern,
                            true,
                            true,
                            index,
                            limits,
                            &mut consumed,
                        )?,
                        id: id.into(),
                    }
                }
                MatchKind::Detector(id) => {
                    if let Some(p) = crate::pii::PII.iter().find(|p| p.id == id) {
                        CompiledMatcher::Detector {
                            regex: build_regex(
                                p.pattern,
                                true,
                                true,
                                index,
                                limits,
                                &mut consumed,
                            )?,
                            id: id.clone(),
                        }
                    } else if let Some(p) = crate::catalog::SECRET_RULES
                        .iter()
                        .find(|p| format!("gitleaks.{}", p.id) == *id)
                    {
                        keywords.extend(p.keywords.iter().map(|s| s.to_string()));
                        for allow in p.allowlists {
                            for pattern in allow.regexes {
                                allows.push((
                                    allow.target.to_string(),
                                    build_regex(pattern, true, true, index, limits, &mut consumed)?,
                                    allow.stopwords.iter().map(|s| s.to_string()).collect(),
                                ));
                            }
                        }
                        CompiledMatcher::Detector {
                            regex: build_regex(p.regex, true, true, index, limits, &mut consumed)?,
                            id: id.clone(),
                        }
                    } else {
                        return Err(CompileError::UnknownDetector { index });
                    }
                }
            })
        })();
        let matcher = match matcher_result {
            Ok(m) => m,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let keyword_ids = keywords
            .into_iter()
            .map(|keyword| {
                let keyword = keyword.to_ascii_lowercase();
                *keyword_lookup.entry(keyword.clone()).or_insert_with(|| {
                    let id = keywords_by_id.len();
                    keywords_by_id.push(keyword);
                    id
                })
            })
            .collect();
        if consumed > limits.regex_total_size_limit {
            errors.push(CompileError::RegexTotalProgramTooLarge {
                index,
                limit: limits.regex_total_size_limit,
                actual: consumed,
            });
            continue;
        }
        let (detector_id, category) = crate::catalog::matcher_metadata(&rule.matcher);
        compiled.push(CompiledRule {
            id: rule.id.clone(),
            revision: rule.revision,
            priority: rule.priority,
            severity: rule.severity,
            actions: rule.actions.clone(),
            matcher,
            options: options.clone(),
            detector_id,
            category,
            keyword_ids,
            allows,
        });
    }
    let prefilter = if keywords_by_id.is_empty() {
        None
    } else {
        match AhoCorasickBuilder::new()
            .ascii_case_insensitive(true)
            .build(&keywords_by_id)
        {
            Ok(filter) => {
                consumed = consumed.saturating_add(filter.memory_usage());
                Some(filter)
            }
            Err(_) => {
                errors.push(CompileError::InvalidRuleOptions { index: 0 });
                None
            }
        }
    };
    if consumed > limits.regex_total_size_limit {
        errors.push(CompileError::RegexTotalProgramTooLarge {
            index: 0,
            limit: limits.regex_total_size_limit,
            actual: consumed,
        });
    }
    compiled.sort_by(|a, b| b.priority.cmp(&a.priority).then(b.id.cmp(&a.id)));
    if errors.is_empty() {
        Ok(CompiledRuleSet {
            rules: compiled,
            max_text_bytes: limits.max_text_bytes.min(1_048_576),
            global_allows,
            prefilter,
            keyword_count: keywords_by_id.len(),
            global_allow_list: vec![],
        })
    } else {
        Err(errors)
    }
}

impl CompiledRuleSet {
    pub fn evaluate(&self, text: &str) -> Option<crate::Verdict> {
        self.scan(text, crate::Channel::Clipboard).verdict()
    }
}
