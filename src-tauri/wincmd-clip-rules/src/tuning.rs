// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{Action, Channel, CompileError, CompiledRuleSet, Rule, RuleSetLimits};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_CONTENT_SUBTREE_BYTES: usize = 256 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "camelCase")]
pub struct BuiltinTuning {
    pub detector_id: String,
    pub enabled: bool,
    pub threshold: u8,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ChannelActions {
    pub channel: Channel,
    pub actions: Vec<Action>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "camelCase", default)]
pub struct ContentTuning {
    pub builtins: Vec<BuiltinTuning>,
    pub allow_list: Vec<String>,
    pub channel_actions: Vec<ChannelActions>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentRulesSubtree {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub schema_version: u32,
    pub revision: u64,
    pub rules: Vec<Rule>,
    pub tuning: ContentTuning,
}
impl Default for ContentRulesSubtree {
    fn default() -> Self {
        Self {
            enabled: true,
            schema_version: 1,
            revision: 0,
            rules: vec![],
            tuning: ContentTuning::default(),
        }
    }
}
fn default_enabled() -> bool {
    true
}
pub fn content_rule_limits() -> RuleSetLimits {
    RuleSetLimits {
        max_enabled_rules: 512,
        ..RuleSetLimits::default()
    }
}
fn valid_shape(subtree: &ContentRulesSubtree) -> bool {
    if subtree.schema_version != 1
        || subtree.rules.len() > 100
        || subtree.tuning.builtins.len() > 512
        || subtree.tuning.allow_list.len() > 256
        || subtree.tuning.channel_actions.len() > 3
    {
        return false;
    }
    let catalog = crate::detector_catalog();
    let mut ids = HashSet::new();
    if subtree.tuning.builtins.iter().any(|p| {
        p.threshold > 100
            || !catalog.iter().any(|c| c.id == p.detector_id)
            || !ids.insert(&p.detector_id)
    }) {
        return false;
    }
    let mut channels = HashSet::new();
    if subtree.tuning.channel_actions.iter().any(|p| {
        !channels.insert(p.channel)
            || p.actions.len() > 6
            || p.actions
                .iter()
                .enumerate()
                .any(|(i, a)| p.actions[..i].contains(a))
    }) {
        return false;
    }
    // Alert-only remains the default until a representative corpus supports stronger actions.
    if subtree.tuning.channel_actions.iter().any(|p| {
        p.actions.iter().any(|a| {
            matches!(
                a,
                Action::MaskMatches | Action::ClearClipboard | Action::QuarantineClipboard
            )
        })
    }) {
        return false;
    }
    !subtree
        .tuning
        .allow_list
        .iter()
        .any(|p| p.is_empty() || p.len() > 2048)
        && serde_json::to_vec(subtree).is_ok_and(|bytes| bytes.len() <= MAX_CONTENT_SUBTREE_BYTES)
}
pub fn resolve_content_rules(
    subtree: &ContentRulesSubtree,
) -> Result<Vec<Rule>, Vec<CompileError>> {
    if !valid_shape(subtree) {
        return Err(vec![CompileError::InvalidRuleOptions { index: 0 }]);
    }
    if !subtree.enabled {
        return Ok(Vec::new());
    }
    let mut rules = crate::builtin_rules();
    for rule in &mut rules {
        let (id, _) = crate::catalog::matcher_metadata(&rule.matcher);
        if let Some(tune) = subtree.tuning.builtins.iter().find(|p| p.detector_id == id) {
            rule.enabled = tune.enabled;
            rule.options.threshold = tune.threshold;
        }
        rule.revision = u32::try_from(subtree.revision).unwrap_or(u32::MAX).max(1);
        rule.options.channel_actions = subtree.tuning.channel_actions.clone();
        rule.actions = vec![Action::NotifyUser, Action::ReportFleet];
    }
    rules.extend(subtree.rules.clone());
    let mut seen = HashSet::new();
    if rules.iter().any(|r| !seen.insert(r.id.clone())) {
        return Err(vec![CompileError::InvalidRuleOptions { index: 0 }]);
    }
    Ok(rules)
}
pub fn compile_content_rules(
    subtree: &ContentRulesSubtree,
) -> Result<CompiledRuleSet, Vec<CompileError>> {
    let rules = resolve_content_rules(subtree)?;
    let mut compiled = crate::compile(&rules, &content_rule_limits())?;
    compiled.global_allow_list = subtree.tuning.allow_list.clone();
    Ok(compiled)
}
pub fn validate_content_rules(subtree: &ContentRulesSubtree) -> Result<(), String> {
    compile_content_rules(subtree)
        .map(|_| ())
        .map_err(|_| "invalid content rules or tuning".into())
}
