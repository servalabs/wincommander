// SPDX-License-Identifier: AGPL-3.0-or-later
// Native, preview-first duplicate finder. IDs are process-local capabilities.

mod safety;
mod scan;

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const CACHE_TTL: Duration = Duration::from_secs(20 * 60);
const MAX_ROOTS: usize = 8;
const MAX_FILES: usize = 100_000;

#[derive(Clone)]
struct CachedFile {
    path: PathBuf,
    root: PathBuf,
    group_id: String,
    bytes: u64,
    modified: Option<SystemTime>,
}

struct Cache {
    created_at: Instant,
    files: HashMap<String, CachedFile>,
    groups: HashMap<String, Vec<String>>,
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
static CANCELLED: AtomicBool = AtomicBool::new(false);

fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            created_at: Instant::now(),
            files: HashMap::new(),
            groups: HashMap::new(),
        })
    })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub id: String,
    pub size: u64,
    pub reclaimable_bytes: u64,
    pub files: Vec<DuplicateFile>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateScan {
    pub groups: Vec<DuplicateGroup>,
    pub scanned_files: usize,
    pub cancelled: bool,
    pub truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateRemoveResult {
    pub files_removed: usize,
    pub bytes_recovered: u64,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn duplicate_finder_scan(roots: Vec<String>) -> Result<DuplicateScan, String> {
    let roots = safety::validate_roots(roots)?;
    CANCELLED.store(false, Ordering::Release);
    let (scan, files, groups) =
        tokio::task::spawn_blocking(move || scan::scan_roots(&roots, &CANCELLED))
            .await
            .map_err(|error| format!("duplicate scan task failed: {error}"))?;
    let mut guard = cache()
        .lock()
        .map_err(|_| "duplicate cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.files = files;
    guard.groups = groups;
    Ok(scan)
}

#[tauri::command]
pub async fn duplicate_finder_remove(
    file_ids: Vec<String>,
) -> Result<DuplicateRemoveResult, String> {
    safety::ensure_mutation_allowed()?;
    if file_ids.is_empty() || file_ids.len() > MAX_FILES || file_ids.iter().any(|id| id.len() > 64)
    {
        return Err("invalid duplicate selection".into());
    }
    let selected: HashSet<String> = file_ids.into_iter().collect();
    let candidates = {
        let guard = cache()
            .lock()
            .map_err(|_| "duplicate cache lock poisoned".to_string())?;
        if guard.created_at.elapsed() > CACHE_TTL {
            return Err("duplicate scan expired; scan again before removing files".into());
        }
        let mut by_group: HashMap<&str, Vec<&CachedFile>> = HashMap::new();
        for id in &selected {
            let file = guard
                .files
                .get(id)
                .ok_or("duplicate selection is stale or invalid; scan again")?;
            by_group.entry(&file.group_id).or_default().push(file);
        }
        for (group_id, removals) in &by_group {
            let members = guard
                .groups
                .get(*group_id)
                .ok_or("duplicate group is stale; scan again")?;
            if removals.len() >= members.len()
                || !safety::retains_live_member(members, &selected, &guard.files)
            {
                return Err(
                    "refused: every duplicate group must retain at least one verified file".into(),
                );
            }
        }
        selected
            .iter()
            .filter_map(|id| guard.files.get(id).cloned())
            .collect::<Vec<_>>()
    };
    CANCELLED.store(false, Ordering::Release);
    tokio::task::spawn_blocking(move || safety::remove_files(candidates, &CANCELLED))
        .await
        .map_err(|error| format!("duplicate removal task failed: {error}"))
}

#[tauri::command]
pub fn duplicate_finder_cancel() {
    CANCELLED.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_after_size_candidates_and_keeps_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), b"same").unwrap();
        std::fs::write(dir.path().join("b"), b"same").unwrap();
        std::fs::write(dir.path().join("c"), b"nope").unwrap();
        let (scan, files, groups) = scan::scan_roots(
            &[std::fs::canonicalize(dir.path()).unwrap()],
            &AtomicBool::new(false),
        );
        assert_eq!(scan.groups.len(), 1);
        assert_eq!(scan.groups[0].files.len(), 2);
        let members = groups.values().next().unwrap();
        assert_eq!(members.len(), 2);
        assert!(safety::retains_live_member(
            members,
            &HashSet::from([members[0].clone()]),
            &files
        ));
        assert!(!safety::retains_live_member(
            members,
            &HashSet::from_iter(members.iter().cloned()),
            &files
        ));
    }

    #[test]
    fn rejects_empty_root_list() {
        assert!(safety::validate_roots(Vec::new()).is_err());
    }
}
