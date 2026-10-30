$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-GameCapturesInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:scenario = 'missing'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absence') }
    if ($script:scenario -eq 'denied' -and $LiteralPath -like '*Packages') { throw [UnauthorizedAccessException]::new('Synthetic denial') }
    if ($LiteralPath -like '*LocalState') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absence') }
    [pscustomobject]@{ PSIsContainer=$true; FullName=$LiteralPath }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Directory, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    if ($Directory) { [pscustomobject]@{ Name='Microsoft.GamingApp_test'; FullName=(Join-Path $LiteralPath 'Microsoft.GamingApp_test') }; return }
    if ($File) {
        [pscustomobject]@{ Name='sample.mp4'; FullName=(Join-Path $LiteralPath 'sample.mp4'); Length=1024; LastWriteTime=[datetime]'2026-01-01' }
        if ($script:scenario -eq 'partial') { throw [UnauthorizedAccessException]::new('Synthetic partial enumeration') }
    }
}
$missing = Get-GameCapturesInfo
if ($missing.partial -or $missing.total -ne 0) { throw 'Missing locations should report an empty result.' }
$script:scenario = 'denied'; $denied = Get-GameCapturesInfo
if (-not $denied.partial -or $denied.total -ne 1 -or $denied.files[0].path -notlike '*sample.mp4') { throw 'Denied packages should retain capture records and report incomplete.' }
$script:scenario = 'partial'; $partial = Get-GameCapturesInfo
if (-not $partial.partial -or $partial.total -ne 1 -or $partial.files[0].path -notlike '*sample.mp4') { throw 'Partial enumeration must retain observed capture paths.' }
Write-Output 'Passed: game capture records retain paths and incomplete state.'
