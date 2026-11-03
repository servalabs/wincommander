// SPDX-License-Identifier: AGPL-3.0-or-later
use serde_json::{json, Value};
use wincmd_clip_rules::{compile, content_rule_limits, Channel, Rule};

#[tauri::command]
pub fn get_content_detector_catalog() -> Vec<Rule> {
    wincmd_clip_rules::builtin_rules()
}

#[tauri::command]
pub fn test_local_content_rules(rules: Vec<Rule>, sample: String) -> Result<Value, String> {
    let limits = content_rule_limits();
    if sample.len() > limits.max_text_bytes || rules.len() > 512 {
        return Err("sample or rules exceed the local test limit".into());
    }
    let compiled = compile(&rules, &limits).map_err(|_| "rules could not be compiled".to_owned())?;
    let report = compiled.scan(&sample, Channel::Clipboard);
    Ok(json!({ "findings": report.findings, "truncated": report.truncated }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_test_rejects_oversized_sample_without_echoing_it() {
        let sample = "private-sample".repeat(100_000);
        let error = test_local_content_rules(Vec::new(), sample).unwrap_err();
        assert!(!error.contains("private-sample"));
    }
}
