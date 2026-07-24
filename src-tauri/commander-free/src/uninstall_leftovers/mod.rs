// SPDX-License-Identifier: AGPL-3.0-or-later
// Conservative residue finder: only stale top-level user AppData directories,
// never a linked/reparse subtree and never a folder matching installed software.

mod filesystem;
mod installed;
mod mutation;
mod scanner;

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const CACHE_TTL: Duration = Duration::from_secs(20 * 60);
pub(super) const MAX_CANDIDATES: usize = 100;
pub(super) const MIN_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub(super) const MIN_BYTES: u64 = 1024;
pub(super) const SAFE_NAMES: &[&str] = &[
    "microsoft",
    "windows",
    "packages",
    "temp",
    "tmp",
    "cache",
    "logs",
    "log",
    "crashdumps",
    "onedrive",
    "google",
    "mozilla",
    "adobe",
    "docker",
    "wsl",
    "nodejs",
    "node_modules",
    "python",
    "java",
    "rust",
    ".cargo",
    ".rustup",
    "git",
    "github",
    "steam",
    "discord",
    "slack",
    "teams",
    "zoom",
];

#[derive(Clone)]
pub(super) struct CachedFolder {
    pub(super) path: PathBuf,
    pub(super) root: PathBuf,
    pub(super) modified: Option<SystemTime>,
}
struct Cache {
    created_at: Instant,
    entries: HashMap<String, CachedFolder>,
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
pub(super) static CANCELLED: AtomicBool = AtomicBool::new(false);
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
pub struct UninstallLeftover {
    pub id: String,
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub scope: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallLeftoverScan {
    pub entries: Vec<UninstallLeftover>,
    pub scanned_folders: usize,
    pub skipped_folders: usize,
    pub cancelled: bool,
    pub truncated: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallLeftoverRemoveResult {
    pub removed: usize,
    pub bytes_recovered: u64,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn uninstall_leftovers_scan() -> Result<UninstallLeftoverScan, String> {
    CANCELLED.store(false, Ordering::Release);
    let (scan, entries) = tokio::task::spawn_blocking(|| scanner::scan_leftovers(&CANCELLED))
        .await
        .map_err(|error| format!("uninstall leftovers scan task failed: {error}"))?;
    let mut guard = cache()
        .lock()
        .map_err(|_| "uninstall leftovers cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.entries = entries;
    Ok(scan)
}
#[tauri::command]
pub async fn uninstall_leftovers_remove(
    ids: Vec<String>,
) -> Result<UninstallLeftoverRemoveResult, String> {
    ensure_mutation_allowed()?;
    if ids.is_empty() || ids.len() > MAX_CANDIDATES || ids.iter().any(|id| id.len() > 64) {
        return Err("invalid uninstall-leftover selection".into());
    }
    let ids: HashSet<_> = ids.into_iter().collect();
    let entries = {
        let guard = cache()
            .lock()
            .map_err(|_| "uninstall leftovers cache lock poisoned".to_string())?;
        if guard.created_at.elapsed() > CACHE_TTL {
            return Err(
                "uninstall leftovers scan expired; scan again before removing folders".into(),
            );
        }
        ids.into_iter()
            .map(|id| {
                guard.entries.get(&id).cloned().ok_or_else(|| {
                    "uninstall-leftover selection is stale or invalid; scan again".to_string()
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    CANCELLED.store(false, Ordering::Release);
    tokio::task::spawn_blocking(move || mutation::remove_leftovers(entries, &CANCELLED))
        .await
        .map_err(|error| format!("uninstall leftovers removal task failed: {error}"))
}
#[tauri::command]
pub fn uninstall_leftovers_cancel() {
    CANCELLED.store(true, Ordering::Release);
}

fn ensure_mutation_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: uninstall-leftover removal is unavailable in Decoy mode.".into());
    }
    if crate::license::is_advanced_mode() {
        return Err("Refused: investigator mode forbids uninstall-leftover removal because it would taint evidence.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::scanner::{is_recent, is_safe_name, matches_installed};
    use std::collections::HashSet;
    use std::fs;

    #[test]
    fn protects_common_folder_names() {
        assert!(is_safe_name("Microsoft"));
        assert!(is_safe_name(".config"));
        assert!(!is_safe_name("OldVendor"));
    }
    #[test]
    fn retains_installed_application_folders() {
        assert!(matches_installed(
            "Example App",
            &HashSet::from(["example app".into()])
        ));
        assert!(!matches_installed(
            "oldvendor",
            &HashSet::from(["example app".into()])
        ));
    }
    #[test]
    fn refuses_recent_directories() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_recent(&fs::metadata(dir.path()).unwrap()));
    }
}
