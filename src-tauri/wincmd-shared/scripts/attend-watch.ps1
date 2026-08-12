# ============================================================================
# attend-watch.ps1 — WinCommander unattended-session guard (runs as SYSTEM)
# ============================================================================
# Enforces "dismount local VeraCrypt vaults once no one is attending" from a
# SYSTEM scheduled task, so the policy holds even when the WinCommander GUI is
# closed (the in-app loops live in the WebView and die with the process).
#
# Determinism model: this does NOT react to a single logoff event. It evaluates
# one aggregate boolean every run — "is anyone attending?" — and dismounts on
# the confirmed falling edge. A session counts as ATTENDING only when it is
# Active AND idle < threshold. Disconnected, idle-past-threshold and signed-off
# all count as "gone".
#
#   attended = ∃ session : State=Active AND idleSeconds < IdleThresholdSeconds
#
# Triggers (registered by attend_watch.rs): a 1-minute repetition (catches the
# IDLE case, which raises no Windows event) plus logoff(23)/disconnect(24)
# event triggers (cut latency). Single-instance via a global mutex; the settle
# window kills the reconnect race; dismount is by device (/d, no letter) so it
# works from session 0 regardless of which session mounted the volume.
# ----------------------------------------------------------------------------
param(
    [int]$IdleThresholdSeconds = 900,   # Active session idle >= this counts as not-attending
    [int]$SettleSeconds        = 25,    # Must stay unattended this long before dismounting
    [switch]$DismountVaults,            # Dismount all VeraCrypt vaults when unattended
    [switch]$SignOffStale               # Also logoff disconnected / idle sessions
)
$ErrorActionPreference = 'SilentlyContinue'  # native tools (quser/logoff/mountvol/VeraCrypt) write to stderr & exit non-zero on the EMPTY case — that must never throw
$LogPath = Join-Path $env:ProgramData 'WinCommander\attend-watch.log'
function Log($m) { try { Add-Content -LiteralPath $LogPath -Value ('{0}  {1}' -f (Get-Date).ToString('yyyy-MM-dd HH:mm:ss'), $m) } catch {} }

# Single-instance guard — never let two evaluations race on the VeraCrypt IPC.
$mtx = New-Object System.Threading.Mutex($false, 'Global\WinCmdAttendWatch')
if (-not $mtx.WaitOne(0)) { return }
try {
    Add-Type -Namespace Wc -Name Dos -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll", CharSet=System.Runtime.InteropServices.CharSet.Auto)] public static extern uint QueryDosDevice(string d, System.Text.StringBuilder t, uint m);' -ErrorAction SilentlyContinue

    # VeraCrypt mounts are kernel devices (\Device\VeraCryptVolumeN), global and
    # SYSTEM-visible. Detect by NT device path, not by VeraCrypt.exe state.
    function Get-VcMounted {
        $out = @()
        foreach ($c in 65..90) {
            $dl = [char]$c; $sb = New-Object System.Text.StringBuilder 1024
            if ([Wc.Dos]::QueryDosDevice("$dl`:", $sb, 1024) -ne 0 -and $sb.ToString() -match '(?i)(Vera|True)CryptVolume') { $out += "$dl`:" }
        }
        , $out
    }

    # quser IDLE TIME column is in MINUTES for a bare number ("5" = 5 min),
    # "h:mm" for hours, "d+hh:mm" for days, "." / "none" for active-now.
    function ConvertFrom-QuserIdle([string]$s) {
        if ([string]::IsNullOrWhiteSpace($s) -or $s -eq '.' -or $s -eq 'none') { return 0 }
        if ($s -match '^(\d+)\+(\d+):(\d+)$') { return [int]$Matches[1] * 86400 + [int]$Matches[2] * 3600 + [int]$Matches[3] * 60 }
        if ($s -match '^(\d+):(\d+)$') { return [int]$Matches[1] * 3600 + [int]$Matches[2] * 60 }
        if ($s -match '^(\d+)$') { return [int]$Matches[1] * 60 }
        return 0
    }

    function Get-Sessions {
        # quser exits non-zero / prints "No User exists for *" to stderr when the box
        # is EMPTY — exactly when we must dismount. Route through cmd so neither the
        # stderr nor the exit code can surface as a PowerShell error; treat empty /
        # "No User exists" as ZERO sessions (-> unattended -> dismount), never an error.
        $raw = cmd /c "quser 2>nul"
        if (-not $raw -or (($raw -join "`n") -match 'No User exists')) { return @() }
        $hdr = $raw | Where-Object { $_ -match 'USERNAME' } | Select-Object -First 1
        if (-not $hdr) { return @() }
        $cU = $hdr.IndexOf('USERNAME'); $cI = $hdr.IndexOf('ID'); $cS = $hdr.IndexOf('STATE'); $cD = $hdr.IndexOf('IDLE TIME'); $cL = $hdr.IndexOf('LOGON TIME')
        $list = @()
        foreach ($line in $raw) {
            if ($line -match 'USERNAME' -or -not $line.Trim()) { continue }
            $r = $line -replace '^>', ' '
            if ($r.Length -le $cS) { continue }
            $idTxt = $r.Substring($cI, [Math]::Max(0, $cS - $cI)).Trim()
            $id = 0; if (-not [int]::TryParse($idTxt, [ref]$id)) { continue }
            $user = $r.Substring($cU, [Math]::Max(0, $cI - $cU - 1)).Trim()
            $state = $r.Substring($cS, [Math]::Max(0, $cD - $cS - 1)).Trim()
            $idle = if ($r.Length -gt $cD) { $r.Substring($cD, [Math]::Min([Math]::Max(0, $cL - $cD), $r.Length - $cD)).Trim() } else { '' }
            if (-not $user) { continue }
            $list += [pscustomobject]@{ User = $user; Id = $id; State = $state; IdleSec = (ConvertFrom-QuserIdle $idle) }
        }
        , $list
    }

    function Test-Attended($sessions) {
        @($sessions | Where-Object { $_.State -match '^Active' -and $_.IdleSec -lt $IdleThresholdSeconds }).Count -gt 0
    }

    $sessions = Get-Sessions
    if (Test-Attended $sessions) { Log ('attended; no action'); return }

    # Optional: sign off lingering disconnected / idle sessions so they release
    # any handles on the vault before we dismount.
    if ($SignOffStale) {
        foreach ($s in ($sessions | Where-Object { $_.State -match '^Disc' -or $_.IdleSec -ge $IdleThresholdSeconds })) {
            try { & logoff $s.Id 2>$null; Log ('signed off stale session {0} ({1}, {2})' -f $s.Id, $s.User, $s.State) } catch {}
        }
    }

    if (-not $DismountVaults) { Log 'unattended; dismount disabled'; return }

    # Confirm the unattended state holds across the settle window (kills the
    # "someone reconnects 8s later" race).
    Log ('unattended; settling {0}s' -f $SettleSeconds)
    Start-Sleep -Seconds $SettleSeconds
    if (Test-Attended (Get-Sessions)) { Log 'someone returned during settle; abort'; return }

    $mounted = Get-VcMounted
    if ($mounted.Count -eq 0) { Log 'unattended; no vaults mounted; done'; return }

    $vc = @("$env:ProgramFiles\VeraCrypt\VeraCrypt.exe", "${env:ProgramFiles(x86)}\VeraCrypt\VeraCrypt.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $vc) { Log 'VeraCrypt.exe not found'; return }

    Log ('DISMOUNT (unattended): {0}' -f ($mounted -join ','))
    & $vc /d /quit /silent /force 2>&1 | Out-Null   # /d with no letter = ALL, device-level, SYSTEM-proof
    for ($i = 0; $i -lt 25; $i++) { if ((Get-VcMounted).Count -eq 0) { break }; Start-Sleep -Milliseconds 400 }
    $left = Get-VcMounted
    if ($left.Count -eq 0) { Log 'dismount OK; all vaults gone' }
    else { foreach ($d in $left) { try { & mountvol "$d\" /D 2>$null } catch {} }; Log ('dismount incomplete; scrubbed stale letters {0}' -f ($left -join ',')) }
}
catch { Log ('ERROR: ' + $_.Exception.Message) }
finally { try { $mtx.ReleaseMutex() } catch {}; $mtx.Dispose() }
