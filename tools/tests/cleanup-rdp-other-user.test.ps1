$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupSummaryAllUsers', 'New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$fixtureUser = 'wc-rdp-fixture'
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID='S-1-5-21-999-999-999-998'; LocalPath="$env:SystemDrive\Users\$fixtureUser" } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@($fixtureUser) } }
function Get-Item {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq 'Registry::HKEY_USERS\S-1-5-21-999-999-999-998') { return [pscustomobject]@{ PSIsContainer=$true } }
    if ($LiteralPath -like '*Terminal Server Client\Default') { return [pscustomobject]@{ Property=@('MRU0', 'other') } }
    if ($LiteralPath -like '*Terminal Server Client\Servers') { return [pscustomobject]@{ PSIsContainer=$true } }
    if ($LiteralPath -like '*Documents\Default.rdp') { return [pscustomobject]@{ LastWriteTime=[datetime]'2026-01-02' } }
    throw [Management.Automation.ItemNotFoundException]::new()
}
function Get-ItemPropertyValue {
    param([Alias('Path')][string]$LiteralPath, [string]$Name, [Alias('EA')]$ErrorAction)
    if ($Name -eq 'MRU0') { return 'fixture-host' }
    throw 'Unexpected property.'
}
function Get-ChildItem {
    param([Alias('Path')][string]$LiteralPath, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -like '*Terminal Server Client\Servers') { return [pscustomobject]@{ PSChildName='fixture-server'; PSPath='fixture-server-key' } }
    throw 'Unexpected enumeration.'
}
function Get-ItemProperty {
    param([Alias('Path')][string]$LiteralPath, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq 'fixture-server-key') { return [pscustomobject]@{ UsernameHint='fixture-user' } }
    throw 'Unexpected registry entry.'
}
$result = Get-CleanupSummaryAllUsers -CategoryIds 'rdpHistory' -TargetUsers $fixtureUser
$category = @($result.users)[0].categories[0]
if ($category.error -or $category.count -ne 3) { throw 'Selected-account RDP count was not retained.' }
$server = @($category.records | Where-Object { $_.type -eq 'Server' })
if ($server.Count -ne 1 -or $server[0].host -ne 'fixture-server' -or $server[0].username -ne 'fixture-user' -or $server[0].key -ne 'fixture-server-key') { throw 'RDP server details were lost.' }
$mru = @($category.records | Where-Object { $_.type -eq 'MRU' })
if ($mru.Count -ne 1 -or $mru[0].host -ne 'fixture-host' -or $mru[0].key -ne 'MRU0') { throw 'RDP MRU details were lost.' }
$file = @($category.records | Where-Object { $_.type -eq 'DefaultRDP' })
if ($file.Count -ne 1 -or $file[0].key -notlike '*Documents\Default.rdp' -or -not $file[0].lastModified) { throw 'RDP file details were lost.' }
Write-Output 'Passed: selected account RDP details preserve source fields.'
