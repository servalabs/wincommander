$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-NetBIOSCacheInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
function nbtstat.exe {
    Set-Variable -Name LASTEXITCODE -Value $script:exitCode -Scope 1
    if ($script:hasEntry) { '  FIXTURE       <20>  UNIQUE  192.0.2.10   123' }
}
$script:exitCode = 0; $script:hasEntry = $true
$success = Get-NetBIOSCacheInfo
if ($success.error -or $success.total -ne 1) { throw 'A successful cache entry was not returned.' }
$script:exitCode = 1; $script:hasEntry = $false
$failed = Get-NetBIOSCacheInfo
if (-not $failed.error -or $failed.total -ne 0) { throw 'Failed provider was presented as an empty successful cache.' }
$script:hasEntry = $true
$partial = Get-NetBIOSCacheInfo
if (-not $partial.error -or $partial.total -ne 1 -or @($partial.files).Count -ne 1) { throw 'Available output from a failed provider was discarded.' }
Write-Output 'Passed: NetBIOS provider failures retain observations and report an error.'
