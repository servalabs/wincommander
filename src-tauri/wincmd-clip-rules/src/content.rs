// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{Action, RuleId, Severity, Verdict};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Clipboard,
    Removable,
    AtRestScan,
}

impl Channel {
    pub const ALL: [Self; 3] = [Self::Clipboard, Self::Removable, Self::AtRestScan];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clipboard => "clipboard",
            Self::Removable => "removable",
            Self::AtRestScan => "at_rest_scan",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum DetectorCategory {
    PaymentCard,
    BankAccount,
    NationalId,
    TaxId,
    Health,
    ContactInfo,
    CloudKey,
    Token,
    PrivateKey,
    ConnectionString,
    Crypto,
    Custom,
}

impl DetectorCategory {
    pub const ALL: [Self; 12] = [
        Self::PaymentCard,
        Self::BankAccount,
        Self::NationalId,
        Self::TaxId,
        Self::Health,
        Self::ContactInfo,
        Self::CloudKey,
        Self::Token,
        Self::PrivateKey,
        Self::ConnectionString,
        Self::Crypto,
        Self::Custom,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PaymentCard => "payment_card",
            Self::BankAccount => "bank_account",
            Self::NationalId => "national_id",
            Self::TaxId => "tax_id",
            Self::Health => "health",
            Self::ContactInfo => "contact_info",
            Self::CloudKey => "cloud_key",
            Self::Token => "token",
            Self::PrivateKey => "private_key",
            Self::ConnectionString => "connection_string",
            Self::Crypto => "crypto",
            Self::Custom => "custom",
        }
    }
}

pub fn valid_content_class(class: &str) -> bool {
    let Some((channel, category)) = class.split_once('.') else {
        return false;
    };
    Channel::ALL.iter().any(|c| c.as_str() == channel)
        && DetectorCategory::ALL.iter().any(|c| c.as_str() == category)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    Base64,
    Hex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(tag = "kind", content = "encoding", rename_all = "snake_case")]
pub enum Reason {
    ChecksumValid,
    ContextWord,
    EntropyAbove,
    Decoded(Encoding),
    KeywordPrefilter,
}

pub fn default_threshold() -> u8 {
    50
}
fn default_channels() -> Vec<Channel> {
    vec![Channel::Clipboard]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "camelCase", default)]
pub struct RuleOptions {
    pub keywords: Vec<String>,
    pub context: Vec<String>,
    pub threshold: u8,
    pub channels: Vec<Channel>,
    pub allow_list: Vec<String>,
    pub channel_actions: Vec<crate::ChannelActions>,
}
impl Default for RuleOptions {
    fn default() -> Self {
        Self {
            keywords: vec![],
            context: vec![],
            threshold: default_threshold(),
            channels: default_channels(),
            allow_list: vec![],
            channel_actions: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub rule_id: RuleId,
    pub rule_revision: u32,
    pub detector_id: String,
    pub category: DetectorCategory,
    pub score: u8,
    pub count: u32,
    pub reasons: Vec<Reason>,
    pub severity: Severity,
    pub actions: Vec<Action>,
    #[serde(skip)]
    #[cfg_attr(feature = "ts-codegen", ts(skip))]
    pub(crate) priority: u16,
}
impl Finding {
    pub fn to_verdict(&self) -> Verdict {
        Verdict {
            rule_id: self.rule_id.clone(),
            rule_revision: self.rule_revision,
            severity: self.severity,
            actions: self.actions.clone(),
        }
    }
}

/// Device-local offsets deliberately have no serialization implementation.
/// ```compile_fail
/// let _ = serde_json::to_string(&wincmd_clip_rules::LocalSpans::default());
/// ```
#[derive(Clone, Default)]
pub struct LocalSpans(pub(crate) Vec<LocalSpan>);
#[derive(Clone)]
pub(crate) struct LocalSpan {
    pub range: std::ops::Range<usize>,
    pub rule_id: RuleId,
    pub category: DetectorCategory,
}
impl std::fmt::Debug for LocalSpans {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSpans")
            .field("count", &self.0.len())
            .finish()
    }
}
impl LocalSpans {
    pub fn extend(&mut self, other: LocalSpans) {
        self.0.extend(other.0);
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub findings: Vec<Finding>,
    pub truncated: bool,
    #[serde(skip)]
    #[cfg_attr(feature = "ts-codegen", ts(skip))]
    pub local_spans: LocalSpans,
}
impl ScanReport {
    pub fn verdict(&self) -> Option<Verdict> {
        self.findings
            .iter()
            .max_by(|a, b| a.priority.cmp(&b.priority).then(a.rule_id.cmp(&b.rule_id)))
            .map(Finding::to_verdict)
    }
}

pub fn mask_matches(text: &str, spans: &LocalSpans) -> String {
    mask_selected(text, spans, |_| true)
}
pub fn mask_matches_for_rules(text: &str, spans: &LocalSpans, rule_ids: &[RuleId]) -> String {
    mask_selected(text, spans, |span| rule_ids.contains(&span.rule_id))
}
fn mask_selected(text: &str, spans: &LocalSpans, keep: impl Fn(&LocalSpan) -> bool) -> String {
    let mut sorted: Vec<_> = spans
        .0
        .iter()
        .filter(|span| keep(span) && text.get(span.range.clone()).is_some())
        .collect();
    sorted.sort_by_key(|span| (span.range.start, std::cmp::Reverse(span.range.end)));
    let mut merged: Vec<(std::ops::Range<usize>, DetectorCategory)> = Vec::new();
    for span in sorted {
        if let Some((range, category)) = merged.last_mut() {
            if span.range.start < range.end {
                range.end = range.end.max(span.range.end);
                // An overlapping secret must never retain a card's trailing digits.
                if span.category != *category {
                    *category = DetectorCategory::Custom;
                }
                continue;
            }
        }
        merged.push((span.range.clone(), span.category));
    }
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for (range, category) in merged {
        result.push_str(&text[cursor..range.start]);
        if category == DetectorCategory::PaymentCard {
            let digits: String = text[range.clone()]
                .chars()
                .filter(char::is_ascii_digit)
                .collect();
            result.push_str("•••• •••• •••• ");
            result.push_str(digits.get(digits.len().saturating_sub(4)..).unwrap_or(""));
        } else {
            result.push_str("[secret removed]");
        }
        cursor = range.end;
    }
    result.push_str(&text[cursor..]);
    result
}
