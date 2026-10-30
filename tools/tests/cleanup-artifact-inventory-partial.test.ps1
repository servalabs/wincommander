$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module could not be parsed.' }
foreach ($name in @('Get-CleanupArtifactFiles','Get-CleanupArtifactInventory','Get-CommunicationCachesInfo','Get-RemoteAccessLogsInfo','Get-GameLauncherLogsInfo')) {
    $def=$ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $def) { throw "Missing reader: $name" }
    . ([scriptblock]::Create($def.Extent.Text))
}
$script:case='normal'; $script:sourceIndex=0
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath, [switch]$Force)
    $script:sourceIndex++
    if ($script:case -eq 'denied' -and $script:sourceIndex -eq 2) { throw [UnauthorizedAccessException]::new('Synthetic denied source') }
    [pscustomobject]@{FullName=$LiteralPath}
}
function Get-ChildItem {
    [CmdletBinding()] param([string]$LiteralPath, [switch]$Recurse, [switch]$File, [string]$Filter, [switch]$Force)
    [pscustomobject]@{FullName=(Join-Path $LiteralPath 'observation.dat'); Length=1024; LastWriteTime=[DateTime]'2026-01-01'}
    if ($script:case -eq 'late' -and $script:sourceIndex -eq 1) { throw [UnauthorizedAccessException]::new('Synthetic late denial') }
}
foreach ($reader in @('Get-CommunicationCachesInfo','Get-RemoteAccessLogsInfo','Get-GameLauncherLogsInfo')) {
    $script:case='normal'; $script:sourceIndex=0; $result=& $reader
    if ($result.error -or $result.partial -or $result.coverage -ne 'limited' -or $result.total -lt 2) { throw "$reader did not retain its limited complete-source inventory." }
    foreach ($case in @('denied','late')) {
        $script:case=$case; $script:sourceIndex=0; $result=& $reader
        if ($result.error -or -not $result.partial -or $result.status -ne 'partial' -or $result.total -lt 2 -or -not $result.files[0].path) { throw "$reader lost observations or failed to report $case as partial." }
    }
}
'Passed: limited artifact inventories retain observations across source and late enumeration failures.'
