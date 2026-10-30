$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupSummaryAllUsers', 'New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$fixtureUser = 'wc-execution-fixture'
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID='S-1-5-21-999-999-999-999'; LocalPath="$env:SystemDrive\Users\$fixtureUser" } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@($fixtureUser) } }
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($LiteralPath -like '*Compatibility Assistant\Store' -or $LiteralPath -like '*\Search\RecentApps' -or $LiteralPath -like '*Session Manager\AppCompatCache' -or $LiteralPath -eq 'Registry::HKEY_USERS\S-1-5-21-999-999-999-999') { return [pscustomobject]@{ PSIsContainer=$true; Property=@() } }
    throw [System.Management.Automation.ItemNotFoundException]::new()
}
function Get-ChildItem {
    param([Alias('LiteralPath')][string]$Path, [Alias('EA')]$ErrorAction)
    if ($Path -like '*\Search\RecentApps') { return [pscustomobject]@{ PSPath='fixture-recent-entry' } }
    throw 'Unexpected child inventory.'
}
function Get-ItemProperty {
    param([Alias('Path')][string]$LiteralPath, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -like '*Compatibility Assistant\Store') { return [pscustomobject]@{ 'C:\Fixture\store.exe' = 1 } }
    if ($LiteralPath -eq 'fixture-recent-entry') { return [pscustomobject]@{ AppId='not-the-executable'; AppPath='C:\Fixture\recent.exe' } }
    if ($LiteralPath -like '*Session Manager\AppCompatCache') { return [pscustomobject]@{ AppCompatCache=[byte[]](1,2,3) } }
    throw 'Unexpected property read.'
}
$summary = Get-CleanupSummaryAllUsers -CategoryIds 'execCache' -TargetUsers $fixtureUser
$category = @($summary.users)[0].categories[0]
if ($category.error -or $category.count -ne 3) { throw 'Execution count did not reflect the actual inventory.' }
$rows = @($category.records)
if (@($rows | Where-Object { $_.path -eq 'C:\Fixture\store.exe' -and $_.source -eq 'Store' }).Count -ne 1) { throw 'Store source details were lost.' }
if (@($rows | Where-Object { $_.path -eq 'C:\Fixture\recent.exe' -and $_.source -eq 'RecentApps' }).Count -ne 1) { throw 'RecentApps executable path was not retained.' }
if (@($rows | Where-Object { $_.path -eq 'not-the-executable' }).Count) { throw 'AppId was incorrectly reported as an executable path.' }
if (@($rows | Where-Object { $_.path -eq 'ShimCache (Binary Blob - 3 bytes)' -and $_.source -eq 'ShimCache' }).Count -ne 1) { throw 'Shared ShimCache details were omitted.' }
Write-Output 'Passed: other-account execution details retain paths and sources.'
