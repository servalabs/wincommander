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
const CHOCOLATEY_WINGET_ID: &str = "Chocolatey.Chocolatey";
const SCOOP_INSTALLER_URL: &str =
    "https://raw.githubusercontent.com/ScoopInstaller/Install/master/install.ps1";
const MAX_SCOOP_INSTALLER_BYTES: usize = 2 * 1024 * 1024;
const OPTIONAL_MANAGERS: [Manager; 2] = [Manager::Chocolatey, Manager::Scoop];
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
            Self::Scoop => vec![
                format!("{program_data}\\WinCommander\\scoop\\shims\\scoop.cmd"),
                format!(
                    "{}\\scoop\\shims\\scoop.cmd",
                    std::env::var("USERPROFILE").unwrap_or_default()
                ),
            ],
            Self::Npm => vec![format!("{program_files}\\nodejs\\npm.cmd")],
        }
    }
    /// Resolve to a runnable path. Chocolatey and npm prefer their machine
    /// locations; Scoop can be installed per-user, so its supported user shim is
    /// considered before PATH. None of these probes installs a manager.
    fn resolve(self) -> String {
        self.resolve_existing()
            .unwrap_or_else(|| self.executable().to_string())
    }

    fn resolve_existing(self) -> Option<String> {
        let name = self.executable();
        if matches!(self, Self::Chocolatey | Self::Scoop | Self::Npm) {
            for candidate in self.fallback_paths() {
                if std::path::Path::new(&candidate).is_file() {
                    return Some(candidate);
                }
            }
        }
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate.to_string_lossy().into_owned());
                }
            }
        }
        for candidate in self.fallback_paths() {
            if std::path::Path::new(&candidate).is_file() {
                return Some(candidate);
            }
        }
        None
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionalManagerInstallResult {
    pub installed: Vec<String>,
    pub already_installed: Vec<String>,
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

/// Install the optional Chocolatey and Scoop package managers that are missing.
/// This command is only invoked by an explicit user action; it is never part of
/// startup inventory scanning. Chocolatey is installed by its exact, official
/// WinGet package ID. Scoop does not currently exist in the configured official
/// WinGet source, so its own published installer is downloaded to a temporary
/// file and run with PowerShell's `-File` argument (never piped to `iex`).
#[tauri::command]
pub async fn package_updates_install_optional_managers(
) -> Result<OptionalManagerInstallResult, String> {
    ensure_mutation_allowed()?;

    let mut result = OptionalManagerInstallResult {
        installed: Vec::new(),
        already_installed: Vec::new(),
        errors: Vec::new(),
    };

    for manager in OPTIONAL_MANAGERS {
        install_optional_manager(manager, &mut result).await;
    }
    Ok(result)
}

async fn install_optional_manager(manager: Manager, result: &mut OptionalManagerInstallResult) {
    let label = manager.label();
    match manager_installed(manager).await {
        Ok(true) => {
            result.already_installed.push(label.to_string());
            return;
        }
        Ok(false) => {}
        Err(error) => {
            result
                .errors
                .push(format!("{label}: could not check installation: {error}"));
            return;
        }
    };

    let install = match manager {
        Manager::Chocolatey => install_chocolatey().await,
        Manager::Scoop => install_scoop().await,
        _ => return,
    };

    match manager_installed(manager).await {
        Ok(true) => {
            // Verification is authoritative. Some MSI installs can return a
            // restart or already-present code after the executable is usable.
            result.installed.push(label.to_string());
        }
        Ok(false) => {
            let detail = match install {
                Ok(()) => "installer returned success, but the manager was not found after install"
                    .to_string(),
                Err(error) => error,
            };
            result
                .errors
                .push(format!("{label}: installation did not complete; {detail}"));
        }
        Err(verify_error) => {
            let install_detail = match install {
                Ok(()) => "installer returned success".to_string(),
                Err(error) => format!("installer failed: {error}"),
            };
            result.errors.push(format!(
                "{label}: install result could not be verified; {install_detail}; {verify_error}"
            ));
        }
    }
}

async fn manager_installed(manager: Manager) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        let Some(executable) = manager.resolve_existing() else {
            return Ok(false);
        };
        process::run(&executable, manager.version_args())
            .map(|_| true)
            .map_err(|error| format!("found {} but could not run it: {error}", executable))
    })
    .await
    .map_err(|error| format!("manager status task failed: {error}"))?
}

async fn install_chocolatey() -> Result<(), String> {
    let Some(winget) = Manager::Winget.resolve_existing() else {
        return Err("WinGet is required to install Chocolatey from its official source.".into());
    };
    let args = chocolatey_winget_install_args();
    let elevated = crate::startup_elevation::is_current_process_elevated();
    tokio::task::spawn_blocking(move || process::run(&winget, &args).map(|_| ()))
        .await
        .map_err(|error| format!("WinGet install task failed: {error}"))?
        .map_err(|error| {
            if elevated {
                error
            } else {
                format!(
                    "Chocolatey installs machine-wide and needs administrator approval. Approve the WinGet elevation prompt, or reopen WinCommander as an administrator and retry. {error}"
                )
            }
        })
}

fn chocolatey_winget_install_args() -> [&'static str; 10] {
    [
        "install",
        "--id",
        CHOCOLATEY_WINGET_ID,
        "--exact",
        "--source",
        "winget",
        "--scope",
        "machine",
        "--accept-source-agreements",
        "--accept-package-agreements",
    ]
}

async fn install_scoop() -> Result<(), String> {
    let elevated = crate::startup_elevation::is_current_process_elevated();

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|error| format!("could not prepare secure Scoop download: {error}"))?;
    let response = client
        .get(SCOOP_INSTALLER_URL)
        .send()
        .await
        .map_err(|error| format!("could not download the official Scoop installer: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "official Scoop installer download returned HTTP {}",
            response.status()
        ));
    }
    if response.url().scheme() != "https"
        || response.url().host_str() != Some("raw.githubusercontent.com")
        || response.url().path() != "/ScoopInstaller/Install/master/install.ps1"
    {
        return Err("Scoop installer URL did not match its official source location".into());
    }
    let expected_length = response
        .content_length()
        .filter(|length| *length > 0 && *length <= MAX_SCOOP_INSTALLER_BYTES as u64)
        .ok_or_else(|| {
            "official Scoop installer has a missing or invalid content length".to_string()
        })?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("could not read the official Scoop installer: {error}"))?;
    if bytes.len() != expected_length as usize {
        return Err("official Scoop installer download length did not match its header".into());
    }
    let script_text = String::from_utf8(bytes.to_vec())
        .map_err(|_| "official Scoop installer is not valid UTF-8".to_string())?;
    if !script_text.contains("Scoop installer")
        || !script_text.contains("function Test-Prerequisite")
    {
        return Err("downloaded file did not match the expected Scoop installer".into());
    }

    // Keep the path alive until PowerShell exits, but close Rust's file handle
    // before launching it. On Windows a NamedTempFile handle can deny the
    // sharing mode PowerShell needs to open the script, which makes -File fail
    // with "the process cannot access the file ... because it is being used by
    // another process" even though this is the only installation in flight.
    let script = create_scoop_installer_file(&script_text)?;
    let script_path = script.to_path_buf().to_string_lossy().into_owned();
    tokio::task::spawn_blocking(move || {
        // TempPath removes this invocation's uniquely named script on drop,
        // after process::run has waited for the installer to finish.
        let _temporary_script = script;
        let powershell = powershell_executable()?;
        let args = scoop_installer_args(&script_path, elevated);
        process::run(&powershell, &args).map(|_| ())
    })
    .await
    .map_err(|error| format!("Scoop install task failed: {error}"))?
}

fn create_scoop_installer_file(script_text: &str) -> Result<tempfile::TempPath, String> {
    use std::io::Write;

    let mut script = tempfile::Builder::new()
        .prefix("wincommander-scoop-install-")
        .suffix(".ps1")
        .tempfile()
        .map_err(|error| format!("could not create a temporary Scoop installer: {error}"))?;
    script
        .write_all(script_text.as_bytes())
        .and_then(|_| script.flush())
        .map_err(|error| format!("could not save the official Scoop installer: {error}"))?;

    // into_temp_path drops the open File while retaining automatic deletion.
    // Each call gets a distinct path from tempfile's exclusive-create logic.
    Ok(script.into_temp_path())
}

fn scoop_installer_args(script_path: &str, elevated: bool) -> Vec<&str> {
    let mut args = vec![
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        script_path,
    ];
    if elevated {
        // Scoop rejects admin sessions by default. Its documented opt-in is
        // appropriate here only because this function is reached from the
        // user's explicit install-button action and the app already has an
        // elevated token; it does not start another elevation prompt.
        args.push("-RunAsAdmin");
    }
    args
}

fn powershell_executable() -> Result<String, String> {
    let windows_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let candidate = format!(r"{windows_root}\System32\WindowsPowerShell\v1.0\powershell.exe");
    if std::path::Path::new(&candidate).is_file() {
        Ok(candidate)
    } else {
        Err("Windows PowerShell could not be found at its system location".into())
    }
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
    fn combined_install_is_limited_to_chocolatey_and_scoop() {
        assert_eq!(OPTIONAL_MANAGERS, [Manager::Chocolatey, Manager::Scoop]);
    }

    #[test]
    fn chocolatey_install_uses_only_its_exact_machine_scope_winget_package() {
        assert_eq!(
            chocolatey_winget_install_args(),
            [
                "install",
                "--id",
                "Chocolatey.Chocolatey",
                "--exact",
                "--source",
                "winget",
                "--scope",
                "machine",
                "--accept-source-agreements",
                "--accept-package-agreements",
            ]
        );
    }

    #[test]
    fn optional_manager_install_result_serializes_camel_case_fields() {
        let value = serde_json::to_value(OptionalManagerInstallResult {
            installed: vec!["chocolatey".into()],
            already_installed: vec!["scoop".into()],
            errors: vec!["npm: not applicable".into()],
        })
        .unwrap();
        assert_eq!(value["installed"], serde_json::json!(["chocolatey"]));
        assert_eq!(value["alreadyInstalled"], serde_json::json!(["scoop"]));
        assert!(value.get("already_installed").is_none());
        assert_eq!(value["errors"], serde_json::json!(["npm: not applicable"]));
    }

    #[test]
    fn scoop_install_script_uses_the_upstream_installer_repository() {
        let url = reqwest::Url::parse(SCOOP_INSTALLER_URL).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("raw.githubusercontent.com"));
        assert_eq!(url.path(), "/ScoopInstaller/Install/master/install.ps1");
    }

    #[test]
    fn scoop_install_script_is_readable_after_handoff_and_removed_after_completion() {
        let contents = "# Scoop installer fixture";
        let first = create_scoop_installer_file(contents).unwrap();
        let second = create_scoop_installer_file(contents).unwrap();
        let first_path = first.to_path_buf();

        assert_ne!(first_path, second.to_path_buf());
        assert_eq!(std::fs::read_to_string(&first).unwrap(), contents);

        drop(first);
        assert!(!first_path.exists());
        assert_eq!(std::fs::read_to_string(&second).unwrap(), contents);
    }

    #[test]
    fn parses_chocolatey_machine_rows() {
        assert_eq!(
            parsing::parse_text("chocolatey", "git|2.45|2.46|false"),
            vec![("git".into(), "2.45".into(), "2.46".into())]
        );
    }

    #[test]
    fn scoop_installer_only_receives_run_as_admin_for_an_already_elevated_app() {
        let normal = scoop_installer_args(r"C:\Temp\install.ps1", false);
        assert!(normal.contains(&"-NonInteractive"));
        assert!(normal.contains(&"-File"));
        assert!(normal.contains(&r"C:\Temp\install.ps1"));
        assert!(!normal.contains(&"-RunAsAdmin"));
        assert!(!normal.contains(&"-Command"));

        let elevated = scoop_installer_args(r"C:\Temp\install.ps1", true);
        assert!(elevated.ends_with(&["-RunAsAdmin"]));
        assert!(!elevated.contains(&"-Command"));
    }
    #[test]
    fn parses_scoop_global_rows() {
        assert_eq!(
            parsing::parse_text("scoop", "git 2.45 2.46"),
            vec![("git".into(), "2.45".into(), "2.46".into())]
        );
    }
    #[test]
    fn finds_an_approved_user_scoop_install_without_bootstrapping() {
        let fallback = Manager::Scoop.fallback_paths();
        let user_scoop = format!(
            "{}\\scoop\\shims\\scoop.cmd",
            std::env::var("USERPROFILE").unwrap_or_default()
        );
        assert!(fallback.contains(&user_scoop));
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
