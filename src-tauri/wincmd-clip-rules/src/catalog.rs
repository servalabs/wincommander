// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{
    Action, Channel, DetectorCategory, MatchKind, Rule, RuleId, RuleOptions, Severity,
    StructuredKind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "camelCase")]
pub struct DetectorDescriptor {
    pub id: String,
    pub name: String,
    pub category: DetectorCategory,
    pub severity: Severity,
    pub default_enabled: bool,
    pub threshold: u8,
}

pub(crate) struct SecretRule {
    pub id: &'static str,
    pub regex: &'static str,
    pub keywords: &'static [&'static str],
    pub entropy: f64,
    pub allowlists: &'static [SecretAllowlist],
    pub default_enabled: bool,
}
pub(crate) struct SecretAllowlist {
    pub target: &'static str,
    pub regexes: &'static [&'static str],
    pub stopwords: &'static [&'static str],
}
#[path = "secret_rules.rs"]
mod secret_rules;
pub(crate) use secret_rules::{GLOBAL_ALLOW_REGEXES, GLOBAL_STOPWORDS, SECRET_RULES};

pub(crate) use crate::legacy::{legacy_category, legacy_id, legacy_keywords, LEGACY};
pub(crate) fn secret_category(id: &str) -> DetectorCategory {
    if id.contains("private-key") || id.contains("secret-key") {
        DetectorCategory::PrivateKey
    } else if id.contains("aws-") || id.contains("gcp-") || id.contains("azure-") {
        DetectorCategory::CloudKey
    } else if id.contains("connection") || id.contains("database") || id.contains("mongodb") {
        DetectorCategory::ConnectionString
    } else {
        DetectorCategory::Token
    }
}

pub fn detector_catalog() -> Vec<DetectorDescriptor> {
    let mut catalog: Vec<_> = LEGACY
        .iter()
        .map(|p| DetectorDescriptor {
            id: legacy_id(*p),
            name: p.display_name().into(),
            category: legacy_category(*p),
            severity: p.default_severity(),
            default_enabled: true,
            threshold: 50,
        })
        .collect();
    catalog.extend(crate::pii::PII.iter().map(|p| DetectorDescriptor {
        id: p.id.into(),
        name: p.name.into(),
        category: p.category,
        severity: if p.category == DetectorCategory::ContactInfo {
            Severity::Info
        } else {
            Severity::Warn
        },
        default_enabled: p.category != DetectorCategory::ContactInfo,
        threshold: if p.id == "payment_card" { 90 } else { 50 },
    }));
    catalog.extend(SECRET_RULES.iter().map(|p| DetectorDescriptor {
        id: format!("gitleaks.{}", p.id),
        name: p.id.replace('-', " "),
        category: secret_category(p.id),
        severity: Severity::Warn,
        default_enabled: p.default_enabled,
        threshold: 50,
    }));
    catalog
}

pub fn builtin_rules() -> Vec<Rule> {
    detector_catalog()
        .into_iter()
        .enumerate()
        .map(|(index, descriptor)| {
            let legacy = LEGACY.iter().find(|p| legacy_id(**p) == descriptor.id);
            let matcher = if let Some(pattern) = legacy {
                MatchKind::Builtin(*pattern)
            } else if descriptor.id == "payment_card" {
                MatchKind::Structured(StructuredKind::PaymentCard)
            } else if descriptor.id == "iban" {
                MatchKind::Structured(StructuredKind::Iban)
            } else {
                MatchKind::Detector(descriptor.id.clone())
            };
            let legacy_index = legacy
                .and_then(|p| LEGACY.iter().position(|v| v == p))
                .map(|i| if i >= 29 { i + 1 } else { i });
            let id = if descriptor.id == "payment_card" {
                format!("{:032x}", 29)
            } else if let Some(i) = legacy_index {
                format!("{i:032x}")
            } else {
                Sha256::digest(descriptor.id.as_bytes())[..16]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect()
            };
            Rule {
                id: RuleId::new(id).expect("generated 32-hex id"),
                revision: 1,
                name: descriptor.name,
                enabled: descriptor.default_enabled,
                priority: 1000u16.saturating_sub(index as u16),
                matcher,
                severity: descriptor.severity,
                actions: vec![Action::NotifyUser],
                cooldown_seconds: 30,
                snoozable: true,
                locked: true,
                options: RuleOptions {
                    channels: Channel::ALL.to_vec(),
                    threshold: descriptor.threshold,
                    ..RuleOptions::default()
                },
            }
        })
        .collect()
}

pub(crate) fn matcher_metadata(matcher: &MatchKind) -> (String, DetectorCategory) {
    match matcher {
        MatchKind::Builtin(pattern) => (legacy_id(*pattern), legacy_category(*pattern)),
        MatchKind::Structured(StructuredKind::PaymentCard) => {
            ("payment_card".into(), DetectorCategory::PaymentCard)
        }
        MatchKind::Structured(StructuredKind::Iban) => {
            ("iban".into(), DetectorCategory::BankAccount)
        }
        MatchKind::Detector(id) => detector_catalog()
            .into_iter()
            .find(|p| p.id == *id)
            .map(|p| (p.id, p.category))
            .unwrap_or(("custom".into(), DetectorCategory::Custom)),
        _ => ("custom".into(), DetectorCategory::Custom),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectorPattern {
    pub detector_id: String,
    pub category: DetectorCategory,
    pub pattern: String,
    pub validator_required: bool,
}
pub fn detector_patterns() -> Vec<DetectorPattern> {
    let mut patterns: Vec<_> = LEGACY
        .iter()
        .filter_map(|p| {
            p.regex_source().map(|pattern| DetectorPattern {
                detector_id: legacy_id(*p),
                category: legacy_category(*p),
                pattern: pattern.into(),
                validator_required: false,
            })
        })
        .collect();
    patterns.extend(crate::pii::PII.iter().map(|p| DetectorPattern {
        detector_id: p.id.into(),
        category: p.category,
        pattern: p.pattern.into(),
        validator_required: true,
    }));
    patterns.extend(SECRET_RULES.iter().map(|p| DetectorPattern {
        detector_id: format!("gitleaks.{}", p.id),
        category: secret_category(p.id),
        pattern: p.regex.into(),
        validator_required: p.entropy > 0.0 || !p.allowlists.is_empty(),
    }));
    patterns
}
