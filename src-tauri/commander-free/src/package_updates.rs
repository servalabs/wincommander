// SPDX-License-Identifier: AGPL-3.0-or-later
//! Preview-first inventory and selected updates for local package managers.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[path = "package_updates/parsing.rs"]
mod parsing;
#[path = "package_updates/process.rs"]
mod process;

const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_SELECTION: usize = 500;
static CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Manager {
    Winget,
    Chocolatey,
    Scoop,
    Npm,
}

impl Manager {
    fn label(self) -> &'static str {
        match self {
            Self::Winget => "winget",
            Self::Chocolatey => "chocolatey",
            Self::Scoop => "scoop",
            Self::Npm => "npm",
        }
    }
    fn executable(self) -> &'static str {
        match self {
            Self::Winget => "winget.exe",
            Self::Chocolatey => "choco.exe",
            Self::Scoop => "scoop.cmd",
            Self::Npm => "npm.cmd",
        }
    }
    fn version_args(self) -> &'static [&'static str] {
        &["--version"]
    }
    /// Absolute fallback locations, tried when a PATH lookup for
    /// `executable()` comes up empty. Elevated sessions do not reliably resolve
    /// App Execution Aliases under `%LOCALAPPDATA%\Microsoft\WindowsApps` — the
    /// same limitation the PowerShell side works around in `Resolve-WingetPath` /
    /// `Get-LocalWingetPath` — so winget needs explicit candidates even though
    /// it's genuinely installed.
    fn fallback_paths(self) -> Vec<String> {
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let program_data = std::env::var("ProgramData").unwrap_or_default();
        let program_files = std::env::var("ProgramFiles").unwrap_or_default();
        match self {
            Self::Winget => vec![
                format!("{local_app_data}\\Microsoft\\WindowsApps\\winget.exe"),
                format!("{local_app_data}\\Microsoft\\WinGet\\Links\\winget.exe"),
            ],
            Self::Chocolatey => {
                vec![format!("{program_data}\\chocolatey\\bin\\choco.exe")]
            }
            Self::Scoop => vec![format!(
                "{program_data}\\WinCommander\\scoop\\shims\\scoop.cmd"
            )],
            Self::Npm => vec![format!("{program_files}\\nodejs\\npm.cmd")],
        }
    }
    /// Resolve to a runnable path. Chocolatey, Scoop, and npm prefer their machine
    /// locations; the other managers use PATH before their fallback candidates.
    fn resolve(self) -> String {
        let name = self.executable();
        if matches!(self, Self::Chocolatey | Self::Scoop | Self::Npm) {
            for candidate in self.fallback_paths() {
                if std::path::Path::new(&candidate).is_file() {
                    return candidate;
                }
            }
        }
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return candidate.to_string_lossy().into_owned();
                }
            }
        }
        for candidate in self.fallback_paths() {
            if std::path::Path::new(&candidate).is_file() {
                return candidate;
            }
        }
        name.to_string()
    }
}

#[derive(Clone)]
struct CachedUpdate {
    manager: Manager,
    package: String,
}
struct Cache {
    created_at: Instant,
    updates: HashMap<String, CachedUpdate>,
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            created_at: Instant::now(),
            updates: HashMap::new(),
        })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageUpdate {
    pub id: String,
    pub manager: String,
    pub package: String,
    pub current_version: String,
    pub available_version: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerInventory {
    pub manager: String,
    pub available: bool,
    pub updates: Vec<PackageUpdate>,
    pub error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageUpdateInventory {
    pub managers: Vec<ManagerInventory>,
    pub cancelled: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageUpdateResult {
    pub updated: usize,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn package_updates_inventory() -> Result<PackageUpdateInventory, String> {
    ensure_read_allowed()?;
    CANCELLED.store(false, Ordering::Release);
    let (inventory, updates) = tokio::task::spawn_blocking(scan_managers)
        .await
        .map_err(|e| format!("package inventory task failed: {e}"))?;
    let mut guard = cache()
        .lock()
        .map_err(|_| "package update cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.updates = updates;
    Ok(inventory)
}

#[tauri::command]
pub async fn package_updates_apply(update_ids: Vec<String>) -> Result<PackageUpdateResult, String> {
    ensure_mutation_allowed()?;
    if update_ids.is_empty()
        || update_ids.len() > MAX_SELECTION
        || update_ids.iter().any(|id| id.len() > 64)
    {
        return Err("invalid package update selection".into());
    }
    let selected = selected_updates(update_ids)?;
    CANCELLED.store(false, Ordering::Release);
    tokio::task::spawn_blocking(move || process::apply_updates(selected))
        .await
        .map_err(|e| format!("package update task failed: {e}"))?
}

#[tauri::command]
pub fn package_updates_cancel() {
    CANCELLED.store(true, Ordering::Release);
}

fn selected_updates(ids: Vec<String>) -> Result<Vec<CachedUpdate>, String> {
    let guard = cache()
        .lock()
        .map_err(|_| "package update cache lock poisoned".to_string())?;
    if guard.created_at.elapsed() > CACHE_TTL {
        return Err("package inventory expired; scan again before updating".into());
    }
    ids.into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|id| {
            guard
                .updates
                .get(&id)
                .cloned()
                .ok_or_else(|| "package selection is stale or invalid; scan again".into())
        })
        .collect()
}

fn scan_managers() -> (PackageUpdateInventory, HashMap<String, CachedUpdate>) {
    let mut managers = Vec::new();
    let mut cached = HashMap::new();
    for manager in [
        Manager::Winget,
        Manager::Chocolatey,
        Manager::Scoop,
        Manager::Npm,
    ] {
        if CANCELLED.load(Ordering::Acquire) {
            break;
        }
        match process::run(&manager.resolve(), manager.version_args()) {
            Err(error) => managers.push(ManagerInventory {
                manager: manager.label().into(),
                available: false,
                updates: Vec::new(),
                error: Some(error),
            }),
            Ok(_) => match inventory_for(manager) {
                Ok(rows) => {
                    let updates = rows
                        .into_iter()
                        .map(|(package, current, available)| {
                            let id = Uuid::new_v4().to_string();
                            cached.insert(
                                id.clone(),
                                CachedUpdate {
                                    manager,
                                    package: package.clone(),
                                },
                            );
                            PackageUpdate {
                                id,
                                manager: manager.label().into(),
                                package,
                                current_version: current,
                                available_version: available,
                            }
                        })
                        .collect();
                    managers.push(ManagerInventory {
                        manager: manager.label().into(),
                        available: true,
                        updates,
                        error: None,
                    });
                }
                Err(error) => managers.push(ManagerInventory {
                    manager: manager.label().into(),
                    available: true,
                    updates: Vec::new(),
                    error: Some(error),
                }),
            },
        }
    }
    (
        PackageUpdateInventory {
            managers,
            cancelled: CANCELLED.load(Ordering::Acquire),
        },
        cached,
    )
}

fn inventory_for(manager: Manager) -> Result<Vec<(String, String, String)>, String> {
    let resolved = manager.resolve();
    let output = match manager {
        Manager::Winget => process::run(
            &resolved,
            &[
                "upgrade",
                "--include-unknown",
                "--accept-source-agreements",
                "--disable-interactivity",
            ],
        )?,
        Manager::Chocolatey => {
            process::run(&resolved, &["outdated", "--limit-output", "--no-color"])?
        }
        Manager::Scoop => process::run(&resolved, &["status", "--global"])?,
        Manager::Npm => process::run_npm_outdated()?,
    };
    let rows = if manager == Manager::Npm {
        parsing::parse_npm(&output)?
    } else {
        parsing::parse_text(manager.label(), &output)
    };
    Ok(rows
        .into_iter()
        .filter(|(package, _, _)| valid_package_name(manager, package))
        .collect())
}

fn valid_package_name(manager: Manager, package: &str) -> bool {
    !package.is_empty()
        && package.len() <= 128
        && package.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '_' | '-' | '+')
                || (manager == Manager::Npm && matches!(character, '@' | '/'))
        })
}

fn ensure_read_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        Err("Refused: package inventory is unavailable in Decoy mode.".into())
    } else {
        Ok(())
    }
}
fn ensure_mutation_allowed() -> Result<(), String> {
    ensure_read_allowed()?;
    if crate::license::is_advanced_mode() {
        return Err(
            "Refused: investigator mode forbids package updates because they alter evidence."
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_chocolatey_machine_rows() {
        assert_eq!(
            parsing::parse_text("chocolatey", "git|2.45|2.46|false"),
            vec![("git".into(), "2.45".into(), "2.46".into())]
        );
    }
    #[test]
    fn parses_scoop_global_rows() {
        assert_eq!(
            parsing::parse_text("scoop", "git 2.45 2.46"),
            vec![("git".into(), "2.45".into(), "2.46".into())]
        );
    }
    #[test]
    fn rejects_npm_non_json() {
        assert!(parsing::parse_npm("not json").is_err());
    }
    #[test]
    fn rejects_batch_metacharacters_from_manager_output() {
        assert!(valid_package_name(Manager::Npm, "@scope/package"));
        assert!(!valid_package_name(Manager::Npm, "safe&whoami"));
        assert!(!valid_package_name(Manager::Scoop, "safe|whoami"));
    }
}
