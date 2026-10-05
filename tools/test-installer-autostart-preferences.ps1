[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$tokens = $null; $errors = $null
$source = Join-Path $PSScriptRoot '../src-tauri/commander-free/nsis/configure-elevated-launchers.ps1'
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Invalid startup helper.' }
foreach ($definition in $ast.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.FunctionDefinitionAst] }) {
    . ([scriptblock]::Create($definition.Extent.Text))
}

# This GUID-owned HKCU CLSID subtree has separate native and redirected views.
# No production preference, startup entry, scheduled task or service is used.
$fixtureId = [guid]::NewGuid()
$fixtureRoot = "Software\Classes\CLSID\{$fixtureId}"
$preferencePath = "Registry::HKEY_CURRENT_USER\$fixtureRoot\WinCommanderInstallerTests"
$preferenceName = 'AutostartEnabled'
$views = @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)
$cases = @(
    @{ native = $null; legacy = 0; expected = $false },
    @{ native = $null; legacy = 1; expected = $true },
    @{ native = 1; legacy = 0; expected = $true },
    @{ native = 0; legacy = 1; expected = $false },
    @{ native = 1; legacy = 'invalid'; expected = $true },
    @{ native = $null; legacy = 2; expected = 'error' },
    @{ native = $null; legacy = 'invalid'; expected = 'error' },
    @{ native = 2; legacy = 0; expected = 'error' },
    @{ native = $null; legacy = $null; expected = $true }
)
try {
    foreach ($case in $cases) {
        Remove-AutostartPreference
        foreach ($index in 0..1) {
            $key = Open-AutostartPreferenceKey $views[$index] $true $true
            try {
                $key.SetValue('ForeignPreference', 'keep')
                $value = if ($index -eq 0) { $case.native } else { $case.legacy }
                if ($null -ne $value) { $key.SetValue($preferenceName, $value) }
            } finally { $key.Dispose() }
        }
        $failed = $false
        try { $enabled = Get-AutostartEnabled $false @() }
        catch {
            if ($_.Exception.Message -ne 'The WinCommander automatic-start preference is invalid.') { throw }
            $failed = $true
        }
        if ($case.expected -is [string]) {
            if (-not $failed) { throw 'Invalid preference silently became a startup default.' }
            if ((Get-AutostartPreferenceValue $views[0]) -ne $case.native) { throw 'Invalid preference was migrated.' }
        } else {
            if ($failed -or $enabled -ne $case.expected) { throw 'Native preference precedence or legacy choice changed.' }
            if ($null -ne $case.native -or $null -ne $case.legacy) {
                if ((Get-AutostartPreferenceValue $views[0]) -ne [int]$case.expected) { throw 'Legacy preference did not migrate to the native view.' }
            }
            if ((Get-AutostartEnabled $false @()) -ne $case.expected) { throw 'Repeated migration changed startup preference.' }
        }
        Remove-AutostartPreference
        foreach ($view in $views) {
            if ($null -ne (Get-AutostartPreferenceValue $view)) { throw 'Uninstall left a preference to resurrect on reinstall.' }
            $key = Open-AutostartPreferenceKey $view $false
            try { if ($key.GetValue('ForeignPreference') -ne 'keep') { throw 'Uninstall removed an unrelated value.' } }
            finally { $key.Dispose() }
        }
    }
} finally {
    foreach ($view in $views) {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, $view)
        try { $base.DeleteSubKeyTree($fixtureRoot, $false) }
        finally { $base.Dispose() }
    }
}
Write-Output "PASS: $($cases.Count) cross-view preference cases in $([IntPtr]::Size * 8)-bit host"
