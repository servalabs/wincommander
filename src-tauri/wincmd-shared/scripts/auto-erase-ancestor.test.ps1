$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
. ([scriptblock]::Create($script:EraseFunctions))

# Synthetic provider records only; no real filesystem paths are touched.
$script:redirectedAncestor = $false
$script:deniedAncestor = $false
$script:missingRoot = $false
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:missingRoot -and $LiteralPath -ieq 'C:\Fixture\missing') { throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic missing path') }
    if ($script:deniedAncestor -and $LiteralPath -ieq 'C:\Fixture\redirect') { throw [UnauthorizedAccessException]::new('Synthetic denial') }
    if ($LiteralPath -like '*.txt') { return [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$false; Attributes=[IO.FileAttributes]::Normal; Length=0 } }
    $attributes = [IO.FileAttributes]::Directory
    if ($script:redirectedAncestor -and $LiteralPath -ieq 'C:\Fixture\redirect') {
        $attributes = $attributes -bor [IO.FileAttributes]::ReparsePoint
    }
    [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$true; Attributes=$attributes }
}
function Get-ChildItem {
    param([string]$LiteralPath, [string]$Filter, [switch]$File, [switch]$Directory, [switch]$Force, $ErrorAction)
    if ($File) { [pscustomobject]@{ FullName="$LiteralPath\sample.txt"; Attributes=[IO.FileAttributes]::Normal } }
}
$root = 'C:\Fixture\redirect\cache'
$normal = @(Get-AutoEraseFiles -Path $root)
if ($normal.Count -ne 1) { throw 'Normal cache inventory was not returned.' }
if (@(Get-AutoEraseFiles -Path 'C:\').Count -ne 1) { throw 'A normal drive root was rejected.' }
$script:missingRoot = $true
if (@(Get-AutoEraseFiles -Path 'C:\Fixture\missing').Count -ne 0) { throw 'A missing root did not return an empty inventory.' }
$script:missingRoot = $false
$script:redirectedAncestor = $true
$failed = $false
try { $null = @(Get-AutoEraseFiles -Path $root) } catch { $failed = $true }
if (-not $failed) { throw 'A redirected ancestor was accepted as a cleanup root.' }
$failed = $false
try { $null = @(Get-AutoEraseDirectories -Path $root) } catch { $failed = $true }
if (-not $failed) { throw 'A redirected ancestor was accepted as a directory inventory root.' }
function Rename-Item { throw 'A redirected file reached the rename step.' }
function Remove-Item { throw 'A redirected file reached the removal step.' }
$script:AutoEraseFailed = 0
if ((Erase-OneFile "$root\sample.txt") -or $script:AutoEraseFailed -ne 1) { throw 'A redirected file was not rejected.' }
$script:redirectedAncestor = $false; $script:deniedAncestor = $true
$failed = $false
try { $null = @(Get-AutoEraseFiles -Path $root) } catch { $failed = $true }
if (-not $failed) { throw 'An inaccessible ancestor was accepted as a cleanup root.' }
Write-Output 'Passed: scheduled cleanup rejects reparse-point ancestors.'
