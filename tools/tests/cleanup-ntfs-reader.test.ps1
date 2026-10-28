$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-NTFSJournals' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:mode = 'populated'; $script:queried = @()
function Get-CimInstance {
    [CmdletBinding()] param([string]$ClassName, [string]$Filter)
    if ($ClassName -ne 'Win32_Volume' -or $Filter -ne "FileSystem='NTFS' AND DriveType=3") { throw 'Incorrect journal volume scope.' }
    if ($script:mode -eq 'empty') { return }
    if ($script:mode -eq 'deniedVolumes') { throw [UnauthorizedAccessException]::new('Denied volume inventory') }
    [pscustomobject]@{ DriveLetter='C:' }
    [pscustomobject]@{ DriveLetter='D:' }
    [pscustomobject]@{ DriveLetter=$null }
}
function fsutil {
    $drive = $args[-1]
    $script:queried += $drive
    if ($script:mode -eq 'deniedQuery') { $global:LASTEXITCODE=5; return 'Access denied' }
    if ($script:mode -eq 'unknownOutput') { $global:LASTEXITCODE=0; return 'Unrecognized localized output' }
    if ($drive -eq 'D:') { $global:LASTEXITCODE=1179; return 'The volume change journal is not active.' }
    $global:LASTEXITCODE=0
    'Usn Journal ID : 0x1234'
    'Maximum Size : 0x2000000'
}
$result = Get-NTFSJournals
if ($result.error -or $result.total -ne 1 -or $result.journals.Count -ne 1 -or $result.scannedVolumeCount -ne 2 -or $result.inactiveVolumeCount -ne 1) { throw 'Journal records and volume coverage disagreed.' }
if ($result.journals[0].journalId -ne '0x1234' -or $result.journals[0].maxSize -ne '0x2000000') { throw 'Journal identity or hexadecimal size was decoded incorrectly.' }
foreach ($mode in @('deniedVolumes', 'deniedQuery', 'unknownOutput')) {
    $script:mode=$mode
    if (-not (Get-NTFSJournals).error) { throw "$mode was reported as a successful empty journal scan." }
}
$script:mode='empty'
$empty=Get-NTFSJournals
if ($empty.error -or $empty.total -ne 0) { throw 'No eligible volumes did not report zero journals.' }
Write-Output 'Passed: fixed NTFS scope; journal identity and hex size; inactive versus denied; unsupported format; empty inventory.'
