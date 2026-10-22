// SPDX-License-Identifier: AGPL-3.0-or-later
// Native preview-first cache cleaner. The webview receives opaque IDs only;
// filesystem paths used for mutation always come from this process-local cache.

mod app_categories;
mod browser_rules;
mod database_rules;
mod mutations;
mod operation;
mod projects;
mod rules;
mod scanner;
mod steam_rules;

pub(crate) use scanner::file_identity;

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use rules::TargetOperation;

const CACHE_TTL: Duration = Duration::from_secs(20 * 60);
pub(super) const MAX_PREVIEW_ENTRIES_PER_TARGET: usize = 100;
const ALLOWED_CATEGORIES: &[&str] = &[
    "system",
    "browsers",
    "applications",
    "gaming",
    "databases",
    "developers",
    "ai",
];

#[derive(Clone, Debug)]
struct CachedFile {
    path: std::path::PathBuf,
    actual_bytes: u64,
    bytes: u64,
    minimum_age: Duration,
    file_identity: Option<(u32, u64)>,
}

#[derive(Clone, Debug)]
struct CachedCleanerItem {
    id: String,
    label: String,
    root: std::path::PathBuf,
    canonical_root: std::path::PathBuf,
    requires_plain_path: bool,
    files: Vec<CachedFile>,
    operation: TargetOperation,
}

struct CleanerCache {
    created_at: Instant,
    items: HashMap<String, CachedCleanerItem>,
}

impl CleanerCache {
    fn empty() -> Self {
        Self {
            created_at: Instant::now(),
            items: HashMap::new(),
        }
    }
}

static CACHE: OnceLock<Mutex<CleanerCache>> = OnceLock::new();
static CANCELLED: AtomicBool = AtomicBool::new(false);

fn cache() -> &'static Mutex<CleanerCache> {
    CACHE.get_or_init(|| Mutex::new(CleanerCache::empty()))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineCleanerEntryPreview {
    pub id: String,
    pub path: Option<String>,
    pub actual_size_bytes: u64,
    pub estimated_reclaim_bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineCleanerItem {
    pub id: String,
    pub category: String,
    pub label: String,
    pub description: Option<String>,
    pub path: String,
    pub path_display: Option<String>,
    pub bytes: u64,
    pub file_count: usize,
    pub entries: Vec<RoutineCleanerEntryPreview>,
    pub entry_count: usize,
    pub entries_limit: usize,
    pub entries_truncated: bool,
    pub recommended: bool,
    pub operation: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineCleanerScan {
    pub scan_root: Option<String>,
    pub items: Vec<RoutineCleanerItem>,
    pub total_bytes: u64,
    pub total_files: usize,
    pub skipped_targets: usize,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineCleanerError {
    pub id: String,
    pub label: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineCleanerCleanResult {
    pub bytes_recovered: u64,
    pub files_cleaned: usize,
    pub items_cleaned: usize,
    pub errors: Vec<RoutineCleanerError>,
    pub cancelled: bool,
}

#[tauri::command]
pub async fn routine_cleaner_scan(
    categories: Option<Vec<String>>,
) -> Result<RoutineCleanerScan, String> {
    if crate::settings::is_decoy_mode() {
        return Ok(RoutineCleanerScan {
            scan_root: None,
            items: Vec::new(),
            total_bytes: 0,
            total_files: 0,
            skipped_targets: 0,
            cancelled: false,
        });
    }
    let categories = validate_categories(categories)?;
    let _operation = operation::begin()?;
    let (scan, cached) = tokio::task::spawn_blocking(move || {
        let targets = rules::build_targets(&categories)?;
        scanner::scan_targets(targets, &CANCELLED)
    })
    .await
    .map_err(|error| format!("routine cleaner scan task failed: {error}"))??;

    let mut guard = cache()
        .lock()
        .map_err(|_| "routine cleaner cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.items = cached
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect();
    Ok(scan)
}

#[tauri::command]
pub async fn routine_cleaner_scan_projects(
    app: tauri::AppHandle,
) -> Result<Option<RoutineCleanerScan>, String> {
    use tauri_plugin_dialog::DialogExt;
    if crate::settings::is_decoy_mode() {
        return Err("Project scanning is unavailable in Decoy mode.".into());
    }
    let _operation = operation::begin()?;
    let picked = app
        .dialog()
        .file()
        .set_title("Choose a project folder to review rebuildable caches")
        .blocking_pick_folder();
    let Some(picked) = picked else {
        return Ok(None);
    };
    let root = picked.into_path().map_err(|error| error.to_string())?;
    let scan_root = root.to_string_lossy().into_owned();
    let (mut scan, cached) = tokio::task::spawn_blocking(move || {
        let (targets, skipped) = projects::discover(&root, &CANCELLED)?;
        let (mut scan, cached) = scanner::scan_targets(targets, &CANCELLED)?;
        scan.skipped_targets += skipped;
        Ok::<_, String>((scan, cached))
    })
    .await
    .map_err(|error| format!("project scan task failed: {error}"))??;
    scan.scan_root = Some(scan_root);
    scan.cancelled = CANCELLED.load(Ordering::Acquire);
    let mut guard = cache()
        .lock()
        .map_err(|_| "routine cleaner cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.items = cached
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect();
    Ok(Some(scan))
}

#[tauri::command]
pub async fn routine_cleaner_clean(
    item_ids: Vec<String>,
) -> Result<RoutineCleanerCleanResult, String> {
    ensure_mutation_allowed()?;
    let _operation = operation::begin()?;
    if item_ids.is_empty() {
        return Err("select at least one scanned cleaner item".into());
    }
    if item_ids.len() > 5_000 || item_ids.iter().any(|id| id.len() > 64) {
        return Err("invalid routine cleaner selection".into());
    }
    let unique: HashSet<String> = item_ids.into_iter().collect();
    let selected = {
        let guard = cache()
            .lock()
            .map_err(|_| "routine cleaner cache lock poisoned".to_string())?;
        if guard.created_at.elapsed() > CACHE_TTL {
            return Err("routine cleaner scan expired; scan again before cleaning".into());
        }
        let mut selected = Vec::with_capacity(unique.len());
        for id in &unique {
            let item = guard.items.get(id).ok_or_else(|| {
                "routine cleaner selection is stale or invalid; scan again".to_string()
            })?;
            selected.push(item.clone());
        }
        selected
    };

    let (result, cleaned_ids) =
        tokio::task::spawn_blocking(move || mutations::clean_items(selected, &CANCELLED))
            .await
            .map_err(|error| format!("routine cleaner task failed: {error}"))?;

    if let Ok(mut guard) = cache().lock() {
        for id in cleaned_ids {
            guard.items.remove(&id);
        }
    }
    Ok(result)
}

#[tauri::command]
pub fn routine_cleaner_cancel() {
    CANCELLED.store(true, Ordering::Release);
}

fn validate_categories(categories: Option<Vec<String>>) -> Result<HashSet<String>, String> {
    let values = categories.unwrap_or_else(|| {
        ALLOWED_CATEGORIES
            .iter()
            .map(|value| (*value).into())
            .collect()
    });
    if values.is_empty() || values.len() > ALLOWED_CATEGORIES.len() {
        return Err("invalid routine cleaner categories".into());
    }
    let set: HashSet<String> = values.into_iter().collect();
    if set
        .iter()
        .any(|value| !ALLOWED_CATEGORIES.contains(&value.as_str()))
    {
        return Err("unknown routine cleaner category".into());
    }
    Ok(set)
}

fn ensure_mutation_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: cleanup is unavailable in Decoy mode.".into());
    }
    if crate::license::is_advanced_mode() {
        let message =
            "Refused: investigator mode forbids routine cleanup because it would taint evidence.";
        crate::log_message("warn", &format!("[Investigator] {message}"));
        return Err(message.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_categories() {
        assert!(validate_categories(Some(vec!["registry".into()])).is_err());
    }

    #[test]
    fn defaults_to_all_categories() {
        assert_eq!(
            validate_categories(None).unwrap().len(),
            ALLOWED_CATEGORIES.len()
        );
    }
}
