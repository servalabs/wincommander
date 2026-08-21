// SPDX-License-Identifier: AGPL-3.0-or-later
// Registry-backed environment audit. The app only repairs values from its live scan cache.

mod registry;
mod repair;
mod scan;

use registry::{Scope, Value};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(20 * 60);
const MAX_SELECTION: usize = 256;
pub(super) const DIRECTORY_VARIABLES: &[&str] = &[
    "JAVA_HOME",
    "JDK_HOME",
    "JRE_HOME",
    "GOROOT",
    "GOBIN",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "ANDROID_HOME",
    "ANDROID_SDK_ROOT",
    "DOTNET_ROOT",
    "NVM_HOME",
    "NVM_SYMLINK",
    "PNPM_HOME",
    "BUN_INSTALL",
    "VCPKG_ROOT",
];

#[derive(Clone)]
enum CachedEntry {
    Path {
        scope: Scope,
        old_path: String,
        missing_entry: String,
        value_type: u32,
    },
    Variable {
        scope: Scope,
        value: Value,
    },
}
struct Cache {
    created_at: Instant,
    entries: HashMap<String, CachedEntry>,
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            created_at: Instant::now(),
            entries: HashMap::new(),
        })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentFinding {
    pub id: String,
    pub scope: String,
    pub variable: String,
    pub value: String,
    pub kind: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentScan {
    pub entries: Vec<EnvironmentFinding>,
    pub skipped_entries: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRepairResult {
    pub repaired: usize,
    pub backup_locations: Vec<String>,
    pub errors: Vec<String>,
    pub environment_broadcast: bool,
}

#[tauri::command]
pub async fn environment_cleaner_scan() -> Result<EnvironmentScan, String> {
    let (scan, entries) = tokio::task::spawn_blocking(scan::scan_environment)
        .await
        .map_err(|error| format!("environment scan task failed: {error}"))?;
    let mut guard = cache()
        .lock()
        .map_err(|_| "environment cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.entries = entries;
    Ok(scan)
}

#[tauri::command]
pub async fn environment_cleaner_repair(
    ids: Vec<String>,
) -> Result<EnvironmentRepairResult, String> {
    ensure_mutation_allowed()?;
    if ids.is_empty() || ids.len() > MAX_SELECTION || ids.iter().any(|id| id.len() > 64) {
        return Err("invalid environment selection".into());
    }
    let selected = select_entries(ids)?;
    tokio::task::spawn_blocking(move || repair::repair_environment(selected))
        .await
        .map_err(|error| format!("environment repair task failed: {error}"))
}

fn select_entries(ids: Vec<String>) -> Result<Vec<CachedEntry>, String> {
    let ids: HashSet<_> = ids.into_iter().collect();
    let guard = cache()
        .lock()
        .map_err(|_| "environment cache lock poisoned".to_string())?;
    if guard.created_at.elapsed() > CACHE_TTL {
        return Err("environment scan expired; scan again before repairing values".into());
    }
    ids.into_iter()
        .map(|id| {
            guard
                .entries
                .get(&id)
                .cloned()
                .ok_or_else(|| "environment selection is stale or invalid; scan again".to_string())
        })
        .collect()
}

fn ensure_mutation_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: environment repair is unavailable in Decoy mode.".into());
    }
    if crate::license::is_advanced_mode() {
        return Err("Refused: investigator mode forbids environment repair because it would taint evidence.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::scan::expand_variables;
    use std::collections::HashMap;

    #[test]
    fn expands_only_known_variables() {
        assert_eq!(
            expand_variables(
                "%ROOT%\\bin",
                &HashMap::from([("root".into(), "C:\\Tool".into())])
            ),
            Some("C:\\Tool\\bin".into())
        );
        assert_eq!(expand_variables("%UNKNOWN%\\bin", &HashMap::new()), None);
    }
    #[test]
    fn never_approves_an_empty_path() {
        let removals: std::collections::HashSet<String> =
            std::collections::HashSet::from(["c:\\missing".into()]);
        let kept: Vec<_> = "C:\\Missing"
            .split(';')
            .filter(|entry| !removals.contains(&entry.to_ascii_lowercase()))
            .collect();
        assert!(kept.is_empty());
    }
}
