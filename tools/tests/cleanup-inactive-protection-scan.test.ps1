$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module did not parse.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-InactiveUserProtectionMetadataInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))

$script:case = 'found'
$sid = 'S-1-5-21-111-222-333-444'
function Get-CimInstance {
    param($ClassName, $ErrorAction)
    if ($script:case -eq 'inventory-denied') { throw [UnauthorizedAccessException]::new('Synthetic inventory denial') }
    [pscustomobject]@{ Special=$false; Loaded=$false; SID=$sid; LocalPath='C:\Synthetic\Inactive' }
}
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:case -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic missing root') }
    if ($script:case -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic access denial') }
    [pscustomobject]@{ PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    if ($script:case -eq 'children-denied') { throw [UnauthorizedAccessException]::new('Synthetic enumeration denial') }
    [pscustomobject]@{ FullName="$LiteralPath\fixture"; Length=1024; LastWriteTime=[DateTime]'2026-01-01'; Attributes=[IO.FileAttributes]::Normal }
    if ($script:case -eq 'late-denied') { throw [UnauthorizedAccessException]::new('Synthetic late denial') }
}

$result = Get-InactiveUserProtectionMetadataInfo
if ($result.error -or $result.partial -or $result.coverage -ne 'limited' -or $result.total -ne 1 -or $result.files[0].userSid -ne $sid -or $result.files[0].path -notlike '*fixture') { throw 'Observed metadata or limited coverage was lost.' }
foreach ($case in @('denied', 'children-denied')) {
    $script:case = $case; $result = Get-InactiveUserProtectionMetadataInfo
    if ($result.error -or -not $result.partial -or $result.status -ne 'partial') { throw "Denied source was reported complete: $case" }
}
$script:case = 'late-denied'; $result = Get-InactiveUserProtectionMetadataInfo
if (-not $result.partial -or $result.total -ne 1 -or $result.files[0].path -notlike '*fixture') { throw 'A later access failure discarded an earlier observation.' }
$script:case = 'missing'; $result = Get-InactiveUserProtectionMetadataInfo
if ($result.error -or $result.partial -or $result.total -ne 0 -or $result.coverage -ne 'limited') { throw 'Missing location was not reported as a limited empty inventory.' }
$script:case = 'inventory-denied'; $result = Get-InactiveUserProtectionMetadataInfo
if (-not $result.error) { throw 'Denied profile inventory appeared empty.' }
'Passed: inactive-user metadata preserves observations and reports denied or limited coverage.'
