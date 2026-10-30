$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupSummaryAllUsers', 'New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$fixtureUser = 'wc-traces-fixture'
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID='S-1-5-21-999-999-999-998'; LocalPath="$env:SystemDrive\Users\$fixtureUser" } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@($fixtureUser) } }
function Get-Item {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq 'Registry::HKEY_USERS\S-1-5-21-999-999-999-998') { return [pscustomobject]@{ PSIsContainer=$true } }
    if ($LiteralPath -like '*Explorer\RunMRU') { return [pscustomobject]@{ Property=@('a') } }
    if ($LiteralPath -like '*Internet Explorer\TypedURLs') { return [pscustomobject]@{ Property=@('url1') } }
    if ($LiteralPath -like '*Explorer\WordWheelQuery') { return [pscustomobject]@{ Property=@('0', 'empty', 'MRUListEx') } }
    throw [System.Management.Automation.ItemNotFoundException]::new()
}
function Get-ItemPropertyValue {
    param([Alias('Path')][string]$LiteralPath, [string]$Name, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -like '*Internet Explorer\TypedURLs' -and $Name -eq 'url1') { return 'https://fixture.invalid/' }
    if ($LiteralPath -like '*Explorer\RunMRU' -and $Name -eq 'a') { return 'example\1' }
    if ($Name -eq '0') { return ,([byte[]](0x41, 0, 0, 0)) }
    if ($Name -eq 'empty') { return '' }
    if ($Name -eq 'MRUListEx') { return ,([byte[]](0, 0, 0, 0)) }
    throw 'Unexpected registry value.'
}
$result = Get-CleanupSummaryAllUsers -CategoryIds 'ntUserTraces' -TargetUsers $fixtureUser
$category = @($result.users)[0].categories[0]
if ($category.error -or $category.count -ne 5) { throw 'Selected account trace count did not include all values.' }
$run = @($category.records | Where-Object { $_.section -eq 'RunMRU' -and $_.key -eq 'a' })
if ($run.Count -ne 1 -or $run[0].value -ne 'example' -or $run[0].rawValue -ne 'example\1') { throw 'RunMRU original value was not retained.' }
$url = @($category.records | Where-Object { $_.section -eq 'TypedURLs' -and $_.key -eq 'url1' })
if ($url.Count -ne 1 -or $url[0].value -ne 'https://fixture.invalid/') { throw 'Internet Explorer TypedURLs was not read.' }
$binary = @($category.records | Where-Object { $_.section -eq 'WordWheelQuery' -and $_.key -eq '0' })
if ($binary.Count -ne 1 -or $binary[0].value -isnot [byte[]] -or $binary[0].value.Length -ne 4) { throw 'Original binary value was not retained.' }
$empty = @($category.records | Where-Object { $_.section -eq 'WordWheelQuery' -and $_.key -eq 'empty' })
if ($empty.Count -ne 1 -or $empty[0].value -ne '') { throw 'An empty value was dropped.' }
if (@($category.records | Where-Object { $_.key -eq 'MRUListEx' }).Count -ne 1) { throw 'WordWheel order bytes were dropped.' }
Write-Output 'Passed: selected account traces retain source and original values.'
