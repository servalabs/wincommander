// SPDX-License-Identifier: AGPL-3.0-or-later
// Conservative registry hygiene: only broken per-user COM class servers are candidates.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
#[path = "registry_hygiene/remove.rs"]
mod remove;
#[path = "registry_hygiene/rules.rs"]
mod rules;
#[path = "registry_hygiene/scan.rs"]
mod scan;
#[path = "registry_hygiene/windows_registry.rs"]
mod windows_registry;
use remove::remove_orphans;
use scan::scan_orphans;

const CLSID_ROOT: &str = "Software\\Classes\\CLSID";
const CACHE_TTL: Duration = Duration::from_secs(20 * 60);

#[derive(Clone)]
struct CachedOrphan {
    id: String,
    subkey: String,
    class_id: String,
    server_kind: String,
    server_path: String,
}

struct OrphanCache {
    created_at: Instant,
    entries: HashMap<String, CachedOrphan>,
}

static ORPHAN_CACHE: OnceLock<Mutex<OrphanCache>> = OnceLock::new();

fn cache() -> &'static Mutex<OrphanCache> {
    ORPHAN_CACHE.get_or_init(|| {
        Mutex::new(OrphanCache {
            created_at: Instant::now(),
            entries: HashMap::new(),
        })
    })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryOrphan {
    pub id: String,
    pub class_id: String,
    pub server_kind: String,
    pub missing_server: String,
    pub hive: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryCleanerScan {
    pub entries: Vec<RegistryOrphan>,
    pub skipped_entries: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryCleanerResult {
    pub removed: usize,
    pub backup_locations: Vec<String>,
}

/// Read-only scan. IDs are random process-local handles; a client can never submit a key path.
#[tauri::command]
pub async fn registry_cleaner_scan() -> Result<RegistryCleanerScan, String> {
    let (scan, cached) = tokio::task::spawn_blocking(scan_orphans)
        .await
        .map_err(|error| format!("registry cleaner scan task failed: {error}"))??;
    let mut guard = cache()
        .lock()
        .map_err(|_| "registry cleaner cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.entries = cached
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect();
    Ok(scan)
}

/// Remove only entries returned by the current scan. Each source key is copied into the
/// app-owned registry backup store before deletion, so a failed deletion still preserves it.
#[tauri::command]
pub async fn registry_cleaner_remove(
    entry_ids: Vec<String>,
) -> Result<RegistryCleanerResult, String> {
    ensure_mutation_allowed()?;
    if entry_ids.is_empty() || entry_ids.len() > 512 || entry_ids.iter().any(|id| id.len() > 64) {
        return Err("invalid registry cleaner selection".into());
    }
    let unique: HashSet<_> = entry_ids.into_iter().collect();
    let selected = {
        let guard = cache()
            .lock()
            .map_err(|_| "registry cleaner cache lock poisoned".to_string())?;
        if guard.created_at.elapsed() > CACHE_TTL {
            return Err("registry cleaner scan expired; scan again before removal".into());
        }
        unique
            .into_iter()
            .map(|id| {
                guard.entries.get(&id).cloned().ok_or_else(|| {
                    "registry cleaner selection is stale or invalid; scan again".to_string()
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    tokio::task::spawn_blocking(move || remove_orphans(&selected))
        .await
        .map_err(|error| format!("registry cleaner removal task failed: {error}"))?
}

fn ensure_mutation_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: registry changes are unavailable in Decoy mode.".into());
    }
    if crate::license::is_advanced_mode() {
        return Err("Refused: investigator mode forbids registry changes because they would taint evidence.".into());
    }
    Ok(())
}
