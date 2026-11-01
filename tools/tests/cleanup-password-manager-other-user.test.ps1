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
$fixtureRoot = "$env:SystemDrive\Users\wc-cache-fixture"
$fixtureCache = "$fixtureRoot\AppData\Local\1Password\Cache"
$script:fixtureMode = 'found'
$script:enumerated = @()
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID=$fixtureSid; LocalPath=$fixtureRoot } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@('wc-cache-fixture') } }
function Get-Item {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq (Split-Path -Parent $fixtureRoot) -or $LiteralPath -eq $fixtureRoot -or
        $LiteralPath -in @("$fixtureRoot\AppData", "$fixtureRoot\AppData\Local", "$fixtureRoot\AppData\Local\1Password", $fixtureCache)) {
        if ($LiteralPath -eq $fixtureCache -and $script:fixtureMode -eq 'missing') { throw [System.Management.Automation.ItemNotFoundException]::new() }
        if ($LiteralPath -eq $fixtureCache -and $script:fixtureMode -eq 'denied') { throw [UnauthorizedAccessException]::new() }
        return [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$true; Attributes=if ($LiteralPath -eq $fixtureCache -and $script:fixtureMode -eq 'link') { 1024 } else { 0 } }
    }
    throw [System.Management.Automation.ItemNotFoundException]::new()
}
function Get-ChildItem {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    $script:enumerated += $LiteralPath
    if ($LiteralPath -ne $fixtureCache) { throw 'Unexpected enumeration.' }
    if ($script:fixtureMode -eq 'large') {
        return @(1..501 | ForEach-Object { [pscustomobject]@{ FullName="$fixtureCache\$_.dat"; PSIsContainer=$false; Attributes=0; Length=1024; LastWriteTime=[datetime]'2026-01-01' } })
    }
    @(
        [pscustomobject]@{ FullName="$fixtureCache\entry.dat"; PSIsContainer=$false; Attributes=0; Length=1024; LastWriteTime=[datetime]'2026-01-01' },
        [pscustomobject]@{ FullName="$fixtureCache\redirect"; PSIsContainer=$true; Attributes=1024 }
    )
}
$result = Get-CleanupSummaryAllUsers -CategoryIds 'passwordManagerCaches' -TargetUsers $fixtureSid
$category = @($result.users)[0].categories[0]
if ($category.error -or -not $category.partial -or $category.count -ne 1 -or @($category.records).Count -ne 1 -or $category.records[0].path -ne "$fixtureCache\entry.dat" -or $category.message -notmatch 'could not be checked') { throw 'Observed cache and skipped redirected folder were not reported accurately.' }
if ($script:enumerated -contains "$fixtureCache\redirect") { throw 'A redirected child was traversed.' }
$script:fixtureMode = 'large'
$category = (Get-CleanupSummaryAllUsers -CategoryIds 'passwordManagerCaches' -TargetUsers $fixtureSid).users[0].categories[0]
if ($category.error -or -not $category.partial -or $category.count -ne 501 -or @($category.records).Count -ne 500 -or -not $category.truncated) { throw 'A bounded cache preview did not retain its full inventory count and truncation marker.' }
foreach ($mode in @('missing','denied','link')) {
    $script:fixtureMode = $mode; $script:enumerated = @()
    $category = (Get-CleanupSummaryAllUsers -CategoryIds 'passwordManagerCaches' -TargetUsers $fixtureSid).users[0].categories[0]
    if ($category.error -or -not $category.partial -or $category.count -ne 0 -or $script:enumerated.Count -ne 0) { throw "Unsafe or inaccurate $mode cache result." }
    if ($mode -ne 'missing' -and $category.message -notmatch 'could not be checked') { throw "Unavailable $mode source was not reported." }
}
Write-Output 'Passed: selected-account cache metadata stays partial and redirected or inaccessible folders are not traversed.'
