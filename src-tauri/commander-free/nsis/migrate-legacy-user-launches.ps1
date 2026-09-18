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
    ForEach-Object { [Environment]::ExpandEnvironmentVariables([string]$_.ProfileImagePath) } |
    Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Container) } |
    ForEach-Object { [IO.Path]::GetFullPath($_) } |
    Sort-Object -Unique

$shell = New-Object -ComObject WScript.Shell
$summary = [ordered]@{ profiles = 0; shortcutsUpdated = 0; staleFilesRemoved = 0; failures = 0 }

foreach ($profile in $profiles) {
    $summary.profiles++
    $legacyRoot = Join-Path $profile 'AppData\Local\WinCommander'
    $legacyExe = Join-Path $legacyRoot 'wincommander-free.exe'
    $shortcutRoots = @(
        (Join-Path $profile 'Desktop'),
        (Join-Path $profile 'AppData\Roaming\Microsoft\Windows\Start Menu\Programs'),
        (Join-Path $profile 'AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup')
    )

    foreach ($root in $shortcutRoots) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
        try {
            Get-ChildItem -LiteralPath $root -Filter '*.lnk' -File -Recurse -Force -ErrorAction Stop | ForEach-Object {
                $shortcut = $shell.CreateShortcut($_.FullName)
                if ([string]::Equals($shortcut.TargetPath, $legacyExe, [StringComparison]::OrdinalIgnoreCase)) {
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
}

"WinCommander legacy launch migration: profiles=$($summary.profiles) shortcuts=$($summary.shortcutsUpdated) files=$($summary.staleFilesRemoved) failures=$($summary.failures)"
