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

use std::path::PathBuf;

const SERVICE_NAME: &str = "WinCommanderSvc";
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
    let payload = resolve_service_payload()?;
    let live_path = service_exe_path()?;

    // Best-effort stop so the executable can be replaced; a missing or
    // already-stopped service is not an error here — `create`/`config`
    // below report the real failure if something is actually wrong.
    let _ = run_sc(&["stop", SERVICE_NAME]).await;

    if live_path != payload {
        std::fs::copy(&payload, &live_path)
            .map_err(|e| format!("could not install service executable: {e}"))?;
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
