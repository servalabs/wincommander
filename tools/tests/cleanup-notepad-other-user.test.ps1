$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse the cleanup module.' }
foreach ($name in @('Get-CleanupSummaryAllUsers', 'New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing $name." }
    . ([scriptblock]::Create($definition.Extent.Text))
}

# Every provider is synthetic: no actual user profile, registry hive or file is read.
$fixtureUser = 'wc-notepad-fixture'
$fixtureRoot = "$env:SystemDrive\Users\$fixtureUser\AppData\Local\Packages"
$script:denySecondPackage = $false
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID='S-1-5-21-999-999-999-999'; LocalPath="$env:SystemDrive\Users\$fixtureUser" } }
function Test-WCVisibleUserProfile { param($Name, $DisplayName, $Path) return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@($fixtureUser) } }
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:denySecondPackage -and $LiteralPath -like '*WindowsNotepad_two*TabState') { throw [UnauthorizedAccessException]::new('Synthetic access denied.') }
    return [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory }
}
function Get-ChildItem {
    param([string]$LiteralPath, [string]$Filter, [switch]$Directory, [switch]$File, [switch]$Recurse, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($Directory) {
        if ($LiteralPath -ne $fixtureRoot -or $Filter -ne 'Microsoft.WindowsNotepad_*') { throw 'Unexpected package inventory.' }
        return @('one','two' | ForEach-Object { [pscustomobject]@{ FullName="$fixtureRoot\Microsoft.WindowsNotepad_$_"; Attributes=[IO.FileAttributes]::Directory } })
    }
    if (-not $File -or -not $Recurse -or $LiteralPath -notlike "$fixtureRoot\Microsoft.WindowsNotepad_*\LocalState\TabState") { throw 'Expected recursive per-package tab files.' }
    return [pscustomobject]@{ Name='tab.bin'; FullName="$LiteralPath\nested\tab.bin"; Length=2048; LastWriteTime=[datetime]'2026-01-01' }
}

$result = Get-CleanupSummaryAllUsers -CategoryIds 'notepadState' -TargetUsers $fixtureUser
$category = @($result.users)[0].categories[0]
if ($result.error -or $category.error -or $category.count -ne 2 -or @($category.records).Count -ne 2) { throw 'Another account must include recursive files from both Notepad packages.' }
if (@($category.records | Where-Object { $_.path -notlike '*\nested\tab.bin' }).Count) { throw 'Notepad detail records lost their nested paths.' }
$script:denySecondPackage = $true
$result = Get-CleanupSummaryAllUsers -CategoryIds 'notepadState' -TargetUsers $fixtureUser
$category = @($result.users)[0].categories[0]
if (-not $category.error -or $category.count -ne -1) { throw 'An inaccessible package must not appear complete or empty.' }
Write-Output 'Passed: other-account Notepad inventory includes every package and reports access failure.'
