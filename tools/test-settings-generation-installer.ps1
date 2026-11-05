[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$hook = [IO.File]::ReadAllText((Join-Path $root 'src-tauri/commander-free/nsis/hooks.nsh'))
$start = $hook.IndexOf('  !ifdef WC_EXPERIMENTAL_SETTINGS_MIGRATION')
if ($start -lt 0) { throw 'Installer migration block is not explicitly gated.' }
$end = $hook.IndexOf('  !endif', $start)
if ($end -lt 0) { throw 'Installer migration gate is not closed.' }
$block = $hook.Substring($start, $end - $start)
foreach ($required in @('WC_CLOSE_OWNED_DESKTOP_APP_OR_ABORT "settings-migration"', 'WC_STOP_OWNED_SERVICE_OR_ABORT "settings-migration"', 'wincommander-install-settings-generation.ps1', 'Abort "WinCommander could not migrate settings safely')) {
    if (-not $block.Contains($required)) { throw "Missing gated installer safety step: $required" }
}
$priorStop = $hook.LastIndexOf('WC_STOP_OWNED_SERVICE_OR_ABORT "install"', $start)
$nextCopy = $hook.IndexOf('System::Call ''kernel32::CopyFileW', $end)
if ($priorStop -lt 0 -or $nextCopy -lt $end) { throw 'Migration is not between service stop and service replacement.' }
$helperPath = Join-Path $root 'src-tauri/commander-free/nsis/install-settings-generation.ps1'
$tokens = $null; $errors = $null
[void][Management.Automation.Language.Parser]::ParseFile($helperPath, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw 'Installer migration helper has a PowerShell syntax error.' }
$helper = [IO.File]::ReadAllText($helperPath)
foreach ($required in @('WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION', '--settings-generation-lab install --offline-confirmed', '$LASTEXITCODE')) {
    if (-not $helper.Contains($required)) { throw "Missing installer helper requirement: $required" }
}
$main = [IO.File]::ReadAllText((Join-Path $root 'src-tauri/commander-free/src/main.rs'))
if (-not $main.Contains('#[cfg(not(feature = "experimental-settings-generation"))]') -or -not $main.Contains('Settings migration is not included in this build')) {
    throw 'Feature-mismatched binaries do not reject the maintenance command.'
}
Write-Output 'Gated installer migration contract passed; no installer or app was run.'
