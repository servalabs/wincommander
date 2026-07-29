# ============================================================================
# WINDOWS AI CONTROL — FIXED COMMAND SURFACE
# ============================================================================

function Get-AIControlStatus {
    [pscustomobject]@{
        isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    }
}

function Invoke-AIControlOperation {
    param(
        [Parameter(Mandatory)]
        [ValidateSet(
            'restore-point', 'package-guard',
            'appx-packages', 'recall-feature', 'cbs-packages', 'ai-files',
            'scheduled-tasks', 'update-cleanup'
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
    }
}
