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
$fixtureRoot = "$env:SystemDrive\Users\wc-docker-fixture"
$dockerRoot = "$fixtureRoot\AppData\Local\Docker\wsl"
$script:mode = 'found'; $script:enumerated = @()
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID=$fixtureSid; LocalPath=$fixtureRoot } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@('wc-docker-fixture') } }
function Get-Item {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -in @((Split-Path -Parent $fixtureRoot), $fixtureRoot, "$fixtureRoot\AppData", "$fixtureRoot\AppData\Local", "$fixtureRoot\AppData\Local\Docker", $dockerRoot, "$fixtureRoot\AppData\Local\Docker\vm-data", "$fixtureRoot\AppData\Roaming", "$fixtureRoot\AppData\Roaming\Docker")) {
        if ($LiteralPath -eq $dockerRoot -and $script:mode -eq 'missing') { throw [System.Management.Automation.ItemNotFoundException]::new() }
        if ($LiteralPath -eq $dockerRoot -and $script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new() }
        return [pscustomobject]@{ FullName=$LiteralPath; Name=(Split-Path -Leaf $LiteralPath); PSIsContainer=$true; Attributes=if ($LiteralPath -eq $dockerRoot -and $script:mode -eq 'link') { 1024 } else { 0 } }
    }
    if ($script:mode -eq 'found' -and $LiteralPath -in @("$fixtureRoot\AppData\Local\Docker\vm-data\DockerDesktop.vhdx", "$fixtureRoot\AppData\Roaming\Docker\log.txt")) {
        return [pscustomobject]@{ FullName=$LiteralPath; Name=(Split-Path -Leaf $LiteralPath); PSIsContainer=$false; Attributes=0; Length=2048; LastWriteTime=[datetime]'2026-01-01' }
    }
    throw [System.Management.Automation.ItemNotFoundException]::new()
}
function Get-ChildItem {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    $script:enumerated += $LiteralPath
    if ($LiteralPath -ne $dockerRoot) { throw 'Unexpected enumeration.' }
    if ($script:mode -eq 'large') {
        return @(1..501 | ForEach-Object { [pscustomobject]@{ Name="$_.vhdx"; FullName="$dockerRoot\$_.vhdx"; PSIsContainer=$false; Attributes=0; Length=1024; LastWriteTime=[datetime]'2026-01-01' } })
    }
    @(
        [pscustomobject]@{ Name='disk.vhdx'; FullName="$dockerRoot\disk.vhdx"; PSIsContainer=$false; Attributes=0; Length=1024; LastWriteTime=[datetime]'2026-01-01' },
        [pscustomobject]@{ Name='other.txt'; FullName="$dockerRoot\other.txt"; PSIsContainer=$false; Attributes=0; Length=42; LastWriteTime=[datetime]'2026-01-01' },
        [pscustomobject]@{ Name='redirect'; FullName="$dockerRoot\redirect"; PSIsContainer=$true; Attributes=1024 }
    )
}
function Get-Command { throw 'No Docker engine command may run.' }
$result = Get-CleanupSummaryAllUsers -CategoryIds 'dockerDesktopData' -TargetUsers $fixtureSid
$category = @($result.users)[0].categories[0]
if ($category.error -or -not $category.partial -or $category.count -ne 3 -or @($category.records).Count -ne 3 -or $category.records[0].path -ne "$dockerRoot\disk.vhdx" -or $category.message -notmatch 'could not be checked') { throw 'Observed disks, log or redirected child were not reported accurately.' }
if (@($category.records | Where-Object { $_.path -eq "$fixtureRoot\AppData\Local\Docker\vm-data\DockerDesktop.vhdx" }).Count -ne 1 -or @($category.records | Where-Object { $_.path -eq "$fixtureRoot\AppData\Roaming\Docker\log.txt" }).Count -ne 1) { throw 'Docker VMM disk or log metadata was lost.' }
if ($script:enumerated -contains "$dockerRoot\redirect") { throw 'Redirected Docker child was traversed.' }
$script:mode = 'large'
$category = (Get-CleanupSummaryAllUsers -CategoryIds 'dockerDesktopData' -TargetUsers $fixtureSid).users[0].categories[0]
if ($category.error -or -not $category.partial -or $category.count -ne 501 -or @($category.records).Count -ne 500 -or -not $category.truncated) { throw 'A bounded Docker preview did not retain its inventory count and truncation marker.' }
foreach ($mode in @('missing','denied','link')) {
    $script:mode = $mode; $script:enumerated = @()
    $category = (Get-CleanupSummaryAllUsers -CategoryIds 'dockerDesktopData' -TargetUsers $fixtureSid).users[0].categories[0]
    if ($category.error -or -not $category.partial -or $category.count -ne 0 -or $script:enumerated.Count -ne 0) { throw "Unsafe or inaccurate $mode Docker result." }
    if ($mode -ne 'missing' -and $category.message -notmatch 'could not be checked') { throw "Unavailable $mode source was not reported." }
}
Write-Output 'Passed: selected-account Docker metadata remains partial and redirected or inaccessible folders are not traversed.'
