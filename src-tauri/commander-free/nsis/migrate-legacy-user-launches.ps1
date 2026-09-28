[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$SharedExecutable
)

# This runs only from the elevated, per-machine NSIS installer. It deliberately
# changes launch records and old executable payloads only; per-user WinCommander
# settings, logs, caches, and stores remain private to their Windows profile.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$shared = [IO.Path]::GetFullPath($SharedExecutable)
if (-not (Test-Path -LiteralPath $shared -PathType Leaf)) {
    throw "The shared WinCommander executable is missing: $shared"
}

$profileList = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\*'
$profiles = Get-ItemProperty -Path $profileList -ErrorAction Stop |
    ForEach-Object {
        $path = [Environment]::ExpandEnvironmentVariables([string]$_.ProfileImagePath)
        if ($path -and (Test-Path -LiteralPath $path -PathType Container)) {
            [pscustomobject]@{
                Sid = [string]$_.PSChildName
                Path = [IO.Path]::GetFullPath($path)
            }
        }
    } |
    Sort-Object -Property Path -Unique

$shell = New-Object -ComObject WScript.Shell
$summary = [ordered]@{ profiles = 0; shortcutsUpdated = 0; startupShortcutsRemoved = 0; staleFilesRemoved = 0; failures = 0 }

foreach ($profile in $profiles) {
    $summary.profiles++
    $legacyRoot = Join-Path $profile.Path 'AppData\Local\WinCommander'
    $legacyExe = Join-Path $legacyRoot 'wincommander-free.exe'
    $shortcutRoots = @(
        (Join-Path $profile.Path 'Desktop'),
        (Join-Path $profile.Path 'AppData\Roaming\Microsoft\Windows\Start Menu\Programs'),
        (Join-Path $profile.Path 'AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup')
    )

    foreach ($root in $shortcutRoots) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
        try {
            Get-ChildItem -LiteralPath $root -Filter '*.lnk' -File -Recurse -Force -ErrorAction Stop | ForEach-Object {
                $shortcut = $shell.CreateShortcut($_.FullName)
                if ($root -like '*\Programs\Startup' -and
                    ([string]::Equals($shortcut.TargetPath, $legacyExe, [StringComparison]::OrdinalIgnoreCase) -or
                     [string]::Equals($shortcut.TargetPath, $shared, [StringComparison]::OrdinalIgnoreCase))) {
                    # Task Scheduler is now the one logon router. Do not
                    # retarget an old Startup-folder shortcut into a second
                    # route, but do not touch shortcuts to anything else.
                    Remove-Item -LiteralPath $_.FullName -Force -ErrorAction Stop
                    $summary.startupShortcutsRemoved++
                } elseif ([string]::Equals($shortcut.TargetPath, $legacyExe, [StringComparison]::OrdinalIgnoreCase)) {
                    $shortcut.TargetPath = $shared
                    $shortcut.WorkingDirectory = Split-Path -Parent $shared
                    $shortcut.IconLocation = "$shared,0"
                    $shortcut.Save()
                    $summary.shortcutsUpdated++
                }
            }
        } catch {
            # Continue with other profiles. Setup must not be blocked by a
            # profile Windows itself has made inaccessible or redirected.
            $summary.failures++
        }
    }

    foreach ($legacyFile in @($legacyExe, (Join-Path $legacyRoot 'uninstall.exe'))) {
        if (-not (Test-Path -LiteralPath $legacyFile -PathType Leaf)) { continue }
        try {
            Remove-Item -LiteralPath $legacyFile -Force -ErrorAction Stop
            $summary.staleFilesRemoved++
        } catch {
            $summary.failures++
        }
    }

    # These folders belong to the old per-user application payload. The shared
    # Program Files build has its own signed resources/scripts, while current
    # per-user settings and telemetry live in different paths (store, logs,
    # file-search, and the encrypted *.dat files) and are intentionally kept.
    foreach ($legacyDirectory in @('resources', 'scripts')) {
        $payloadPath = Join-Path $legacyRoot $legacyDirectory
        if (-not (Test-Path -LiteralPath $payloadPath -PathType Container)) { continue }
        try {
            Remove-Item -LiteralPath $payloadPath -Recurse -Force -ErrorAction Stop
            $summary.staleFilesRemoved++
        } catch {
            $summary.failures++
        }
    }

    # A loaded profile hive can retain the old per-user uninstall entry even
    # after its payload is gone. Remove that exact registration only; never
    # alter unrelated application registrations or user settings.
    $uninstallKey = "Registry::HKEY_USERS\$($profile.Sid)\Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander"
    if (Test-Path -LiteralPath $uninstallKey) {
        try {
            Remove-Item -LiteralPath $uninstallKey -Recurse -Force -ErrorAction Stop
        } catch {
            $summary.failures++
        }
    }
}

# A machine-wide Startup folder is also a second logon route. It normally only
# contains shortcuts to the shared Program Files payload, so remove exactly
# those WinCommander links and leave every other vendor's startup item alone.
$commonStartup = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Startup'
if (Test-Path -LiteralPath $commonStartup -PathType Container) {
    try {
        Get-ChildItem -LiteralPath $commonStartup -Filter '*.lnk' -File -Force -ErrorAction Stop | ForEach-Object {
            $shortcut = $shell.CreateShortcut($_.FullName)
            if ([string]::Equals($shortcut.TargetPath, $shared, [StringComparison]::OrdinalIgnoreCase)) {
                Remove-Item -LiteralPath $_.FullName -Force -ErrorAction Stop
                $summary.startupShortcutsRemoved++
            }
        }
    } catch {
        $summary.failures++
    }
}

"WinCommander legacy launch migration: profiles=$($summary.profiles) shortcuts=$($summary.shortcutsUpdated) startupShortcuts=$($summary.startupShortcutsRemoved) files=$($summary.staleFilesRemoved) failures=$($summary.failures)"
