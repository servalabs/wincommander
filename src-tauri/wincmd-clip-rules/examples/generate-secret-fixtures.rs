// SPDX-License-Identifier: AGPL-3.0-or-later
// Offline authoring aid: generated strings are synthetic regex fixtures, never credentials.
use regex_syntax::hir::{Class, Hir, HirKind};
use serde_json::json;
use wincmd_clip_rules::*;

fn next(seed: &mut u64) -> usize {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed as usize
}
fn emit(hir: &Hir, seed: &mut u64, result: &mut Vec<u8>) {
    match hir.kind() {
        HirKind::Empty | HirKind::Look(_) => {}
        HirKind::Literal(literal) => result.extend_from_slice(&literal.0),
        HirKind::Class(class) => {
            let alphabet: Vec<u8> = match class {
                Class::Bytes(class) => class
                    .ranges()
                    .iter()
                    .flat_map(|range| range.start()..=range.end())
                    .filter(|b| (32..=126).contains(b))
                    .collect(),
                Class::Unicode(class) => (32u8..=126)
                    .filter(|b| {
                        class.ranges().iter().any(|range| {
                            range.start() <= char::from(*b) && range.end() >= char::from(*b)
                        })
                    })
                    .collect(),
            };
            if !alphabet.is_empty() {
                result.push(alphabet[next(seed) % alphabet.len()]);
            }
        }
        HirKind::Repetition(rep) => {
            let min = rep.min as usize;
            let max = rep.max.map(|n| n as usize).unwrap_or(min + 8).min(512);
            let count = min + (next(seed) % (max.saturating_sub(min) + 1));
            for _ in 0..count {
                emit(&rep.sub, seed, result);
            }
        }
        HirKind::Capture(capture) => emit(&capture.sub, seed, result),
        HirKind::Concat(parts) => {
            for part in parts {
                emit(part, seed, result);
            }
        }
        HirKind::Alternation(parts) => emit(&parts[next(seed) % parts.len()], seed, result),
    }
}
pub fn generate() -> Vec<serde_json::Value> {
    let patterns = detector_patterns();
    let rules = builtin_rules();
    let mut corpus = Vec::new();
    let mut missing = Vec::new();
    let mut seed = 0xe719facb70921u64;
    for pattern in patterns
        .iter()
        .filter(|p| p.detector_id.starts_with("gitleaks."))
    {
        let mut rule = rules
            .iter()
            .find(|r| matches!(&r.matcher,MatchKind::Detector(id) if *id==pattern.detector_id))
            .expect("catalog rule")
            .clone();
        rule.enabled = true;
        let compiled = compile(&[rule], &content_rule_limits()).expect("valid rule");
        let hir = regex_syntax::ParserBuilder::new()
            .unicode(false)
            .utf8(false)
            .build()
            .parse(&pattern.pattern)
            .expect("valid regex syntax");
        let mut fixture = None;
        for _ in 0..2048 {
            let mut bytes = Vec::new();
            emit(&hir, &mut seed, &mut bytes);
            let text = match pattern.detector_id.as_str() {
                "gitleaks.facebook-access-token" => {
                    format!("facebook {}", String::from_utf8_lossy(&bytes))
                }
                "gitleaks.sidekiq-sensitive-url" => {
                    format!("https://{:08x}:{:08x}@{}", 0x1a2b3c4du32, 0x4f5e6d7cu32, "gems.contribsys.com")
                }
                "gitleaks.slack-webhook-url" => {
                    format!("https://{}/{}/{}", "hooks.slack.com", "services", "A1b2C3d4".repeat(6))
                }
                _ => String::from_utf8_lossy(&bytes).into_owned(),
            };
            let report = compiled.scan(&text, Channel::Clipboard);
            if !report.findings.is_empty() && !report.truncated {
                fixture = Some(text);
                break;
            }
        }
        if let Some(text) = fixture {
            corpus.push(json!({"detectorId":pattern.detector_id,"text":text}));
        } else {
            missing.push(pattern.detector_id.clone());
        }
    }
    assert!(missing.is_empty(), "uncovered detector ids: {missing:?}");
    corpus
}

#[allow(dead_code)]
fn main() {
    println!("{}", serde_json::to_string_pretty(&generate()).expect("fixtures JSON"));
}
