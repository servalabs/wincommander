$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupExplorerSearchInventory', 'Get-ExplorerSearchHistoryInfo', 'Get-CleanupSummaryAllUsers', 'New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:denyPaths = $false
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq 'Registry::HKEY_USERS\S-1-5-21-999-999-999-998') { return [pscustomobject]@{ PSIsContainer=$true } }
    if ($LiteralPath -like '*\TypedPaths' -and $script:denyPaths) { throw [UnauthorizedAccessException]::new('fixture denied') }
    if ($LiteralPath -like '*\WordWheelQuery') {
        $key = [pscustomobject]@{ Property=@('0', 'MRUListEx') }
        $key | Add-Member ScriptMethod GetValue { param($name, $default, $options) if ($name -eq '0') { return ,([Text.Encoding]::Unicode.GetBytes("fixture search`0")) }; return ,([byte[]](0, 0, 0, 0)) }
        $key | Add-Member ScriptMethod GetValueKind { param($name) 'Binary' }
        return $key
    }
    if ($LiteralPath -like '*\TypedPaths') {
        $key = [pscustomobject]@{ Property=@('url1') }
        $key | Add-Member ScriptMethod GetValue { param($name, $default, $options) 'C:\Fixture\folder' }
        $key | Add-Member ScriptMethod GetValueKind { param($name) 'String' }
        return $key
    }
    throw [Management.Automation.ItemNotFoundException]::new('fixture absent')
}
$current = Get-ExplorerSearchHistoryInfo
if ($current.partial -or $current.total -ne 3 -or $current.files[0].text -ne 'fixture search' -or @($current.files[0].value).Count -lt 2 -or $current.files[2].value -ne 'C:\Fixture\folder') { throw 'Current-user Explorer search details were lost.' }
$script:denyPaths = $true
$partial = Get-ExplorerSearchHistoryInfo
if (-not $partial.partial -or $partial.total -ne 2 -or $partial.files[0].text -ne 'fixture search') { throw 'Partial Explorer scan lost available records.' }
$fixtureUser = 'wc-explorer-fixture'
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID='S-1-5-21-999-999-999-998'; LocalPath="$env:SystemDrive\Users\$fixtureUser" } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@($fixtureUser) } }
$result = Get-CleanupSummaryAllUsers -CategoryIds 'explorerSearchHistory' -TargetUsers $fixtureUser
$category = @($result.users)[0].categories[0]
if (-not $category.partial -or $category.count -ne 2 -or $category.records[0].text -ne 'fixture search') { throw 'Selected-account Explorer search details or partial status were lost.' }
Write-Output 'Passed: Explorer search preserves values and partial results.'
