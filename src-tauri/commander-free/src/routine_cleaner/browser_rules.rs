// SPDX-License-Identifier: AGPL-3.0-or-later

use super::rules::{normalize_relative, parse_rules, resolve_path, ScanTarget, TargetOperation};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const BROWSER_RULES: &str = include_str!("../../resources/maintenance-rules/win32/browsers.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserRules {
    chromium_cache_dirs: ChromiumCacheDirs,
    chromium: Vec<ChromiumBrowser>,
    firefox: FirefoxBrowser,
    #[serde(default)]
    firefox_forks: Vec<FirefoxFork>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChromiumCacheDirs {
    cache: String,
    code_cache: String,
    gpu_cache: String,
    service_worker: String,
}

#[derive(Deserialize)]
struct ChromiumBrowser {
    key: String,
    base: String,
}

#[derive(Deserialize)]
struct FirefoxBrowser {
    cache: String,
}

#[derive(Deserialize)]
struct FirefoxFork {
    key: String,
    cache: String,
}

pub(super) fn add_targets(
    targets: &mut Vec<ScanTarget>,
    variables: &HashMap<&'static str, String>,
) -> Result<(), String> {
    let rules: BrowserRules = parse_rules(BROWSER_RULES, "browsers")?;
    let cache_dirs = [
        ("Cache", rules.chromium_cache_dirs.cache),
        ("Code Cache", rules.chromium_cache_dirs.code_cache),
        ("GPU Cache", rules.chromium_cache_dirs.gpu_cache),
        (
            "Service Worker Cache",
            rules.chromium_cache_dirs.service_worker,
        ),
    ];
    for browser in rules.chromium {
        let base = resolve_path(&browser.base, variables)?;
        let profiles = if matches!(browser.key.as_str(), "opera" | "operaGX") {
            vec![(String::new(), base)]
        } else {
            chromium_profiles(&base)
        };
        for (profile, profile_path) in profiles {
            for (cache_label, relative) in &cache_dirs {
                let suffix = if profile.is_empty() {
                    String::new()
                } else {
                    format!(" - {profile}")
                };
                targets.push(ScanTarget {
                    category: "browsers".into(),
                    label: format!("{}{suffix} {cache_label}", browser_label(&browser.key)),
                    path: profile_path.join(normalize_relative(relative)),
                    operation: TargetOperation::Delete,
                    recommended: true,
                    minimum_age: std::time::Duration::ZERO,
                    containment_root: None,
                    containment_source: None,
                });
            }
        }
    }
    add_firefox_profiles(
        targets,
        "Firefox",
        &resolve_path(&rules.firefox.cache, variables)?,
    );
    for fork in rules
        .firefox_forks
        .into_iter()
        .filter(|fork| fork.key != "zen")
    {
        add_firefox_profiles(
            targets,
            browser_label(&fork.key),
            &resolve_path(&fork.cache, variables)?,
        );
    }
    Ok(())
}

fn chromium_profiles(base: &Path) -> Vec<(String, PathBuf)> {
    let mut profiles = vec![("Default".into(), base.join("Default"))];
    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("Profile ")
                && entry
                    .file_type()
                    .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
            {
                profiles.push((name, entry.path()));
            }
        }
    }
    profiles
}

fn add_firefox_profiles(targets: &mut Vec<ScanTarget>, label: &str, base: &Path) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    for entry in entries.flatten().filter(|entry| {
        entry
            .file_type()
            .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
    }) {
        let profile = entry.file_name().to_string_lossy().into_owned();
        targets.push(ScanTarget {
            category: "browsers".into(),
            label: format!("{label} - {profile} Cache"),
            path: entry.path().join("cache2"),
            operation: TargetOperation::Delete,
            recommended: true,
            minimum_age: std::time::Duration::ZERO,
            containment_root: None,
            containment_source: None,
        });
    }
}

fn browser_label(key: &str) -> &str {
    match key {
        "chrome" => "Chrome",
        "edge" => "Edge",
        "brave" => "Brave",
        "opera" => "Opera",
        "operaGX" => "Opera GX",
        "vivaldi" => "Vivaldi",
        "arc" => "Arc",
        "chromium" => "Chromium",
        "thorium" => "Thorium",
        "supermium" => "Supermium",
        "helium" => "Helium",
        "cromite" => "Cromite",
        "catsxp" => "CatsXP",
        "librewolf" => "LibreWolf",
        "waterfox" => "Waterfox",
        "floorp" => "Floorp",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_browser_rules_parse() {
        assert!(parse_rules::<BrowserRules>(BROWSER_RULES, "browsers").is_ok());
    }
}
