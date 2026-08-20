// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    pub minimum_age: Duration,
    pub containment_root: Option<PathBuf>,
    pub containment_source: Option<PathBuf>,
}

#[derive(Debug)]
struct ExpandedPath {
    path: PathBuf,
    containment_root: Option<PathBuf>,
    containment_source: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemRules {
    clean_targets: Vec<SystemTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct SystemTarget {
    path: String,
    subcategory: String,
    #[serde(default)]
    needs_admin: bool,
    child_subdir: Option<String>,
    min_age_days: Option<u16>,
    recursive_match: Option<RecursivePathMatch>,
    #[serde(rename = "description")]
    _description: Option<String>,
}

#[derive(Deserialize)]
struct AppRules {
    apps: Vec<AppTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct AppTarget {
    #[serde(rename = "id")]
    _id: String,
    name: String,
    paths: Vec<String>,
    child_subdir: Option<String>,
    min_age_days: Option<u16>,
    recursive_match: Option<RecursivePathMatch>,
    #[serde(rename = "description")]
    _description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct RecursivePathMatch {
    anchor: String,
    targets: Vec<String>,
    #[serde(default)]
    excluded_ancestors: Vec<String>,
    max_depth: Option<u8>,
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
            for expanded in expand_rule_paths(
                &base,
                rule.child_subdir.as_deref(),
                rule.recursive_match.as_ref(),
            )? {
                targets.push(ScanTarget {
                    category: "system".into(),
                    label: rule.subcategory.clone(),
                    path: expanded.path,
                    operation: TargetOperation::Delete,
                    recommended: !rule.needs_admin,
                    minimum_age: minimum_age(rule.min_age_days)?,
                    containment_root: expanded.containment_root,
                    containment_source: expanded.containment_source,
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
            for expanded in expand_rule_paths(
                &base,
                app.child_subdir.as_deref(),
                app.recursive_match.as_ref(),
            )? {
                targets.push(ScanTarget {
                    category: category.into(),
                    label: app.name.clone(),
                    path: expanded.path,
                    operation: TargetOperation::Delete,
                    recommended: true,
                    minimum_age: minimum_age(app.min_age_days)?,
                    containment_root: expanded.containment_root,
                    containment_source: expanded.containment_source,
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

fn minimum_age(days: Option<u16>) -> Result<Duration, String> {
    let days = days.unwrap_or(0);
    if days > 3650 {
        return Err("routine cleaner rule minimum age exceeds 3650 days".into());
    }
    Ok(Duration::from_secs(u64::from(days) * 24 * 60 * 60))
}

fn expand_rule_paths(
    base: &Path,
    child_subdir: Option<&str>,
    recursive_match: Option<&RecursivePathMatch>,
) -> Result<Vec<ExpandedPath>, String> {
    if child_subdir.is_some() && recursive_match.is_some() {
        return Err("routine cleaner rule cannot combine childSubdir and recursiveMatch".into());
    }
    match recursive_match {
        Some(rule) => resolve_recursive_paths(base, rule),
        None => Ok(expand_child_subdir(base, child_subdir)
            .into_iter()
            .map(|path| ExpandedPath {
                path,
                containment_root: None,
                containment_source: None,
            })
            .collect()),
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

const MAX_RECURSIVE_ENTRIES: usize = 100_000;
const DEFAULT_RECURSIVE_DEPTH: u8 = 12;

fn resolve_recursive_paths(
    base: &Path,
    rule: &RecursivePathMatch,
) -> Result<Vec<ExpandedPath>, String> {
    resolve_recursive_paths_with_limit(base, rule, MAX_RECURSIVE_ENTRIES)
}

fn resolve_recursive_paths_with_limit(
    base: &Path,
    rule: &RecursivePathMatch,
    max_entries: usize,
) -> Result<Vec<ExpandedPath>, String> {
    if !valid_directory_name(&rule.anchor)
        || rule.targets.is_empty()
        || rule
            .targets
            .iter()
            .any(|value| !valid_directory_name(value))
        || rule
            .excluded_ancestors
            .iter()
            .any(|value| !valid_directory_name(value))
    {
        return Err("routine cleaner recursive rule contains an unsafe directory name".into());
    }
    let max_depth = rule.max_depth.unwrap_or(DEFAULT_RECURSIVE_DEPTH);
    if max_depth == 0 || max_depth > 32 {
        return Err("routine cleaner recursive rule depth must be between 1 and 32".into());
    }
    let normalize = |name: &str| {
        if cfg!(windows) {
            name.to_ascii_lowercase()
        } else {
            name.to_string()
        }
    };
    let anchor = normalize(&rule.anchor);
    let targets: HashSet<String> = rule.targets.iter().map(|value| normalize(value)).collect();
    let excluded: HashSet<String> = rule
        .excluded_ancestors
        .iter()
        .map(|value| normalize(value))
        .collect();
    let base_is_anchor = base
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| normalize(name) == anchor);
    if !base_is_anchor {
        return Err("routine cleaner recursive rule base must be its declared anchor".into());
    }
    if !base.exists() {
        return Ok(Vec::new());
    }
    reject_reparse_components(base)?;
    let canonical_base = fs::canonicalize(base)
        .map_err(|error| format!("routine cleaner recursive rule base is unavailable: {error}"))?;
    if !canonical_matches_resolved_base(&canonical_base, base) {
        return Err(
            "routine cleaner recursive rule base resolves outside its approved path".into(),
        );
    }
    let base_metadata = fs::symlink_metadata(&canonical_base)
        .map_err(|error| format!("routine cleaner recursive rule base is unavailable: {error}"))?;
    if !base_metadata.is_dir()
        || base_metadata.file_type().is_symlink()
        || is_reparse_point(&base_metadata)
    {
        return Err("routine cleaner recursive rule base is not a safe directory".into());
    }
    let mut resolved = Vec::new();
    let mut stack = vec![(canonical_base.clone(), 0u8)];
    let mut inspected = 0usize;
    while let Some((directory, depth)) = stack.pop() {
        let Ok(metadata) = fs::symlink_metadata(&directory) else {
            continue;
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            continue;
        }
        let Ok(canonical_directory) = fs::canonicalize(&directory) else {
            continue;
        };
        if canonical_directory != canonical_base
            && !canonical_directory.starts_with(&canonical_base)
        {
            continue;
        }
        let Ok(entries) = fs::read_dir(canonical_directory) else {
            continue;
        };
        if depth >= max_depth {
            continue;
        }
        for entry in entries.flatten() {
            inspected = inspected.saturating_add(1);
            if inspected > max_entries {
                return Err("routine cleaner recursive rule exceeded its entry limit".into());
            }
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || is_reparse_point(&metadata)
            {
                continue;
            }
            let name = normalize(&entry.file_name().to_string_lossy());
            if excluded.contains(&name) {
                continue;
            }
            let Ok(canonical_path) = fs::canonicalize(&path) else {
                continue;
            };
            if !canonical_path.starts_with(&canonical_base) || canonical_path == canonical_base {
                continue;
            }
            if targets.contains(&name) {
                resolved.push(ExpandedPath {
                    path: canonical_path,
                    containment_root: Some(canonical_base.clone()),
                    containment_source: Some(base.to_path_buf()),
                });
                continue;
            }
            if depth < max_depth {
                stack.push((canonical_path, depth + 1));
            }
        }
    }
    Ok(resolved)
}

fn reject_reparse_components(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            format!("routine cleaner recursive rule path component is unavailable: {error}")
        })?;
        if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            return Err("routine cleaner recursive rule base crosses a reparse point".into());
        }
    }
    Ok(())
}

pub(super) fn validate_recursive_containment(source: &Path, expected: &Path) -> bool {
    if reject_reparse_components(source).is_err() {
        return false;
    }
    fs::canonicalize(source).is_ok_and(|canonical| {
        canonical == expected && canonical_matches_resolved_base(&canonical, source)
    })
}

fn canonical_matches_resolved_base(canonical: &Path, resolved: &Path) -> bool {
    #[cfg(windows)]
    {
        let normalize = |path: &Path| {
            path.to_string_lossy()
                .trim_start_matches(r"\\?\")
                .replace('/', r"\")
                .trim_end_matches('\\')
                .to_ascii_lowercase()
        };
        normalize(canonical) == normalize(resolved)
    }
    #[cfg(not(windows))]
    {
        canonical == resolved
    }
}

fn valid_directory_name(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\'])
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn create_junction(link: &Path, target: &Path) {
        let output = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("junction fixture command must start");
        assert!(
            output.status.success(),
            "junction fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn embedded_rule_files_parse() {
        assert!(parse_rules::<SystemRules>(SYSTEM_RULES, "system").is_ok());
        assert!(parse_rules::<AppRules>(APPS_RULES, "apps").is_ok());
    }

    #[test]
    fn recursive_rules_reject_path_shaped_directory_names() {
        assert!(valid_directory_name("Cache"));
        assert!(!valid_directory_name("../Cache"));
        assert!(!valid_directory_name("Default/Cache"));
    }

    #[test]
    fn minimum_age_is_bounded() {
        assert_eq!(minimum_age(Some(1)).unwrap(), Duration::from_secs(86_400));
        assert!(minimum_age(Some(3651)).is_err());
    }

    #[test]
    fn app_rules_reject_unsupported_safety_fields() {
        let source = r#"{"apps":[{"id":"unsafe","name":"Unsafe","paths":["C:/cache"],"fileMatch":{"names":["x"]}}]}"#;
        assert!(parse_rules::<AppRules>(source, "apps").is_err());
    }

    #[test]
    fn recursive_rules_require_the_base_to_be_the_anchor() {
        let temp = tempfile::tempdir().unwrap();
        let rule = RecursivePathMatch {
            anchor: "Partitions".into(),
            targets: vec!["Cache".into()],
            excluded_ancestors: Vec::new(),
            max_depth: Some(8),
        };
        assert!(resolve_recursive_paths(temp.path(), &rule).is_err());
    }

    #[test]
    fn recursive_rules_return_only_named_caches_below_the_anchor() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Partitions");
        let allowed = root.join("preview").join("Cache");
        let excluded = root.join("preview").join("IndexedDB").join("Cache");
        fs::create_dir_all(&allowed).unwrap();
        fs::create_dir_all(&excluded).unwrap();
        let matches = resolve_recursive_paths(
            &root,
            &RecursivePathMatch {
                anchor: "Partitions".into(),
                targets: vec!["Cache".into()],
                excluded_ancestors: vec!["IndexedDB".into()],
                max_depth: Some(8),
            },
        )
        .unwrap();
        assert_eq!(
            matches
                .into_iter()
                .map(|expanded| expanded.path)
                .collect::<Vec<_>>(),
            vec![fs::canonicalize(allowed).unwrap()]
        );
    }

    #[test]
    fn recursive_rules_bound_broad_directory_discovery() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Partitions");
        fs::create_dir_all(root.join("one")).unwrap();
        fs::create_dir_all(root.join("two")).unwrap();
        fs::create_dir_all(root.join("three")).unwrap();
        let error = resolve_recursive_paths_with_limit(
            &root,
            &RecursivePathMatch {
                anchor: "Partitions".into(),
                targets: vec!["Cache".into()],
                excluded_ancestors: Vec::new(),
                max_depth: Some(8),
            },
            2,
        )
        .unwrap_err();
        assert!(error.contains("entry limit"));
    }

    #[cfg(windows)]
    #[test]
    fn recursive_rules_reject_a_reparse_base() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside").join("Partitions");
        fs::create_dir_all(outside.join("Cache")).unwrap();
        let linked = temp.path().join("Partitions");
        create_junction(&linked, &outside);
        let result = resolve_recursive_paths(
            &linked,
            &RecursivePathMatch {
                anchor: "Partitions".into(),
                targets: vec!["Cache".into()],
                excluded_ancestors: Vec::new(),
                max_depth: Some(8),
            },
        );
        assert!(result.is_err());
    }

    #[cfg(windows)]
    #[test]
    fn recursive_containment_rejects_a_base_replaced_after_discovery() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("Partitions");
        let outside = temp.path().join("outside").join("Partitions");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let expected = fs::canonicalize(&base).unwrap();
        fs::remove_dir(&base).unwrap();
        create_junction(&base, &outside);

        assert!(!validate_recursive_containment(&base, &expected));
    }
}
