$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module did not parse.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Invoke-CleanupClearForUser' }, $true)
if (-not $definition) { throw 'Per-user clearer is missing.' }
. ([scriptblock]::Create($definition.Extent.Text))

function Assert-CleanupCategoriesAllowed { param($CategoryIds, $AllowDefaultExcluded) }
function Get-Item {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:denied -and $LiteralPath -like '*\Temp') { throw [UnauthorizedAccessException]::new('Synthetic denial.') }
    if ($LiteralPath -notlike '*\Temp') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absent root.') }
    [pscustomobject]@{ PSIsContainer = $true; Attributes = [IO.FileAttributes]::Directory }
}
function Get-ChildItem {
    param($LiteralPath, $Path, [switch]$Recurse, [int]$Depth, [switch]$File, $Filter, $Include, [switch]$Force, $ErrorAction)
    if ($LiteralPath -notlike '*\Temp' -or $Filter -ne '*.tmp') { throw 'Unexpected inventory path.' }
    foreach ($name in @($script:files)) { [pscustomobject]@{ FullName = $name } }
}
function Remove-ItemSecure {
    param($Path)
    $script:removed++
    if (-not $script:retain) { $script:files = @($script:files | Where-Object { $_ -ne $Path }) }
}

$script:files = @(0..500 | ForEach-Object { "C:\Fixture\Temp\$_.tmp" })
$script:removed = 0; $script:denied = $false; $script:retain = $false
$result = Invoke-CleanupClearForUser -UserProfilePath 'C:\Fixture' -CategoryIds @('officeTempFiles')
if ($result.officeTempFiles -ne 'cleaned' -or $script:removed -ne 501) { throw 'All 501 temporary records must be cleared and verified.' }
$script:files = @('C:\Fixture\Temp\retained.tmp'); $script:retain = $true
if ((Invoke-CleanupClearForUser -UserProfilePath 'C:\Fixture' -CategoryIds @('officeTempFiles')).officeTempFiles -ne 'failed') { throw 'A retained temporary file must fail verification.' }
$script:denied = $true
if ((Invoke-CleanupClearForUser -UserProfilePath 'C:\Fixture' -CategoryIds @('officeTempFiles')).officeTempFiles -ne 'failed') { throw 'Denied inventory must not be reported as clean.' }
Write-Output 'Passed: other-account Office temp cleanup covers all files and reports retention or access failure.'
