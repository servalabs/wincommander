# Start or stop a shared Vite server and a debug WinCommander instance in each
# interactive Windows session.  This is development tooling only; Tauri's
# release bundle never invokes or packages these scripts.

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet("Start", "Stop", "Status")]
    [string]$Action
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$stateRoot = Join-Path $env:ProgramData "WinCommander\\dev-multiuser"
$statePath = Join-Path $stateRoot "state.json"
$futureTaskName = "WinCommander Dev MultiUser Logon"

function Assert-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Run this command from an elevated Administrator PowerShell window."
    }
}

function Get-State {
    if (-not (Test-Path -LiteralPath $statePath)) { return $null }
    try { return Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json } catch { return $null }
}

function Set-State($State) {
    $temporaryPath = "$statePath.next"
    $State | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $temporaryPath -Encoding UTF8
    Move-Item -LiteralPath $temporaryPath -Destination $statePath -Force
}

function Initialize-StateDirectory {
    New-Item -ItemType Directory -Path $stateRoot -Force | Out-Null
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($entry in @(
        @{ Identity = "SYSTEM"; Rights = "FullControl" },
        @{ Identity = "BUILTIN\\Administrators"; Rights = "FullControl" },
        @{ Identity = "BUILTIN\\Users"; Rights = "ReadAndExecute" }
    )) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            $entry.Identity, $entry.Rights, "ContainerInherit,ObjectInherit", "None", "Allow"
        )
        $acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $stateRoot -AclObject $acl
}

function Get-InteractiveSessions {
    if ($null -eq ("WinCommander.Dev.Wts" -as [type])) {
        Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
namespace WinCommander.Dev {
  public static class Wts {
    [StructLayout(LayoutKind.Sequential)] public struct SessionInfo { public Int32 SessionId; [MarshalAs(UnmanagedType.LPWStr)] public string StationName; public Int32 State; }
    [DllImport("wtsapi32.dll", SetLastError=true)] static extern bool WTSEnumerateSessions(IntPtr server, int reserved, int version, out IntPtr sessions, out int count);
    [DllImport("wtsapi32.dll")] static extern void WTSFreeMemory(IntPtr memory);
    [DllImport("wtsapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool WTSQuerySessionInformation(IntPtr server, int sessionId, int infoClass, out IntPtr buffer, out int bytes);
    static string Value(int sessionId, int infoClass) { IntPtr buffer; int bytes; if (!WTSQuerySessionInformation(IntPtr.Zero, sessionId, infoClass, out buffer, out bytes) || buffer == IntPtr.Zero) return null; try { return Marshal.PtrToStringUni(buffer); } finally { WTSFreeMemory(buffer); } }
    public static string[] ActiveUsers() { IntPtr raw; int count; if (!WTSEnumerateSessions(IntPtr.Zero, 0, 1, out raw, out count)) return new string[0]; var users = new List<string>(); try { int size = Marshal.SizeOf(typeof(SessionInfo)); for (int i = 0; i < count; i++) { var item = (SessionInfo)Marshal.PtrToStructure(IntPtr.Add(raw, i * size), typeof(SessionInfo)); if (item.State != 0) continue; var user = Value(item.SessionId, 5); var domain = Value(item.SessionId, 7); if (!String.IsNullOrWhiteSpace(user)) users.Add(String.IsNullOrWhiteSpace(domain) ? user : domain + "\\" + user); } } finally { WTSFreeMemory(raw); } return users.ToArray(); }
  }
}
'@
    }
    [WinCommander.Dev.Wts]::ActiveUsers() | Sort-Object -Unique
}

function New-AgentAction {
    $agent = Join-Path $repoRoot "tools\\dev-multiuser-agent.ps1"
    $arguments = "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$agent`" -StatePath `"$statePath`""
    New-ScheduledTaskAction -Execute (Join-Path $PSHOME "powershell.exe") -Argument $arguments
}

function Start-AgentForUser([string]$User, [string]$RunId) {
    $taskName = "WinCommander Dev MultiUser $RunId $($User -replace '[^A-Za-z0-9_-]', '_')"
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
    # Highest means "highest token already available to this account". It lets
    # an administrator stop and restore the installed elevated app, while a
    # standard account remains standard and is never elevated by this tool.
    $principal = New-ScheduledTaskPrincipal -UserId $User -LogonType Interactive -RunLevel Highest
    Register-ScheduledTask -TaskName $taskName -Action (New-AgentAction) -Principal $principal -Settings $settings -Force | Out-Null
    Start-ScheduledTask -TaskName $taskName
    return $taskName
}

function Register-FutureLogonAgent {
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
    $principal = New-ScheduledTaskPrincipal -GroupId "S-1-5-32-545" -RunLevel Highest
    Register-ScheduledTask -TaskName $futureTaskName -Action (New-AgentAction) -Trigger (New-ScheduledTaskTrigger -AtLogOn) -Principal $principal -Settings $settings -Force | Out-Null
}

function Stop-ServerTree([int]$ProcessId) {
    if ($ProcessId -gt 0 -and (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) {
        & taskkill.exe /PID $ProcessId /T /F | Out-Null
    }
}

switch ($Action) {
    "Status" {
        $state = Get-State
        if ($null -eq $state -or -not $state.active) { Write-Output "Multi-user development is stopped."; break }
        Write-Output "Multi-user development is running - Vite PID $($state.serverPid), debug executable $($state.devExecutable)."
        break
    }
    "Start" {
        Assert-Administrator
        $existing = Get-State
        if ($null -ne $existing -and $existing.active) { throw "Multi-user development is already running. Use -Action Stop first." }
        Initialize-StateDirectory
        & (Join-Path $PSScriptRoot "ensure-dev-environment.ps1")
        $bun = (Get-Command bun -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Path)
        if (-not $bun) { throw "bun.exe was not found after the development environment bootstrap." }

        $server = Start-Process -FilePath $bun -ArgumentList @("run", "tools/dev-server.ts", "--free", "--multi-user") -WorkingDirectory $repoRoot -PassThru
        $deadline = (Get-Date).AddMinutes(3)
        do {
            Start-Sleep -Milliseconds 500
            try { $ready = (Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:1420" -TimeoutSec 2).StatusCode -eq 200 } catch { $ready = $false }
        } while (-not $ready -and (Get-Date) -lt $deadline -and -not $server.HasExited)
        if (-not $ready) { Stop-ServerTree $server.Id; throw "Vite did not become ready on http://127.0.0.1:1420." }

        Push-Location (Join-Path $repoRoot "src-tauri")
        try { & cargo build -p commander-free } finally { Pop-Location }
        if ($LASTEXITCODE -ne 0) { Stop-ServerTree $server.Id; throw "Could not build the debug WinCommander executable." }
        $debugExecutable = Join-Path $repoRoot "src-tauri\\target\\debug\\wincommander-free.exe"
        if (-not (Test-Path -LiteralPath $debugExecutable -PathType Leaf)) { Stop-ServerTree $server.Id; throw "Debug executable was not produced: $debugExecutable" }

        $runId = [guid]::NewGuid().ToString("N")
        $state = [ordered]@{ active = $true; runId = $runId; serverPid = $server.Id; devExecutable = $debugExecutable; startedUtc = (Get-Date).ToUniversalTime().ToString("o"); taskNames = @() }
        Set-State $state
        Register-FutureLogonAgent
        $taskNames = @()
        foreach ($user in @(Get-InteractiveSessions)) {
            try { $taskNames += Start-AgentForUser -User $user -RunId $runId } catch { Write-Warning ("Could not start the dev agent for {0}: {1}" -f $user, $_.Exception.Message) }
        }
        $state.taskNames = $taskNames
        Set-State $state
        Write-Output ("Multi-user development is running. Vite is shared on port 1420; {0} active session(s) were switched to the debug app." -f $taskNames.Count)
        break
    }
    "Stop" {
        Assert-Administrator
        $state = Get-State
        if ($null -eq $state -or -not $state.active) { Write-Output "Multi-user development is already stopped."; break }
        $state.active = $false
        Set-State $state
        Start-Sleep -Seconds 5
        foreach ($taskName in @($state.taskNames) + $futureTaskName) { Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue }
        Stop-ServerTree ([int]$state.serverPid)
        Remove-Item -LiteralPath $statePath -Force -ErrorAction SilentlyContinue
        Write-Output "Multi-user development stopped. Per-user agents closed the debug app and restored their previous app where Windows permits it."
        break
    }
}
