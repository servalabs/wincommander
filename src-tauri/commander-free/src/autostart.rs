// Machine-scoped logon autostart via a Scheduled Task — replaces the per-user
// HKCU `Run` key the tauri-plugin-autostart wrote.
//
// Why a Scheduled Task, not a Run key:
//   1. A Run value only autostarts for the ONE account that wrote it; this app
//      keeps its state machine-wide, so autostart should cover every account.
//   2. A Run value goes stale if a portable exe is moved; we re-point the task
//      at the CURRENT exe on every launch (idempotent), so it self-heals.
//   3. The app is `requireAdministrator` — a Run key can't auto-elevate, so it
//      UAC-prompts (or fails) at every logon. A logon task with RunLevel
//      Highest elevates without a prompt for admins.
//
// The task uses a BUILTIN\Users (S-1-5-32-545) group principal + an at-logon
// trigger, so it fires for any user's logon inside their interactive session
// (the GUI shows) and elevates for admins.

const COVERED_TASK_NAME: &str = "System Update Service";

fn task_name(covered: bool) -> String {
    if covered {
        COVERED_TASK_NAME.to_string()
    } else {
        format!("{} Autostart", crate::paths::app_display_name())
    }
}

fn covered_identity_active() -> bool {
    crate::paths::hide_flag_path()
        .map(|path| path.exists())
        .unwrap_or(false)
        || crate::startup_auth::startup_pin_is_configured_sync()
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Create or refresh the machine-wide logon autostart task, pointing it at the
/// CURRENT exe (a moved portable exe re-binds on the next launch). Idempotent
/// via `-Force`. Also clears the legacy HKCU `Run` value so we don't double-launch.
#[cfg(windows)]
#[tauri::command]
pub fn ensure_autostart_task() -> Result<(), String> {
    ensure_autostart_task_named(covered_identity_active())
}

#[cfg(windows)]
fn ensure_autostart_task_named(covered: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let exe = std::env::current_exe()
        .map_err(|e| format!("current_exe: {e}"))?
        .to_string_lossy()
        .to_string();
    // PowerShell single-quoted literals: double any embedded single quote.
    let exe_ps = exe.replace('\'', "''");
    let name_ps = task_name(covered).replace('\'', "''");
    let stale_name_ps = task_name(!covered).replace('\'', "''");
    let run_value = crate::paths::app_display_name().replace('\'', "''");

    let script = format!(
        "$ErrorActionPreference='Stop'
$a = New-ScheduledTaskAction -Execute '{exe}' -Argument '--minimized'
$t = New-ScheduledTaskTrigger -AtLogOn
$p = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-545' -RunLevel Highest
$s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName '{name}' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null
Unregister-ScheduledTask -TaskName '{stale_name}' -Confirm:$false -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName 'Sys Health Checker' -Confirm:$false -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName 'WinCommander Input Service' -Confirm:$false -ErrorAction SilentlyContinue
Remove-Item -LiteralPath \"$env:ProgramData\\WinCommander\\reopen.cfg\" -Force -ErrorAction SilentlyContinue
Remove-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name '{run}' -ErrorAction SilentlyContinue",
        exe = exe_ps,
        name = name_ps,
        stale_name = stale_name_ps,
        run = run_value,
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
        crate::log::log_message(
            "info",
            &format!("autostart task registered/refreshed: {}", name_ps),
        );
        Ok(())
    } else {
        let err = format!(
            "autostart task registration failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        crate::log::log_message("warn", &err);
        Err(err)
    }
}

#[cfg(windows)]
#[tauri::command]
pub fn update_autostart_task_identity(covered: bool) -> Result<(), String> {
    ensure_autostart_task_named(covered)
}

#[cfg(not(windows))]
#[tauri::command]
pub fn update_autostart_task_identity(_covered: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn ensure_autostart_task() -> Result<(), String> {
    Ok(())
}

/// Remove the autostart task (for a future "disable autostart" control; the
/// Lockdown cascade already sweeps `*WinCommander*` tasks on self-destruct).
#[cfg(windows)]
#[tauri::command]
pub fn remove_autostart_task() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let normal_name_ps = task_name(false).replace('\'', "''");
    let covered_name_ps = task_name(true).replace('\'', "''");
    let script = format!(
        "Unregister-ScheduledTask -TaskName '{normal}' -Confirm:$false -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName '{covered}' -Confirm:$false -ErrorAction SilentlyContinue",
        normal = normal_name_ps,
        covered = covered_name_ps,
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
pub fn remove_autostart_task() -> Result<(), String> {
    Ok(())
}
