[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$hook = [IO.File]::ReadAllText((Join-Path $root 'src-tauri/commander-free/nsis/hooks.nsh'))
# A post-install cutover can strand the replacement binary on failure. Do not
# reconnect migration until an interruption-safe installer protocol is tested.
if ($hook.Contains('wincommander-install-settings-generation.ps1') -or $hook.Contains('WC_INSTALL_SETTINGS_GENERATION')) {
    throw 'Unsafe installer migration route must remain disconnected.'
}
$consentCheck = $hook.IndexOf('!insertmacro WC_CHECK_NEW_IDENTITY_CONSENT', [StringComparison]::Ordinal)
$identityCall = $hook.IndexOf('-Action new-identity -ConfirmNewIdentity', [StringComparison]::Ordinal)
if ($consentCheck -lt 0 -or $identityCall -le $consentCheck) {
    throw 'Identity creation must follow the installer consent check and explicit confirmation.'
}
$helperPath = Join-Path $root 'src-tauri/commander-free/nsis/install-settings-generation.ps1'
$tokens = $null; $errors = $null
[void][Management.Automation.Language.Parser]::ParseFile($helperPath, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw 'Installer migration helper has a PowerShell syntax error.' }
$helper = [IO.File]::ReadAllText($helperPath)
foreach ($required in @('WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION', '--settings-source-proof-lab stage --offline-confirmed', '$LASTEXITCODE', 'ConfirmNewIdentity')) {
    if (-not $helper.Contains($required)) { throw "Missing installer helper requirement: $required" }
}
$main = [IO.File]::ReadAllText((Join-Path $root 'src-tauri/commander-free/src/main.rs'))
if (-not $main.Contains('#[cfg(not(feature = "experimental-settings-generation"))]') -or -not $main.Contains('Settings migration is not included in this build')) {
    throw 'Feature-mismatched binaries do not reject the maintenance command.'
}
Write-Output 'Installer migration remains disconnected; lab staging helper syntax passed. No installer or app was run.'
