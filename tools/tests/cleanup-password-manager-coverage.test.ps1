$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-PasswordManagerCachesInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:fixtureMode = 'found'
$script:fixtureCalls = 0
function Get-CleanupArtifactFiles {
    param([array]$Sources, [Collections.Generic.List[object]]$Collector)
    $script:fixtureCalls++
    if ($script:fixtureMode -eq 'denied' -or ($script:fixtureMode -eq 'mixed' -and $script:fixtureCalls -eq 2)) { throw 'Synthetic access denial' }
    if ($script:fixtureMode -in @('found', 'mixed', 'late') -and $script:fixtureCalls -eq 1) { $Collector.Add(@{ name='Synthetic cache'; path='C:\Fixture\item'; sizeKB=1; modified='fixture' }) }
    if ($script:fixtureMode -eq 'late' -and $script:fixtureCalls -eq 1) { throw 'Synthetic late access denial' }
}
$result = Get-PasswordManagerCachesInfo
if ($result.error -or $result.total -ne 1 -or $result.files[0].path -ne 'C:\Fixture\item' -or $result.coverage -ne 'limited' -or $result.coverageNote -notmatch 'custom locations') { throw 'Observed cache or limited coverage was lost.' }
$script:fixtureMode = 'empty'; $script:fixtureCalls = 0; $result = Get-PasswordManagerCachesInfo
if ($result.error -or $result.total -ne 0 -or $result.coverage -ne 'limited') { throw 'An empty selected-location scan must remain limited.' }
$script:fixtureMode = 'denied'; $script:fixtureCalls = 0; $result = Get-PasswordManagerCachesInfo
if (-not $result.partial -or $result.status -ne 'partial') { throw 'Denied selected locations must not appear complete.' }
$script:fixtureMode = 'mixed'; $script:fixtureCalls = 0; $result = Get-PasswordManagerCachesInfo
if (-not $result.partial -or $result.total -ne 1 -or $result.files[0].path -ne 'C:\Fixture\item') { throw 'A later denial must not discard an earlier observation.' }
$script:fixtureMode = 'late'; $script:fixtureCalls = 0; $result = Get-PasswordManagerCachesInfo
if (-not $result.partial -or $result.total -ne 1 -or $result.files[0].path -ne 'C:\Fixture\item') { throw 'A late denial within a source must not discard an earlier observation.' }
Write-Output 'Passed: password-manager cache scans preserve records and report limited coverage.'
