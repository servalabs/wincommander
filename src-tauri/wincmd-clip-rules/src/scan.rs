// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::candidates::candidates;
use crate::content::LocalSpan;
use crate::score::score_candidate;
use crate::{Channel, CompiledRuleSet, Finding, Reason, ScanReport};
use std::time::{Duration, Instant};

pub(crate) const MAX_SPANS: usize = 4096;
pub fn scan(text: &str, rules: &CompiledRuleSet, channel: Channel) -> ScanReport {
    rules.scan(text, channel)
}
impl CompiledRuleSet {
    pub fn scan(&self, text: &str, channel: Channel) -> ScanReport {
        self.scan_with_budget(text, channel, Duration::from_millis(50))
    }
    fn present_keywords(&self, text: &str) -> Vec<bool> {
        let mut present = vec![false; self.keyword_count];
        if let Some(prefilter) = &self.prefilter {
            for matched in prefilter.find_overlapping_iter(text) {
                present[matched.pattern().as_usize()] = true;
            }
        }
        present
    }
    /// Cooperative deadline checked between bounded matcher searches and findings.
    pub fn scan_with_budget(&self, text: &str, channel: Channel, budget: Duration) -> ScanReport {
        let start = Instant::now();
        let original_len = text.len();
        let bounded = crate::truncate_for_match(text, self.max_text_bytes);
        let text = bounded;
        let mut report = ScanReport {
            truncated: text.len() < original_len,
            ..ScanReport::default()
        };
        let decoded = crate::decoding::decode_candidates(text);
        let mut inputs = vec![(text, None, None, self.present_keywords(text))];
        inputs.extend(decoded.iter().map(|d| {
            (
                d.text.as_str(),
                Some(d.range.clone()),
                Some(d.encoding),
                self.present_keywords(&d.text),
            )
        }));
        let mut exhausted = false;
        for rule in &self.rules {
            if start.elapsed() >= budget || report.local_spans.len() >= MAX_SPANS {
                exhausted = true;
                break;
            }
            if !rule.options.channels.contains(&channel) {
                continue;
            }
            let mut finding: Option<Finding> = None;
            for (input, encoded_range, encoding, present) in &inputs {
                if start.elapsed() >= budget {
                    exhausted = true;
                    break;
                }
                if encoding.is_some()
                    && !matches!(
                        rule.category,
                        crate::DetectorCategory::CloudKey
                            | crate::DetectorCategory::Token
                            | crate::DetectorCategory::PrivateKey
                            | crate::DetectorCategory::ConnectionString
                    )
                {
                    continue;
                }
                if !rule.keyword_ids.is_empty() && !rule.keyword_ids.iter().any(|id| present[*id]) {
                    continue;
                }
                let candidates = candidates(rule, input);
                for candidate in candidates {
                    if start.elapsed() >= budget || report.local_spans.len() >= MAX_SPANS {
                        exhausted = true;
                        break;
                    }
                    let Some(value) = input.get(candidate.secret.clone()) else {
                        continue;
                    };
                    if value.is_empty()
                        || self
                            .global_allow_list
                            .iter()
                            .any(|allow| allow.eq_ignore_ascii_case(value))
                        || rule
                            .options
                            .allow_list
                            .iter()
                            .any(|allow| allow.eq_ignore_ascii_case(value))
                    {
                        continue;
                    }
                    let mut reasons = Vec::new();
                    let score = score_candidate(
                        rule,
                        input,
                        &candidate.secret,
                        &candidate.full,
                        &mut reasons,
                        &self.global_allows,
                    );
                    if score == 0 || score < rule.options.threshold {
                        continue;
                    }
                    if !rule.keyword_ids.is_empty() {
                        reasons.push(Reason::KeywordPrefilter);
                    }
                    if let Some(encoding) = encoding {
                        reasons.push(Reason::Decoded(*encoding));
                    }
                    let span = encoded_range.clone().unwrap_or(candidate.full);
                    if report
                        .local_spans
                        .0
                        .iter()
                        .any(|existing| existing.rule_id == rule.id && existing.range == span)
                    {
                        continue;
                    }
                    report.local_spans.0.push(LocalSpan {
                        range: span,
                        rule_id: rule.id.clone(),
                        category: rule.category,
                    });
                    let entry = finding.get_or_insert_with(|| Finding {
                        rule_id: rule.id.clone(),
                        rule_revision: rule.revision,
                        detector_id: rule.detector_id.clone(),
                        category: rule.category,
                        score,
                        count: 0,
                        reasons: vec![],
                        severity: rule.severity,
                        actions: rule
                            .options
                            .channel_actions
                            .iter()
                            .find(|p| p.channel == channel)
                            .map(|p| p.actions.clone())
                            .unwrap_or_else(|| rule.actions.clone()),
                        priority: rule.priority,
                    });
                    entry.count = entry.count.saturating_add(1);
                    entry.score = entry.score.max(score);
                    for reason in reasons {
                        if !entry.reasons.contains(&reason) {
                            entry.reasons.push(reason);
                        }
                    }
                }
                if exhausted {
                    break;
                }
            }
            if let Some(finding) = finding {
                report.findings.push(finding);
            }
            if exhausted {
                break;
            }
        }
        report.truncated |=
            exhausted || report.local_spans.len() >= MAX_SPANS || decoded.len() >= 64;
        report
    }
}
