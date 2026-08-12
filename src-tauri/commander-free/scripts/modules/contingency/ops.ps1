# ============================================================================
# CONTINGENCY MODULE
# Handles RDP session control, contingency signals, USB key gating,
# and the full contingency sequence (dismount → disconnect → shutdown).
# ============================================================================

# ── Shared helpers ───────────────────────────────────────────────────────────

# Extract a fixed-width column substring safely (no IndexOutOfRange).
$script:SafeSlice = {
    param([string]$str, [int]$start, [int]$end)
    if ($start -lt 0 -or $start -ge $str.Length) { return "" }
    $len = [Math]::Min($end - $start, $str.Length - $start)
    if ($len -le 0) { return "" }
    return $str.Substring($start, $len).Trim()
}

# Return unique remote-server IPs that mstsc.exe is currently connected to.
function Get-MstscRemoteHosts {
    # Pure .NET — avoids Get-NetTCPConnection which calls WMI and can hang 5-10 s.
    # GetActiveTcpConnections() is synchronous and returns in <50 ms.
    $hostsList = [System.Collections.Generic.List[string]]::new()
    try {
        $conns = [System.Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpConnections()
        foreach ($c in $conns) {
            if ($c.RemoteEndPoint.Port -eq 3389) {
                $addr = $c.RemoteEndPoint.Address.ToString()
                if ($addr -and $addr -notin @('0.0.0.0', '::') -and -not $hostsList.Contains($addr)) {
                    $hostsList.Add($addr)
                }
            }
        }
    } catch {}
    return $hostsList
}

function Invoke-SchtasksWithTimeout {
    param(
        [Parameter(Mandatory)][string[]]$Arguments,
        [int]$TimeoutMs = 3000
    )
    $argLine = ($Arguments | ForEach-Object {
        if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ }
    }) -join ' '

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName               = 'schtasks.exe'
    $psi.Arguments              = $argLine
    $psi.UseShellExecute        = $false
    $psi.CreateNoWindow         = $true   # no console at all — no credential-dialog hang
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError  = $true

    $p = [System.Diagnostics.Process]::Start($psi)
    # Async reads prevent pipe-buffer fill from blocking WaitForExit
    $outTask = $p.StandardOutput.ReadToEndAsync()
    $errTask = $p.StandardError.ReadToEndAsync()

    if (-not $p.WaitForExit($TimeoutMs)) {
        # Kill schtasks.exe and every child it spawned (NTLM auth helpers etc.)
        try { & taskkill.exe /F /T /PID $p.Id 2>&1 | Out-Null } catch {}
        try { $p.Kill() } catch {}
        return @{ exitCode = 124; timedOut = $true; output = 'schtasks timed out' }
    }
    # 2 s cap — NTLM auth helpers can inherit the pipe handles and keep them open
    # after schtasks exits, causing an unbounded WaitAll hang.
    [System.Threading.Tasks.Task]::WaitAll([System.Threading.Tasks.Task[]]@($outTask, $errTask), 2000) | Out-Null
    $out = ($(if ($outTask.IsCompleted) { $outTask.Result } else { '' }) + ' ' + $(if ($errTask.IsCompleted) { $errTask.Result } else { '' })).Trim()
    return @{ exitCode = $p.ExitCode; timedOut = $false; output = $out }
}

function Dismount-RDPServerVaults {
    param([string[]]$Hosts = @())   # optional pre-captured hosts; falls back to live detection
    try {
        $hosts = if ($Hosts.Count -gt 0) { $Hosts } else { [string[]]@(Get-MstscRemoteHosts) }
        $results = @()
        if ($hosts.Count -eq 0) {
            return @{ success = $false; dismounted = 0; results = @(); error = 'No active RDP server host found' }
        }

        # Compact cmd action using 8.3 short paths — avoids spaces (no quoting needed)
        # and stays well under schtasks' 261-char /TR limit (~131 chars total).
        # Progra~1 = "Program Files" (64-bit), Progra~2 = "Program Files (x86)" (32-bit).
        # || means: if the 64-bit path doesn't exist/fails, try the x86 path.
        $action = 'cmd /c C:\Progra~1\VeraCrypt\VeraCrypt.exe /d /quit /silent /force || C:\Progra~2\VeraCrypt\VeraCrypt.exe /d /quit /silent /force'

        foreach ($hostName in $hosts) {
            $taskName = "\WinCommander-RdpIdleDismount-$([guid]::NewGuid().ToString('N'))"
            $startAt = (Get-Date).AddMinutes(2).ToString('HH:mm')
            $created = $false
            $ran = $false
            $deleted = $false
            $errorText = $null

            try {
                # 10 s create — first TCP/RPC connection over VPN can take 5-8 s.
                # NOTE: do NOT pass /Z (delete-after-run). schtasks rejects /Z unless the
                # task carries an EndBoundary, which '/SC ONCE /ST <time>' alone never
                # produces — it fails XML validation ("the task XML is missing a required
                # element or attribute ... EndBoundary") and /Create exits 1, so the whole
                # dismount silently no-opped. We delete the task ourselves below instead.
                $createOut = Invoke-SchtasksWithTimeout -Arguments @('/Create', '/S', $hostName, '/TN', $taskName, '/SC', 'ONCE', '/ST', $startAt, '/TR', $action, '/RU', 'SYSTEM', '/RL', 'HIGHEST', '/F') -TimeoutMs 10000
                if ($createOut.exitCode -ne 0) { throw "create failed (exit=$($createOut.exitCode) timed=$($createOut.timedOut)): $($createOut.output)" }
                $created = $true

                # 5 s run — signals Task Scheduler to start the task immediately
                $runOut = Invoke-SchtasksWithTimeout -Arguments @('/Run', '/S', $hostName, '/TN', $taskName) -TimeoutMs 5000
                if ($runOut.exitCode -ne 0) { throw "run failed (exit=$($runOut.exitCode) timed=$($runOut.timedOut)): $($runOut.output)" }
                $ran = $true
                # Give Task Scheduler a moment to actually spawn VeraCrypt before we delete
                # the task definition in the finally block. The launched process keeps
                # running after the task is removed, so the dismount still completes.
                Start-Sleep -Milliseconds 1500
            } catch {
                $errorText = $_.Exception.Message
            } finally {
                # Always remove the task we created (ran or not). Without /Z nothing
                # auto-cleans it, so leaving it would (a) accumulate orphaned ONCE tasks on
                # the remote and (b) let the scheduled /ST trigger fire a second dismount
                # minutes later. The /Run above has already launched VeraCrypt by now.
                if ($created) {
                    try {
                        $deleteOut = Invoke-SchtasksWithTimeout -Arguments @('/Delete', '/S', $hostName, '/TN', $taskName, '/F') -TimeoutMs 5000
                        $deleted = ($deleteOut.exitCode -eq 0)
                        if (-not $deleted -and -not $errorText) { $errorText = "delete failed (exit=$($deleteOut.exitCode)): $($deleteOut.output)" }
                    } catch {
                        $deleted = $false
                        if (-not $errorText) { $errorText = $_.Exception.Message }
                    }
                }
            }

            $results += @{
                host = $hostName
                taskName = $taskName
                created = $created
                ran = $ran
                deleted = $deleted
                success = ($created -and $ran)
                error = $errorText
            }
        }

        $ok = @($results | Where-Object { $_.success }).Count
        return @{ success = ($ok -gt 0); dismounted = $ok; results = $results; hosts = $hosts }
    }
    catch {
        return @{ success = $false; dismounted = 0; results = @(); error = $_.Exception.Message }
    }
}

# ── RDP Session Management ────────────────────────────────────────────────────

# Parse quser idle-time string (e.g. "none", "0:05", "1:23", "2+03:00") → seconds
function ConvertFrom-QuserIdleTime {
    param([string]$IdleStr)
    if ([string]::IsNullOrWhiteSpace($IdleStr) -or $IdleStr -eq 'none' -or $IdleStr -eq '.') { return 0 }
    if ($IdleStr -match '^(\d+)\+(\d+):(\d+)$') {
        return [int]$Matches[1] * 86400 + [int]$Matches[2] * 3600 + [int]$Matches[3] * 60
    }
    if ($IdleStr -match '^(\d+):(\d+)$') { return [int]$Matches[1] * 3600 + [int]$Matches[2] * 60 }
    # quser reports a bare idle value in MINUTES (e.g. "5" = 5 minutes), not seconds.
    if ($IdleStr -match '^(\d+)$')       { return [int]$Matches[1] * 60 }
    return 0
}

function Get-ActiveRDPSessions {
    param([string]$ServerIP = "")
    try {
        $qsArgs    = if ($ServerIP) { @("/server:$ServerIP") } else { @() }
        $qsRaw     = & query session @qsArgs 2>&1
        $rdpIds    = @{}
        foreach ($line in ($qsRaw -split "`n" | Select-Object -Skip 1)) {
            if ($line -match 'rdpwd|rdp-tcp#' -and $line -match '\s+(\d+)\s+') {
                $rdpIds[[int]$Matches[1]] = $true
            }
        }
        if ($rdpIds.Count -eq 0) { return @{ sessions = @(); error = $null } }

        $quserArgs  = if ($ServerIP) { @("/server:$ServerIP") } else { @() }
        $quserRaw   = & quser @quserArgs 2>&1
        $headerLine = $quserRaw | Where-Object { $_ -match 'USERNAME' } | Select-Object -First 1
        if (-not $headerLine) { return @{ sessions = @(); error = $null } }

        $cUser = $headerLine.IndexOf('USERNAME')
        $cSess = $headerLine.IndexOf('SESSIONNAME')
        $cId   = $headerLine.IndexOf('ID')
        $cSt   = $headerLine.IndexOf('STATE')
        $cIdle = $headerLine.IndexOf('IDLE TIME')
        $cLogon= $headerLine.IndexOf('LOGON TIME')

        $sessions = @()
        foreach ($line in ($quserRaw -split "`n")) {
            if ($line -match 'USERNAME' -or -not $line.Trim()) { continue }
            $raw = $line -replace '^>', ' '
            if ($raw.Length -le $cId) { continue }
            $id = 0
            if (-not [int]::TryParse((& $script:SafeSlice $raw $cId ($cSt - 1)), [ref]$id)) { continue }
            if (-not $rdpIds.ContainsKey($id)) { continue }
            $idleStr = & $script:SafeSlice $raw $cIdle ($cLogon - 1)
            $sessions += @{
                username    = & $script:SafeSlice $raw $cUser ($cSess - 1)
                session     = & $script:SafeSlice $raw $cSess ($cId   - 1)
                id          = $id
                state       = & $script:SafeSlice $raw $cSt   ($cIdle - 1)
                idleTime    = $idleStr
                idleSeconds = ConvertFrom-QuserIdleTime $idleStr
                logonTime   = if ($cLogon -lt $raw.Length) { $raw.Substring($cLogon).Trim() } else { "" }
            }
        }
        return @{ sessions = $sessions; error = $null }
    }
    catch { return @{ sessions = @(); error = $_.Exception.Message } }
}

function Disconnect-RDPSession {
    param([Parameter(Mandatory)][int]$SessionId, [string]$ServerIP = "")
    try {
        $logoffArgs = @($SessionId); if ($ServerIP) { $logoffArgs += "/server:$ServerIP" }
        & logoff @logoffArgs 2>&1 | Out-Null
        return @{ success = $true }
    }
    catch { return @{ success = $false; error = $_.Exception.Message } }
}

function Disconnect-AllRDPSessions {
    param([string]$ServerIP = "")
    try {
        $result = Get-ActiveRDPSessions -ServerIP $ServerIP
        $n = 0
        foreach ($s in $result.sessions) {
            $logoffArgs = @($s.id); if ($ServerIP) { $logoffArgs += "/server:$ServerIP" }
            & logoff @logoffArgs 2>&1 | Out-Null
            $n++
        }
        return @{ success = $true; disconnected = $n }
    }
    catch { return @{ success = $false; error = $_.Exception.Message } }
}

# ── Idle detection + local kill ───────────────────────────────────────────────

function Hide-RDPClientWindow {
    try {
        if (-not ([System.Management.Automation.PSTypeName]'WC.WinShow').Type) {
            Add-Type @"
using System; using System.Runtime.InteropServices;
namespace WC { public class WinShow {
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int n);
    public const int SW_MINIMIZE = 6;
} }
"@
        }
        $n = 0
        foreach ($p in @(Get-Process -Name mstsc -EA SilentlyContinue)) {
            if ($p.MainWindowHandle -ne [IntPtr]::Zero) {
                [WC.WinShow]::ShowWindow($p.MainWindowHandle, [WC.WinShow]::SW_MINIMIZE) | Out-Null
                $n++
            }
        }
        return @{ success = $true; minimized = $n }
    }
    catch { return @{ success = $false; error = $_.Exception.Message } }
}

function Show-RDPIdleWarning {
    param([string]$IdleTime = "unknown", [int]$SecondsLeft = 60)
    return @{
        success     = $true
        method      = "tauri-custom-notification"
        delegated   = $true
        idleTime    = $IdleTime
        secondsLeft = $SecondsLeft
    }
}

# Watch system-wide idle time via GetLastInputInfo. If idle >= TimeoutSeconds
# AND mstsc.exe is running, kill the local client.
# Returns: { success, idleSeconds, killed, rdpOpen, remoteHosts }
function Watch-RDPClientIdle {
    param([int]$TimeoutSeconds = 120)
    try {
        if (-not ([System.Management.Automation.PSTypeName]'WC.IdleDetect').Type) {
            Add-Type @"
using System; using System.Runtime.InteropServices;
namespace WC { public class IdleDetect {
    [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LASTINPUTINFO p);
    [StructLayout(LayoutKind.Sequential)] struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
    public static uint GetIdleSeconds() {
        var i = new LASTINPUTINFO(); i.cbSize = (uint)System.Runtime.InteropServices.Marshal.SizeOf(i);
        GetLastInputInfo(ref i); return ((uint)Environment.TickCount - i.dwTime) / 1000;
    }
} }
"@
        }
        $procs = Get-Process -Name mstsc -EA SilentlyContinue
        if (-not $procs) {
            return @{ success = $true; idleSeconds = 0; killed = 0; rdpOpen = $false; remoteHosts = [System.Collections.Generic.List[string]]::new(); processCount = 0; sampledAt = (Get-Date).ToString('o') }
        }
        $remoteHosts  = [string[]]@(Get-MstscRemoteHosts)
        $idleSeconds  = [WC.IdleDetect]::GetIdleSeconds()
        $processCount = @($procs).Count
        $killed = 0
        if ($TimeoutSeconds -le 0 -or $idleSeconds -ge $TimeoutSeconds) {
            $beforeIds = @($procs | ForEach-Object { [int]$_.Id })
            foreach ($p in $procs) {
                try { Stop-Process -Id $p.Id -Force -EA Stop } catch {}
            }
            $remaining = @(Get-Process -Name mstsc -EA SilentlyContinue)
            if ($remaining.Count -gt 0) {
                try { & taskkill.exe /F /IM mstsc.exe /T 2>&1 | Out-Null } catch {}
                $remaining = @(Get-Process -Name mstsc -EA SilentlyContinue)
            }
            $remainingIds = @($remaining | ForEach-Object { [int]$_.Id })
            $killed = @($beforeIds | Where-Object { $remainingIds -notcontains $_ }).Count
        }
        $afterCount = @(Get-Process -Name mstsc -EA SilentlyContinue).Count
        return @{ success = $true; idleSeconds = [int]$idleSeconds; killed = $killed; rdpOpen = ($afterCount -gt 0); remoteHosts = $remoteHosts; processCount = $processCount; remainingProcessCount = $afterCount; sampledAt = (Get-Date).ToString('o') }
    }
    catch { return @{ success = $false; error = $_.Exception.Message; idleSeconds = 0; killed = 0; rdpOpen = $false; remoteHosts = [System.Collections.Generic.List[string]]::new(); processCount = 0; sampledAt = (Get-Date).ToString('o') } }
}

function Disconnect-RDPClientIdle {
    param(
        [bool]$DismountServerVaults = $false,
        [long]$_ts = 0  # ignored; unique per call so TS de-dup never reuses a prior promise
    )

    # 1. Capture remote hosts (safe — never blocks the kill below).
    $capturedHosts = @()
    try { $capturedHosts = [string[]]@(Get-MstscRemoteHosts) } catch {}

    # 2. Kill mstsc unconditionally — both methods fire regardless of each other's outcome.
    $beforeCount = @(Get-Process -Name mstsc -EA SilentlyContinue).Count
    try { Stop-Process -Name mstsc -Force -EA SilentlyContinue } catch {}
    try { & taskkill.exe /F /IM mstsc.exe 2>&1 | Out-Null } catch {}
    Start-Sleep -Milliseconds 500
    $afterCount = @(Get-Process -Name mstsc -EA SilentlyContinue).Count
    $killed = [Math]::Max(0, $beforeCount - $afterCount)

    # 3. Dismount on the remote PC — SYNCHRONOUS so it actually completes before this
    #    PowerShell process exits. The old Start-Job was a background job that got killed
    #    on process exit (the schtasks create/run never finished), so the dismount
    #    silently no-opped. Dismount-RDPServerVaults uses the hosts captured in step 1
    #    (while the RDP connection was still alive) and wraps every schtasks call in a
    #    bounded timeout, so it can't hang.
    $remoteDismount = $null
    if ($DismountServerVaults -and $capturedHosts.Count -gt 0) {
        try {
            $remoteDismount = Dismount-RDPServerVaults -Hosts $capturedHosts
        } catch {
            $remoteDismount = @{ success = $false; error = $_.Exception.Message }
        }
    }

    return @{
        success        = ($afterCount -eq 0)
        remoteDismount = $remoteDismount
        kill           = @{ success = $true; killed = $killed; rdpOpen = ($afterCount -gt 0); remainingProcessCount = $afterCount }
    }
}

# ── RDP Access Control (Windows Firewall) ────────────────────────────────────

function Lock-RDPAccess {
    try {
        Remove-NetFirewallRule -DisplayName "WC-RDP-Lock" -EA SilentlyContinue
        New-NetFirewallRule -DisplayName "WC-RDP-Lock" -Direction Inbound `
            -LocalPort 3389 -Protocol TCP -Action Block -Profile Any -Enabled True | Out-Null
        return @{ success = $true; locked = $true }
    }
    catch { return @{ success = $false; error = $_.Exception.Message } }
}

function Unlock-RDPAccess {
    try {
        Remove-NetFirewallRule -DisplayName "WC-RDP-Lock" -EA SilentlyContinue
        return @{ success = $true; locked = $false }
    }
    catch { return @{ success = $false; error = $_.Exception.Message } }
}

function Dismount-LocalVaults {
    try {
        $vc64 = 'C:\Program Files\VeraCrypt\VeraCrypt.exe'
        $vc32 = 'C:\Program Files (x86)\VeraCrypt\VeraCrypt.exe'
        $vc   = if (Test-Path $vc64) { $vc64 } elseif (Test-Path $vc32) { $vc32 } else { $null }
        if (-not $vc) { return @{ success = $false; error = 'VeraCrypt not found' } }
        & $vc /d /quit /silent /force 2>&1 | Out-Null
        return @{ success = $true }
    }
    catch { return @{ success = $false; error = $_.Exception.Message } }
}

# ── Paid contingency surface lives in commander-pro ──────────────────────────
#
# Send-ContingencySignal, New-ContingencyNotification, Start-ContingencySequence,
# Get-USBKeyStatus, Register-USBKeySerial were moved to commander-pro/src/handlers.rs
# as part of A-2 (Pro-extraction). Strings-grep CI gate (A-5) verifies these
# names no longer appear in the Free binary's .enc bundle after build.
