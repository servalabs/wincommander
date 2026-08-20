// SPDX-License-Identifier: AGPL-3.0-or-later
// Native preview-first cache cleaner. The webview receives opaque IDs only;
// filesystem paths used for mutation always come from this process-local cache.

mod browser_rules;
mod database_rules;
mod mutations;
mod rules;
mod scanner;
mod steam_rules;

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use rules::TargetOperation;

const CACHE_TTL: Duration = Duration::from_secs(20 * 60);
const ALLOWED_CATEGORIES: &[&str] = &["system", "browsers", "applications", "gaming", "databases"];

#[derive(Clone, Debug)]
struct CachedFile {
    path: std::path::PathBuf,
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
pub struct RoutineCleanerItem {
    pub id: String,
    pub category: String,
    pub label: String,
    pub path: String,
    pub bytes: u64,
    pub file_count: usize,
    pub recommended: bool,
    pub operation: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineCleanerScan {
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
            items: Vec::new(),
            total_bytes: 0,
            total_files: 0,
            skipped_targets: 0,
            cancelled: false,
        });
    }
    let categories = validate_categories(categories)?;
    CANCELLED.store(false, Ordering::Release);
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
pub async fn routine_cleaner_clean(
    item_ids: Vec<String>,
) -> Result<RoutineCleanerCleanResult, String> {
    ensure_mutation_allowed()?;
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

    CANCELLED.store(false, Ordering::Release);
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
