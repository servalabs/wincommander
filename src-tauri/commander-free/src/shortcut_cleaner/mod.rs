// SPDX-License-Identifier: AGPL-3.0-or-later
// Preview-first broken shortcut cleaner. Link parsing intentionally only accepts
// local filesystem targets encoded in LinkInfo; unresolved shell links are skipped.

mod filesystem;
mod mutation;
mod scanner;

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const CACHE_TTL: Duration = Duration::from_secs(20 * 60);
const MAX_SHORTCUTS: usize = 25_000;

#[derive(Clone)]
pub(super) struct CachedShortcut {
    pub(super) path: PathBuf,
    pub(super) root: PathBuf,
    pub(super) bytes: u64,
    pub(super) modified: Option<SystemTime>,
}
struct Cache {
    created_at: Instant,
    entries: HashMap<String, CachedShortcut>,
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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokenShortcut {
    pub id: String,
    pub name: String,
    pub path: String,
    pub target: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutScan {
    pub shortcuts: Vec<BrokenShortcut>,
    pub scanned_shortcuts: usize,
    pub cancelled: bool,
    pub truncated: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutRemoveResult {
    pub removed: usize,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn shortcut_cleaner_scan() -> Result<ShortcutScan, String> {
    CANCELLED.store(false, Ordering::Release);
    let (scan, entries) = tokio::task::spawn_blocking(|| scanner::scan_shortcuts(&CANCELLED))
        .await
        .map_err(|error| format!("shortcut scan task failed: {error}"))?;
    let mut guard = cache()
        .lock()
        .map_err(|_| "shortcut cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.entries = entries;
    Ok(scan)
}

#[tauri::command]
pub async fn shortcut_cleaner_remove(ids: Vec<String>) -> Result<ShortcutRemoveResult, String> {
    ensure_mutation_allowed()?;
    if ids.is_empty() || ids.len() > MAX_SHORTCUTS || ids.iter().any(|id| id.len() > 64) {
        return Err("invalid shortcut selection".into());
    }
    let selected: HashSet<_> = ids.into_iter().collect();
    let entries =
        {
            let guard = cache()
                .lock()
                .map_err(|_| "shortcut cache lock poisoned".to_string())?;
            if guard.created_at.elapsed() > CACHE_TTL {
                return Err("shortcut scan expired; scan again before removing shortcuts".into());
            }
            selected
                .into_iter()
                .map(|id| {
                    guard.entries.get(&id).cloned().ok_or_else(|| {
                        "shortcut selection is stale or invalid; scan again".to_string()
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        };
    CANCELLED.store(false, Ordering::Release);
    tokio::task::spawn_blocking(move || mutation::remove_shortcuts(entries, &CANCELLED))
        .await
        .map_err(|error| format!("shortcut removal task failed: {error}"))
}

#[tauri::command]
pub fn shortcut_cleaner_cancel() {
    CANCELLED.store(true, Ordering::Release);
}

fn ensure_mutation_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: shortcut removal is unavailable in Decoy mode.".into());
    }
    if crate::license::is_advanced_mode() {
        return Err(
            "Refused: investigator mode forbids shortcut removal because it would taint evidence."
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::scanner::{is_system_shortcut_directory, lnk_local_target, read_u32};
    use std::path::Path;

    #[test]
    fn refuses_malformed_links() {
        assert!(lnk_local_target(Path::new("missing.lnk")).is_none());
    }
    #[test]
    fn excludes_system_start_menu_folders() {
        assert!(is_system_shortcut_directory(Path::new(
            "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\System Tools\\x.lnk"
        )));
    }
    #[test]
    fn parses_lnk_header_bounds() {
        assert!(read_u32(&[0; 3], 0).is_none());
    }
}
