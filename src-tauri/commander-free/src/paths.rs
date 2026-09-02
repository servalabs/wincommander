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

const MACHINE_STATE_SUBDIR: &str = "machine-state";
static MACHINE_STATE_ACL_INITIALIZED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static MACHINE_DATA_ACL_INITIALIZED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
const MACHINE_STATE_LOCK_TIMEOUT_MS: u32 = 5_000;

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
    // This directory stores device security policy (including Startup PIN
    // material). Interactive users may read it, but must never inherit write
    // access from ProgramData. The one-shot call also repairs installations
    // created before this policy existed. Failure is load-bearing: accepting a
    // newly-created writable ProgramData directory would let a standard user
    // alter machine policy and Startup PIN material.
    if MACHINE_DATA_ACL_INITIALIZED.get().is_none() {
        harden_machine_data_dir_acl(&dir)?;
        let _ = MACHINE_DATA_ACL_INITIALIZED.set(());
    }
    Ok(dir)
}

const MACHINE_DATA_ACL_GRANTS: [&str; 3] = [
    "*S-1-5-18:(OI)(CI)F",
    "*S-1-5-32-544:(OI)(CI)F",
    "*S-1-5-32-545:(OI)(CI)RX",
];

#[cfg(windows)]
fn require_machine_data_acl_success(output: std::process::Output) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "Failed to harden machine data directory ACL (icacls exit {}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

/// ACL for the device-owned ProgramData root. Unlike `harden_dir_acl`, Users
/// deliberately receive only read/execute access.
fn harden_machine_data_dir_acl(dir: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let dir_str = dir.to_string_lossy();
        let output = std::process::Command::new("icacls")
            .args([
                dir_str.as_ref(),
                "/inheritance:r",
                "/grant:r",
                MACHINE_DATA_ACL_GRANTS[0],
                MACHINE_DATA_ACL_GRANTS[1],
                MACHINE_DATA_ACL_GRANTS[2],
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| {
                format!("Failed to launch icacls for machine data directory: {error}")
            })?;
        require_machine_data_acl_success(output)
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        Ok(())
    }
}

fn is_valid_state_filename(filename: &str) -> bool {
    !filename.is_empty()
        && filename.len() <= 96
        && filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn state_file_from_dir(dir: PathBuf, filename: &str) -> Result<PathBuf, String> {
    if !is_valid_state_filename(filename) {
        return Err("invalid state filename".to_string());
    }
    Ok(dir.join(filename))
}

fn machine_state_dir() -> Result<PathBuf, String> {
    let dir = machine_data_dir()?.join(MACHINE_STATE_SUBDIR);
    let was_missing = !dir.exists();
    fs::create_dir_all(&dir)
        .map_err(|error| format!("Failed to create machine state directory: {error}"))?;

    // The root also contains installer-owned assets which predate this policy
    // boundary. Harden this dedicated directory once per process (and whenever
    // recreated) so low-privilege profiles cannot replace device-security
    // state, without spawning icacls on every USB event.
    if was_missing || MACHINE_STATE_ACL_INITIALIZED.get().is_none() {
        harden_dir_acl(&dir);
        let _ = MACHINE_STATE_ACL_INITIALIZED.set(());
    }
    Ok(dir)
}

/// A fixed-name state file that is part of the device policy, rather than an
/// interactive user's scratch data.  Keep filenames constrained here because
/// several security monitors use this helper while handling startup recovery.
pub fn machine_state_file(filename: &str) -> Result<PathBuf, String> {
    state_file_from_dir(machine_state_dir()?, filename)
}

/// Location used by released builds before a state file was moved to the
/// machine scope.  Callers may read this only to perform a one-time, validated
/// import; new writes must always use `machine_state_file`.
pub fn legacy_user_state_file(filename: &str) -> Result<PathBuf, String> {
    state_file_from_dir(user_data_dir()?, filename)
}

/// A bounded cross-session guard for one named machine-state resource. Use it
/// around a full read-modify-write sequence, not just the final file write.
pub struct MachineStateLock {
    #[cfg(windows)]
    handle: isize,
}

fn is_valid_machine_state_resource(resource: &str) -> bool {
    !resource.is_empty()
        && resource.len() <= 64
        && resource
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub fn acquire_machine_state_lock(resource: &str) -> Result<MachineStateLock, String> {
    if !is_valid_machine_state_resource(resource) {
        return Err("invalid machine state lock name".to_string());
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};

        const WAIT_OBJECT_0: u32 = 0;
        const WAIT_ABANDONED: u32 = 0x80;
        const WAIT_TIMEOUT: u32 = 0x102;
        let name: Vec<u16> = format!("Global\\WinCommander_{resource}_lock\0")
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err("machine-state lock is unavailable".to_string());
        }
        match unsafe { WaitForSingleObject(handle, MACHINE_STATE_LOCK_TIMEOUT_MS) } {
            // Atomic replacement means an abandoned writer cannot leave the
            // destination half-written, so ownership can safely transfer.
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(MachineStateLock {
                handle: handle as isize,
            }),
            WAIT_TIMEOUT => {
                unsafe { CloseHandle(handle) };
                Err("machine-state operation is busy; retry shortly".to_string())
            }
            _ => {
                unsafe { CloseHandle(handle) };
                Err("machine-state lock wait failed".to_string())
            }
        }
    }
    #[cfg(not(windows))]
    {
        Ok(MachineStateLock {})
    }
}

impl Drop for MachineStateLock {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.handle != 0 {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::ReleaseMutex;
            unsafe {
                ReleaseMutex(self.handle as _);
                CloseHandle(self.handle as _);
            }
        }
    }
}

/// Durably replace one file inside the ACL-hardened machine-state directory.
/// `std::fs::rename` cannot replace an existing destination on Windows, so
/// use MoveFileExW with replacement + write-through there.
pub fn atomic_write_machine_state(path: &std::path::Path, data: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "machine-state file has no parent directory".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "machine-state file has no valid filename".to_string())?;
    let expected_parent = machine_state_dir()?;
    if parent != expected_parent || !is_valid_state_filename(file_name) {
        return Err("machine-state write escaped the policy directory".to_string());
    }
    let temp = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    {
        let mut file =
            fs::File::create(&temp).map_err(|error| format!("create state temp: {error}"))?;
        use std::io::Write;
        file.write_all(data)
            .map_err(|error| format!("write state temp: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("sync state temp: {error}"))?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
        let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let moved = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved == 0 {
            let _ = fs::remove_file(&temp);
            return Err("replace machine-state file failed".to_string());
        }
    }
    #[cfg(not(windows))]
    {
        fs::rename(&temp, path).map_err(|error| {
            let _ = fs::remove_file(&temp);
            format!("replace machine-state file: {error}")
        })?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        is_valid_machine_state_resource, is_valid_state_filename, state_file_from_dir,
        MACHINE_DATA_ACL_GRANTS,
    };

    #[test]
    fn machine_data_acl_keeps_users_read_only() {
        assert_eq!(MACHINE_DATA_ACL_GRANTS[0], "*S-1-5-18:(OI)(CI)F");
        assert_eq!(MACHINE_DATA_ACL_GRANTS[1], "*S-1-5-32-544:(OI)(CI)F");
        assert_eq!(MACHINE_DATA_ACL_GRANTS[2], "*S-1-5-32-545:(OI)(CI)RX");
    }

    #[cfg(windows)]
    #[test]
    fn machine_data_acl_hardening_failure_is_returned() {
        let output = std::process::Command::new("cmd")
            .args(["/c", "echo denied 1>&2 & exit 7"])
            .output()
            .unwrap();
        let error = super::require_machine_data_acl_success(output).unwrap_err();
        assert!(error.contains("icacls exit"));
        assert!(error.contains("denied"));
    }

    #[test]
    fn machine_state_filenames_cannot_escape_programdata() {
        for invalid in [
            "",
            "../usb_timeline.json",
            "folder\\state.json",
            "state/name",
            "state\u{0}",
        ] {
            assert!(
                !is_valid_state_filename(invalid),
                "{invalid:?} must not be usable as a machine-state filename"
            );
        }
        assert!(is_valid_state_filename("usb_auto_sandbox.json"));
        assert!(is_valid_state_filename("f6-verify-boot-armed.json"));
    }

    #[test]
    fn machine_state_files_are_anchored_to_the_shared_device_directory() {
        let root = PathBuf::from(r"C:\ProgramData\WinCommander\machine-state");
        assert_eq!(
            state_file_from_dir(root, "usb_hid_guard.json").unwrap(),
            PathBuf::from(r"C:\ProgramData\WinCommander\machine-state\usb_hid_guard.json")
        );
    }

    #[test]
    fn machine_state_lock_names_reject_registry_or_path_syntax() {
        for invalid in [
            "usb/timeline",
            "usb\\timeline",
            "Global\\other",
            "state name",
        ] {
            assert!(
                !is_valid_machine_state_resource(invalid),
                "{invalid:?} must not be usable as a machine-state lock name"
            );
        }
        assert!(is_valid_machine_state_resource("usb-policy"));
    }
}
