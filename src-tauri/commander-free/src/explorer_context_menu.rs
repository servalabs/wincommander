// SPDX-License-Identifier: AGPL-3.0-or-later
// Explorer verb audit. It deliberately excludes shell-extension handlers: a CLSID alone is
// insufficient to identify ownership safely. Only explicit third-party executable verbs qualify.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[path = "explorer_context_menu/filters.rs"]
mod filters;
#[path = "explorer_context_menu/remediate.rs"]
mod remediate;
#[path = "explorer_context_menu/scan.rs"]
mod scan;
#[path = "explorer_context_menu/windows_registry.rs"]
mod windows_registry;
use remediate::remediate;
use scan::scan_verbs;

const CACHE_TTL: Duration = Duration::from_secs(20 * 60);
const SHELL_ROOTS: &[&str] = &[
    "Software\\Classes\\*\\shell",
    "Software\\Classes\\AllFilesystemObjects\\shell",
    "Software\\Classes\\Directory\\shell",
    "Software\\Classes\\Directory\\Background\\shell",
    "Software\\Classes\\Drive\\shell",
];
const DISABLED_ROOT: &str = "Software\\WinCommander\\ContextMenuDisabled";
const BACKUP_ROOT: &str = "Software\\WinCommander\\RegistryBackups\\ExplorerContextMenu";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hive {
    CurrentUser,
    LocalMachine,
}
impl Hive {
    fn label(self) -> &'static str {
        match self {
            Self::CurrentUser => "HKCU",
            Self::LocalMachine => "HKLM",
        }
    }
}

#[derive(Clone)]
struct CachedVerb {
    id: String,
    hive: Hive,
    subkey: String,
    label: String,
    command: String,
    enabled: bool,
    disabled_id: Option<String>,
}
struct VerbCache {
    created_at: Instant,
    entries: HashMap<String, CachedVerb>,
}
static VERB_CACHE: OnceLock<Mutex<VerbCache>> = OnceLock::new();
fn cache() -> &'static Mutex<VerbCache> {
    VERB_CACHE.get_or_init(|| {
        Mutex::new(VerbCache {
            created_at: Instant::now(),
            entries: HashMap::new(),
        })
    })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerContextEntry {
    pub id: String,
    pub label: String,
    pub location: String,
    pub command: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerContextScan {
    pub entries: Vec<ExplorerContextEntry>,
    pub skipped_entries: usize,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerContextResult {
    pub changed: usize,
    pub backup_locations: Vec<String>,
}

#[tauri::command]
pub async fn explorer_context_menu_scan() -> Result<ExplorerContextScan, String> {
    let (scan, entries) = tokio::task::spawn_blocking(scan_verbs)
        .await
        .map_err(|e| format!("Explorer menu scan task failed: {e}"))??;
    let mut guard = cache()
        .lock()
        .map_err(|_| "Explorer menu cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.entries = entries
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect();
    Ok(scan)
}

/// action is deliberately an enum-shaped string rather than a client-supplied registry operation.
#[tauri::command]
pub async fn explorer_context_menu_remediate(
    action: String,
    entry_ids: Vec<String>,
) -> Result<ExplorerContextResult, String> {
    ensure_mutation_allowed()?;
    if !matches!(action.as_str(), "disable" | "enable" | "remove")
        || entry_ids.is_empty()
        || entry_ids.len() > 512
        || entry_ids.iter().any(|id| id.len() > 64)
    {
        return Err("invalid Explorer menu remediation request".into());
    }
    let unique: HashSet<_> = entry_ids.into_iter().collect();
    let selected = {
        let guard = cache()
            .lock()
            .map_err(|_| "Explorer menu cache lock poisoned".to_string())?;
        if guard.created_at.elapsed() > CACHE_TTL {
            return Err("Explorer menu scan expired; scan again before changing entries".into());
        }
        unique
            .into_iter()
            .map(|id| {
                guard.entries.get(&id).cloned().ok_or_else(|| {
                    "Explorer menu selection is stale or invalid; scan again".to_string()
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    tokio::task::spawn_blocking(move || remediate(&action, &selected))
        .await
        .map_err(|e| format!("Explorer menu remediation task failed: {e}"))?
}

fn ensure_mutation_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: Explorer menu changes are unavailable in Decoy mode.".into());
    }
    if crate::license::is_advanced_mode() {
        return Err("Refused: investigator mode forbids Explorer menu changes because they would taint evidence.".into());
    }
    Ok(())
}
