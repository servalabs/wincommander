$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupSummaryAllUsers', 'New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$fixtureUser = 'wc-shellbag-fixture'
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID='S-1-5-21-999-999-999-998'; LocalPath="$env:SystemDrive\Users\$fixtureUser" } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@($fixtureUser) } }
function Get-Item {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq 'Registry::HKEY_USERS\S-1-5-21-999-999-999-998' -or $LiteralPath -like '*\Shell\BagMRU') { return [pscustomobject]@{ PSIsContainer=$true } }
    throw [Management.Automation.ItemNotFoundException]::new()
}
function Get-ChildItem {
    param([Alias('Path')][string]$LiteralPath, [switch]$Recurse, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -like '*\Classes\Local Settings\*') { throw 'Fixture source unavailable.' }
    if ($LiteralPath -like '*\Shell\BagMRU') { return [pscustomobject]@{ PSPath='fixture-bag-key'; Property=@('NodeSlot','0') } }
    throw 'Unexpected enumeration.'
}
function Get-ItemPropertyValue {
    param([Alias('Path')][string]$LiteralPath, [string]$Name, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -ne 'fixture-bag-key') { throw 'Unexpected registry entry.' }
    if ($Name -eq 'NodeSlot') { return 4 }
    if ($Name -eq '0') { return ,([byte[]](1, 2, 3)) }
    throw 'Unexpected registry value.'
}
$result = Get-CleanupSummaryAllUsers -CategoryIds 'shellBags' -TargetUsers $fixtureUser
$category = @($result.users)[0].categories[0]
if (-not $category.partial -or $category.count -ne 1) { throw 'Selected account ShellBags partial count was lost.' }
$row = @($category.records)[0]
if ($row.path -ne 'fixture-bag-key' -or $row.registryValues.NodeSlot -ne 4 -or @($row.registryValues['0']).Count -ne 3) { throw 'ShellBag registry details were lost.' }
Write-Output 'Passed: selected account ShellBags retain observed registry details and partial status.'
