// SPDX-License-Identifier: AGPL-3.0-or-later

use super::rules::{normalize_relative, parse_rules, resolve_path, ScanTarget, TargetOperation};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const DATABASE_RULES: &str = include_str!("../../resources/maintenance-rules/win32/databases.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DatabaseRules {
    #[serde(default)]
    shared_db_file_sets: HashMap<String, Vec<String>>,
    targets: Vec<DatabaseTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DatabaseTarget {
    label: String,
    base_path: String,
    db_files: DbFiles,
    #[serde(default)]
    multi_profile: bool,
    profile_pattern: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DbFiles {
    Reference(String),
    Files(Vec<String>),
}

pub(super) fn add_targets(
    targets: &mut Vec<ScanTarget>,
    variables: &HashMap<&'static str, String>,
) -> Result<(), String> {
    let rules: DatabaseRules = parse_rules(DATABASE_RULES, "databases")?;
    for target in rules.targets {
        let files = match target.db_files {
            DbFiles::Files(files) => files,
            DbFiles::Reference(reference) => rules
                .shared_db_file_sets
                .get(reference.trim_start_matches('$'))
                .cloned()
                .ok_or_else(|| format!("unknown database rule reference: {reference}"))?,
        };
        let base = resolve_path(&target.base_path, variables)?;
        for profile in database_profiles(
            &base,
            target.multi_profile,
            target.profile_pattern.as_deref(),
        ) {
            for file in &files {
                targets.push(ScanTarget {
                    category: "databases".into(),
                    label: target.label.clone(),
                    path: profile.join(normalize_relative(file)),
                    operation: TargetOperation::Vacuum,
                    recommended: false,
                    minimum_age: std::time::Duration::ZERO,
                    containment_root: None,
                    containment_source: None,
                });
            }
        }
    }
    Ok(())
}

fn database_profiles(
    base: &Path,
    multi_profile: bool,
    patterns: Option<&[String]>,
) -> Vec<PathBuf> {
    if !multi_profile {
        return vec![base.to_path_buf()];
    }
    let Ok(entries) = fs::read_dir(base) else {
        return vec![base.to_path_buf()];
    };
    let profiles: Vec<_> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
        })
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            patterns.map_or_else(
                || name == "Default" || name.starts_with("Profile "),
                |items| items.iter().any(|pattern| wildcard_match(pattern, &name)),
            )
        })
        .map(|entry| entry.path())
        .collect();
    if profiles.is_empty() {
        vec![base.to_path_buf()]
    } else {
        profiles
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let (mut p, mut v, mut star, mut mark) = (0, 0, None, 0);
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p].eq_ignore_ascii_case(&value[v])) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            mark = v;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            mark += 1;
            v = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wildcard_profile_matching_is_case_insensitive() {
        assert!(wildcard_match("*.default*", "abc.DEFAULT-release"));
        assert!(!wildcard_match("*.default*", "Profile 2"));
    }
    #[test]
    fn embedded_database_rules_parse() {
        assert!(parse_rules::<DatabaseRules>(DATABASE_RULES, "databases").is_ok());
    }
}
