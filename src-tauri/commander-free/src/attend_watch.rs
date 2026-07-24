// SYSTEM-context "unattended session guard" Scheduled Task.
//
// WinCommander's in-app RDP idle/dismount loops live in the WebView, so they
// only run while the GUI process is alive. This task runs the embedded
// `attend-watch.ps1` as SYSTEM on a 1-minute repeating trigger plus
// logoff(23)/disconnect(24) event triggers, so the "dismount local vaults once
// no one is attending" policy is enforced even when the app is closed.
//
// We verified empirically that SYSTEM (session 0) can both SEE and force-
// dismount the user-session VeraCrypt mounts (they land in the global object
// namespace and the VeraCrypt service runs as System), so the mount path needs
// no change — only this independent trigger. Gated on `incoming_dismount_on_empty`
// so it is a no-op for users who have not opted into the feature.
//
// Mirrors `autostart.rs`: a Rust fn emits a `Register-ScheduledTask` script and
// runs it via powershell.exe; idempotent via `-Force`; safe to call on every
// launch (it re-deploys the script + refreshes the configured threshold).

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
fn task_name() -> String {
    // Contains the app token so the Lockdown self-destruct sweep (`*WinCommander*`)
    // removes it along with the other app tasks.
    format!("{} Session Guard", crate::paths::app_display_name())
}

/// Where the embedded watcher is deployed (machine-wide, SYSTEM-readable, no
/// spaces in the path so it needs no quoting in the task argument string).
#[cfg(windows)]
fn script_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::paths::machine_data_dir()?.join("attend-watch.ps1"))
}

/// Create/refresh (or remove) the SYSTEM unattended-guard task to match
/// settings. Idempotent; safe to call on every launch.
#[cfg(windows)]
#[tauri::command]
pub fn ensure_attend_watch_task() -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    // If settings can't be read, do nothing rather than guess.
    let s = match crate::settings::read_settings() {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    let rdp = &s.ideal.tweaks.rdp;

    // Feature off → make sure no stale task lingers.
    if !rdp.incoming_dismount_on_empty.unwrap_or(false) {
        return remove_attend_watch_task();
    }

    let idle_secs = rdp
        .incoming_idle_timeout_seconds
        .or_else(|| rdp.incoming_idle_timeout_minutes.map(|m| m * 60))
        .unwrap_or(900)
        .clamp(10, 86_400);
    let sign_off = rdp.incoming_sign_off_on_disconnect.unwrap_or(false);

    // Deploy the embedded script to ProgramData (self-heals if moved/edited).
    let path = script_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create data dir: {e}"))?;
    }
    std::fs::write(&path, wincmd_shared::ATTEND_WATCH_PS_MODULE)
        .map_err(|e| format!("write attend-watch.ps1: {e}"))?;

    // Path has no spaces (ProgramData\WinCommander\…) so no inner quoting needed.
    let path_arg = path.to_string_lossy().to_string();
    let sign_flag = if sign_off { " -SignOffStale" } else { "" };
    let task_args = format!(
        "-NonInteractive -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File {path} -IdleThresholdSeconds {idle} -SettleSeconds 25 -DismountVaults{sign}",
        path = path_arg,
        idle = idle_secs,
        sign = sign_flag,
    );
    let name_ps = task_name().replace('\'', "''");

    // Two triggers, same action: a 1-minute repetition (the only thing that can
    // catch the IDLE transition, which raises no event) + logoff/disconnect
    // events (low latency). MultipleInstances IgnoreNew prevents overlap.
    let script = format!(
        "$ErrorActionPreference='Stop'
$a = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument '{args}'
$tPoll = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 1) -RepetitionDuration (New-TimeSpan -Days 3650)
$cls = Get-CimClass MSFT_TaskEventTrigger -Namespace Root/Microsoft/Windows/TaskScheduler
$tEvt = New-CimInstance -CimClass $cls -ClientOnly -Property @{{ Enabled = $true; Delay = 'PT3S'; Subscription = '<QueryList><Query Id=\"0\"><Select Path=\"Microsoft-Windows-TerminalServices-LocalSessionManager/Operational\">*[System[(EventID=23 or EventID=24)]]</Select></Query></QueryList>' }}
$p = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
$set = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 3) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName '{name}' -Action $a -Trigger $tPoll, $tEvt -Principal $p -Settings $set -Force | Out-Null",
        args = task_args,
        name = name_ps,
    );

    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("spawn powershell: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "attend-watch task registration failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Remove the SYSTEM unattended-guard task.
#[cfg(windows)]
#[tauri::command]
pub fn remove_attend_watch_task() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let name_ps = task_name().replace('\'', "''");
    let script = format!(
        "Unregister-ScheduledTask -TaskName '{name}' -Confirm:$false -ErrorAction SilentlyContinue",
        name = name_ps,
    );
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn ensure_attend_watch_task() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn remove_attend_watch_task() -> Result<(), String> {
    Ok(())
}
