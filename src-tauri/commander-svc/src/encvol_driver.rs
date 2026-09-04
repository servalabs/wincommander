// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fixed-path lifecycle guard for the WinCommander encryption driver.
//!
//! This is deliberately not a pipe verb.  It is invoked only from the already
//! authenticated vault-mount path, accepts no caller input, and refuses to
//! start a driver whose payload or containing engine tree is not locked down.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

const SERVICE_NAME: &str = "WinCommanderEncVol";
const COMPATIBLE_VERA_CRYPT_SERVICE_NAME: &str = "VeraCrypt";
const DRIVER_PATH: &str = r"C:\ProgramData\WinCommander\bin\engine\EncVolKm.sys";
// SCM stores `binPath=` verbatim into ImagePath, and a *kernel* service's
// ImagePath is resolved through the NT object namespace, not the Win32 path
// parser. It must therefore be the unquoted `\??\` form: surrounding quotes
// are part of the name, so the object manager rejects it and StartService
// fails with ERROR_INVALID_NAME (123) on every attempt. Only user-mode
// service ImagePaths (which are command lines) may be quoted.
const NT_DRIVER_PATH: &str = r"\??\C:\ProgramData\WinCommander\bin\engine\EncVolKm.sys";
const DRIVER_SHA256: &str = "1F0C6DB3559D1356C38A1486A967CD90DB5E6202E433FEA1DFE510DDB884FFB6";
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const ERROR_SERVICE_EXISTS: i32 = 1073;
const ERROR_SERVICE_ALREADY_RUNNING: i32 = 1056;

#[derive(Clone, PartialEq, Eq)]
struct DriverIdentity {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

static VALIDATED_DRIVER: OnceLock<Mutex<Option<DriverIdentity>>> = OnceLock::new();
static READY_DRIVER: OnceLock<Mutex<Option<DriverIdentity>>> = OnceLock::new();

#[derive(Debug)]
pub(crate) enum EnsureDriverError {
    PayloadValidation,
    ServiceInspection,
    ServiceOwnership,
    ServiceCreate,
    ServiceConfigure,
    ServiceStart,
}

impl EnsureDriverError {
    pub(crate) fn public_message(&self) -> &'static str {
        match self {
            Self::PayloadValidation => "encryption driver payload validation failed",
            Self::ServiceInspection => "encryption driver service could not be inspected",
            Self::ServiceOwnership => "encryption driver service is not owned by the fixed payload",
            Self::ServiceCreate => "encryption driver service could not be created",
            Self::ServiceConfigure => "encryption driver service could not be configured",
            Self::ServiceStart => "encryption driver service could not be started",
        }
    }
}

/// A cheap existence probe for the service's post-install repair loop.  This
/// is not a trust decision; `ensure_for_vault_mount` always performs the full
/// fixed-path validation before any SCM action.
pub(crate) fn fixed_payload_present() -> bool {
    std::path::Path::new(DRIVER_PATH).is_file()
}

/// Ensure the fixed, system-start kernel driver is available before brokered
/// encryption work.  Validation precedes every SCM change; the client cannot
/// influence the service name, driver path, signer policy, or ACL policy.
pub(crate) fn ensure_for_vault_mount() -> Result<(), EnsureDriverError> {
    let identity = driver_identity().map_err(|_| EnsureDriverError::PayloadValidation)?;
    if cached_identity_matches(&READY_DRIVER, &identity) {
        return Ok(());
    }
    validate_payload_cached(&identity).map_err(|_| EnsureDriverError::PayloadValidation)?;

    // VeraCrypt exposes one machine-wide `\\.\VeraCrypt` control device.
    // Its official driver and the signed WinCommander payload intentionally use
    // the same protocol and device name. Starting our dedicated service while
    // VeraCrypt already owns that device makes the second driver's DriverEntry
    // fail (commonly surfaced by SCM as ERROR_INVALID_PARAMETER). The engine
    // already supports this coexistence; the service gate must not reject it
    // first. This is a fixed, system-owned service/device probe -- no caller
    // input selects an alternate driver or path.
    if compatible_veracrypt_driver_is_ready() {
        cache_identity(&READY_DRIVER, identity);
        return Ok(());
    }

    let query =
        run_sc(&["query", SERVICE_NAME]).map_err(|_| EnsureDriverError::ServiceInspection)?;
    let created = match query.status.code() {
        Some(0) => {
            let config =
                run_sc(&["qc", SERVICE_NAME]).map_err(|_| EnsureDriverError::ServiceInspection)?;
            if !config.status.success() {
                return Err(EnsureDriverError::ServiceInspection);
            }
            // A development service can run under the interactive user while
            // the installed kernel driver is already running under SCM. Do
            // not demand SERVICE_CHANGE_CONFIG merely to reconfirm that known
            // good state: normal users are correctly denied that right.
            if service_config_is_fixed(&config) && service_is_running(&query) {
                cache_identity(&READY_DRIVER, identity);
                return Ok(());
            }
            // Older builds registered the same fixed service name with a
            // build-tree path.  The fixed payload has already passed hash,
            // signer, ownership, and ACL validation above, so repair that
            // stale SCM record below instead of making every mount fail.
            // Nothing from the caller determines the replacement path.
            false
        }
        Some(ERROR_SERVICE_DOES_NOT_EXIST) => {
            let create = run_sc(&[
                "create",
                SERVICE_NAME,
                "type=",
                "kernel",
                "start=",
                "system",
                "binPath=",
                NT_DRIVER_PATH,
            ])
            .map_err(|_| EnsureDriverError::ServiceCreate)?;
            if !create.status.success() && create.status.code() != Some(ERROR_SERVICE_EXISTS) {
                return Err(EnsureDriverError::ServiceCreate);
            }
            true
        }
        _ => return Err(EnsureDriverError::ServiceInspection),
    };

    let configure = run_sc(&[
        "config",
        SERVICE_NAME,
        "type=",
        "kernel",
        "start=",
        "system",
        "binPath=",
        NT_DRIVER_PATH,
    ])
    .map_err(|_| EnsureDriverError::ServiceConfigure)?;
    if !configure.status.success() {
        rollback_new_service(created);
        return Err(EnsureDriverError::ServiceConfigure);
    }

    // `sc config` reports command acceptance, not the final registry value.
    // Read it back before starting anything so a blocked or rewritten service
    // record cannot make the mount path trust a different driver image.
    let repaired =
        run_sc(&["qc", SERVICE_NAME]).map_err(|_| EnsureDriverError::ServiceInspection)?;
    if !repaired.status.success() || !service_config_is_fixed(&repaired) {
        rollback_new_service(created);
        return Err(EnsureDriverError::ServiceOwnership);
    }

    let start = run_sc(&["start", SERVICE_NAME]).map_err(|_| EnsureDriverError::ServiceStart)?;
    if !start.status.success() && start.status.code() != Some(ERROR_SERVICE_ALREADY_RUNNING) {
        rollback_new_service(created);
        return Err(EnsureDriverError::ServiceStart);
    }
    cache_identity(&READY_DRIVER, identity);
    Ok(())
}

/// Validation is expensive (hash, signature, and ACL inspection) but the
/// driver is immutable for a normal boot. Revalidate if its file identity
/// changes; otherwise reuse only a successful in-process verification.
fn driver_identity() -> std::io::Result<DriverIdentity> {
    let metadata = std::fs::metadata(DRIVER_PATH)?;
    Ok(DriverIdentity {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn cached_identity_matches(
    cache: &OnceLock<Mutex<Option<DriverIdentity>>>,
    identity: &DriverIdentity,
) -> bool {
    cache
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map(|cached| cached.as_ref() == Some(identity))
        .unwrap_or(false)
}

fn cache_identity(cache: &OnceLock<Mutex<Option<DriverIdentity>>>, identity: DriverIdentity) {
    if let Ok(mut cached) = cache.get_or_init(|| Mutex::new(None)).lock() {
        *cached = Some(identity);
    }
}

fn validate_payload_cached(identity: &DriverIdentity) -> std::io::Result<()> {
    if cached_identity_matches(&VALIDATED_DRIVER, identity) {
        return Ok(());
    }
    validate_payload()?;
    cache_identity(&VALIDATED_DRIVER, identity.clone());
    Ok(())
}

fn rollback_new_service(created: bool) {
    if created {
        let _ = run_sc(&["delete", SERVICE_NAME]);
    }
}

fn run_sc(args: &[&str]) -> std::io::Result<Output> {
    let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let sc = PathBuf::from(system_root).join("System32").join("sc.exe");
    Command::new(sc).args(args).output()
}

fn compatible_veracrypt_driver_is_ready() -> bool {
    let Ok(query) = run_sc(&["query", COMPATIBLE_VERA_CRYPT_SERVICE_NAME]) else {
        return false;
    };
    compatible_veracrypt_service_is_running(&query) && veracrypt_control_device_is_open()
}

fn compatible_veracrypt_service_is_running(query: &Output) -> bool {
    query.status.success() && service_is_running(query)
}

#[cfg(windows)]
fn veracrypt_control_device_is_open() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let device: Vec<u16> = r"\\.\VeraCrypt"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `device` is NUL-terminated; zero desired access is the same
    // non-mutating probe used by the native engine, and all pointer arguments
    // are null where the Win32 API requires optional values.
    let handle = unsafe {
        CreateFileW(
            device.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return false;
    }
    // SAFETY: `handle` was returned by CreateFileW and is neither null nor
    // INVALID_HANDLE_VALUE.
    unsafe { CloseHandle(handle) };
    true
}

#[cfg(not(windows))]
fn veracrypt_control_device_is_open() -> bool {
    false
}

/// `sc qc` is used only to check the fixed, literal service record.  It is
/// never used to obtain a path that is later executed.
fn service_config_is_fixed(output: &Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("BINARY_PATH_NAME")
            && fixed_image_path(trimmed.split_once(':').map_or("", |(_, value)| value))
    })
}

fn service_is_running(output: &Output) -> bool {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .any(|line| line.starts_with("STATE") && line.contains("4") && line.contains("RUNNING"))
}

fn fixed_image_path(value: &str) -> bool {
    let value = value.trim().trim_matches('"');
    let value = value.strip_prefix(r"\??\").unwrap_or(value);
    value.eq_ignore_ascii_case(DRIVER_PATH)
}

/// Uses a fixed, non-interactive system PowerShell invocation to check the
/// Authenticode chain, pinned SHA-256, reparse-free parents, and the driver's
/// own ownership/DACL.  The engine directories can legitimately inherit a
/// create-child ACE from ProgramData; the pinned file itself must still not be
/// writable by ordinary users. No command text or path is derived from the
/// pipe request. ACL failures are intentionally indistinguishable to the
/// caller, avoiding a filesystem-discovery oracle.
fn validate_payload() -> std::io::Result<()> {
    const SCRIPT: &str = r#"
$ErrorActionPreference='Stop'
$driver='C:\ProgramData\WinCommander\bin\engine\EncVolKm.sys'
$hash='{driver_hash}'
$trusted=@('S-1-5-18','S-1-5-32-544')
$write=[System.Security.AccessControl.FileSystemRights]::WriteData -bor [System.Security.AccessControl.FileSystemRights]::AppendData -bor [System.Security.AccessControl.FileSystemRights]::WriteExtendedAttributes -bor [System.Security.AccessControl.FileSystemRights]::WriteAttributes -bor [System.Security.AccessControl.FileSystemRights]::Delete -bor [System.Security.AccessControl.FileSystemRights]::ChangePermissions -bor [System.Security.AccessControl.FileSystemRights]::TakeOwnership
function Get-Sid([object]$identity) { ([System.Security.Principal.NTAccount]$identity).Translate([System.Security.Principal.SecurityIdentifier]).Value }
function Test-WcParent([string]$path) {
  $item=Get-Item -LiteralPath $path -Force
  if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'reparse' }
}
function Test-WcDriver([string]$path) {
  Test-WcParent $path
  $acl=Get-Acl -LiteralPath $path
  if ($trusted -notcontains (Get-Sid $acl.Owner)) { throw 'owner' }
  foreach($ace in $acl.Access) {
    if ($ace.AccessControlType -eq [Security.AccessControl.AccessControlType]::Allow -and (($ace.FileSystemRights -band $write) -ne 0) -and ($trusted -notcontains (Get-Sid $ace.IdentityReference))) { throw 'acl' }
  }
}
foreach($path in @('C:\ProgramData\WinCommander','C:\ProgramData\WinCommander\bin','C:\ProgramData\WinCommander\bin\engine')) { Test-WcParent $path }
Test-WcDriver $driver
$actual=(Get-FileHash -Algorithm SHA256 -LiteralPath $driver).Hash
if (-not $actual.Equals($hash,[StringComparison]::OrdinalIgnoreCase)) { throw 'hash' }
$signature=Get-AuthenticodeSignature -LiteralPath $driver
if ($signature.Status -ne 'Valid') { throw 'signature' }
"#;
    let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let powershell = PathBuf::from(system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let script = SCRIPT.replace("{driver_hash}", DRIVER_SHA256);
    let result = Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()?;
    if result.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("driver validation failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_fixed_driver_image_path() {
        assert!(fixed_image_path(NT_DRIVER_PATH));
        // SCM keeps reporting the pre-fix quoted form on machines registered by
        // an older build, so ownership must still recognise it.
        assert!(fixed_image_path(
            r#""C:\ProgramData\WinCommander\bin\engine\EncVolKm.sys""#
        ));
        assert!(!fixed_image_path(r"C:\Temp\EncVolKm.sys"));
        assert!(!fixed_image_path(
            r"C:\ProgramData\WinCommander\bin\engine\other.sys"
        ));
    }

    #[test]
    fn recognises_only_a_running_driver_service() {
        let running = Command::new("cmd")
            .args(["/c", "echo STATE : 4 RUNNING"])
            .output()
            .unwrap();
        let stopped = Command::new("cmd")
            .args(["/c", "echo STATE : 1 STOPPED"])
            .output()
            .unwrap();
        assert!(service_is_running(&running));
        assert!(!service_is_running(&stopped));
    }

    /// Regression: a quoted `binPath=` lands in ImagePath verbatim, and the
    /// object manager cannot parse a kernel image name wrapped in quotes —
    /// every StartService then fails with ERROR_INVALID_NAME (123).
    #[test]
    fn kernel_image_path_is_registered_unquoted_in_nt_form() {
        assert!(NT_DRIVER_PATH.starts_with(r"\??\"));
        assert!(!NT_DRIVER_PATH.contains('"'));
        assert_eq!(NT_DRIVER_PATH.trim_start_matches(r"\??\"), DRIVER_PATH);
    }

    #[test]
    fn validation_has_no_caller_controlled_path_or_unsigned_bypass() {
        let source = include_str!("encvol_driver.rs");
        assert!(source.contains("Get-AuthenticodeSignature -LiteralPath $driver"));
        assert!(source.contains("Test-WcDriver $driver"));
        assert!(source.contains(DRIVER_SHA256));
        assert!(source.contains("$signature.Status -ne 'Valid'"));
        assert!(!source.contains(&["fn existing_control", "_device_is_available"].concat()));
    }

    #[test]
    fn compatible_veracrypt_requires_a_running_service_and_open_control_device() {
        let running = Command::new("cmd")
            .args(["/c", "echo STATE : 4 RUNNING"])
            .output()
            .unwrap();
        let stopped = Command::new("cmd")
            .args(["/c", "echo STATE : 1 STOPPED"])
            .output()
            .unwrap();
        assert!(compatible_veracrypt_service_is_running(&running));
        assert!(!compatible_veracrypt_service_is_running(&stopped));
        assert_eq!(COMPATIBLE_VERA_CRYPT_SERVICE_NAME, "VeraCrypt");
    }

    #[test]
    fn readiness_cache_requires_the_exact_driver_identity() {
        let cache = OnceLock::new();
        let first = DriverIdentity {
            len: 10,
            modified: Some(std::time::UNIX_EPOCH),
        };
        let changed = DriverIdentity {
            len: 11,
            modified: Some(std::time::UNIX_EPOCH),
        };
        assert!(!cached_identity_matches(&cache, &first));
        cache_identity(&cache, first.clone());
        assert!(cached_identity_matches(&cache, &first));
        assert!(!cached_identity_matches(&cache, &changed));
    }
}
