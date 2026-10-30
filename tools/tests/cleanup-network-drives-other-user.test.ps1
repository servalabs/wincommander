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
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID=$fixtureSid; LocalPath="$env:SystemDrive\Users\wc-drive-fixture" } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@('wc-drive-fixture') } }
function Get-Item {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq "Registry::HKEY_USERS\$fixtureSid" -or $LiteralPath -eq "Registry::HKEY_USERS\$fixtureSid\Network") { return [pscustomobject]@{ PSIsContainer=$true } }
    throw [System.Management.Automation.ItemNotFoundException]::new()
}
function Get-ChildItem {
    param([Alias('Path')][string]$LiteralPath, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq "Registry::HKEY_USERS\$fixtureSid\Network") {
        return @([pscustomobject]@{ PSChildName='Z'; PSPath="$LiteralPath\Z" }, [pscustomobject]@{ PSChildName='Y'; PSPath="$LiteralPath\Y" })
    }
    throw 'Unexpected registry enumeration.'
}
function Get-ItemProperty {
    param([Alias('Path')][string]$LiteralPath, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -like '*\Z') { return [pscustomobject]@{ RemotePath='\\fixture\share'; UserName='fixture'; ProviderName='fixture provider'; PSPath=$LiteralPath } }
    if ($LiteralPath -like '*\Y') { throw 'Synthetic access failure.' }
    throw 'Unexpected registry read.'
}
$result = Get-CleanupSummaryAllUsers -CategoryIds 'netDrives' -TargetUsers $fixtureSid
$category = @($result.users)[0].categories[0]
if ($category.error -or -not $category.partial -or $category.count -ne 2 -or @($category.records).Count -ne 2) { throw 'Unavailable mapping did not produce partial coverage with both retained records.' }
$record = @($category.records)[0]
if ($record.name -ne 'Z' -or $record.DisplayRoot -ne '\\fixture\share' -or $record.RemotePath -ne '\\fixture\share' -or $record.ProviderName -ne 'fixture provider' -or $record.UserName -ne 'fixture') { throw 'Mapped-drive path or source details were not retained.' }
$unreadable = @($category.records | Where-Object { $_.name -eq 'Y' })
if ($unreadable.Count -ne 1 -or $unreadable[0].path -ne "Registry::HKEY_USERS\$fixtureSid\Network\Y" -or $unreadable[0].readStatus -ne 'unavailable') { throw 'Unavailable mapping lost its observed drive name or registry path.' }
Write-Output 'Passed: selected account drive mappings retain network path and available details; inaccessible mappings remain partial.'
