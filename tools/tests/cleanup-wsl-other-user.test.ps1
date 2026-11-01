$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupSummaryAllUsers', 'New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$fixtureSid = 'S-1-5-21-999-999-999-998'
$fixtureRoot = "$env:SystemDrive\Users\wc-wsl-fixture"
$fixtureHive = "Registry::HKEY_USERS\$fixtureSid"
$fixtureLxss = "$fixtureHive\Software\Microsoft\Windows\CurrentVersion\Lxss"
$script:fixtureMode = 'found'
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID=$fixtureSid; LocalPath=$fixtureRoot } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@('wc-wsl-fixture') } }
function Get-Item {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($script:fixtureMode -eq 'hive-denied' -and $LiteralPath -eq $fixtureHive) { throw [UnauthorizedAccessException]::new() }
    if ($LiteralPath -eq $fixtureHive) { return [pscustomobject]@{ PSIsContainer=$true } }
    if ($LiteralPath -eq $fixtureLxss -and $script:fixtureMode -ne 'absent') { return [pscustomobject]@{ PSIsContainer=$true } }
    throw [System.Management.Automation.ItemNotFoundException]::new()
}
function Get-ChildItem {
    param([Alias('Path')][string]$LiteralPath, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -ne $fixtureLxss) { throw 'Unexpected enumeration.' }
    @([pscustomobject]@{ PSChildName='{fixture-1}'; PSPath="$fixtureLxss\{fixture-1}"; Property=@('DistributionName','BasePath','Version','Extra') },
      [pscustomobject]@{ PSChildName='{fixture-2}'; PSPath="$fixtureLxss\{fixture-2}"; Property=@('DistributionName','Denied') })
}
function Get-ItemPropertyValue {
    param([Alias('Path')][string]$LiteralPath, [string]$Name, [Alias('EA')]$ErrorAction)
    if ($Name -eq 'Denied') { throw [UnauthorizedAccessException]::new() }
    switch ($Name) {
        DistributionName { return 'Fixture Linux' }
        BasePath { return 'D:\external-fixture' }
        Version { return 2 }
        Extra { return ,([byte[]](1,2,3)) }
        default { throw 'Unexpected property.' }
    }
}
$category = (Get-CleanupSummaryAllUsers -CategoryIds 'wslData' -TargetUsers $fixtureSid).users[0].categories[0]
if ($category.error -or -not $category.partial -or $category.count -ne 2 -or @($category.records).Count -ne 2) { throw 'WSL registration inventory did not retain its partial results.' }
if ($category.records[0].registryValues.DistributionName -ne 'Fixture Linux' -or $category.records[0].registryValues.BasePath -ne 'D:\external-fixture' -or $category.records[0].registryValues.Version -ne 2 -or $category.records[0].registryValues.Extra.Length -ne 3) { throw 'WSL registration values were not retained.' }
if ($category.records[1].registryValues.DistributionName -ne 'Fixture Linux' -or $category.records[1].readStatus -ne 'unavailable') { throw 'Unreadable registration did not retain available values.' }
if ($category.message -notmatch 'disks' -or $category.coverage -notmatch 'registry') { throw 'Limited scope was not reported.' }
$script:fixtureMode = 'absent'
$category = (Get-CleanupSummaryAllUsers -CategoryIds 'wslData' -TargetUsers $fixtureSid).users[0].categories[0]
if ($category.error -or -not $category.partial -or $category.count -ne 0) { throw 'Missing Lxss registration must remain partial.' }
$script:fixtureMode = 'hive-denied'
$category = (Get-CleanupSummaryAllUsers -CategoryIds 'wslData' -TargetUsers $fixtureSid).users[0].categories[0]
if (-not $category.error -or $category.count -ne -1) { throw 'Denied hive must stay unknown.' }
Write-Output 'Passed: selected-account WSL registration values are retained with partial disk coverage and denied hive handling.'
