$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupOfficeMruInventory', 'Get-OfficeMruInfo', 'Get-CleanupSummaryAllUsers', 'New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:denyPlace = $false
function Get-Item {
    param([string]$Path, [string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq 'Registry::HKEY_USERS\S-1-5-21-999-999-999-998') { return [pscustomobject]@{ PSIsContainer=$true } }
    if ($Path -like '*\File MRU') { return [pscustomobject]@{ Name='HKEY_USERS\Fixture\Software\Microsoft\Office\16.0\Word\File MRU'; PSPath='fixture-file-key' } }
    if ($Path -like '*\Place MRU' -and $script:denyPlace) { throw [UnauthorizedAccessException]::new('fixture denied') }
}
function Get-ItemProperty {
    param([string]$Path, [Alias('EA')]$ErrorAction)
    if ($Path -eq 'fixture-file-key') { return [pscustomobject]@{ 'Item 1'='[F00000000]C:\Fixture\document.docx'; PSPath=$Path } }
    throw 'Unexpected registry entry.'
}
$current = Get-OfficeMruInfo
if ($current.partial -or $current.total -ne 1 -or $current.files[0].value -ne '[F00000000]C:\Fixture\document.docx' -or $current.files[0].key -ne 'Item 1' -or $current.files[0].path -ne 'fixture-file-key') { throw 'Current-user Office values were lost.' }
$script:denyPlace = $true
$partial = Get-OfficeMruInfo
if (-not $partial.partial -or $partial.total -ne 1 -or $partial.files[0].value -ne '[F00000000]C:\Fixture\document.docx') { throw 'Office partial scan lost available records.' }
$fixtureUser = 'wc-office-fixture'
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID='S-1-5-21-999-999-999-998'; LocalPath="$env:SystemDrive\Users\$fixtureUser" } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@($fixtureUser) } }
$result = Get-CleanupSummaryAllUsers -CategoryIds 'officeMru' -TargetUsers $fixtureUser
$category = @($result.users)[0].categories[0]
if (-not $category.partial -or $category.count -ne 1 -or $category.records[0].value -ne '[F00000000]C:\Fixture\document.docx') { throw 'Selected-account Office values or partial status were lost.' }
Write-Output 'Passed: Office MRU preserves values and partial results.'
