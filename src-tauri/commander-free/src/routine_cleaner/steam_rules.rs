// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

use super::rules::{resolve_path, ScanTarget, TargetOperation};

const STEAM_RULES: &str = include_str!("../../resources/maintenance-rules/win32/steam.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SteamRules {
    libraries: Vec<String>,
    redist_patterns: Vec<String>,
}

pub(super) fn add_targets(
    targets: &mut Vec<ScanTarget>,
    variables: &HashMap<&'static str, String>,
) -> Result<(), String> {
    let rules: SteamRules = super::rules::parse_rules(STEAM_RULES, "steam")?;
    for raw_library in rules.libraries {
        let library = resolve_path(&raw_library, variables)?;
        if !library.is_dir() {
            continue;
        }
        targets.push(ScanTarget {
            category: "gaming".into(),
            label: "Steam shader cache".into(),
            path: library.join("steamapps").join("shadercache"),
            operation: TargetOperation::Delete,
            recommended: true,
            minimum_age: std::time::Duration::ZERO,
            containment_root: None,
            containment_source: None,
        });

        let common = library.join("steamapps").join("common");
        let Ok(games) = fs::read_dir(common) else {
            continue;
        };
        for game in games.flatten().filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
        }) {
            for pattern in &rules.redist_patterns {
                let path = game.path().join(super::rules::normalize_relative(pattern));
                if path.exists() {
                    targets.push(ScanTarget {
                        category: "gaming".into(),
                        label: "Steam redistributable cache".into(),
                        path,
                        operation: TargetOperation::Delete,
                        // Keep large redistributable removals opt-in even though
                        // Steam can restore them for installed games.
                        recommended: false,
                        minimum_age: std::time::Duration::ZERO,
                        containment_root: None,
                        containment_source: None,
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_steam_rules_parse() {
        let parsed: Result<SteamRules, _> = serde_json::from_str(STEAM_RULES);
        assert!(parsed.is_ok());
    }
}
