// SPDX-License-Identifier: AGPL-3.0-or-later
// Preview-first empty-folder cleaner. The client receives opaque IDs only.

mod safety;
mod scan;

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(20 * 60);
const MAX_ROOTS: usize = 8;
const MAX_FOLDERS: usize = 100_000;

#[derive(Clone)]
struct CachedFolder {
    path: PathBuf,
    root: PathBuf,
}

struct Cache {
    created_at: Instant,
    folders: HashMap<String, CachedFolder>,
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
static CANCELLED: AtomicBool = AtomicBool::new(false);

fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            created_at: Instant::now(),
            folders: HashMap::new(),
        })
    })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmptyFolder {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmptyFolderScan {
    pub folders: Vec<EmptyFolder>,
    pub scanned_folders: usize,
    pub cancelled: bool,
    pub truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmptyFolderRemoveResult {
    pub folders_removed: usize,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn empty_folder_cleaner_scan(roots: Vec<String>) -> Result<EmptyFolderScan, String> {
    let roots = safety::validate_roots(roots)?;
    CANCELLED.store(false, Ordering::Release);
    let (scan, folders) = tokio::task::spawn_blocking(move || scan::scan_roots(&roots, &CANCELLED))
        .await
        .map_err(|error| format!("empty folder scan task failed: {error}"))?;
    let mut guard = cache()
        .lock()
        .map_err(|_| "empty folder cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.folders = folders;
    Ok(scan)
}

#[tauri::command]
pub async fn empty_folder_cleaner_remove(
    folder_ids: Vec<String>,
) -> Result<EmptyFolderRemoveResult, String> {
    safety::ensure_mutation_allowed()?;
    if folder_ids.is_empty()
        || folder_ids.len() > MAX_FOLDERS
        || folder_ids.iter().any(|id| id.len() > 64)
    {
        return Err("invalid empty-folder selection".into());
    }
    let selected: HashSet<String> = folder_ids.into_iter().collect();
    let folders = {
        let guard = cache()
            .lock()
            .map_err(|_| "empty folder cache lock poisoned".to_string())?;
        if guard.created_at.elapsed() > CACHE_TTL {
            return Err("empty-folder scan expired; scan again before removing folders".into());
        }
        selected
            .iter()
            .map(|id| {
                guard
                    .folders
                    .get(id)
                    .cloned()
                    .ok_or("empty-folder selection is stale or invalid; scan again".into())
            })
            .collect::<Result<Vec<_>, String>>()?
    };
    CANCELLED.store(false, Ordering::Release);
    tokio::task::spawn_blocking(move || safety::remove_folders(folders, &CANCELLED))
        .await
        .map_err(|error| format!("empty-folder removal task failed: {error}"))
}

#[tauri::command]
pub fn empty_folder_cleaner_cancel() {
    CANCELLED.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_removes_only_empty_descendants() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("empty")).unwrap();
        std::fs::create_dir(dir.path().join("not-empty")).unwrap();
        std::fs::write(dir.path().join("not-empty").join("file"), b"x").unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let (scan, cached) = scan::scan_roots(&[root], &AtomicBool::new(false));
        assert_eq!(scan.folders.len(), 1);
        assert_eq!(scan.folders[0].name, "empty");
        assert_eq!(
            safety::remove_folders(
                vec![cached.get(&scan.folders[0].id).unwrap().clone()],
                &AtomicBool::new(false)
            )
            .folders_removed,
            1
        );
    }

    #[test]
    fn refuses_empty_root_list() {
        assert!(safety::validate_roots(Vec::new()).is_err());
    }
}
