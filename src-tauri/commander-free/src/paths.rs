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
static USER_DATA_ACL_CACHE: std::sync::Mutex<DirectoryAclCache> =
    std::sync::Mutex::new(DirectoryAclCache(Vec::new()));
const MACHINE_STATE_LOCK_TIMEOUT_MS: u32 = 5_000;

#[derive(Default)]
struct DirectoryAclCache(Vec<(PathBuf, (u64, u64))>);

impl DirectoryAclCache {
    fn ensure(
        &mut self,
        path: &std::path::Path,
        identity: Option<(u64, u64)>,
        apply: impl FnOnce() -> bool,
    ) {
        if identity.is_some_and(|id| self.0.iter().any(|(p, cached)| p == path && *cached == id)) {
            return;
        }
        self.0.retain(|(p, _)| p != path);
        if apply() {
            if let Some(identity) = identity {
                self.0.push((path.to_path_buf(), identity));
            }
        }
    }
}

fn directory_identity(path: &std::path::Path) -> Option<(u64, u64)> {
    #[cfg(windows)]
    {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        };
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .ok()?;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return None;
        }
        Some((
            info.dwVolumeSerialNumber as u64,
            ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

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
    // Repeated settings/log accesses must not launch another icacls process.
    let mut cache = USER_DATA_ACL_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    cache.ensure(&dir, directory_identity(&dir), || {
        harden_dir_acl_result(&dir)
    });
    Ok(dir)
}

/// A current-user NSIS install lives directly below the caller's LocalAppData
/// directory.  That location is a per-user security boundary: it must never
/// make the desktop shell depend on a writable ProgramData directory merely to
/// read or save its own settings.
///
/// Keep this deliberately narrow.  A portable/debug copy (or an arbitrary
/// executable nested below LocalAppData) does not become a current-user install
/// just because of its parent directory.  Only the installed primary binary is
/// eligible for the per-user datastore route.
fn is_current_user_install_executable(
    executable: &std::path::Path,
    local_app_data: &std::path::Path,
) -> bool {
    let expected = local_app_data
        .join(APP_DIR_NAME)
        .join("wincommander-free.exe");
    executable
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy())
}

/// Whether the running desktop binary is the NSIS current-user installation.
///
/// Machine/service state always stays in ProgramData.  The general application
/// datastore, however, is per-user for this install mode so a standard account
/// can open WinCommander without being asked to modify a machine-owned folder.
pub fn current_user_install_uses_local_datastore() -> bool {
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    let Ok(local_app_data) = env_path("LOCALAPPDATA") else {
        return false;
    };
    is_current_user_install_executable(&executable, &local_app_data)
}

/// Root for general application data (settings, local licence cache, and
/// similarly user-facing state).  A machine install retains ProgramData so its
/// service and shared policy keep one source of truth.  A current-user install
/// uses the interactive user's LocalAppData instead.
pub fn datastore_data_dir() -> Result<PathBuf, String> {
    if current_user_install_uses_local_datastore() {
        user_data_dir()
    } else {
        machine_data_dir()
    }
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
    let _ = harden_dir_acl_result(dir);
}

fn harden_dir_acl_result(dir: &std::path::Path) -> bool {
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

        let child = std::process::Command::new("icacls")
            .args([
                dir_str.as_ref(),
                "/inheritance:r",
                "/grant:r",
                "*S-1-5-18:(OI)(CI)F",     // SYSTEM
                "*S-1-5-32-544:(OI)(CI)F", // Administrators
                grant_user.as_str(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        child.is_ok_and(|mut child| {
            wait_for_acl_child(&mut child, std::time::Duration::from_secs(5))
        })
    }
    #[cfg(not(windows))]
    {
        let _ = dir; // no-op on non-Windows
        true
    }
}

#[cfg(windows)]
fn wait_for_acl_child(child: &mut std::process::Child, budget: std::time::Duration) -> bool {
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started.elapsed() < budget => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            _ => {
                // This Child owns the process handle: never kill by name or PID.
                // Windows releases process resources when the handle is closed;
                // don't turn a best-effort timeout into another blocking wait.
                let _ = child.kill();
                return false;
            }
        }
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
    let was_missing = !dir.exists();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create machine data directory: {}", e))?;
    // ACL repair is an installation/administrator operation. Reapplying it on
    // every launch asks a standard account to modify ProgramData, which blocks
    // otherwise read-only settings loads. A newly-created directory is still
    // hardened before use; an existing installer-owned directory is only read.
    if should_harden_machine_data_dir(was_missing) && MACHINE_DATA_ACL_INITIALIZED.get().is_none() {
        harden_machine_data_dir_acl(&dir)?;
        let _ = MACHINE_DATA_ACL_INITIALIZED.set(());
    }
    Ok(dir)
}

fn should_harden_machine_data_dir(was_missing: bool) -> bool {
    was_missing
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
    acquire_machine_state_lock_with_timeout(resource, MACHINE_STATE_LOCK_TIMEOUT_MS)
}

pub(crate) fn acquire_shield_action_lock() -> Result<MachineStateLock, String> {
    acquire_machine_state_lock_with_timeout("fleet-shield-policy", 45_000).map_err(|error| {
        if error == "machine-state operation is busy; retry shortly" {
            "Privacy Shield policy is still changing. Wait for the current transition to finish, then retry.".into()
        } else { error }
    })
}

fn acquire_machine_state_lock_with_timeout(resource: &str, timeout_ms: u32) -> Result<MachineStateLock, String> {
    if !is_valid_machine_state_resource(resource) {
        return Err("invalid machine state lock name".to_string());
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED};
        use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};

        const WAIT_OBJECT_0: u32 = 0;
        const WAIT_ABANDONED: u32 = 0x80;
        const WAIT_TIMEOUT: u32 = 0x102;
        let name: Vec<u16> = format!("Global\\WinCommander_{resource}_lock\0")
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            if unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
                return Err("machine-state lock access denied".to_string());
            }
            return Err("machine-state lock is unavailable".to_string());
        }
        match unsafe { WaitForSingleObject(handle, timeout_ms) } {
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
        let _ = timeout_ms;
        Ok(MachineStateLock {})
    }
}

/// Coordinate one profile's log across sessions without blocking other profiles.
pub(crate) fn acquire_diagnostic_log_lock() -> Result<MachineStateLock, String> {
    let directory = fs::canonicalize(user_logs_dir()?)
        .map_err(|_| "diagnostic log directory is unavailable".to_string())?;
    acquire_machine_state_lock(&diagnostic_log_lock_resource(&directory))
}

fn diagnostic_log_lock_resource(directory: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    let identity = directory
        .to_string_lossy()
        .replace('/', "\\")
        .to_lowercase();
    let digest = Sha256::digest(identity.as_bytes());
    // Keep private profile paths out of the kernel object name.
    let suffix: String = digest[..24]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("diagnostics-{suffix}")
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

pub(crate) fn legacy_settings_exist(path: &std::path::Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("Could not inspect legacy settings".into()),
    }
}

fn copy_legacy_settings_if_absent(old: &std::path::Path, new: &std::path::Path) -> Result<(), String> {
    use std::io::Read;
    const MAX_LEGACY_BYTES: u64 = 16 * 1024 * 1024;
    if legacy_settings_exist(new)? {
        return Ok(());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = match options.open(old) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) if legacy_settings_exist(new)? => return Ok(()),
        Err(_) => return Err("Could not open legacy settings for migration".into()),
    };
    let metadata = file.metadata().map_err(|_| "Could not inspect legacy settings for migration")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("Legacy settings cannot be redirected".into());
        }
    }
    if !metadata.is_file() || metadata.len() > MAX_LEGACY_BYTES {
        return Err("Legacy settings are not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_LEGACY_BYTES + 1).read_to_end(&mut bytes)
        .map_err(|_| "Could not read legacy settings for migration")?;
    if bytes.len() as u64 > MAX_LEGACY_BYTES {
        return Err("Legacy settings exceed the size limit".into());
    }
    // A second startup must see a complete file or no file. Never overwrite a
    // migrated copy that another process already published.
    crate::datastore_io::publish_new(new, &bytes).map(|_| ())
}

pub(crate) fn migrate_legacy_settings_if_required() -> Result<(), String> {
    let new_settings = user_settings_path()?;
    if !legacy_settings_exist(&new_settings)? {
        let old_settings = old_roaming_settings_path()?;
        copy_legacy_settings_if_absent(&old_settings, &new_settings)?;
    }
    Ok(())
}

pub fn migrate_user_data_layout() -> Result<(), String> {
    let marker = migration_marker_path()?;

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
    #[cfg(windows)]
    #[test]
    fn legacy_copy_child_process_fixture() {
        let Some(directory) = std::env::var_os("WINCMD_LEGACY_COPY_FIXTURE") else { return };
        let directory = std::path::PathBuf::from(directory);
        let id = std::env::var("WINCMD_LEGACY_COPY_FIXTURE_ID").unwrap();
        std::fs::write(directory.join(format!("ready-{id}")), b"ready").unwrap();
        let start = std::time::Instant::now();
        while !directory.join("go").exists() {
            assert!(start.elapsed() < std::time::Duration::from_secs(20));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let old = directory.join("legacy.json");
        let new = directory.join("settings.json");
        super::copy_legacy_settings_if_absent(&old, &new).unwrap();
        assert_eq!(std::fs::read(&new).unwrap(), std::fs::read(&old).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn independent_processes_publish_only_complete_legacy_settings() {
        let directory = tempfile::tempdir().unwrap();
        let payload = vec![b'x'; 128 * 1024];
        std::fs::write(directory.path().join("legacy.json"), &payload).unwrap();
        let module = module_path!().split_once("::").map_or(module_path!(), |(_, name)| name);
        let child_name = format!("{module}::legacy_copy_child_process_fixture");
        let mut children: Vec<_> = (0..2).map(|id| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &child_name, "--nocapture"])
                .env("WINCMD_LEGACY_COPY_FIXTURE", directory.path())
                .env("WINCMD_LEGACY_COPY_FIXTURE_ID", id.to_string())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn().unwrap()
        }).collect();
        let start = std::time::Instant::now();
        while !(directory.path().join("ready-0").exists() && directory.path().join("ready-1").exists()) {
            if start.elapsed() >= std::time::Duration::from_secs(15) { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let both_ready = directory.path().join("ready-0").exists() && directory.path().join("ready-1").exists();
        std::fs::write(directory.path().join("go"), b"go").unwrap();
        for child in children.drain(..) {
            let result = child.wait_with_output().unwrap();
            assert!(result.status.success(), "child failed: {}", String::from_utf8_lossy(&result.stderr));
        }
        assert!(both_ready, "both child processes must reach the launch barrier");
        assert_eq!(std::fs::read(directory.path().join("settings.json")).unwrap(), payload);
    }

    #[test]
    fn concurrent_legacy_copies_publish_only_complete_settings_and_keep_the_winner() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old-settings.json");
        let new = directory.path().join("settings.json");
        let bytes = vec![b'x'; 128 * 1024];
        std::fs::write(&old, &bytes).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8).map(|_| {
            let old = old.clone();
            let new = new.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                super::copy_legacy_settings_if_absent(&old, &new)
            })
        }).collect();
        for worker in workers { worker.join().unwrap().unwrap(); }
        assert_eq!(std::fs::read(&new).unwrap(), bytes);
        std::fs::write(&old, b"newer legacy copy").unwrap();
        super::copy_legacy_settings_if_absent(&old, &new).unwrap();
        assert_eq!(std::fs::read(&new).unwrap(), bytes);
    }

    #[test]
    fn diagnostic_locks_share_one_profile_but_never_another_profiles_log() {
        let first = super::diagnostic_log_lock_resource(std::path::Path::new(
            r"C:\Users\AccountOne\AppData\Local\WinCommander\logs",
        ));
        let equivalent = super::diagnostic_log_lock_resource(std::path::Path::new(
            "c:/users/accountone/appdata/local/wincommander/logs",
        ));
        let other = super::diagnostic_log_lock_resource(std::path::Path::new(
            r"C:\Users\AccountTwo\AppData\Local\WinCommander\logs",
        ));
        assert_eq!(first, equivalent);
        assert_ne!(first, other);
        assert!(super::is_valid_machine_state_resource(&first));
        assert!(!first.contains("accountone"));
    }

    #[cfg(windows)]
    #[test]
    fn diagnostic_lock_for_another_profile_progresses_while_first_profile_is_busy() {
        use std::sync::mpsc;
        use std::time::Duration;
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let first = super::diagnostic_log_lock_resource(&root.join("first"));
        let other = super::diagnostic_log_lock_resource(&root.join("other"));
        let held = super::acquire_machine_state_lock(&first).unwrap();
        let (done, finished) = mpsc::channel();
        let independent = std::thread::spawn(move || {
            let _lock = super::acquire_machine_state_lock(&other).unwrap();
            done.send(()).unwrap();
        });
        finished.recv_timeout(Duration::from_secs(1)).unwrap();
        independent.join().unwrap();
        let (done, finished) = mpsc::channel();
        let shared = std::thread::spawn(move || {
            let _lock = super::acquire_machine_state_lock(&first).unwrap();
            done.send(()).unwrap();
        });
        assert!(finished.recv_timeout(Duration::from_millis(20)).is_err());
        drop(held);
        finished.recv_timeout(Duration::from_secs(1)).unwrap();
        shared.join().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn machine_state_wait_is_bounded_and_waiters_resume_after_a_transition() {
        use std::sync::mpsc;
        use std::time::Duration;
        let name = format!("shield-test-{}", uuid::Uuid::new_v4().simple());
        let held = super::acquire_machine_state_lock(&name).unwrap();
        let blocked_name = name.clone();
        let start = std::time::Instant::now();
        let timed_out = std::thread::spawn(move || {
            super::acquire_machine_state_lock_with_timeout(&blocked_name, 30).is_err()
        }).join().unwrap();
        assert!(timed_out);
        assert!(start.elapsed() < Duration::from_secs(1));
        let (done, finished) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let _guard = super::acquire_machine_state_lock_with_timeout(&name, 1000).unwrap();
            done.send(()).unwrap();
        });
        assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
        drop(held);
        finished.recv_timeout(Duration::from_secs(1)).unwrap();
        waiter.join().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn fleet_lifecycle_lock_child_probe() {
        let Ok(name) = std::env::var("WINCMD_FLEET_LOCK_TEST_NAME") else { return; };
        let expected = std::env::var("WINCMD_FLEET_LOCK_TEST_EXPECT").unwrap();
        let result = super::acquire_machine_state_lock_with_timeout(&name, 100);
        assert_eq!(result.is_ok(), expected == "available");
    }

    #[cfg(windows)]
    #[test]
    fn fleet_lifecycle_lock_serializes_separate_processes_and_releases() {
        let name = format!("fleet-life-test-{}", uuid::Uuid::new_v4().simple());
        let run = |expect: &str| {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "paths::tests::fleet_lifecycle_lock_child_probe", "--nocapture"])
                .env("WINCMD_FLEET_LOCK_TEST_NAME", &name)
                .env("WINCMD_FLEET_LOCK_TEST_EXPECT", expect)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn().unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while child.try_wait().unwrap().is_none() {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("child lock probe timed out");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let result = child.wait_with_output().unwrap();
            assert!(result.status.success(), "child lock probe failed: {}", String::from_utf8_lossy(&result.stderr));
            assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"), "child lock probe did not run");
        };
        let held = super::acquire_machine_state_lock_with_timeout(&name, 100).unwrap();
        run("blocked");
        drop(held);
        run("available");
    }

    #[cfg(windows)]
    #[test]
    fn an_abandoned_owner_does_not_leave_a_stale_machine_lock() {
        let name = format!("shield-abandoned-{}", uuid::Uuid::new_v4().simple());
        let abandoned_name = name.clone();
        let leaked_handle = std::thread::spawn(move || {
            let guard = super::acquire_machine_state_lock(&abandoned_name).unwrap();
            let handle = guard.handle;
            std::mem::forget(guard);
            handle
        }).join().unwrap();
        let recovered = super::acquire_machine_state_lock_with_timeout(&name, 100).unwrap();
        // The old owner's handle is deliberately retained only to exercise
        // Windows' WAIT_ABANDONED path, then closed after ownership transfers.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(leaked_handle as _); }
        drop(recovered);
    }

    use std::path::PathBuf;

    use super::{
        is_current_user_install_executable, is_valid_machine_state_resource,
        is_valid_state_filename, should_harden_machine_data_dir, state_file_from_dir,
        MACHINE_DATA_ACL_GRANTS,
    };

    #[cfg(windows)]
    fn harmless_acl_test_child(script: &str) -> std::process::Child {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(0x08000000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn acl_child_wait_preserves_success_and_failure_status() {
        for (script, expected) in [("exit 0", true), ("exit 7", false)] {
            let mut child = harmless_acl_test_child(script);
            assert_eq!(
                super::wait_for_acl_child(&mut child, std::time::Duration::from_secs(5)),
                expected
            );
            assert!(child.try_wait().unwrap().is_some());
        }
    }

    #[cfg(windows)]
    #[test]
    fn acl_child_timeout_terminates_only_the_owned_child_without_blocking() {
        let mut child = harmless_acl_test_child("Start-Sleep -Seconds 30");
        let started = std::time::Instant::now();
        assert!(!super::wait_for_acl_child(
            &mut child,
            std::time::Duration::from_millis(100)
        ));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        // TerminateProcess is asynchronous. Poll only this disposable child's
        // handle to prove termination without an unbounded test cleanup wait.
        let terminated = std::time::Instant::now();
        while child.try_wait().unwrap().is_none() {
            assert!(terminated.elapsed() < std::time::Duration::from_secs(2));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn user_directory_acl_is_applied_once_per_directory_identity() {
        let mut cache = super::DirectoryAclCache::default();
        let path = std::path::Path::new("user-data");
        let mut attempts = 0;
        for identity in [Some((1, 10)), Some((1, 10)), Some((1, 11))] {
            cache.ensure(path, identity, || {
                attempts += 1;
                true
            });
        }
        assert_eq!(attempts, 2, "recreated directories must be hardened again");
    }

    #[test]
    fn user_directory_acl_retries_failures_and_unknown_identities() {
        let mut cache = super::DirectoryAclCache::default();
        let path = std::path::Path::new("user-data");
        let mut attempts = 0;
        for (identity, success) in [
            (Some((1, 10)), false),
            (Some((1, 10)), true),
            (None, true),
            (None, true),
            (Some((1, 10)), true),
        ] {
            cache.ensure(path, identity, || {
                attempts += 1;
                success
            });
        }
        assert_eq!(attempts, 5);
    }

    #[test]
    fn user_directory_acl_cache_keeps_paths_separate() {
        let mut cache = super::DirectoryAclCache::default();
        let mut attempts = 0;
        for path in ["user-a", "user-b", "user-a"] {
            cache.ensure(std::path::Path::new(path), Some((1, 10)), || {
                attempts += 1;
                true
            });
        }
        assert_eq!(attempts, 2);
    }

    #[cfg(windows)]
    #[test]
    fn user_directory_identity_changes_when_path_is_recreated() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("user-data");
        std::fs::create_dir(&path).unwrap();
        let original = super::directory_identity(&path).unwrap();
        std::fs::rename(&path, temp.path().join("old-user-data")).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_ne!(super::directory_identity(&path).unwrap(), original);
    }

    #[test]
    fn machine_data_acl_keeps_users_read_only() {
        assert_eq!(MACHINE_DATA_ACL_GRANTS[0], "*S-1-5-18:(OI)(CI)F");
        assert_eq!(MACHINE_DATA_ACL_GRANTS[1], "*S-1-5-32-544:(OI)(CI)F");
        assert_eq!(MACHINE_DATA_ACL_GRANTS[2], "*S-1-5-32-545:(OI)(CI)RX");
    }

    #[test]
    fn existing_machine_data_directory_is_not_acl_rewritten_by_standard_user_launch() {
        assert!(!should_harden_machine_data_dir(false));
        assert!(should_harden_machine_data_dir(true));
    }

    #[test]
    fn only_the_expected_current_user_binary_uses_local_datastore() {
        let local = PathBuf::from(r"C:\Users\Test\AppData\Local");
        assert!(is_current_user_install_executable(
            &local.join("WinCommander").join("wincommander-free.exe"),
            &local,
        ));
        assert!(is_current_user_install_executable(
            &PathBuf::from(r"c:\users\test\appdata\local\wincommander\WINCOMMANDER-FREE.EXE"),
            &local,
        ));
    }

    #[test]
    fn portable_or_machine_binaries_cannot_redirect_the_datastore() {
        let local = PathBuf::from(r"C:\Users\Test\AppData\Local");
        assert!(!is_current_user_install_executable(
            &PathBuf::from(r"C:\Program Files\WinCommander\wincommander-free.exe"),
            &local,
        ));
        assert!(!is_current_user_install_executable(
            &local
                .join("WinCommander")
                .join("versions")
                .join("wincommander-free.exe"),
            &local,
        ));
        assert!(!is_current_user_install_executable(
            &local.join("WinCommander").join("other.exe"),
            &local,
        ));
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
