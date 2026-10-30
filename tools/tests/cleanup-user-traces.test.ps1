$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module did not parse.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-NTUserTraces' }, $true)
if (-not $definition) { throw 'User activity scan is missing.' }
. ([scriptblock]::Create($definition.Extent.Text))
function Get-Item {
    param($Path, $ErrorAction)
    if ($script:mode -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic missing registry key') }
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic registry denial') }
    return [pscustomobject]@{ Name = $Path }
}
function Get-ItemProperty {
    param($Path, $ErrorAction)
    if ($script:mode -eq 'property-denied') { throw [UnauthorizedAccessException]::new('Synthetic property denial') }
    if ($Path -like '*RunMRU') { return [pscustomobject]@{ MRUList = 'a'; a = 'example\1' } }
    return [pscustomobject]@{ item = 'example' }
}
$script:mode = 'valid'; $result = Get-NTUserTraces
if ($result.error -or $result.total -ne 4 -or $result.sections[0].entries[0].value -ne 'example') { throw 'Readable registry details were not returned.' }
$script:mode = 'missing'; $result = Get-NTUserTraces
if ($result.error -or $result.total -ne 0) { throw 'Missing registry keys were not reported as empty.' }
foreach ($mode in @('denied', 'property-denied')) {
    $script:mode = $mode; $result = Get-NTUserTraces
    if (-not $result.error) { throw "An inaccessible registry key was reported as empty: $mode" }
}
Write-Output 'Passed: readable, missing and denied user activity registry scans.'
