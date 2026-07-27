# Kill app processes from a previous dev session, then free port 1420.
# Run via: powershell -File tools/kill-dev.ps1  (called by the kill:dev npm script)

Get-Process wincommander-free, wincommander-pro, WinCommander -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue

# Free port 1420 (Vite dev server) if held by a stale bun/node process.
# vite.config.ts uses strictPort: true, so an occupied port aborts startup.
#
# Uses netstat (native binary) instead of Get-NetTCPConnection: the latter's
# NetTCPIP module has a slow first-load in Windows PowerShell (measured
# ~740ms just for that one cmdlet, vs netstat's ~90-150ms end-to-end) — this
# single cmdlet was the dominant cost in the whole script. The regex anchors
# ":1420" on a following run of whitespace so it can't false-match a port
# that merely contains 1420 as a substring (e.g. :11420 or :14200).
$netstatLine = netstat -ano | Select-String -Pattern ':1420\s+\S+\s+LISTENING\s+(\d+)\s*$' | Select-Object -First 1
if ($netstatLine) {
    $ownerId = $netstatLine.Matches[0].Groups[1].Value
    Stop-Process -Id $ownerId -Force -ErrorAction SilentlyContinue
}

exit 0
