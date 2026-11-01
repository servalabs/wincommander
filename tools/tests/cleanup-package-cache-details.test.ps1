$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
foreach ($name in @('Get-CleanupPackageCacheInventory', 'Get-SpotlightCacheInfo', 'Get-PhotosCacheInfo', 'Get-XboxCacheInfo')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:scenario = 'empty'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'empty') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absence') }
    if ($script:scenario -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denial') }
    if ($LiteralPath -like '*XboxApp_denied*') {
        if ($script:scenario -eq 'missing-one') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absence') }
        throw [UnauthorizedAccessException]::new('Synthetic denial')
    }
    return [pscustomobject]@{ PSIsContainer=$true; FullName=$LiteralPath }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Directory, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    if ($Directory) {
        foreach ($name in @('Microsoft.Windows.ContentDeliveryManager_test', 'Microsoft.Windows.Photos_test', 'Microsoft.GamingApp_test', 'Microsoft.XboxApp_denied', 'Unrelated_test')) {
            [pscustomobject]@{ Name=$name; FullName=(Join-Path $LiteralPath $name) }
        }
    } elseif ($File) {
        [pscustomobject]@{ Name='sample.dat'; FullName=(Join-Path $LiteralPath 'sample.dat'); Length=1024; LastWriteTime=[datetime]'2026-01-01' }
        if ($script:scenario -eq 'partial-files') { throw [UnauthorizedAccessException]::new('Synthetic partial enumeration') }
    }
}
$script:scenario = 'empty'; $empty = Get-PhotosCacheInfo
if ($empty.partial -or $empty.total -ne 0) { throw 'Missing package root should be empty.' }
$script:scenario = 'denied'; $denied = Get-SpotlightCacheInfo
if (-not $denied.partial) { throw 'Denied root must report incomplete.' }
$script:scenario = 'partial'
$spotlight = Get-SpotlightCacheInfo; $photos = Get-PhotosCacheInfo; $xbox = Get-XboxCacheInfo
if ($spotlight.partial -or $spotlight.total -ne 1 -or $spotlight.files[0].path -notlike '*ContentDeliveryManager*sample.dat') { throw 'Spotlight path was lost.' }
if ($photos.partial -or $photos.total -ne 1 -or $photos.files[0].path -notlike '*Photos*sample.dat') { throw 'Photos path was lost.' }
if (-not $xbox.partial -or $xbox.total -ne 1 -or $xbox.files[0].path -notlike '*GamingApp*sample.dat') { throw 'Available Xbox data or partial status was lost.' }
$script:scenario = 'missing-one'; $missingOne = Get-XboxCacheInfo
if ($missingOne.partial -or $missingOne.total -ne 1) { throw 'An absent package cache must not make another package incomplete.' }
$script:scenario = 'partial-files'; $partialFiles = Get-PhotosCacheInfo
if (-not $partialFiles.partial -or $partialFiles.total -ne 1) { throw 'Files observed before an enumeration failure must remain visible.' }
Write-Output 'Passed: package caches retain paths and report inaccessible sources.'
