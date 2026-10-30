$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\..\src-tauri\wincmd-shared\scripts\auto-erase.ps1')
. ([scriptblock]::Create($script:ScheduledDockerFunctions))

function Assert-DockerRemainder($Condition, $Message) { if (-not $Condition) { throw $Message } }

# Provider calls are synthetic; no Docker disk or log is inspected.
$script:scenario = 'missing'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    $isRoot = $LiteralPath -like '*Docker\wsl'
    $isFixedDisk = $LiteralPath -like '*DockerDesktop.vhdx'
    if ($script:scenario -eq 'missing' -or ($script:scenario -eq 'disk-only' -and -not $isRoot) -or
        ($isFixedDisk -and $script:scenario -notin @('fixed-disks','denied-fixed'))) {
        throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path.')
    }
    if ($script:scenario -eq 'fixed-disks' -and -not $isFixedDisk) { throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path.') }
    if (($script:scenario -eq 'denied-root' -and $isRoot) -or ($script:scenario -eq 'denied-log' -and -not $isRoot -and -not $isFixedDisk) -or ($script:scenario -eq 'denied-fixed' -and $isFixedDisk)) {
        throw [UnauthorizedAccessException]::new('Synthetic access denial.')
    }
    $attributes = if (($script:scenario -eq 'reparse-root' -and $isRoot) -or ($script:scenario -eq 'reparse-log' -and -not $isRoot -and -not $isFixedDisk)) { [IO.FileAttributes]::ReparsePoint } else { [IO.FileAttributes]::Normal }
    [pscustomobject]@{ PSIsContainer = $isRoot; Attributes = $attributes; Name = if ($isRoot) { 'wsl' } else { 'log.txt' } }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'denied-contents') { throw [UnauthorizedAccessException]::new('Synthetic access denial.') }
    if ($script:scenario -eq 'reparse-entry') { return [pscustomobject]@{ Name='link'; PSIsContainer=$true; Attributes=[IO.FileAttributes]::ReparsePoint } }
    if ($script:scenario -in @('disk-and-log','disk-only')) { return [pscustomobject]@{ Name='fixture.vhdx'; PSIsContainer=$false; Attributes=[IO.FileAttributes]::Normal } }
    return @()
}

Assert-DockerRemainder ((Get-WCDockerArtifactRemainders) -eq 0) 'Missing Docker artifacts should be an empty inventory.'
$script:scenario = 'disk-and-log'
Assert-DockerRemainder ((Get-WCDockerArtifactRemainders) -eq 2) 'Both a disk image and a log must be counted.'
$script:scenario = 'disk-only'
Assert-DockerRemainder ((Get-WCDockerArtifactRemainders) -eq 1) 'An existing disk must be counted when the log is absent.'
$script:scenario = 'fixed-disks'
Assert-DockerRemainder ((Get-WCDockerArtifactRemainders) -eq 2) 'Documented Hyper-V and VMM disks must be counted.'
foreach ($scenario in @('denied-root','denied-log','denied-fixed','denied-contents','reparse-root','reparse-log','reparse-entry')) {
    $script:scenario = $scenario
    $failed = $false
    try { $null = Get-WCDockerArtifactRemainders } catch { $failed = $true }
    Assert-DockerRemainder $failed "Docker inventory must report an incomplete result for $scenario."
}
function Test-Path { param([string]$LiteralPath, [string]$PathType, $ErrorAction) return $true }
function Get-Process { param($ErrorAction) return @([pscustomobject]@{ ProcessName='unrelated' }) }
function New-WCDockerStartInfo {
    param([string]$Executable, [string]$ConfigPath, [string[]]$Arguments, [bool]$Mutation)
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $env:ComSpec; $start.Arguments = '/d /c exit 0'
    $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    return $start
}
$Error.Clear()
$null = Invoke-WCDocker -ConfigPath 'C:\Fixture' -Arguments @('info')
Assert-DockerRemainder ($Error.Count -eq 0) 'An absent Docker CLI process must not add a false PowerShell error.'
Write-Output 'Passed: Docker disk/log readback distinguishes absent, denied and reparse paths.'
