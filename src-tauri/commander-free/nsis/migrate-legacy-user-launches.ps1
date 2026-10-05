[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$SharedExecutable,

    # A full uninstall must not retarget shortcuts to an executable that is
    # about to be removed or delete profile data.
    [switch]$Uninstall
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Only old launch payloads/routes are touched. Per-profile store, logs,
# file-search, caches, and encrypted *.dat settings remain user-owned.

$shared = [IO.Path]::GetFullPath($SharedExecutable)
if (-not (Test-Path -LiteralPath $shared -PathType Leaf)) {
    throw "The shared WinCommander executable is missing: $shared"
}

$canonicalRunValueNames = @('WinCommander', 'WinCommander Free', 'WinCommander Pro')
$runValueNames = @($canonicalRunValueNames) + @($canonicalRunValueNames | ForEach-Object { "${_}__SystemCache"; "${_}__WC_Hidden" })
$systemProfileSids = @('S-1-5-18', 'S-1-5-19', 'S-1-5-20')
$shell = New-Object -ComObject WScript.Shell
$summary = [ordered]@{ profiles = 0; runValuesRemoved = 0; startupShortcutsRemoved = 0; shortcutsUpdated = 0; staleFilesRemoved = 0; registryHivesDeferred = 0; failures = 0 }
$failureMessages = [System.Collections.Generic.List[string]]::new()

function Get-OptionalRegistryValue([string]$Path, [string]$Name) {
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $key = Get-Item -LiteralPath $Path -ErrorAction Stop
    try {
        # REG_EXPAND_SZ must be resolved for the profile being migrated, not
        # for the administrator who supplied credentials to setup.
        return $key.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    } finally { $key.Close() }
}

function Expand-ProfileEnvironment([string]$Value, [string]$ProfilePath) {
    if (-not [string]::IsNullOrWhiteSpace($ProfilePath)) {
        $variables = @{
            USERPROFILE = $ProfilePath
            LOCALAPPDATA = Join-Path $ProfilePath 'AppData\Local'
            APPDATA = Join-Path $ProfilePath 'AppData\Roaming'
        }
        $Value = [regex]::Replace($Value, '(?i)%(USERPROFILE|LOCALAPPDATA|APPDATA)%', {
            param($match)
            return $variables[$match.Groups[1].Value]
        })
    }
    return [Environment]::ExpandEnvironmentVariables($Value)
}

function Test-OwnedExecutablePath([AllowNull()][string]$Path, [string[]]$OwnedPaths, [string]$ProfilePath = '') {
    if ([string]::IsNullOrWhiteSpace($Path)) { return $false }
    try {
        $resolved = [IO.Path]::GetFullPath((Expand-ProfileEnvironment $Path $ProfilePath))
        return @($OwnedPaths | Where-Object { [string]::Equals($_, $resolved, [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0
    } catch {
        return $false
    }
}

function Test-OwnedExecutableCommand([AllowNull()][string]$Command, [string[]]$OwnedPaths, [string]$ProfilePath = '') {
    if ([string]::IsNullOrWhiteSpace($Command)) { return $false }
    $expanded = (Expand-ProfileEnvironment $Command $ProfilePath).Trim()
    $match = [regex]::Match($expanded, '^\s*(?:"(?<path>[^"]+)"|(?<path>[^\s]+))(?=\s|$)')
    return $match.Success -and (Test-OwnedExecutablePath $match.Groups['path'].Value $OwnedPaths $ProfilePath)
}

function Remove-OwnedRunValues([string[]]$Paths, [string[]]$OwnedPaths, [string]$ProfilePath = '') {
    $removed = 0
    foreach ($path in $Paths) {
        foreach ($name in $runValueNames) {
            $value = Get-OptionalRegistryValue $path $name
            if ($null -ne $value -and (Test-OwnedExecutableCommand ([string]$value) $OwnedPaths $ProfilePath)) {
                Remove-ItemProperty -LiteralPath $path -Name $name -ErrorAction Stop
                $removed++
            }
        }
    }
    return $removed
}

function Remove-OwnedStartupShortcuts([string]$Root, [string[]]$OwnedPaths, [string]$ProfilePath = '') {
    if (-not (Test-Path -LiteralPath $Root -PathType Container)) { return 0 }
    $removed = 0
    Get-ChildItem -LiteralPath $Root -Filter '*.lnk' -File -Recurse -Force -ErrorAction Stop | ForEach-Object {
        $shortcut = $shell.CreateShortcut($_.FullName)
        if (Test-OwnedExecutablePath $shortcut.TargetPath $OwnedPaths $ProfilePath) {
            Remove-Item -LiteralPath $_.FullName -Force -ErrorAction Stop
            $removed++
        }
    }
    return $removed
}

function Update-LegacyShortcuts([string]$Root, [string]$StartupRoot, [string[]]$LegacyExecutables, [string]$ProfilePath = '') {
    if (-not (Test-Path -LiteralPath $Root -PathType Container)) { return 0 }
    $updated = 0
    Get-ChildItem -LiteralPath $Root -Filter '*.lnk' -File -Recurse -Force -ErrorAction Stop | ForEach-Object {
        if (-not $_.FullName.StartsWith($StartupRoot, [StringComparison]::OrdinalIgnoreCase)) {
            $shortcut = $shell.CreateShortcut($_.FullName)
            if (Test-OwnedExecutablePath $shortcut.TargetPath $LegacyExecutables $ProfilePath) {
                $shortcut.TargetPath = $shared
                $shortcut.WorkingDirectory = Split-Path -Parent $shared
                $shortcut.IconLocation = "$shared,0"
                $shortcut.Save()
                $updated++
            }
        }
    }
    return $updated
}

function Test-ProfileHiveUnavailable([object]$Failure) {
    # A different account's NTUSER.DAT can be actively in use or protected
    # from this elevated installer. Do not take ownership or weaken its ACL
    # merely to clean an optional legacy launch value. The account's packaged
    # app cleans its own exact owned routes at its next normal launch.
    $exception = $null
    if ($Failure -is [System.Management.Automation.ErrorRecord]) {
        $exception = $Failure.Exception
    } elseif ($Failure -is [System.Exception]) {
        $exception = $Failure
    }
    while ($null -ne $exception) {
        if ($exception -is [System.UnauthorizedAccessException] -or $exception.HResult -in @(-2147024891, -2147024864)) {
            return $true
        }
        $exception = $exception.InnerException
    }
    # Some registry-provider and reg.exe errors have no usable HRESULT. These
    # are the two normal forms for a foreign hive that is unavailable now.
    return ([string]$Failure) -match '(?i)\baccess\s+(is\s+)?denied\b|\bsharing\s+violation\b|\bbeing\s+used\s+by\s+another\s+process\b'
}

function Invoke-ProfileHive($Profile, [scriptblock]$Action) {
    $sid = [string]$Profile.Sid
    $loadedRoot = "Registry::HKEY_USERS\$sid"
    try {
        if (Test-Path -LiteralPath $loadedRoot -ErrorAction Stop) {
            try {
                $null = & $Action $loadedRoot
                return $true
            } catch {
                if (Test-ProfileHiveUnavailable $_) { return $false }
                throw
            }
        }
    } catch {
        if (Test-ProfileHiveUnavailable $_) { return $false }
        throw
    }

    # Offline hive mounts can outlive a failed installer and block every retry.
    # Only use canonical Windows-loaded SID roots. Leave old temporary mounts
    # untouched; this account's normal app launch cleans its own Run values.
    return $false
}

try {
    $profiles = @(
        Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\*' -ErrorAction Stop |
            ForEach-Object {
                $sid = [string]$_.PSChildName
                $path = [Environment]::ExpandEnvironmentVariables([string]$_.ProfileImagePath)
                if ($sid -and $sid -notin $systemProfileSids -and $path -and (Test-Path -LiteralPath $path -PathType Container)) {
                    [pscustomobject]@{ Sid = $sid; Path = [IO.Path]::GetFullPath($path) }
                }
            } |
            Sort-Object -Property Path -Unique
    )
} catch {
    throw "Could not enumerate Windows user profiles for WinCommander launch cleanup: $($_.Exception.Message)"
}

$allOwnedPaths = @($shared)
foreach ($profile in $profiles) {
    $allOwnedPaths += Join-Path $profile.Path 'AppData\Local\WinCommander\wincommander-free.exe'
    $allOwnedPaths += Join-Path $profile.Path 'AppData\Local\Programs\WinCommander\wincommander-free.exe'
}
$allOwnedPaths = @($allOwnedPaths | ForEach-Object { [IO.Path]::GetFullPath($_) } | Sort-Object -Unique)

foreach ($profile in $profiles) {
    $summary.profiles++
    $legacyRoots = @(
        (Join-Path $profile.Path 'AppData\Local\WinCommander'),
        (Join-Path $profile.Path 'AppData\Local\Programs\WinCommander')
    )
    $legacyExecutables = @($legacyRoots | ForEach-Object { Join-Path $_ 'wincommander-free.exe' })
    $ownedPaths = @($shared) + $legacyExecutables
    $startupRoot = Join-Path $profile.Path 'AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup'

    try {
        $summary.startupShortcutsRemoved += Remove-OwnedStartupShortcuts $startupRoot $ownedPaths $profile.Path
        if (-not $Uninstall) {
            $summary.shortcutsUpdated += Update-LegacyShortcuts (Join-Path $profile.Path 'Desktop') $startupRoot $legacyExecutables $profile.Path
            $summary.shortcutsUpdated += Update-LegacyShortcuts (Join-Path $profile.Path 'AppData\Roaming\Microsoft\Windows\Start Menu\Programs') $startupRoot $legacyExecutables $profile.Path
        }
    } catch {
        $summary.failures++
        $failureMessages.Add("shortcuts for $($profile.Sid): $($_.Exception.Message)")
    }

    try {
        $registryHiveCleaned = Invoke-ProfileHive $profile {
            param($hiveRoot)
            $runPaths = @(
                "$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Run",
                "$hiveRoot\Software\Microsoft\Windows\CurrentVersion\RunOnce",
                "$hiveRoot\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
                "$hiveRoot\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce"
            )
            $summary.runValuesRemoved += Remove-OwnedRunValues $runPaths $ownedPaths $profile.Path
            $uninstallKey = "$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander"
            if (Test-Path -LiteralPath $uninstallKey) {
                Remove-Item -LiteralPath $uninstallKey -Recurse -Force -ErrorAction Stop
            }
        }
        if (-not $registryHiveCleaned) {
            # This is an optional migration for another account's protected
            # hive, not a reason to leave the new machine-wide app uninstalled.
            # We never take ownership or edit an inaccessible user profile.
            $summary.registryHivesDeferred++
        }
    } catch {
        $summary.failures++
        $failureMessages.Add("registry for $($profile.Sid): $($_.Exception.Message)")
    }

    # The old payload must not survive an explicit uninstall either. These are
    # exact product files/directories only; no profile root or user data is
    # removed here.
    foreach ($legacyRoot in $legacyRoots) {
        foreach ($legacyFile in @((Join-Path $legacyRoot 'wincommander-free.exe'), (Join-Path $legacyRoot 'uninstall.exe'))) {
            if (-not (Test-Path -LiteralPath $legacyFile -PathType Leaf)) { continue }
            try {
                Remove-Item -LiteralPath $legacyFile -Force -ErrorAction Stop
                $summary.staleFilesRemoved++
            } catch {
                $summary.failures++
                $failureMessages.Add("payload ${legacyFile}: $($_.Exception.Message)")
            }
        }
        foreach ($legacyDirectory in @('resources', 'scripts')) {
            $payloadPath = Join-Path $legacyRoot $legacyDirectory
            if (-not (Test-Path -LiteralPath $payloadPath -PathType Container)) { continue }
            try {
                Remove-Item -LiteralPath $payloadPath -Recurse -Force -ErrorAction Stop
                $summary.staleFilesRemoved++
            } catch {
                $summary.failures++
                $failureMessages.Add("payload ${payloadPath}: $($_.Exception.Message)")
            }
        }
    }
}

$commonStartup = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Startup'
try {
    $summary.startupShortcutsRemoved += Remove-OwnedStartupShortcuts $commonStartup $allOwnedPaths
} catch {
    $summary.failures++
    $failureMessages.Add("common Startup folder: $($_.Exception.Message)")
}

if ($summary.failures -gt 0) {
    throw "WinCommander launch-route cleanup failed: $($failureMessages -join '; ')"
}

"WinCommander launch-route cleanup: profiles=$($summary.profiles) runValues=$($summary.runValuesRemoved) startupShortcuts=$($summary.startupShortcutsRemoved) shortcuts=$($summary.shortcutsUpdated) files=$($summary.staleFilesRemoved) deferredRegistryHives=$($summary.registryHivesDeferred)"
