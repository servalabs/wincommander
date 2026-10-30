$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
foreach ($name in @('Get-PCAInfo', 'Get-PrintSpoolerInfo', 'Get-CleanupArtifactFiles', 'Get-BITSQueueInfo', 'Get-FontCacheInfo')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing reader: $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:paths = @(); $script:filters = @(); $script:deny = $false
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath, [switch]$Force)
    if ($script:deny) { throw [UnauthorizedAccessException]::new('Denied') }
    if ($LiteralPath -eq 'C:\Absent') { throw [System.Management.Automation.ItemNotFoundException]::new('Missing') }
    [pscustomobject]@{ Name = 'mock directory'; FullName = [IO.Path]::GetFullPath($LiteralPath); PSIsContainer = $true }
}
function Get-ChildItem {
    [CmdletBinding()] param([string]$LiteralPath, [switch]$File, [switch]$Force, [switch]$Recurse, [string]$Filter)
    $script:paths += $LiteralPath; $script:filters += $Filter
    [pscustomobject]@{ Name='mock.dat'; FullName=(Join-Path $LiteralPath 'mock.dat'); Length=1024; LastWriteTime=[DateTime]'2026-10-01'; Extension='.dat' }
}
function Get-Service {
    [CmdletBinding()] param([Parameter(Position=0)][string]$Name)
    [pscustomobject]@{ Status='Running' }
}
$script:denyBits = $false
function Get-BitsTransfer {
    [CmdletBinding()] param([switch]$AllUsers)
    if ($script:denyBits) { throw [UnauthorizedAccessException]::new('Denied') }
    [pscustomobject]@{ DisplayName='Mock transfer'; JobState='Queued' }
}
$pca = Get-PCAInfo
if ($pca.error -or $pca.total -ne 1 -or $script:paths[0] -ne "$env:SystemRoot\appcompat\pca") { throw 'PCA reader did not scan its intended target.' }
$script:paths = @(); $script:filters = @()
$spool = Get-PrintSpoolerInfo
if ($spool.error -or $spool.total -ne 2 -or $script:paths[0] -ne "$env:SystemRoot\System32\spool\PRINTERS" -or $script:paths[1] -ne "$env:LOCALAPPDATA\Microsoft\Windows\Spool" -or $script:filters[1] -ne '*.spl') { throw 'Spool reader did not scan its intended targets.' }
$script:deny = $true
if (-not (Get-PCAInfo).error -or -not (Get-PrintSpoolerInfo).error) { throw 'A denied target was reported as a successful empty scan.' }
$script:deny = $false
$artifacts = @(Get-CleanupArtifactFiles -Sources @(@{ path='C:\Mock'; label='Mock'; patterns=@('*.dat') }))
if ($artifacts.Count -ne 1 -or $artifacts[0].name -ne 'Mock: mock.dat' -or $artifacts[0].path -ne 'C:\Mock\mock.dat') { throw 'Artifact reader lost the record metadata.' }
$missing = @(Get-CleanupArtifactFiles -Sources @(@{ path='C:\Absent'; label='Mock'; patterns=@('*') }))
if ($missing.Count -ne 0) { throw 'Missing artifact folder was not empty.' }
$script:deny = $true
$blocked = $false
try { Get-CleanupArtifactFiles -Sources @(@{ path='C:\Mock'; label='Mock'; patterns=@('*') }) } catch [UnauthorizedAccessException] { $blocked = $true }
if (-not $blocked) { throw 'Denied artifact traversal was treated as empty.' }
$script:deny = $false
if ((Get-BITSQueueInfo).total -ne 2) { throw 'BITS job and store counts were incorrect.' }
$script:denyBits = $true
$bits = Get-BITSQueueInfo
if (-not $bits.partial -or $bits.status -ne 'partial' -or $bits.total -ne 1) { throw 'An incomplete all-user BITS scan was reported as complete or lost its readable store.' }
$script:denyBits = $false; $script:deny = $false; $script:paths = @(); $script:filters = @()
$font = Get-FontCacheInfo
$fontRoot = Join-Path $env:WINDIR 'ServiceProfiles\LocalService\AppData\Local\FontCache'
if ($font.error -or $font.total -ne 1 -or $script:paths.Count -ne 1 -or $script:paths[0] -ne $fontRoot -or $script:filters[0] -ne '*.dat') {
    throw "Font cache reader included installed user fonts or an unapproved target: total=$($font.total) error=$($font.error) paths=$($script:paths -join ',') expected=$fontRoot filters=$($script:filters -join ',')."
}
$script:deny = $true
$font = Get-FontCacheInfo
if (-not $font.partial -or $font.status -ne 'partial') { throw 'Denied font-cache enumeration was reported as clean.' }
Write-Output 'Passed: PCA, spool and font-cache targets, counts, and denied-read behavior.'
