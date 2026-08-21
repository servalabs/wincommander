# ============================================================================
# NETWORK - PORTS MODULE
# Live view of active TCP/UDP connections joined with the owning process.
# ============================================================================

# State name → terse label used by the UI.
# Keep this in sync with the React component's filter buttons.
$Script:WC_TCP_STATE_LABELS = @{
    'Listen'      = 'LISTEN'
    'Established' = 'ESTABLISHED'
    'CloseWait'   = 'CLOSE_WAIT'
    'TimeWait'    = 'TIME_WAIT'
    'SynSent'     = 'SYN_SENT'
    'SynReceived' = 'SYN_RCVD'
    'FinWait1'    = 'FIN_WAIT1'
    'FinWait2'    = 'FIN_WAIT2'
    'Closing'     = 'CLOSING'
    'LastAck'     = 'LAST_ACK'
    'Bound'       = 'BOUND'
}

function Get-NetworkPorts {
    [CmdletBinding()]
    param(
        # Cap returned rows to avoid pathological cases (>5000 sockets on busy
        # boxes). UI shows "+N more" if truncation kicks in.
        [int]$MaxRows = 1500
    )

    Assert-IsAdmin
    $sw = [System.Diagnostics.Stopwatch]::StartNew()

    # Build a PID → process-info map ONCE; Get-Process is much faster than
    # repeatedly calling it per-row, and we already pay for it during normal
    # process intelligence runs.
    $procIndex = @{}
    try {
        Get-Process -ErrorAction SilentlyContinue | ForEach-Object {
            $procIndex[$_.Id] = @{
                name = $_.ProcessName
                path = $null
            }
            try {
                $p = $_.Path
                if ($p) { $procIndex[$_.Id].path = $p }
            } catch { }
        }
    } catch { }

    $rows = New-Object System.Collections.Generic.List[hashtable]
    $tcpCount = 0
    $udpCount = 0

    # ── TCP ────────────────────────────────────────────────────────────────
    try {
        $tcpConns = Get-NetTCPConnection -ErrorAction SilentlyContinue
        foreach ($c in $tcpConns) {
            if ($rows.Count -ge $MaxRows) { break }
            $tcpCount++
            $procEntry = $null
            if ($c.OwningProcess -and $procIndex.ContainsKey([int]$c.OwningProcess)) {
                $procEntry = $procIndex[[int]$c.OwningProcess]
            }
            $stateLabel = if ($Script:WC_TCP_STATE_LABELS.ContainsKey([string]$c.State)) {
                $Script:WC_TCP_STATE_LABELS[[string]$c.State]
            } else {
                [string]$c.State
            }
            $rows.Add(@{
                proto       = 'TCP'
                localAddr   = [string]$c.LocalAddress
                localPort   = [int]$c.LocalPort
                remoteAddr  = [string]$c.RemoteAddress
                remotePort  = [int]$c.RemotePort
                state       = $stateLabel
                pid         = if ($c.OwningProcess) { [int]$c.OwningProcess } else { 0 }
                processName = if ($procEntry) { $procEntry.name } else { $null }
                processPath = if ($procEntry) { $procEntry.path } else { $null }
            })
        }
    } catch {
        # Fall through — UDP block runs independently.
    }

    # ── UDP ────────────────────────────────────────────────────────────────
    try {
        $udpEndpoints = Get-NetUDPEndpoint -ErrorAction SilentlyContinue
        foreach ($u in $udpEndpoints) {
            if ($rows.Count -ge $MaxRows) { break }
            $udpCount++
            $procEntry = $null
            if ($u.OwningProcess -and $procIndex.ContainsKey([int]$u.OwningProcess)) {
                $procEntry = $procIndex[[int]$u.OwningProcess]
            }
            $rows.Add(@{
                proto       = 'UDP'
                localAddr   = [string]$u.LocalAddress
                localPort   = [int]$u.LocalPort
                remoteAddr  = ''
                remotePort  = 0
                state       = 'LISTEN'
                pid         = if ($u.OwningProcess) { [int]$u.OwningProcess } else { 0 }
                processName = if ($procEntry) { $procEntry.name } else { $null }
                processPath = if ($procEntry) { $procEntry.path } else { $null }
            })
        }
    } catch { }

    $sw.Stop()

    return @{
        status     = 'ok'
        durationMs = [int]$sw.ElapsedMilliseconds
        truncated  = ($rows.Count -ge $MaxRows)
        totals     = @{
            tcp   = $tcpCount
            udp   = $udpCount
            shown = $rows.Count
        }
        rows       = @($rows)
    }
}
