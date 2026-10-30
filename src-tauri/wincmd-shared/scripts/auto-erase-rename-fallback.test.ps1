$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
. ([scriptblock]::Create($script:EraseFunctions))

# All paths and operations are synthetic; no file is created or removed.
$script:present = $true
function Assert-AutoEraseAncestors { param($Path) }
function Get-Item {
    [CmdletBinding()] param($LiteralPath, [switch]$Force)
    if (-not $script:present) { throw [System.Management.Automation.ItemNotFoundException]::new('synthetic absent item') }
    [pscustomobject]@{ FullName=$LiteralPath; Length=0; PSIsContainer=$false; Attributes=0 }
}
function Rename-Item { [CmdletBinding()] param($LiteralPath, $NewName, [switch]$Force) throw 'synthetic optional rename failure' }
function Remove-Item { [CmdletBinding()] param($LiteralPath, [switch]$Force) $script:present = $false }

$Error.Clear()
$result = Erase-OneFile 'C:\synthetic\item.txt'
if (-not $result -or $script:AutoEraseRemoved -ne 1 -or $script:AutoEraseFailed -ne 0) { throw 'The successful fallback did not report removal.' }
if ($Error.Count -ne 0) { throw 'A handled rename failure incorrectly marks the generated task incomplete.' }
$script:present = $true
Write-Error 'synthetic earlier failure' -ErrorAction SilentlyContinue
$result = Erase-OneFile 'C:\synthetic\item.txt'
if (-not $result -or $Error.Count -ne 1 -or $Error[0].Exception.Message -ne 'synthetic earlier failure') { throw 'The fallback must not discard an earlier error.' }
Write-Output 'Passed: a successful delete fallback does not retain the optional rename error.'
