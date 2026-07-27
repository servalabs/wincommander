# ============================================================================
# WINDOWS AI CONTROL — FIXED COMMAND SURFACE
# ============================================================================

function Get-AIControlStatus {
    [pscustomobject]@{
        isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
        classicApps = [pscustomobject]@{
            photoViewer = Test-Path -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows Photo Viewer\Capabilities\FileAssociations'
            paint = Test-AIControlLegacyBinaryInstalled -App paint
            snipping = Test-AIControlLegacyBinaryInstalled -App snipping
            notepad = [bool](Get-WindowsCapability -Online -Name 'Microsoft.Windows.Notepad.System*' -ErrorAction SilentlyContinue | Where-Object State -eq 'Installed')
            photosLegacy = [bool](Get-AppxPackage -AllUsers -Name '*PhotosLegacy*' -ErrorAction SilentlyContinue)
        }
    }
}

function Invoke-AIControlOperation {
    param(
        [Parameter(Mandatory)]
        [ValidateSet(
            'restore-point', 'package-guard',
            'appx-packages', 'recall-feature', 'cbs-packages', 'ai-files',
            'scheduled-tasks', 'update-cleanup',
            'classic-photo-viewer', 'classic-paint',
            'classic-snipping', 'classic-notepad', 'photos-legacy'
        )]
        [string]$Operation,
        [ValidateSet('apply', 'revert')][string]$Mode = 'apply',
        [bool]$Backup = $true
    )
    Assert-AIControlAdmin
    switch ($Operation) {
        'restore-point' { New-AIControlRestorePoint }
        'package-guard' { Set-AIControlPackageGuard -Mode $Mode }
        'appx-packages' { Remove-AIControlAppxPackages -Mode $Mode -Backup $Backup }
        'recall-feature' { Remove-AIControlRecallFeature -Mode $Mode }
        'cbs-packages' { Remove-AIControlCbsPackages -Mode $Mode }
        'ai-files' { Remove-AIControlFiles -Mode $Mode -Backup $Backup }
        'scheduled-tasks' { Set-AIControlScheduledTasks -Mode $Mode }
        'update-cleanup' { Set-AIControlUpdateCleanup -Mode $Mode }
        'classic-photo-viewer' { Install-AIControlPhotoViewer }
        'classic-paint' { Install-AIControlLegacyBinary -App paint }
        'classic-snipping' { Install-AIControlLegacyBinary -App snipping }
        'classic-notepad' { Install-AIControlNotepad }
        'photos-legacy' { Install-AIControlPhotosLegacy }
    }
}
