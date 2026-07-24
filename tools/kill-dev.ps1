# Kill app processes from a previous dev session, then free port 1420.
# Run via: powershell -File tools/kill-dev.ps1  (called by the kill:dev npm script)

Get-Process wincommander-free, wincommander-pro, WinCommander -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue

# Free port 1420 (Vite dev server) if held by a stale bun/node process.
# vite.config.ts uses strictPort: true, so an occupied port aborts startup.
$conn = Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($conn) {
    Stop-Process -Id $conn.OwningProcess -Force -ErrorAction SilentlyContinue
}

exit 0
