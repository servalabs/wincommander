$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-ConnectivityHistory' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:mode = 'normal'
function Get-Item {
    param($LiteralPath, $ErrorAction)
    if ($script:mode -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new() }
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new() }
    [pscustomobject]@{ PSPath=$LiteralPath }
}
function Get-ChildItem {
    param($LiteralPath, $ErrorAction)
    if ($LiteralPath -like '*Unmanaged') { throw [UnauthorizedAccessException]::new() }
    if ($LiteralPath -like '*Managed') { return }
    [pscustomobject]@{ PSPath='synthetic-good'; PSChildName='profile-1' }
    [pscustomobject]@{ PSPath='synthetic-denied'; PSChildName='profile-2' }
}
function Get-ItemProperty {
    param($LiteralPath, $ErrorAction)
    if ($LiteralPath -eq 'synthetic-denied') { throw [UnauthorizedAccessException]::new() }
    [pscustomobject]@{ ProfileName='sample'; Description='example'; DnsSuffix='example.test'; Category=1; ExtraBytes=[byte[]](1,2,3); PSPath='synthetic-good' }
}
$result = Get-ConnectivityHistory
if (-not $result.partial -or $result.total -ne 1 -or $result.entries[0].name -ne 'sample' -or $result.entries[0].registryPath -ne 'synthetic-good' -or @($result.entries[0].registryValues.ExtraBytes).Count -ne 3 -or $result.entries[0].registryValues.ContainsKey('PSPath')) { throw 'Connectivity records or partial status were lost.' }
$script:mode = 'missing'; $result = Get-ConnectivityHistory
if ($result.partial -or $result.total -ne 0) { throw 'Missing roots should yield an empty scan.' }
$script:mode = 'denied'; $result = Get-ConnectivityHistory
if (-not $result.partial -or $result.total -ne 0) { throw 'Denied roots must not yield a complete empty scan.' }
Write-Output 'Passed: connectivity scan retains values and reports incomplete sources.'
