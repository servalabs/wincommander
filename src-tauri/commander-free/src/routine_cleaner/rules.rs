// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TargetOperation {
    Delete,
    Vacuum,
}

#[derive(Clone, Debug)]
pub(super) struct ScanTarget {
    pub category: String,
    pub label: String,
    pub path: PathBuf,
    pub operation: TargetOperation,
    pub recommended: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemRules {
    clean_targets: Vec<SystemTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemTarget {
    path: String,
    subcategory: String,
    #[serde(default)]
    needs_admin: bool,
    child_subdir: Option<String>,
}

#[derive(Deserialize)]
struct AppRules {
    apps: Vec<AppTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppTarget {
    name: String,
    paths: Vec<String>,
    child_subdir: Option<String>,
}

const SYSTEM_RULES: &str = include_str!("../../resources/maintenance-rules/win32/system.json");
const APPS_RULES: &str = include_str!("../../resources/maintenance-rules/win32/apps.json");
const GAMING_RULES: &str = include_str!("../../resources/maintenance-rules/win32/gaming.json");
const GPU_RULES: &str = include_str!("../../resources/maintenance-rules/win32/gpu-cache.json");

// Security and audit histories are excluded because the AV-clean invariant
// forbids security-reducing execution logic. This allowlist is limited to
// regenerable cache and temporary data.
const SAFE_SYSTEM_TARGETS: &[&str] = &[
    "User Temp Files",
    "System Temp Files",
    "Thumbnail & Icon Cache",
    "Font Cache",
    "DirectX Shader Cache",
    "Internet Cache",
    "Windows Update Cache",
    "Delivery Optimization Cache",
    "Error Reports",
    "System Error Reports",
    "Crash Dumps",
    "Minidump Files",
    ".NET Usage Logs",
    ".NET Usage Logs (32-bit)",
    "Network Service Temp",
    "Windows Caches",
    "Local Service Temp",
    "Delivery Optimization User Cache",
    "Power Efficiency Reports",
    "Kernel Live Dump Files",
    "Local System Temp",
    "WinSAT Results",
    "Local Service Certificate Cache",
    "Network Service Certificate Cache",
    "System Certificate Cache",
    "qWAVE Cache",
    "Elevated Diagnostics",
];

pub(super) fn build_targets(categories: &HashSet<String>) -> Result<Vec<ScanTarget>, String> {
    let variables = path_variables();
    let mut targets = Vec::new();

    if categories.contains("system") {
        let rules: SystemRules = parse_rules(SYSTEM_RULES, "system")?;
        for rule in rules.clean_targets {
            if !SAFE_SYSTEM_TARGETS.contains(&rule.subcategory.as_str()) {
                continue;
            }
            let base = resolve_path(&rule.path, &variables)?;
            for path in expand_child_subdir(&base, rule.child_subdir.as_deref()) {
                targets.push(ScanTarget {
                    category: "system".into(),
                    label: rule.subcategory.clone(),
                    path,
                    operation: TargetOperation::Delete,
                    recommended: !rule.needs_admin,
                });
            }
        }
    }

    if categories.contains("applications") {
        add_app_targets(&mut targets, APPS_RULES, "applications", &variables)?;
    }
    if categories.contains("gaming") {
        add_app_targets(&mut targets, GAMING_RULES, "gaming", &variables)?;
        add_app_targets(&mut targets, GPU_RULES, "gaming", &variables)?;
        super::steam_rules::add_targets(&mut targets, &variables)?;
    }
    if categories.contains("browsers") {
        super::browser_rules::add_targets(&mut targets, &variables)?;
    }
    if categories.contains("databases") {
        super::database_rules::add_targets(&mut targets, &variables)?;
    }

    targets.sort_by(|a, b| a.category.cmp(&b.category).then(a.label.cmp(&b.label)));
    targets.dedup_by(|a, b| a.operation == b.operation && a.path == b.path);
    Ok(targets)
}

fn add_app_targets(
    targets: &mut Vec<ScanTarget>,
    source: &str,
    category: &str,
    variables: &HashMap<&'static str, String>,
) -> Result<(), String> {
    let rules: AppRules = parse_rules(source, category)?;
    for app in rules.apps {
        for raw in app.paths {
            let base = resolve_path(&raw, variables)?;
            for path in expand_child_subdir(&base, app.child_subdir.as_deref()) {
                targets.push(ScanTarget {
                    category: category.into(),
                    label: app.name.clone(),
                    path,
                    operation: TargetOperation::Delete,
                    recommended: true,
                });
            }
        }
    }
    Ok(())
}

pub(super) fn parse_rules<T: for<'de> Deserialize<'de>>(
    source: &str,
    name: &str,
) -> Result<T, String> {
    serde_json::from_str(source).map_err(|error| format!("invalid embedded {name} rules: {error}"))
}

fn path_variables() -> HashMap<&'static str, String> {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    HashMap::from([
        ("HOME", home.clone()),
        (
            "LOCALAPPDATA",
            env_or("LOCALAPPDATA", &format!(r"{home}\AppData\Local")),
        ),
        (
            "APPDATA",
            env_or("APPDATA", &format!(r"{home}\AppData\Roaming")),
        ),
        ("WINDIR", env_or("WINDIR", r"C:\Windows")),
        ("PROGRAMDATA", env_or("ProgramData", r"C:\ProgramData")),
        ("PROGRAMFILES", env_or("ProgramFiles", r"C:\Program Files")),
        (
            "PROGRAMFILES_X86",
            env_or("ProgramFiles(x86)", r"C:\Program Files (x86)"),
        ),
        (
            "TMPDIR",
            std::env::temp_dir().to_string_lossy().into_owned(),
        ),
    ])
}

fn env_or(name: &str, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.into())
}

pub(super) fn resolve_path(
    raw: &str,
    variables: &HashMap<&'static str, String>,
) -> Result<PathBuf, String> {
    let mut resolved = raw.to_string();
    for (name, value) in variables {
        resolved = resolved.replace(&format!("${{{name}}}"), value);
    }
    if resolved.contains("${") {
        return Err(format!("unresolved cleaner path template: {raw}"));
    }
    Ok(PathBuf::from(normalize_relative(&resolved)))
}

pub(super) fn normalize_relative(value: &str) -> String {
    if cfg!(windows) {
        value.replace('/', r"\")
    } else {
        value.to_string()
    }
}

fn expand_child_subdir(base: &Path, child_subdir: Option<&str>) -> Vec<PathBuf> {
    let Some(child_subdir) = child_subdir else {
        return vec![base.to_path_buf()];
    };
    fs::read_dir(base)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
        })
        .map(|entry| entry.path().join(normalize_relative(child_subdir)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_rule_files_parse() {
        assert!(parse_rules::<SystemRules>(SYSTEM_RULES, "system").is_ok());
        assert!(parse_rules::<AppRules>(APPS_RULES, "apps").is_ok());
    }
}
