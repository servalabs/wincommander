// src-tauri/src/service_repair.rs (commander-free crate)
// ═══════════════════════════════════════════════════════════════════════
// Recovers the WinCommanderSvc Windows service when it has been left
// unregistered or stopped — e.g. after a manual swap of its executable that
// never completed — by re-running the exact `sc.exe` sequence the NSIS
// installer performs post-install (see `nsis/hooks.nsh`'s
// `NSIS_HOOK_POSTINSTALL`). Exposed as a Tauri command so the Privacy
// panel's RDP lock can offer a "Repair service" action instead of surfacing
// a raw pipe-connect failure (`svc_client::call`'s "service connect
// failed: ...").
//
// The service payload is resolved the same way `pro_install::pro_dev_path`
// resolves the Pro sidecar: a debug build looks for the workspace sibling
// next to the running exe (so `tauri dev` can exercise this same repair
// path against a locally built `commander-svc`); a release build looks for
// the bundled payload under `resources/`, matching the installer's
// `WC_BUNDLED_SERVICE`.

use sha2::{Digest, Sha256};
use std::path::PathBuf;

const SERVICE_NAME: &str = wincmd_shared::svc::SVC_WINDOWS_SERVICE_NAME;
const ERROR_SERVICE_EXISTS: i32 = 1073;
const ERROR_SERVICE_ALREADY_RUNNING: i32 = 1056;
const ERROR_ACCESS_DENIED: i32 = 5;

fn resolve_service_payload() -> Result<PathBuf, String> {
    let current =
        std::env::current_exe().map_err(|e| format!("could not read current exe path: {e}"))?;
    let dir = current
        .parent()
        .ok_or_else(|| "current exe has no parent directory".to_string())?;
    let candidate = if cfg!(debug_assertions) {
        dir.join("wincommander-svc.exe")
    } else {
        dir.join("resources").join("wincommander-svc.exe")
    };
    if !candidate.exists() {
        return Err(format!(
            "service payload not found at {} — build commander-svc first",
            candidate.display()
        ));
    }
    Ok(candidate)
}

/// Where the live, SCM-registered service executable belongs — always a
/// direct sibling of the running app, matching `WC_SERVICE_EXE` in hooks.nsh.
fn service_exe_path() -> Result<PathBuf, String> {
    let current =
        std::env::current_exe().map_err(|e| format!("could not read current exe path: {e}"))?;
    let dir = current
        .parent()
        .ok_or_else(|| "current exe has no parent directory".to_string())?;
    Ok(dir.join("wincommander-svc.exe"))
}

fn path_is_within(candidate: &std::path::Path, expected_parent: &std::path::Path) -> bool {
    candidate
        .components()
        .zip(expected_parent.components())
        .all(|(left, right)| {
            left.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
        })
        && candidate.components().count() > expected_parent.components().count()
}

fn paths_equal_ignore_case(left: &std::path::Path, right: &std::path::Path) -> bool {
    left.components().count() == right.components().count()
        && left
            .components()
            .zip(right.components())
            .all(|(left, right)| {
                left.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
            })
}

fn file_sha256(path: &std::path::Path) -> Result<[u8; 32], String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)
        .map_err(|_| "service executable could not be opened for verification".to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "service executable could not be read for verification".to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().into())
}

#[cfg(windows)]
fn require_elevated_administrator() -> Result<(), String> {
    // This checks the process token, not a renderer checkbox or Windows group
    // name. A split-token administrator must explicitly elevate first.
    if unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() } == 0 {
        return Err(
            "Administrator privileges required to repair the WinCommander service.".to_string(),
        );
    }
    Ok(())
}

#[cfg(windows)]
fn validate_release_paths(
    payload: &std::path::Path,
    live_path: &std::path::Path,
) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    let program_files = std::env::var_os("ProgramW6432")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .map(PathBuf::from)
        .ok_or_else(|| "Windows Program Files location is unavailable".to_string())?;
    let install_dir = std::fs::canonicalize(program_files.join("WinCommander"))
        .map_err(|_| "protected WinCommander installation could not be resolved".to_string())?;
    let resolved_payload = std::fs::canonicalize(payload)
        .map_err(|_| "service payload could not be resolved".to_string())?;
    let live_parent = live_path
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .ok_or_else(|| "service installation directory could not be resolved".to_string())?;
    if !path_is_within(&resolved_payload, &install_dir)
        || !paths_equal_ignore_case(&live_parent, &install_dir)
    {
        return Err(
            "service repair is available only from the protected WinCommander installation"
                .to_string(),
        );
    }
    let payload_metadata = std::fs::symlink_metadata(payload)
        .map_err(|_| "service payload could not be inspected".to_string())?;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    use std::os::windows::fs::MetadataExt;
    if payload_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("service payload cannot be a reparse point".to_string());
    }
    if let Ok(live_metadata) = std::fs::symlink_metadata(live_path) {
        if live_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("installed service executable cannot be a reparse point".to_string());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub async fn repair() -> Result<String, String> {
    Err("WinCommander service repair is only available on Windows".to_string())
}

#[cfg(windows)]
struct ScResult {
    code: i32,
    output: String,
}

#[cfg(windows)]
async fn run_sc(args: &[&str]) -> Result<ScResult, String> {
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = tokio::process::Command::new("sc.exe");
    cmd.args(args).creation_flags(CREATE_NO_WINDOW);
    let out = cmd
        .output()
        .await
        .map_err(|e| format!("could not run sc.exe: {e}"))?;
    let code = out.status.code().unwrap_or(-1);
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(ScResult {
        code,
        output: text.trim().to_string(),
    })
}

#[cfg(windows)]
fn access_denied_message(result: &ScResult) -> Option<String> {
    (result.code == ERROR_ACCESS_DENIED).then(|| {
        "Administrator privileges required to repair the WinCommander service.".to_string()
    })
}

/// Re-installs and starts `WinCommanderSvc`. Safe to call whether the
/// service is missing, stopped, or already running — every step tolerates
/// the "already in the desired state" SCM error the installer also treats
/// as success.
#[cfg(windows)]
pub async fn repair() -> Result<String, String> {
    require_elevated_administrator()?;
    let payload = resolve_service_payload()?;
    let live_path = service_exe_path()?;
    validate_release_paths(&payload, &live_path)?;

    // Best-effort stop so the executable can be replaced; a missing or
    // already-stopped service is not an error here — `create`/`config`
    // below report the real failure if something is actually wrong.
    let _ = run_sc(&["stop", SERVICE_NAME]).await;

    if live_path != payload {
        std::fs::copy(&payload, &live_path)
            .map_err(|e| format!("could not install service executable: {e}"))?;
    }
    if file_sha256(&payload)? != file_sha256(&live_path)? {
        return Err("installed service executable did not match the bundled payload".to_string());
    }

    let bin_path_value = format!("\"{}\"", live_path.display());

    let create = run_sc(&[
        "create",
        SERVICE_NAME,
        "binPath=",
        &bin_path_value,
        "start=",
        "auto",
        "obj=",
        "LocalSystem",
    ])
    .await?;
    if create.code != 0 && create.code != ERROR_SERVICE_EXISTS {
        if let Some(msg) = access_denied_message(&create) {
            return Err(msg);
        }
        return Err(format!(
            "service could not be created (code {}): {}",
            create.code, create.output
        ));
    }

    let config = run_sc(&[
        "config",
        SERVICE_NAME,
        "binPath=",
        &bin_path_value,
        "start=",
        "auto",
        "obj=",
        "LocalSystem",
    ])
    .await?;
    if config.code != 0 {
        if let Some(msg) = access_denied_message(&config) {
            return Err(msg);
        }
        return Err(format!(
            "service could not be configured (code {}): {}",
            config.code, config.output
        ));
    }

    // Best-effort — a missing recovery policy degrades resilience, not
    // correctness, so it does not fail the repair.
    let _ = run_sc(&[
        "failure",
        SERVICE_NAME,
        "reset=",
        "86400",
        "actions=",
        "restart/5000/restart/5000/none/0",
    ])
    .await;

    let start = run_sc(&["start", SERVICE_NAME]).await?;
    if start.code != 0 && start.code != ERROR_SERVICE_ALREADY_RUNNING {
        if let Some(msg) = access_denied_message(&start) {
            return Err(msg);
        }
        return Err(format!(
            "service could not be started (code {}): {}",
            start.code, start.output
        ));
    }

    Ok("WinCommander service repaired and running.".to_string())
}

#[tauri::command]
pub async fn repair_commander_service() -> Result<String, String> {
    repair().await
}

#[cfg(test)]
mod tests {
    use super::{path_is_within, paths_equal_ignore_case};
    use std::path::Path;

    #[test]
    fn repair_path_boundary_rejects_lookalike_and_parent_locations() {
        let installed = Path::new(r"C:\Program Files\WinCommander");
        assert!(path_is_within(
            Path::new(r"C:\Program Files\WinCommander\resources\wincommander-svc.exe"),
            installed,
        ));
        assert!(!path_is_within(
            Path::new(r"C:\Program Files\WinCommander-Evil\wincommander-svc.exe"),
            installed,
        ));
        assert!(!path_is_within(
            Path::new(r"C:\Program Files\WinCommander"),
            installed
        ));
        assert!(paths_equal_ignore_case(
            Path::new(r"c:\PROGRAM FILES\wincommander"),
            installed,
        ));
    }
}
