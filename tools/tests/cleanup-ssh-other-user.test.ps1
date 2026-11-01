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
$fixtureRoot = "$env:SystemDrive\Users\wc-ssh-fixture"
$script:fixtureMode = 'found'
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID=$fixtureSid; LocalPath=$fixtureRoot } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@('wc-ssh-fixture') } }
function Get-Item {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq "Registry::HKEY_USERS\$fixtureSid") { return [pscustomobject]@{ PSIsContainer=$true } }
    if ($LiteralPath -eq (Split-Path -Parent $fixtureRoot)) { return [pscustomobject]@{ PSIsContainer=$true; Attributes=0 } }
    if ($LiteralPath -eq $fixtureRoot) { return [pscustomobject]@{ PSIsContainer=$true; Attributes=0 } }
    if ($LiteralPath -eq "$fixtureRoot\.ssh") {
        if ($script:fixtureMode -eq 'absent') { throw [System.Management.Automation.ItemNotFoundException]::new() }
        return [pscustomobject]@{ PSIsContainer=$true; Attributes=if ($script:fixtureMode -eq 'link') { 1024 } else { 0 } }
    }
    if ($LiteralPath -eq "$fixtureRoot\.ssh\known_hosts") { return [pscustomobject]@{ PSIsContainer=$false; Attributes=0; FullName=$LiteralPath; Length=512; LastWriteTime=[datetime]'2026-01-01' } }
    throw [System.Management.Automation.ItemNotFoundException]::new()
}
$result = Get-CleanupSummaryAllUsers -CategoryIds 'sshState' -TargetUsers $fixtureSid
$category = @($result.users)[0].categories[0]
if ($category.error -or -not $category.partial -or $category.count -ne 1 -or @($category.records).Count -ne 1 -or $category.records[0].path -ne "$fixtureRoot\.ssh\known_hosts" -or $category.message -notmatch 'agent') { throw 'SSH observations or partial agent coverage were lost.' }
$script:fixtureMode = 'absent'
$category = (Get-CleanupSummaryAllUsers -CategoryIds 'sshState' -TargetUsers $fixtureSid).users[0].categories[0]
if ($category.error -or -not $category.partial -or $category.count -ne 0) { throw 'Absent known_hosts must remain a partial scan.' }
$script:fixtureMode = 'link'
$category = (Get-CleanupSummaryAllUsers -CategoryIds 'sshState' -TargetUsers $fixtureSid).users[0].categories[0]
if (-not $category.error -or $category.count -ne -1) { throw 'A redirected SSH folder must fail closed.' }
Write-Output 'Passed: other-account SSH metadata is visible with partial agent coverage and redirected folders are rejected.'
