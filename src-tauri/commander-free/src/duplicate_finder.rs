// SPDX-License-Identifier: AGPL-3.0-or-later
mod index;
mod safety;
mod scan;
mod scope;
mod verified_file;

use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

const CACHE_TTL: Duration = Duration::from_secs(20 * 60);
const MAX_ROOTS: usize = 32;
const MAX_SELECTION: usize = 100_000;
const GROUP_PAGE_SIZE: usize = 100;
const MEMBER_PAGE_SIZE: usize = 200;
/// Broad scans default to a useful file size so caches and source trees do not
/// turn a duplicate review into an hours-long comparison of tiny files.
pub(super) const DEFAULT_MINIMUM_SIZE_BYTES: u64 = 200 * 1024;
const ALLOWED_MINIMUM_SIZES: [u64; 5] = [
    0,
    DEFAULT_MINIMUM_SIZE_BYTES,
    1024 * 1024,
    10 * 1024 * 1024,
    100 * 1024 * 1024,
];

#[derive(Clone)]
struct CachedFile {
    path: PathBuf,
    root: PathBuf,
    group_id: String,
    bytes: u64,
    modified: Option<SystemTime>,
    hash: [u8; 32],
    identity: Option<(u32, u64)>,
}

static CACHE: OnceLock<Mutex<Option<index::ScanIndex>>> = OnceLock::new();
static CANCELLED: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
fn cache() -> &'static Mutex<Option<index::ScanIndex>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
    /// Nanoseconds since the Unix epoch, captured after the file content was verified.
    pub modified: Option<String>,
    /// Protected system/application locations can be inspected but never
    /// selected for duplicate removal.
    pub removable: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub id: String,
    pub size: u64,
    pub reclaimable_bytes: u64,
    pub files: Vec<DuplicateFile>,
    pub file_count: usize,
    pub member_offset: usize,
    pub has_more_files: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateScan {
    pub groups: Vec<DuplicateGroup>,
    pub scanned_files: usize,
    pub cancelled: bool,
    pub truncated: bool,
    pub skipped_entries: usize,
    pub skipped_links: usize,
    pub scan_id: String,
    pub scope_roots: Vec<String>,
    pub excluded_volumes: Vec<String>,
    pub hashed_files: usize,
    pub errors: Vec<String>,
    pub complete: bool,
    pub total_groups: usize,
    pub total_duplicate_bytes: u64,
    pub group_offset: usize,
    pub has_more_groups: bool,
    pub minimum_size_bytes: u64,
    pub skipped_small_files: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateRemoveResult {
    pub files_removed: usize,
    pub bytes_recovered: u64,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

struct RunningGuard;
impl RunningGuard {
    fn acquire() -> Result<Self, String> {
        RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "A duplicate scan or removal is already running".to_string())?;
        CANCELLED.store(false, Ordering::Release);
        Ok(Self)
    }
}
impl Drop for RunningGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

fn validate_minimum_size(minimum_size_bytes: Option<u64>) -> Result<u64, String> {
    let minimum = minimum_size_bytes.unwrap_or(DEFAULT_MINIMUM_SIZE_BYTES);
    ALLOWED_MINIMUM_SIZES
        .contains(&minimum)
        .then_some(minimum)
        .ok_or_else(|| "Unsupported duplicate minimum size".to_string())
}

async fn start_scan(
    roots: Vec<PathBuf>,
    excluded: Vec<String>,
    minimum_size_bytes: u64,
) -> Result<DuplicateScan, String> {
    let running = RunningGuard::acquire()?;
    scope::begin(&roots, excluded, minimum_size_bytes);
    *cache()
        .lock()
        .map_err(|_| "duplicate cache lock poisoned")? = None;
    tokio::task::spawn_blocking(move || {
        let _running = running;
        let result = scan::scan_roots(&roots, minimum_size_bytes, &CANCELLED);
        match result {
            Ok(index) => {
                scope::finish(CANCELLED.load(Ordering::Acquire));
                let page = index.page(0)?;
                *cache()
                    .lock()
                    .map_err(|_| "duplicate cache lock poisoned")? = Some(index);
                Ok(page)
            }
            Err(error) => {
                scope::fail(&error);
                Err(error)
            }
        }
    })
    .await
    .map_err(|error| {
        let message = format!("duplicate scan task failed: {error}");
        scope::fail(&message);
        message
    })?
}

#[tauri::command]
pub async fn duplicate_finder_scan(
    roots: Vec<String>,
    minimum_size_bytes: Option<u64>,
) -> Result<DuplicateScan, String> {
    start_scan(
        safety::validate_roots(roots)?,
        Vec::new(),
        validate_minimum_size(minimum_size_bytes)?,
    )
    .await
}
#[tauri::command]
pub async fn duplicate_finder_scan_local(
    minimum_size_bytes: Option<u64>,
) -> Result<DuplicateScan, String> {
    let (roots, excluded) = scope::local_fixed_roots()?;
    start_scan(roots, excluded, validate_minimum_size(minimum_size_bytes)?).await
}
#[tauri::command]
pub fn duplicate_finder_progress() -> scope::DuplicateProgress {
    scope::snapshot()
}

fn with_index<T>(
    scan_id: &str,
    read: impl FnOnce(&index::ScanIndex) -> Result<T, String>,
) -> Result<T, String> {
    let guard = cache()
        .lock()
        .map_err(|_| "duplicate cache lock poisoned")?;
    let index = guard
        .as_ref()
        .ok_or("Scan again before reviewing duplicates")?;
    if index.scan_id != scan_id || index.created_at.elapsed() > CACHE_TTL {
        return Err("Duplicate scan expired or changed; scan again".into());
    }
    read(index)
}
#[tauri::command]
pub async fn duplicate_finder_page(
    scan_id: String,
    offset: usize,
) -> Result<DuplicateScan, String> {
    tokio::task::spawn_blocking(move || with_index(&scan_id, |index| index.page(offset)))
        .await
        .map_err(|error| error.to_string())?
}
#[tauri::command]
pub async fn duplicate_finder_group_page(
    scan_id: String,
    group_id: String,
    offset: usize,
) -> Result<DuplicateGroup, String> {
    tokio::task::spawn_blocking(move || {
        with_index(&scan_id, |index| index.group(&group_id, offset))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn duplicate_finder_open(scan_id: String, file_id: String) -> Result<(), String> {
    if file_id.is_empty() || file_id.len() > 64 {
        return Err("invalid duplicate file reference".into());
    }
    let target = tokio::task::spawn_blocking(move || {
        with_index(&scan_id, |index| {
            let file = index.file(&file_id)?;
            safety::validated_review_file(&file)
        })
    })
    .await
    .map_err(|error| error.to_string())??;
    #[cfg(windows)]
    {
        let windows = std::env::var_os("SystemRoot").ok_or("Windows directory unavailable.")?;
        std::process::Command::new(PathBuf::from(windows).join("explorer.exe"))
            .arg("/select,")
            .arg(target)
            .spawn()
            .map_err(|error| format!("Could not open Explorer: {error}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err("Open in Explorer requires Windows.".into())
    }
}

#[tauri::command]
pub async fn duplicate_finder_remove(
    file_ids: Vec<String>,
) -> Result<DuplicateRemoveResult, String> {
    safety::ensure_mutation_allowed()?;
    if file_ids.is_empty()
        || file_ids.len() > MAX_SELECTION
        || file_ids.iter().any(|id| id.len() > 64)
    {
        return Err("invalid duplicate selection".into());
    }
    let running = RunningGuard::acquire()?;
    let selected: HashSet<String> = file_ids.into_iter().collect();
    let (candidates, keepers) = {
        let guard = cache()
            .lock()
            .map_err(|_| "duplicate cache lock poisoned")?;
        let index = guard
            .as_ref()
            .ok_or("duplicate selection is stale; scan again")?;
        if index.created_at.elapsed() > CACHE_TTL {
            return Err("duplicate scan expired; scan again".into());
        }
        index.removal_selection(&selected)?
    };
    tokio::task::spawn_blocking(move || {
        let _running = running;
        safety::remove_files(candidates, keepers, &CANCELLED)
    })
    .await
    .map_err(|error| format!("duplicate removal task failed: {error}"))
}
#[tauri::command]
pub fn duplicate_finder_cancel() {
    CANCELLED.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests;
