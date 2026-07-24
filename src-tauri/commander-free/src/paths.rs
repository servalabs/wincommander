use std::fs;
use std::path::PathBuf;

// White-label: the data-dir / registry name. Defaults to "WinCommander"; an OEM
// build overrides it at compile time via the WINCMD_APP_DIR env var. Changing it
// relocates %LOCALAPPDATA%\<name>, %ProgramData%\<name>, the roaming fallback,
// logs, settings.json and the icon cache. Build-time only.
const APP_DIR_NAME: &str = match option_env!("WINCMD_APP_DIR") {
    Some(v) => v,
    None => "WinCommander",
};

// Human-readable display name used in window titles, notifications, tray menus,
// shortcut names, and uninstall registry entries. Override at build time via
// WINCMD_APP_NAME (for OEM builds). P4 flips this to the decoy identity.
const APP_DISPLAY_NAME: &str = match option_env!("WINCMD_APP_NAME") {
    Some(v) => v,
    None => "WinCommander",
};

/// The base display name (e.g. "WinCommander").
pub fn app_display_name() -> &'static str {
    APP_DISPLAY_NAME
}

/// Edition-qualified display name (e.g. "WinCommander Pro" / "WinCommander Free").
pub fn app_display_name_with_edition(pro: bool) -> String {
    if pro {
        format!("{} Pro", APP_DISPLAY_NAME)
    } else {
        format!("{} Free", APP_DISPLAY_NAME)
    }
}

/// The one-shot post-install scheduled task name written by the NSIS hook.
pub fn scheduled_task_launch_name() -> String {
    format!("{}LaunchOnce", APP_DISPLAY_NAME)
}

/// Suffix appended to renamed registry/Run values in hidden mode.
pub fn hidden_marker_suffix() -> &'static str {
    "__SystemCache"
}

/// Path to the flag file that persists hidden-mode across cold starts.
/// Machine-wide (%ProgramData%) so hidden state is shared across all Windows
/// accounts, consistent with the per-machine license + settings.
pub fn hide_flag_path() -> Result<PathBuf, String> {
    Ok(machine_data_dir()?.join("session_state.dat"))
}

pub fn hide_flag_legacy_path() -> Result<PathBuf, String> {
    Ok(old_roaming_data_dir()?.join("hide_wincommander.flag"))
}

/// Firewall rule name for the internet kill-switch (e.g. "WinCommander-KillSwitch-Out").
pub fn firewall_rule_name(suffix: &str) -> String {
    format!("{}-{}", APP_DISPLAY_NAME, suffix)
}

fn env_path(name: &str) -> Result<PathBuf, String> {
    std::env::var(name)
        .map(PathBuf::from)
        .map_err(|_| format!("{} not available", name))
}

#[allow(dead_code)]
pub fn install_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
    exe.parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "current executable has no parent directory".to_string())
}

pub fn user_data_dir() -> Result<PathBuf, String> {
    let dir = env_path("LOCALAPPDATA")?.join(APP_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create user data directory: {}", e))?;
    harden_dir_acl(&dir);
    Ok(dir)
}

/// Best-effort: lock a sensitive app-data dir to SYSTEM + Administrators + owner.
/// Windows-only; no-op elsewhere. Errors are swallowed (defense-in-depth, not
/// load-bearing). Uses well-known SIDs (*S-1-5-18 = SYSTEM, *S-1-5-32-544 =
/// Administrators) so the command is locale-independent.
///
/// Spawns a blocking `icacls.exe` process — callers that run per-command (e.g.
/// every FTS search) MUST NOT call this on every call; gate it so it only runs
/// on directory (re)creation (see `file_search::open_engine`, which hardens the
/// index dir only when it (re)opens the engine).
pub fn harden_dir_acl(dir: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        // Resolve current user as DOMAIN\user (or just user for local accounts).
        let user = {
            let domain = std::env::var("USERDOMAIN").unwrap_or_default();
            let name = std::env::var("USERNAME").unwrap_or_default();
            if domain.is_empty() || domain == std::env::var("COMPUTERNAME").unwrap_or_default() {
                name
            } else {
                format!("{}\\{}", domain, name)
            }
        };

        let dir_str = dir.to_string_lossy();
        let grant_user = format!("{}:(OI)(CI)F", user);

        let _ = std::process::Command::new("icacls")
            .args([
                dir_str.as_ref(),
                "/inheritance:r",
                "/grant:r",
                "*S-1-5-18:(OI)(CI)F",     // SYSTEM
                "*S-1-5-32-544:(OI)(CI)F", // Administrators
                grant_user.as_str(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = dir; // no-op on non-Windows
    }
}

pub fn old_roaming_data_dir() -> Result<PathBuf, String> {
    Ok(env_path("APPDATA")?.join(APP_DIR_NAME))
}

#[allow(dead_code)]
pub fn legacy_user_program_install_dir() -> Result<PathBuf, String> {
    Ok(env_path("LOCALAPPDATA")?
        .join("Programs")
        .join(APP_DIR_NAME))
}

pub fn machine_data_dir() -> Result<PathBuf, String> {
    let base = std::env::var("ProgramData")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("ALLUSERSPROFILE").map(PathBuf::from))
        .map_err(|_| "ProgramData not available".to_string())?;
    let dir = base.join(APP_DIR_NAME);
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create machine data directory: {}", e))?;
    Ok(dir)
}

pub fn user_settings_path() -> Result<PathBuf, String> {
    Ok(user_data_dir()?.join("settings.json"))
}

pub fn old_roaming_settings_path() -> Result<PathBuf, String> {
    Ok(old_roaming_data_dir()?.join("settings.json"))
}

pub fn user_logs_dir() -> Result<PathBuf, String> {
    let dir = user_data_dir()?.join("logs");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create log directory: {}", e))?;
    Ok(dir)
}

#[allow(dead_code)]
pub fn icon_cache_dir() -> Result<PathBuf, String> {
    let dir = user_data_dir()?.join("icon-cache");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create icon cache directory: {}", e))?;
    Ok(dir)
}

pub fn pro_sidecar_path() -> Result<PathBuf, String> {
    let dir = machine_data_dir()?.join("bin");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create Pro sidecar directory: {}", e))?;
    Ok(dir.join("wincommander-pro.exe"))
}

pub fn legacy_pro_sidecar_path() -> Result<PathBuf, String> {
    Ok(old_roaming_data_dir()?
        .join("bin")
        .join("wincommander-pro.exe"))
}

pub fn migration_marker_path() -> Result<PathBuf, String> {
    Ok(user_data_dir()?.join("migration-v3.done"))
}

pub fn migrate_user_data_layout() -> Result<(), String> {
    let marker = migration_marker_path()?;
    let new_settings = user_settings_path()?;

    if !new_settings.exists() {
        if let Ok(old_settings) = old_roaming_settings_path() {
            if old_settings.exists() {
                let _ = fs::copy(&old_settings, &new_settings);
            }
        }
    }

    if let (Ok(old_flag), Ok(new_flag)) = (hide_flag_legacy_path(), hide_flag_path()) {
        if old_flag.exists() && !new_flag.exists() {
            let _ = fs::rename(&old_flag, &new_flag);
        }
    }

    if let (Ok(old_pro), Ok(new_pro)) = (legacy_pro_sidecar_path(), pro_sidecar_path()) {
        if old_pro.exists() && !new_pro.exists() {
            if let Some(parent) = new_pro.parent() {
                let _ = fs::create_dir_all(parent);
            }
            // KT: only remove the legacy Roaming copy when the copy to ProgramData
            // succeeded — deleting unconditionally would leave the user with no Pro
            // binary if the copy failed (e.g. disk full, permissions).
            if fs::copy(&old_pro, &new_pro).is_ok() {
                let _ = fs::remove_file(&old_pro);
            }
        } else if old_pro.exists() && new_pro.exists() {
            // KT: ProgramData already has the binary (migration ran previously or the
            // installer placed it directly), but the legacy Roaming copy still lingers.
            // Remove it so pro_resolve_path never picks the stale duplicate.  Safe to
            // delete unconditionally here because new_pro.exists() guarantees the user
            // still has a working copy after the removal.
            let _ = fs::remove_file(&old_pro);
            // Prune the now-empty legacy Roaming bin/ dir (best-effort; ignore errors
            // if it is non-empty or was already removed).
            if let Some(old_bin_dir) = old_pro.parent() {
                let _ = fs::remove_dir(old_bin_dir);
            }
        }
    }

    if !marker.exists() {
        let _ = fs::write(
            &marker,
            format!(
                "migrationVersion=3\ncompletedAt={}\n",
                chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
            ),
        );
    }

    Ok(())
}
