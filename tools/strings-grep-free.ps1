# tools/strings-grep-free.ps1
#
# =======================================================================
# CI INVARIANT -- Free binary AV-clean strings check
# =======================================================================
#
# Scans the WinCommander Free binary for forbidden ASCII tokens that
# indicate paid / Privacy Clean code has leaked into the open-source
# build. The Free binary must be AV-clean -- Defender / SmartScreen /
# VirusTotal should never flag it on download. Paid commands physically
# move into wincommander-pro.exe in Phase 6 of the rollout; until then
# this check is expected to fail (the current single binary contains
# everything). After Phase 6 (P4), -HardGate is flipped on in CI.
#
# Forbidden tokens are loaded from tools/strings-grep-forbidden.txt so
# P2 can append duress-feature symbols without touching this script.
#
# Usage:
#   pwsh -ExecutionPolicy Bypass -File tools/strings-grep-free.ps1
#   pwsh ... -Binary "src-tauri\target\release\wincommander-free.exe"
#   pwsh ... -HardGate   # exit 1 on any hit (P4 CI mode)
#
# Exit codes:
#   0 = clean (zero forbidden tokens found)
#   1 = at least one forbidden token found AND -HardGate is set
#   2 = binary not found (warning, not a hard failure)
#   3 = forbidden list file not found

param(
  # commander-free's Cargo.toml declares `[[bin]] name = "wincommander-free"`
  # so cargo's output matches the shipped binary name.
  [string]$Binary   = "src-tauri/target/release/wincommander-free.exe",

  # Path to the forbidden-token list (one token per line; # = comment).
  [string]$ListFile = "tools/strings-grep-forbidden.txt",

  # When set, a hit causes exit 1 (CI hard gate). Default: report-only (exit 0).
  # P4 flips this on after the sidecar split lands.
  [switch]$HardGate
)

# -- Resolve paths --------------------------------------------------------
$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Split-Path -Parent $ScriptDir

$BinaryPath = if ([System.IO.Path]::IsPathRooted($Binary)) {
  $Binary
} else {
  Join-Path -Path $RepoRoot -ChildPath $Binary
}

$ListPath = if ([System.IO.Path]::IsPathRooted($ListFile)) {
  $ListFile
} else {
  Join-Path -Path $RepoRoot -ChildPath $ListFile
}

# -- Check prerequisites --------------------------------------------------
if (-not (Test-Path -Path $ListPath)) {
  Write-Warning "[strings-grep-free] forbidden list not found: $ListPath"
  exit 3
}

if (-not (Test-Path -Path $BinaryPath)) {
  Write-Warning "[strings-grep-free] binary not found: $BinaryPath"
  Write-Warning "  Build first: cd src-tauri && cargo build --release -p commander-free"
  Write-Warning "  Or pass an explicit path: -Binary <path-to-exe>"
  exit 2
}

# -- Load forbidden tokens (skip blank lines and # comments) --------------
$ForbiddenTokens = Get-Content -Path $ListPath |
  Where-Object { $_ -and ($_ -notmatch '^\s*#') } |
  ForEach-Object { $_.Trim() } |
  Where-Object { $_ -ne '' }

if ($ForbiddenTokens.Count -eq 0) {
  Write-Warning "[strings-grep-free] forbidden list is empty: $ListPath"
  exit 0
}

# -- Read binary as latin-1 so ASCII strings survive null bytes -----------
$bytes = [System.IO.File]::ReadAllBytes($BinaryPath)
$text  = [System.Text.Encoding]::GetEncoding("latin1").GetString($bytes)

$found = @()
foreach ($token in $ForbiddenTokens) {
  $idx = $text.IndexOf($token, [StringComparison]::Ordinal)
  if ($idx -ge 0) {
    $found += [PSCustomObject]@{ Token = $token; Offset = $idx }
  }
}

if ($found.Count -eq 0) {
  Write-Host "[strings-grep-free] OK -- $($ForbiddenTokens.Count) forbidden tokens absent from $BinaryPath" -ForegroundColor Green
  exit 0
}

$mode = if ($HardGate) { "FAIL (hard gate)" } else { "WARN (soft gate -- P4 will harden)" }
Write-Host "[strings-grep-free] $mode -- $($found.Count) forbidden token(s) found in $BinaryPath" -ForegroundColor $(if ($HardGate) { "Red" } else { "Yellow" })
foreach ($hit in $found) {
  Write-Host ("  - {0,-40} @ offset 0x{1:X8}" -f $hit.Token, $hit.Offset) -ForegroundColor $(if ($HardGate) { "Red" } else { "Yellow" })
}
Write-Host ""
Write-Host "These tokens MUST live only in commander-pro / WinCommander Pro."
Write-Host "If you added a new paid command, register it in get_command_tier and"
Write-Host "ensure its source code lives in commander-pro/, not commander-free/."
if (-not $HardGate) {
  Write-Host "(Running in soft-gate mode -- no CI failure until P4 flips -HardGate on.)" -ForegroundColor DarkGray
}

if ($HardGate) { exit 1 } else { exit 0 }
