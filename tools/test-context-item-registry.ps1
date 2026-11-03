# Exercises only a fresh, disposable HKCU test key; never touches shell entries.
$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/modules/tweaks/ui.ps1'
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$null, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
foreach ($name in @('Get-ContextHandlerValue','Set-ContextHandlerValue','Remove-ContextHandlerValue')) {
    $node = $ast.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }.GetNewClosure(), $true)
    . ([scriptblock]::Create($node.Extent.Text))
}
$path = 'SOFTWARE\WinCommander\Tests\ContextItem-' + [Guid]::NewGuid().ToString('N')
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
$created = $false
try {
    if ($null -ne $base.OpenSubKey($path, $false)) { throw 'Unexpected test key collision.' }
    $key = $base.CreateSubKey($path, $true)
    if ($null -eq $key) { throw 'Could not create disposable test key.' }
    $created = $true
    try { $key.SetValue('', '{f81e9010-6ea4-11ce-a7ff-00aa003ca9f6}', 'String'); $key.SetValue('OtherData', 'keep', 'String'); $child=$key.CreateSubKey('Child'); $child.Dispose() } finally { $key.Dispose() }
    Remove-ContextHandlerValue 'CurrentUser' $path '{f81e9010-6ea4-11ce-a7ff-00aa003ca9f6}'
    $read = Get-ContextHandlerValue 'CurrentUser' $path
    if (-not $read.keyExists -or $read.exists) { throw 'Default was not removed as expected.' }
    $key = $base.OpenSubKey($path, $false)
    try { if ($key.GetValue('OtherData') -cne 'keep' -or 'Child' -notin $key.GetSubKeyNames()) { throw 'Other key contents were changed.' } } finally { $key.Dispose() }
    Set-ContextHandlerValue 'CurrentUser' $path '{f81e9010-6ea4-11ce-a7ff-00aa003ca9f6}'
    $read = Get-ContextHandlerValue 'CurrentUser' $path
    if (-not $read.exists -or $read.value -cne '{f81e9010-6ea4-11ce-a7ff-00aa003ca9f6}') { throw 'Default was not restored.' }
    $failed = $false
    try { Set-ContextHandlerValue 'CurrentUser' $path 'other' } catch { $failed = $true }
    if (-not $failed) { throw 'Existing default was overwritten.' }
    Write-Output 'Disposable right-click registry test passed.'
} finally {
    if ($created) { $base.DeleteSubKeyTree($path, $false) }
    $base.Dispose()
}
