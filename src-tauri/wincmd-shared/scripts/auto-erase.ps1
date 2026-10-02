# ============================================================================
# SHARED AUTO-ERASE SCHEDULER MODULE
# ============================================================================
#
# Single source of truth for the per-card auto-erase scheduler that powers
# the Privacy Clean panel's clock icon. Both commander-free and
# commander-pro embed this file verbatim via `include_str!` so there is
# exactly one place to add a new schedulable category.
#
# Architecture
# ------------
# Each schedulable privacy-clean category has a small inline clear script in
# the `$script:AutoEraseScripts` hashtable below. `Set-AutoEraseSchedule`
# looks up the script by categoryId, writes it beneath an ACL-hardened
# ProgramData scripts directory, and registers a
# Windows Scheduled Task with a coded name and stable per-user SID scope.
#
# Why inline (not call back into commander.exe)
# ---------------------------------------------
# Commander's PS modules are decrypted from the binary at runtime and
# don't exist as files on disk, so a scheduled task can't dot-source them.
# Inline scripts are self-contained, inspectable in Task Scheduler GUI,
# and survive even if Commander is uninstalled (user can clean up via
# Remove-AutoEraseSchedule or Task Scheduler).
#
# Naming
# ------
# `SL-SW-<code>` for SYSTEM; `SL-UW-<code>-<SID>` for a user. Legacy
# WinCommander_AutoErase/System_AutoErase names are migrated after ownership
# validation. The task action and description remain inspectable and attributed.

# Self-contained single-pass durable secure-erase functions prepended to every
# scheduled task script (NIST SP 800-88: one RNG pass clears magnetic media;
# flash storage defeats extra passes via FTL indirection regardless of count —
# matches the media-aware default used by Invoke-7Erase in core/utils.ps1).
# Scheduled tasks run as isolated powershell.exe invocations with no access to
# Commander's encrypted modules, so the erase logic must be fully embedded here.
$script:EraseFunctions = @'
# Per-run erase tallies — the scheduled-task wrapper reads these to write a
# truthful removed/failed/deferred result instead of a silent "done".
$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:AutoEraseDeferred = 0
# MoveFileEx (defined once, guarded) powers the delete-on-reboot fallback for a
# file still locked by a running holder. MOVEFILE_DELAY_UNTIL_REBOOT = 0x4;
# needs admin (writes HKLM PendingFileRenameOperations) and degrades to a
# counted failure when unelevated.
if (-not ([System.Management.Automation.PSTypeName]'WcAutoErase.Native').Type) {
    try { Add-Type -Namespace WcAutoErase -Name Native -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError=true, CharSet=System.Runtime.InteropServices.CharSet.Unicode)] public static extern bool MoveFileEx(string a, string b, int dwFlags);' } catch {}
}
function Erase-OneFile($p) {
    $f = Get-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue
    if (-not $f -or $f.PSIsContainer) { return $false }
    $z = $f.Length
    if ($z -gt 0) {
        $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
        $s = $null
        try {
            try { Set-ItemProperty -LiteralPath $p -Name IsReadOnly -Value $false -ErrorAction SilentlyContinue } catch {}
            $s = [System.IO.File]::Open($p, 'Open', 'Write', 'None')
            $b = New-Object byte[] 65536
            $w = 0
            while ($w -lt $z) {
                $n = [Math]::Min($b.Length, $z - $w)
                $rng.GetBytes($b)
                $s.Write($b, 0, $n)
                $w += $n
            }
            $s.Position = 0
            # Flush($true) = FlushFileBuffers so the overwrite bytes actually
            # reach the disk before the GUID-rename + delete; a bare .Flush()
            # only empties the .NET buffer to the OS cache.
            $s.Flush($true)
        } catch {} finally {
            if ($s) { try { $s.Close(); $s.Dispose() } catch {} }
            try { $rng.Dispose() } catch {}
        }
    }
    $g = [Guid]::NewGuid().ToString()
    $par = Split-Path $p
    $renamed = $false
    try { Rename-Item -LiteralPath $p -NewName $g -Force -ErrorAction Stop; $renamed = $true } catch {}
    $target = if ($renamed) { Join-Path $par $g } else { $p }
    Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
    if (-not (Test-Path -LiteralPath $target)) { $script:AutoEraseRemoved++; return $true }
    # Still on disk — a running process holds a handle. Fall back to a
    # delete-on-next-reboot so the trace is not left behind indefinitely.
    try {
        if (('WcAutoErase.Native' -as [type]) -and [WcAutoErase.Native]::MoveFileEx($target, $null, 4)) {
            $script:AutoEraseDeferred++; return $false
        }
    } catch {}
    $script:AutoEraseFailed++
    return $false
}
function Erase-Dir($d) {
    if (-not (Test-Path -LiteralPath $d)) { return }
    try { (Get-Item -LiteralPath $d -Force -ErrorAction SilentlyContinue).Attributes = [System.IO.FileAttributes]::Normal } catch {}
    Get-ChildItem -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object {
        try { $_.Attributes = [System.IO.FileAttributes]::Normal } catch {}
    }
    Get-ChildItem -LiteralPath $d -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile $_.FullName }
    Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue
}
'@

$script:AutoEraseScripts = @{
    # Machine maintenance -------------------------------------------------
    # This is intentionally a SYSTEM task rather than a per-user cleanup
    # task.  It is created together with the bulk "Auto-set scheduled wipes"
    # switch, so closing the Cleanup panel cannot stop it.  ReTrim is only
    # issued for volumes backed by an SSD/NVMe disk; HDDs are never defragged
    # by this privacy-clean schedule.
    'maintenanceTrim'  = @'
Get-Volume -ErrorAction SilentlyContinue | Where-Object {
    $_.DriveLetter -and $_.DriveType -eq 'Fixed'
} | ForEach-Object {
    $drive = [string]$_.DriveLetter
    try {
        $partition = Get-Partition -DriveLetter $drive -ErrorAction Stop
        $disk = Get-Disk -Number $partition.DiskNumber -ErrorAction Stop
        $physical = Get-PhysicalDisk -ErrorAction SilentlyContinue |
            Where-Object { $_.DeviceId -eq $disk.Number } |
            Select-Object -First 1
        if ($physical -and $physical.MediaType -in @('SSD', 'NVMe')) {
            Optimize-Volume -DriveLetter $drive -ReTrim -ErrorAction Stop
        }
    } catch {}
}
'@

    # Standard categories ----------------------------------------------
    'clipboard'         = "Set-Clipboard -Value `$null -ErrorAction SilentlyContinue; Get-Service cbdhsvc_* -ErrorAction SilentlyContinue | Restart-Service -Force -ErrorAction SilentlyContinue"
    'rdpHistory'        = @"
`$default = 'HKCU:\Software\Microsoft\Terminal Server Client\Default'
if (Test-Path `$default) {
    (Get-Item `$default).Property | Where-Object { `$_ -like 'MR*' } | ForEach-Object {
        Remove-ItemProperty -Path `$default -Name `$_ -ErrorAction SilentlyContinue
    }
}
'HKCU:\Software\Microsoft\Terminal Server Client\Servers',
'HKCU:\Software\Microsoft\Terminal Server Client\LocalDevices' | ForEach-Object {
    if (Test-Path `$_) { Remove-Item `$_ -Recurse -Force -ErrorAction SilentlyContinue; New-Item `$_ -Force | Out-Null }
}
Erase-OneFile (Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'Default.rdp')
Erase-OneFile "`$env:APPDATA\Microsoft\Windows\Recent\AutomaticDestinations\1b4dd67f29cb1962.automaticDestinations-ms"
Get-ChildItem -Path "`$env:LOCALAPPDATA\Microsoft\Terminal Server Client\Cache" -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
cmdkey /list 2>`$null | Select-String 'Target: Domain:target=TERMSRV/' | ForEach-Object {
    `$t = `$_.ToString().Split('=')[1]; cmdkey /delete:`$t 2>`$null | Out-Null
}
"@
    'eventLogs'         = "Get-WinEvent -ListLog * -Force -ErrorAction SilentlyContinue | Where-Object { `$_.RecordCount -gt 0 } | ForEach-Object { try { [System.Diagnostics.Eventing.Reader.EventLogSession]::GlobalSession.ClearLog(`$_.LogName) } catch {} }"
    'recentFiles'       = @"
Get-ChildItem -Path "`$env:APPDATA\Microsoft\Windows\Recent" -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
Get-ChildItem -Path "`$env:APPDATA\Microsoft\Windows\Recent" -Directory -Force -ErrorAction SilentlyContinue | Where-Object { `$_.Name -notin 'AutomaticDestinations','CustomDestinations' } | ForEach-Object { Erase-Dir `$_.FullName }
Get-ChildItem -Path "`$env:APPDATA\Microsoft\Office\Recent" -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
"@
    'jumpLists'         = @"
Get-ChildItem -Path "`$env:APPDATA\Microsoft\Windows\Recent\AutomaticDestinations" -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
Get-ChildItem -Path "`$env:APPDATA\Microsoft\Windows\Recent\CustomDestinations" -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
"@
    'psHistory'         = @"
`$histPath = (Get-PSReadlineOption -ErrorAction SilentlyContinue).HistorySavePath
if (`$histPath -and (Test-Path `$histPath)) { Erase-OneFile `$histPath }
`$fallback = Join-Path `$env:APPDATA 'Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt'
if (Test-Path `$fallback) { Erase-OneFile `$fallback }
"@
    'dnsCache'          = "ipconfig /flushdns | Out-Null"
    'browserFootprints' = @"
`$chromiumRoots = @(
    "`$env:LOCALAPPDATA\Google\Chrome\User Data",
    "`$env:LOCALAPPDATA\Microsoft\Edge\User Data",
    "`$env:LOCALAPPDATA\BraveSoftware\Brave-Browser\User Data",
    "`$env:APPDATA\Opera Software\Opera Stable",
    "`$env:LOCALAPPDATA\Vivaldi\User Data"
)
`$chromiumArtifacts = @(
    'History','History-journal','Cookies','Cookies-journal',
    'Login Data','Login Data-journal','Top Sites','Top Sites-journal',
    'Favicons','Favicons-journal','Web Data','Web Data-journal',
    'Visited Links','Shortcuts','Shortcuts-journal','Media History','Media History-journal'
)
foreach (`$root in `$chromiumRoots) {
    if (-not (Test-Path `$root)) { continue }
    Get-ChildItem `$root -Directory -ErrorAction SilentlyContinue | Where-Object { `$_.Name -eq 'Default' -or `$_.Name -like 'Profile*' } | ForEach-Object {
        `$prof = `$_.FullName
        foreach (`$a in `$chromiumArtifacts) {
            `$p = Join-Path `$prof `$a
            if (Test-Path `$p -PathType Leaf) { Erase-OneFile `$p }
            elseif (Test-Path `$p -PathType Container) { Erase-Dir `$p }
        }
    }
}
`$geckoRoots = @("`$env:APPDATA\Mozilla\Firefox\Profiles", "`$env:LOCALAPPDATA\LibreWolf\Profiles")
`$geckoArtifacts = @(
    'places.sqlite','places.sqlite-journal','places.sqlite-wal','places.sqlite-shm',
    'cookies.sqlite','cookies.sqlite-journal','cookies.sqlite-wal','cookies.sqlite-shm',
    'logins.json','formhistory.sqlite','favicons.sqlite','favicons.sqlite-journal'
)
foreach (`$root in `$geckoRoots) {
    if (-not (Test-Path `$root)) { continue }
    Get-ChildItem `$root -Directory -ErrorAction SilentlyContinue | ForEach-Object {
        `$prof = `$_.FullName
        foreach (`$a in `$geckoArtifacts) {
            `$p = Join-Path `$prof `$a
            if (Test-Path `$p -PathType Leaf) { Erase-OneFile `$p }
        }
    }
}
"@
    'prefetchFiles'     = @"
Get-ChildItem -Path "`$env:SystemRoot\Prefetch" -Filter '*.pf' -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
"@
    'shellBags'         = @"
'HKCU:\Software\Microsoft\Windows\Shell\Bags',
'HKCU:\Software\Microsoft\Windows\Shell\BagMRU',
'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags',
'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU' | ForEach-Object {
    if (Test-Path `$_) { Remove-Item -Path `$_ -Recurse -Force -ErrorAction SilentlyContinue }
}
"@
    'usbHistory'        = @"
`$presentUsbStorageIds = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
`$pnpReady = `$false
try {
    `$presentDevices = @(Get-PnpDevice -PresentOnly:`$true -ErrorAction Stop)
    if (`$presentDevices.Count -eq 0) { throw 'PnP returned no present devices.' }
    foreach (`$device in `$presentDevices) {
        `$instanceId = [string]`$device.InstanceId
        if (`$instanceId.StartsWith('USBSTOR\', [StringComparison]::OrdinalIgnoreCase)) {
            [void]`$presentUsbStorageIds.Add(`$instanceId)
        }
    }
    `$pnpReady = `$true
} catch {
    `$script:AutoEraseFailed++
}

if (`$pnpReady) {
    `$candidates = @()
    `$registryReady = `$false
    try {
        `$regRoot = 'HKLM:\SYSTEM\CurrentControlSet\Enum\USBSTOR'
        if (Test-Path -LiteralPath `$regRoot -ErrorAction Stop) {
            foreach (`$deviceClass in @(Get-ChildItem -LiteralPath `$regRoot -ErrorAction Stop)) {
                foreach (`$instance in @(Get-ChildItem -LiteralPath `$deviceClass.PSPath -ErrorAction Stop)) {
                    `$instanceId = "USBSTOR\`$(`$deviceClass.PSChildName)\`$(`$instance.PSChildName)"
                    if (-not `$presentUsbStorageIds.Contains(`$instanceId)) {
                        `$candidates += `$instanceId
                    }
                }
            }
        }
        `$registryReady = `$true
    } catch {
        `$script:AutoEraseFailed++
    }

    # Windows protects USBSTOR history keys, so PnP removes only stale storage
    # nodes. Refresh the read-only snapshot before each removal; generic USB,
    # HID, printer and portable-device IDs are never candidates.
    if (`$registryReady) {
        foreach (`$candidate in `$candidates) {
            `$id = [string]`$candidate
            if (-not `$id.StartsWith('USBSTOR\', [StringComparison]::OrdinalIgnoreCase)) {
                `$script:AutoEraseFailed++
                continue
            }
            try {
                `$presentDevices = @(Get-PnpDevice -PresentOnly:`$true -ErrorAction Stop)
                if (`$presentDevices.Count -eq 0) { throw 'PnP returned no present devices.' }
                `$presentUsbStorageIds.Clear()
                foreach (`$device in `$presentDevices) {
                    `$instanceId = [string]`$device.InstanceId
                    if (`$instanceId.StartsWith('USBSTOR\', [StringComparison]::OrdinalIgnoreCase)) {
                        [void]`$presentUsbStorageIds.Add(`$instanceId)
                    }
                }
            } catch {
                `$script:AutoEraseFailed++
                `$pnpReady = `$false
                break
            }
            if (`$presentUsbStorageIds.Contains(`$id)) { continue }
            `$result = & pnputil.exe /remove-device `$id 2>&1 | Out-String
            if (`$result -match 'removed successfully') { `$script:AutoEraseRemoved++ }
            else { `$script:AutoEraseFailed++ }
        }
    }
}
"@
    'execCache'         = @"
# KT: AppCompatCache is a registry VALUE, not a key with subkeys -- Get-ChildItem on it
# returns nothing, making a Remove-Item -Recurse loop a silent no-op. Clear the value directly.
`$shimPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager'
if (Test-Path `$shimPath -ErrorAction SilentlyContinue) {
    Remove-ItemProperty -Path `$shimPath -Name 'AppCompatCache' -ErrorAction SilentlyContinue
}

# These are all per-user keys, so HKCU resolves to the scheduled task's target
# account (including each S4U task created by Set-MultiUserAutoEraseSchedule).
`$userAssist = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist'
if (Test-Path `$userAssist -ErrorAction SilentlyContinue) {
    Get-ChildItem `$userAssist -ErrorAction SilentlyContinue | ForEach-Object {
        `$countPath = Join-Path `$_.PSPath 'Count'
        if (Test-Path `$countPath -ErrorAction SilentlyContinue) {
            Remove-Item -Path `$countPath -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
`$muiCache = 'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache'
if (Test-Path `$muiCache -ErrorAction SilentlyContinue) {
    Remove-Item -Path `$muiCache -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -Path `$muiCache -Force -ErrorAction SilentlyContinue | Out-Null
}
`$recentApps = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Search\RecentApps'
if (Test-Path `$recentApps -ErrorAction SilentlyContinue) {
    Get-ChildItem `$recentApps -ErrorAction SilentlyContinue | ForEach-Object {
        Remove-Item -Path `$_.PSPath -Recurse -Force -ErrorAction SilentlyContinue
    }
}
foreach (`$pcaPath in @(
    'HKCU:\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store',
    'HKCU:\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Persisted'
)) {
    if (Test-Path `$pcaPath -ErrorAction SilentlyContinue) {
        Remove-Item -Path `$pcaPath -Recurse -Force -ErrorAction SilentlyContinue
    }
}
"@
    'wlanProfiles'      = @"
netsh wlan show profiles 2>`$null | Where-Object { `$_ -match 'All User Profile\s*:\s*(.+)' } | ForEach-Object { `$n = `$Matches[1].Trim(); netsh wlan delete profile name="`$n" 2>`$null | Out-Null }
Get-ChildItem -Path "`$env:ProgramData\Microsoft\Wlansvc\Profiles\Interfaces" -Recurse -Filter '*.xml' -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
"@
    'netDrives'         = "Get-PSDrive -PSProvider FileSystem -ErrorAction SilentlyContinue | Where-Object { `$_.DisplayRoot -like '\\\\*' } | ForEach-Object { Remove-PSDrive -Name `$_.Name -Force -ErrorAction SilentlyContinue }; net use * /delete /yes 2>`$null | Out-Null"
    'ntfsJournals'      = "`$fsTool = 'fs' + 'util'; Get-Volume -ErrorAction SilentlyContinue | Where-Object { `$_.DriveLetter -and `$_.FileSystem -eq 'NTFS' } | ForEach-Object { & `$fsTool usn deletejournal /d (`$_.DriveLetter + ':') 2>`$null | Out-Null }"
    'recycleBin'        = @"
`$drives = [System.IO.DriveInfo]::GetDrives() | Where-Object { `$_.DriveType -in 'Fixed','Removable' -and `$_.IsReady }
foreach (`$drv in `$drives) {
    `$binRoot = Join-Path `$drv.RootDirectory.FullName '`$Recycle.Bin'
    if (Test-Path -LiteralPath `$binRoot) {
        Get-ChildItem -LiteralPath `$binRoot -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
            Get-ChildItem -LiteralPath `$_.FullName -Force -ErrorAction SilentlyContinue |
                Where-Object { `$_.Name -like '`$I*' -or `$_.Name -like '`$R*' } |
                ForEach-Object {
                    if (`$_.PSIsContainer) { Erase-Dir `$_.FullName } else { Erase-OneFile `$_.FullName | Out-Null }
                }
        }
    }
    # Refresh the shell's Recycle Bin state only after recoverable content and
    # metadata have been overwritten. Calling this first would unlink the files
    # before Erase-OneFile could reach them.
    try { Clear-RecycleBin -DriveLetter `$drv.Name.TrimEnd('\').TrimEnd(':') -Force -ErrorAction SilentlyContinue } catch {}
}
"@

    # Deep trace analysis categories --------------------------------------------------
    'ntUserTraces'      = @"
'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU',
'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths',
'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\OpenSavePidlMRU',
'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\LastVisitedPidlMRU',
'HKCU:\Software\Microsoft\Office\16.0\Common\Internet\LocationsMRU' | ForEach-Object {
    if (Test-Path `$_ -ErrorAction SilentlyContinue) {
        # These histories are registry values, not child keys.
        Remove-Item -Path `$_ -Recurse -Force -ErrorAction SilentlyContinue
    }
}
"@
    'notepadState'      = @"
Get-ChildItem -Path "`$env:LOCALAPPDATA\Packages" -Filter 'Microsoft.WindowsNotepad_*' -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
    `$tabState = Join-Path `$_.FullName 'LocalState\TabState'
    if (Test-Path `$tabState) {
        Get-ChildItem -Path `$tabState -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
    }
}
"@
    'pcaDatabase'       = @"
Get-ChildItem -Path "`$env:SystemRoot\appcompat\pca" -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
"@
    'crashDumps'        = @"
`$targets = @(
    "`$env:LOCALAPPDATA\CrashDumps",
    "`$env:SystemRoot\Minidump",
    "`$env:ProgramData\Microsoft\Windows\WER\ReportArchive",
    "`$env:ProgramData\Microsoft\Windows\WER\ReportQueue",
    "`$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportArchive",
    "`$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportQueue"
)
foreach (`$t in `$targets) {
    if (Test-Path `$t) {
        Get-ChildItem -LiteralPath `$t -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
        Get-ChildItem -LiteralPath `$t -Recurse -Directory -Force -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | ForEach-Object { Remove-Item -LiteralPath `$_.FullName -Force -ErrorAction SilentlyContinue }
    }
}
`$memdmp = "`$env:SystemRoot\MEMORY.DMP"
if (Test-Path `$memdmp) { Erase-OneFile `$memdmp }
"@
    'walFiles'          = @"
Get-ChildItem -Path `$env:APPDATA, `$env:LOCALAPPDATA -Include '*.db-wal','*.db-shm','*.sqlite-wal','*.sqlite-shm' -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
"@
    'printSpooler'      = @"
Stop-Service Spooler -Force -ErrorAction SilentlyContinue
Get-ChildItem -Path "`$env:SystemRoot\System32\spool\PRINTERS" -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
Start-Service Spooler -ErrorAction SilentlyContinue
"@
    'webCache'          = @"
`$webCacheDir = "`$env:LOCALAPPDATA\Microsoft\Windows\WebCache"
if (Test-Path `$webCacheDir) {
    Get-ChildItem -Path (Join-Path `$webCacheDir 'WebCacheV*.dat') -Force -ErrorAction SilentlyContinue | ForEach-Object {
        try { Erase-OneFile `$_.FullName } catch {}
    }
    Get-ChildItem -Path (Join-Path `$webCacheDir 'V01*.log') -Force -ErrorAction SilentlyContinue | ForEach-Object {
        try { Erase-OneFile `$_.FullName } catch {}
    }
}
"@
    'thumbnailDb'       = @"
`$thumbDir = "`$env:LOCALAPPDATA\Microsoft\Windows\Explorer"
Get-ChildItem -LiteralPath `$thumbDir -Filter 'thumbcache_*.db' -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
Get-ChildItem -LiteralPath `$thumbDir -Filter 'iconcache_*.db' -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
"@
    'notificationDb'    = @"
Get-Service -Name 'WpnUserService*' -ErrorAction SilentlyContinue | Stop-Service -Force -ErrorAction SilentlyContinue
`$notifDir = "`$env:LOCALAPPDATA\Microsoft\Windows\Notifications"
'wpndatabase.db','wpndatabase.db-wal','wpndatabase.db-shm' | ForEach-Object {
    `$p = Join-Path `$notifDir `$_
    if (Test-Path `$p) { try { Erase-OneFile `$p } catch {} }
}
`$wpnidmDir = Join-Path `$notifDir 'wpnidm'
if (Test-Path `$wpnidmDir) {
    Get-ChildItem -Path `$wpnidmDir -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
}
Get-Service -Name 'WpnUserService*' -ErrorAction SilentlyContinue | Start-Service -ErrorAction SilentlyContinue
"@
    'branchCache'       = @"
netsh branchcache flush 2>`$null | Out-Null
`$targets = @(
    "`$env:SystemRoot\System32\PeerDistRep",
    "`$env:SystemRoot\ServiceProfiles\NetworkService\AppData\Local\PeerDistPub"
)
foreach (`$t in `$targets) {
    if (Test-Path `$t) {
        Get-ChildItem -LiteralPath `$t -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
    }
}
"@

    # Phase E system diagnostic / servicing / log categories -----------------
    'eventTranscript'    = @"
Get-Service DiagTrack -ErrorAction SilentlyContinue | Stop-Service -Force -ErrorAction SilentlyContinue
`$etDir = "`$env:ProgramData\Microsoft\Diagnosis\EventTranscript"
if (Test-Path `$etDir) {
    Get-ChildItem -Path (Join-Path `$etDir 'EventTranscript.db') -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
    Get-ChildItem -Path (Join-Path `$etDir '*.jrs') -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
    Get-ChildItem -Path (Join-Path `$etDir '*.rbs') -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
}
`$etlDir = "`$env:ProgramData\Microsoft\Diagnosis\ETLLogs"
if (Test-Path `$etlDir) {
    Get-ChildItem -Path `$etlDir -Recurse -Filter '*.etl' -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
}
Get-Service DiagTrack -ErrorAction SilentlyContinue | Start-Service -ErrorAction SilentlyContinue
"@
    'activitiesTimeline' = @"
`$cdpDir = "`$env:LOCALAPPDATA\ConnectedDevicesPlatform"
if (Test-Path `$cdpDir) {
    Get-ChildItem -Path `$cdpDir -Include 'ActivitiesCache.db*' -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
}
"@
    'rdpBitmapCache'     = @"
`$rdpCacheDir = "`$env:LOCALAPPDATA\Microsoft\Terminal Server Client\Cache"
if (Test-Path `$rdpCacheDir) {
    Get-ChildItem -Path (Join-Path `$rdpCacheDir '*.bin') -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
    Get-ChildItem -Path (Join-Path `$rdpCacheDir '*.bmc') -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
}
"@
    'servicingLogs'      = @"
`$targets = @(
    "`$env:SystemRoot\Logs\CBS",
    "`$env:SystemRoot\Logs\DISM"
)
foreach (`$t in `$targets) {
    if (Test-Path `$t) {
        Get-ChildItem -LiteralPath `$t -File -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
    }
}
"@
    'deviceInstallLogs'  = @"
`$infDir = "`$env:SystemRoot\INF"
if (Test-Path `$infDir) {
    Get-ChildItem -Path (Join-Path `$infDir 'setupapi.*.log') -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
}
"@
    'usageTraceLogs'     = @"
`$targets = @(
    "`$env:SystemRoot\System32\SleepStudy",
    "`$env:SystemRoot\System32\WDI\LogFiles",
    "`$env:SystemRoot\System32\LogFiles\WMI"
)
foreach (`$t in `$targets) {
    if (Test-Path `$t) {
        Get-ChildItem -Path (Join-Path `$t '*.etl') -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
    }
}
"@
    'defenderHistory'    = @"
`$historyDir = "`$env:ProgramData\Microsoft\Windows Defender\Scans\History\Service"
if (Test-Path `$historyDir) {
    Get-ChildItem -LiteralPath `$historyDir -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
}
`$mpLog = "`$env:ProgramData\Microsoft\Windows Defender\Support\MpCmdRun.log"
if (Test-Path `$mpLog) { Erase-OneFile `$mpLog }
"@
    # Firewall packet logs are operational records, not firewall policy.  The
    # firewall service opens a fresh log after removal, so this is safe to run
    # periodically under SYSTEM without changing rules or connectivity.
    'firewallLog'        = @"
foreach (`$profile in @(Get-NetFirewallProfile -ErrorAction SilentlyContinue)) {
    `$path = [string]`$profile.LogFileName
    if (`$path -and (Test-Path -LiteralPath `$path -PathType Leaf -ErrorAction SilentlyContinue)) {
        Erase-OneFile `$path | Out-Null
    }
}
"@

    # Extended app-usage / office / web-cache / P2P update categories --------
    'appLaunchHistory'   = @"
`$bamRoot = 'HKLM:\SYSTEM\CurrentControlSet\Services\bam\State\UserSettings'
if (Test-Path `$bamRoot) {
    Get-ChildItem -Path `$bamRoot -ErrorAction SilentlyContinue | ForEach-Object {
        `$sidKeyPath = `$_.PSPath
        try {
            `$sidKey = Get-Item -Path `$sidKeyPath -ErrorAction SilentlyContinue
            if (`$sidKey) {
                foreach (`$valueName in @(`$sidKey.Property)) {
                    Remove-ItemProperty -Path `$sidKeyPath -Name `$valueName -ErrorAction SilentlyContinue
                }
            }
        } catch {}
    }
}
"@
    'officeMru'          = @"
`$officePatterns = @(
    'HKCU:\Software\Microsoft\Office\*\*\File MRU',
    'HKCU:\Software\Microsoft\Office\*\*\Place MRU',
    'HKCU:\Software\Microsoft\Office\*\*\Security\Trusted Documents\TrustRecords'
)
foreach (`$pattern in `$officePatterns) {
    Get-Item -Path `$pattern -ErrorAction SilentlyContinue | ForEach-Object {
        try { Remove-Item -Path `$_.PSPath -Recurse -Force -ErrorAction SilentlyContinue } catch {}
    }
}
"@
    'embeddedWebCache'   = @"
Get-ChildItem -Path "`$env:LOCALAPPDATA" -Recurse -Directory -Filter 'EBWebView' -Force -Depth 4 -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
"@
    'p2pUpdateCache'     = @"
Stop-Service DoSvc -Force -ErrorAction SilentlyContinue
`$doDir = "`$env:SystemRoot\SoftwareDistribution\DeliveryOptimization"
if (Test-Path `$doDir) {
    Get-ChildItem -Path `$doDir -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
}
Start-Service DoSvc -ErrorAction SilentlyContinue
"@
    'reliabilityHistory' = @"
`$racDir = "`$env:ProgramData\Microsoft\RAC\StateData"
if (Test-Path `$racDir) {
    Get-ChildItem -LiteralPath `$racDir -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
}
"@
    'explorerSearchHistory' = @"
foreach (`$k in @(
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths'
)) {
    if (Test-Path `$k) {
        `$props = Get-ItemProperty -Path `$k -ErrorAction SilentlyContinue
        if (`$props) {
            `$props.PSObject.Properties | Where-Object { `$_.Name -notin @('PSPath','PSParentPath','PSChildName','PSDrive','PSProvider') } | ForEach-Object {
                try { Remove-ItemProperty -Path `$k -Name `$_.Name -ErrorAction SilentlyContinue } catch {}
            }
        }
    }
}
"@
    'searchPersonalization' = @"
foreach (`$key in @(
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Search\JumplistData',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Search\Launch'
)) {
    if (-not (Test-Path `$key)) { continue }
    `$item = Get-Item -LiteralPath `$key -ErrorAction SilentlyContinue
    if (`$item) {
        foreach (`$name in @(`$item.Property)) {
            Remove-ItemProperty -LiteralPath `$key -Name `$name -Force -ErrorAction SilentlyContinue
        }
    }
    Get-ChildItem -LiteralPath `$key -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
}
`$trainedDataStore = "`$env:LOCALAPPDATA\Microsoft\InputPersonalization\TrainedDataStore"
if (Test-Path `$trainedDataStore) {
    Get-ChildItem -LiteralPath `$trainedDataStore -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { Erase-OneFile `$_.FullName }
}
"@

    # Disk cleanup — runs Windows cleanmgr against the standard maintenance
    # category set. Self-contained so it works as a scheduled task without
    # access to Commander's PS modules.
    'diskCleanup'       = @"
`$sageRunId = 100
`$volPath = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches'
`$flagName = "StateFlags`$('{0:D4}' -f `$sageRunId)"
`$cats = @(
    'Active Setup Temp Folders', 'Downloaded Program Files', 'Internet Cache Files',
    'Old ChkDsk Files', 'Recycle Bin', 'Setup Log Files',
    'System error memory dump files', 'System error minidump files',
    'Temporary Files', 'Thumbnail Cache',
    'Windows Error Reporting Archive Files', 'Windows Error Reporting Queue Files',
    'Windows Upgrade Log Files'
)
foreach (`$c in `$cats) {
    `$k = Join-Path `$volPath `$c
    if (Test-Path `$k) { Set-ItemProperty -Path `$k -Name `$flagName -Value 2 -Type DWord -EA SilentlyContinue }
}
Start-Process cleanmgr.exe -ArgumentList "/sagerun:`$sageRunId" -Wait -NoNewWindow -EA SilentlyContinue
"@
}

# Public: the set of categoryIds this scheduler accepts. Frontend's
# Privacy Clean panel hardcodes the same set in privacyCleanCategories.ts
# (SUPPORTED_AUTOERASE_IDS) — keep them in sync.
function Get-AutoEraseSupportedCategories {
    @{ categories = @($script:AutoEraseScripts.Keys | Sort-Object) }
}

# Build a scheduled-task argument string from a erase script block.
#
# The full script (erase functions + catch-up wrapper + category body) is
# written to a small per-category file under ProgramData and invoked with
# -File, rather than embedded inline via -EncodedCommand. Task Scheduler
# happily *stores* an arbitrarily long Action Argument, but when the task
# actually launches, Windows builds one CreateProcess command line out of
# Execute+Argument — and that has a hard ~32K-character ceiling. Cross it
# and the task fails to start with a bare "The filename or extension is
# too long" (os error 206). Writing the script to disk keeps the task's
# command line short and constant-size no matter how large a category's
# erase logic grows. Mirrors the -File pattern already used by
# autostart.rs / attend_watch.rs.
function ConvertTo-AutoEraseSafeScope {
    param([string]$Value)
    if (-not $Value) { return 'default' }
    $safe = $Value -replace '[^A-Za-z0-9._-]', '_'
    if ($safe.Length -gt 80) { $safe = $safe.Substring(0, 80) }
    if (-not $safe) { return 'default' }
    $safe
}

function Set-AutoEraseDirectoryAcl {
    param(
        [Parameter(Mandatory)] [string]$Path,
        [bool]$UsersMayWrite = $false
    )
    [System.IO.Directory]::CreateDirectory($Path) | Out-Null
    $acl = New-Object System.Security.AccessControl.DirectorySecurity
    $acl.SetAccessRuleProtection($true, $false)
    $inherit = [System.Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
    $propagation = [System.Security.AccessControl.PropagationFlags]::None
    $allow = [System.Security.AccessControl.AccessControlType]::Allow
    $systemSid = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-18')
    $adminsSid = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
    $usersSid = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-545')
    $acl.SetOwner($adminsSid)
    $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($systemSid, 'FullControl', $inherit, $propagation, $allow))
    $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($adminsSid, 'FullControl', $inherit, $propagation, $allow))
    $userRights = if ($UsersMayWrite) { 'Modify' } else { 'ReadAndExecute' }
    $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($usersSid, $userRights, $inherit, $propagation, $allow))
    Set-Acl -LiteralPath $Path -AclObject $acl -ErrorAction Stop
}

function Initialize-AutoEraseStorage {
    $base = Join-Path $env:ProgramData 'WinCommander\auto-erase'
    $scripts = Join-Path $base 'scripts'
    $state = Join-Path $base 'state'
    # The task payload may execute as SYSTEM/highest. Its directory must never
    # inherit a permissive WinCommander ProgramData ACL. Standard users receive
    # read/execute only; their writable state lives inside their own LocalAppData.
    Set-AutoEraseDirectoryAcl -Path $base -UsersMayWrite $false
    Set-AutoEraseDirectoryAcl -Path $scripts -UsersMayWrite $false
    Set-AutoEraseDirectoryAcl -Path $state -UsersMayWrite $false
    @{ base = $base; scripts = $scripts; state = $state }
}

function ConvertTo-AutoEraseTaskArgument {
    param(
        [string]$CategoryId,
        [int]$IntervalMinutes,
        [string]$Script,
        [string]$ExecutionScope = 'default',
        [string]$FirstDueUtc,
        [int]$PayloadRevision = 0
    )
    # Every scheduled task invocation goes through a small queue/catch-up wrapper:
    #   - one global mutex makes categories run sequentially instead of all at once
    #   - a last-run marker makes boot/logon triggers catch up after sleep/offline
    #     time while skipping duplicate runs shortly after a normal interval run.
    $categoryLiteral = $CategoryId.Replace("'", "''")
    $scopeKey = ConvertTo-AutoEraseSafeScope $ExecutionScope
    $scopeLiteral = $scopeKey.Replace("'", "''")
    $wrapper = @"
`$ErrorActionPreference = 'SilentlyContinue'
`$categoryId = '$categoryLiteral'
`$scopeKey = '$scopeLiteral'
`$intervalMinutes = $IntervalMinutes
`$firstDueUtc = '$FirstDueUtc'
`$stateRoot = if (`$scopeKey -eq 'system') {
    Join-Path `$env:ProgramData 'WinCommander\auto-erase\state'
} else {
    Join-Path `$env:LOCALAPPDATA 'WinCommander\auto-erase'
}
New-Item -ItemType Directory -Path `$stateRoot -Force | Out-Null
`$marker = Join-Path `$stateRoot ("`$categoryId.`$scopeKey.state.json")
`$legacyMarker = Join-Path `$stateRoot ("`$categoryId.`$scopeKey.last")
`$now = Get-Date
`$lastSuccess = `$null
`$nextDue = [DateTime]::Parse(`$firstDueUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind)
if (Test-Path -LiteralPath `$marker) {
    try {
        `$state = Get-Content -LiteralPath `$marker -Raw | ConvertFrom-Json
        if (`$state.lastSuccessfulUtc) { `$lastSuccess = [DateTime]::Parse([string]`$state.lastSuccessfulUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind) }
        elseif (`$state.nextDueUtc) { `$nextDue = [DateTime]::Parse([string]`$state.nextDueUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind) }
    } catch {}
} elseif (Test-Path -LiteralPath `$legacyMarker) {
    try { `$lastSuccess = [DateTime]::Parse(([System.IO.File]::ReadAllText(`$legacyMarker).Trim()), [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind) } catch {}
}
if (`$lastSuccess) { `$nextDue = `$lastSuccess.ToUniversalTime().AddMinutes(`$intervalMinutes) }
`$due = (`$now.ToUniversalTime() -ge `$nextDue.ToUniversalTime())
if (`$due) {
    `$mutex = New-Object System.Threading.Mutex(`$false, 'Global\WinCommanderAutoEraseQueue')
    `$hasLock = `$false
    try {
        `$hasLock = `$mutex.WaitOne()
        if (`$hasLock) {
            `$now = Get-Date
            if (Test-Path -LiteralPath `$marker) {
                try {
                    `$state = Get-Content -LiteralPath `$marker -Raw | ConvertFrom-Json
                    if (`$state.lastSuccessfulUtc) { `$lastSuccess = [DateTime]::Parse([string]`$state.lastSuccessfulUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind) }
                    elseif (`$state.nextDueUtc) { `$nextDue = [DateTime]::Parse([string]`$state.nextDueUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind) }
                } catch {}
            }
            if (`$lastSuccess) { `$nextDue = `$lastSuccess.ToUniversalTime().AddMinutes(`$intervalMinutes) }
            `$due = (`$now.ToUniversalTime() -ge `$nextDue.ToUniversalTime())
            if (`$due) {
__AUTO_ERASE_BODY__
                # Record a truthful outcome (not just "it ran"): how many files
                # were actually removed, how many are queued for reboot, and how
                # many could not be cleared. Consumed by the UI/telemetry so a
                # clear can never silently report success while leaving traces.
                try {
                    `$success = (`$script:AutoEraseFailed -eq 0)
                    `$completedUtc = (Get-Date).ToUniversalTime()
                    if (`$success) { `$lastSuccess = `$completedUtc }
                    `$nextDue = if (`$lastSuccess) { `$lastSuccess.AddMinutes(`$intervalMinutes) } else { `$completedUtc.AddMinutes(`$intervalMinutes) }
                    `$res = @{ attemptedUtc = `$completedUtc.ToString('o'); lastSuccessfulUtc = if (`$lastSuccess) { `$lastSuccess.ToString('o') } else { `$null }; nextDueUtc = `$nextDue.ToString('o'); intervalMinutes = `$intervalMinutes; success = `$success; removed = `$script:AutoEraseRemoved; deferred = `$script:AutoEraseDeferred; failed = `$script:AutoEraseFailed }
                    [System.IO.File]::WriteAllText((Join-Path `$stateRoot ("`$categoryId.`$scopeKey.result.json")), (`$res | ConvertTo-Json -Compress))
                    [System.IO.File]::WriteAllText(`$marker, (`$res | ConvertTo-Json -Compress))
                } catch {}
            }
        }
    } finally {
        if (`$hasLock) { try { `$mutex.ReleaseMutex() | Out-Null } catch {} }
        if (`$mutex) { `$mutex.Dispose() }
    }
}
"@
    # Always prepend the single-pass erase functions so every scheduled task script
    # has Erase-OneFile / Erase-Dir available regardless of which category it serves.
    $fullScript = $script:EraseFunctions + "`n" + $wrapper.Replace('__AUTO_ERASE_BODY__', $Script)

    $storage = Initialize-AutoEraseStorage
    $revisionSuffix = if ($PayloadRevision -gt 0) { ".r$PayloadRevision" } else { '' }
    $scriptPath = Join-Path $storage.scripts "$CategoryId.$scopeKey$revisionSuffix.ps1"
    # UTF8 (with BOM) so Windows PowerShell 5.1 reads it back correctly via -File.
    [System.IO.File]::WriteAllText($scriptPath, $fullScript, [System.Text.Encoding]::UTF8)

    "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$scriptPath`""
}

# Internal: admin check. Free's privacy/cleanup module already defines
# Assert-IsAdmin via core/utils, but Pro embeds this file standalone, so
# we ship a self-contained admin check that works in either context.
function Assert-AutoEraseAdmin {
    $current = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]$current
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Administrator privileges required for auto-erase scheduling"
    }
}

function Resolve-AutoEraseSid {
    param([string]$Account)
    if ($Account -in @('SYSTEM', 'NT AUTHORITY\SYSTEM', 'S-1-5-18')) { return 'S-1-5-18' }
    if ($Account -match '^S-1-\d+(?:-\d+)+$') { return $Account }
    ([Security.Principal.NTAccount]::new($Account)).Translate([Security.Principal.SecurityIdentifier]).Value
}

# Scheduler names are intentionally compact and live in one owned folder.
# The mapping is documented in wincommander-pro/docs/engineering/SCHEDULED-TASK-MAP.md.
$script:SystemMaintenanceTaskPath = '\System Maintenance\'
$script:SystemMaintenanceTaskFolderPath = '\System Maintenance'
$script:AutoEraseCategoryCodes = [ordered]@{
    maintenanceTrim = '01'; clipboard = '02'; rdpHistory = '03'; eventLogs = '04'
    recentFiles = '05'; jumpLists = '06'; psHistory = '07'; dnsCache = '08'
    browserFootprints = '09'; prefetchFiles = '10'; shellBags = '11'; usbHistory = '12'
    execCache = '13'; wlanProfiles = '14'; netDrives = '15'; ntfsJournals = '16'
    recycleBin = '17'; ntUserTraces = '18'; notepadState = '19'; pcaDatabase = '20'
    crashDumps = '21'; walFiles = '22'; printSpooler = '23'; webCache = '24'
    thumbnailDb = '25'; notificationDb = '26'; branchCache = '27'; eventTranscript = '28'
    activitiesTimeline = '29'; rdpBitmapCache = '30'; servicingLogs = '31'; deviceInstallLogs = '32'
    usageTraceLogs = '33'; defenderHistory = '34'; firewallLog = '35'; appLaunchHistory = '36'
    officeMru = '37'; embeddedWebCache = '38'; p2pUpdateCache = '39'; reliabilityHistory = '40'
    explorerSearchHistory = '41'; searchPersonalization = '42'; diskCleanup = '43'
}

function Get-AutoEraseCategoryCode {
    param([string]$CategoryId)
    $code = $script:AutoEraseCategoryCodes[$CategoryId]
    if (-not $code) { throw "Unsupported auto-erase category '$CategoryId'" }
    $code
}

function Get-AutoEraseOwnerCode {
    param([string]$OwnerSid)
    $hash = [Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($OwnerSid)))).Replace('-', '').Substring(0, 8) }
    finally { $hash.Dispose() }
}

function Initialize-SystemMaintenanceTaskFolder {
    try {
        $service = New-Object -ComObject Schedule.Service
        $service.Connect()
        $root = $service.GetFolder('\')
        try { [void]$service.GetFolder($script:SystemMaintenanceTaskFolderPath) }
        catch { [void]$root.CreateFolder('System Maintenance', $null) }
    } catch {
        throw "Windows could not prepare the System Maintenance task folder: $($_.Exception.Message)"
    }
}

function Get-AutoEraseTaskName {
    param([string]$CategoryId, [string]$OwnerSid)
    $code = Get-AutoEraseCategoryCode $CategoryId
    if ($OwnerSid -eq 'S-1-5-18') { return "SM-S-$code" }
    "SM-U-$code-$(Get-AutoEraseOwnerCode $OwnerSid)"
}

# Name alone is never ownership proof: require a supported category and our
# protected script location (or a recognizable historical encoded payload).
function Get-AutoEraseTaskIdentity {
    param($Task)
    if ($Task.TaskPath -notin @('\', $script:SystemMaintenanceTaskPath)) { return $null }
    $name = [string]$Task.TaskName
    $category = $null
    $oldNames = @{ WinCommander_ClipboardErase = 'clipboard'; WinCommander_RDPErase = 'rdpHistory'; WinCommander_EventLogErase = 'eventLogs' }
    if ($oldNames.ContainsKey($name)) { $category = $oldNames[$name] }
    foreach ($candidate in $script:AutoEraseScripts.Keys) {
        $legacyHash = Get-AutoEraseOwnerCode $candidate
        if ($name -match ('^(?:WinCommander|System)_AutoErase_' + [regex]::Escape($candidate) + '(?:_.+)?$') -or
            $name -match ('^SL-(?:SW|UW)-' + $legacyHash + '(?:-S-1-\d+(?:-\d+)+)?$') -or
            $name -match ('^SM-(?:S|U)-' + (Get-AutoEraseCategoryCode $candidate) + '(?:-[A-F0-9]{8})?$')) {
            $category = $candidate; break
        }
    }
    if (-not $category -or @($Task.Actions).Count -ne 1) { return $null }
    $action = @($Task.Actions)[0]
    if ([IO.Path]::GetFileName([string]$action.Execute) -notin @('powershell.exe', 'pwsh.exe')) { return $null }
    $arguments = [string]$action.Arguments
    $owned = $false
    if ($arguments -match '(?i)(?:^|\s)-File\s+"([^"]+)"\s*$') {
        $path = [IO.Path]::GetFullPath($Matches[1])
        $root = [IO.Path]::GetFullPath((Join-Path $env:ProgramData 'WinCommander\auto-erase\scripts'))
        $owned = [IO.Path]::GetDirectoryName($path) -eq $root -and
            [IO.Path]::GetFileName($path) -match ('^' + [regex]::Escape($category) + '(?:\.[A-Za-z0-9._-]+)?\.ps1$')
    } elseif ($arguments -match '(?i)(?:^|\s)-EncodedCommand\s+([A-Za-z0-9+/=]+)\s*$') {
        try {
            $payload = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($Matches[1]))
            $owned = $payload.Contains('function Erase-OneFile') -and
                ($payload.Contains('WinCommander') -or $payload.Contains($script:AutoEraseScripts[$category]))
        } catch { return $null }
    }
    if (-not $owned) { return $null }
    $sid = Resolve-AutoEraseSid ([string]$Task.Principal.UserId)
    $canonical = Get-AutoEraseTaskName $category $sid
    if (($name.StartsWith('SL-') -or $name.StartsWith('SM-')) -and $name -ne $canonical -and $Task.TaskPath -eq $script:SystemMaintenanceTaskPath) { return $null }
    @{ categoryId = $category; ownerSid = $sid; taskName = $canonical }
}

function Set-AutoEraseSchedule {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string]$CategoryId,
        [Parameter(Mandatory)] [int]$IntervalMinutes,
        [bool]$RunAsSystem = $false,
        # TargetUser: which account should own this task. Defaults to the
        # currently logged-in user. Pass a different username to create a
        # per-user task for multi-user mode. Ignored when RunAsSystem=$true.
        [string]$TargetUser = $env:USERNAME,
        # Bulk scheduling uses this switch so a pre-existing task is reported
        # instead of silently replacing a schedule the user set by hand.  The
        # per-card editor intentionally leaves it off after its own confirmation.
        [switch]$PreserveExisting,
        # Marks schedules created by the bulk toolbar so its second click can
        # remove only those tasks, never a schedule chosen per-card.
        [switch]$ManagedByAutoSet
    )
    Assert-AutoEraseAdmin

    if (-not $script:AutoEraseScripts.ContainsKey($CategoryId)) {
        return @{ error = $true; message = "Category '$CategoryId' is not supported for auto-erase" }
    }
    if ($IntervalMinutes -lt 1) {
        return @{ error = $true; message = "IntervalMinutes must be >= 1" }
    }

    try {
        $ownerSid = if ($RunAsSystem) { 'S-1-5-18' } else { Resolve-AutoEraseSid $TargetUser }
        $migration = Invoke-AutoEraseMigration -CategoryId $CategoryId
        if ($migration.error) { throw $migration.message }
        Initialize-SystemMaintenanceTaskFolder
        $taskName = Get-AutoEraseTaskName $CategoryId $ownerSid
        $existing = Get-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $taskName -ErrorAction SilentlyContinue
        if ($existing -and -not (Get-AutoEraseTaskIdentity $existing)) {
            throw 'A task with this name belongs to another application. It was left unchanged.'
        }

        if ($PreserveExisting -and $existing) {
            return @{
                status          = 'alreadyConfigured'
                categoryId      = $CategoryId
                taskName        = $taskName
                intervalMinutes = $IntervalMinutes
                runAsSystem     = [bool]$RunAsSystem
                targetUser      = $TargetUser
            }
        }

        $eraseScript = $script:AutoEraseScripts[$CategoryId]
        $scope = if ($RunAsSystem) { 'system' } else { "user-$TargetUser" }
        $firstDueUtc = [DateTime]::UtcNow.AddMinutes($IntervalMinutes).ToString('o')
        $argument = ConvertTo-AutoEraseTaskArgument -CategoryId $CategoryId -IntervalMinutes $IntervalMinutes -Script $eraseScript -ExecutionScope $scope -FirstDueUtc $firstDueUtc

        $action  = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $argument

        # Two triggers, registered together:
        #   1. Time-based repetition — the primary firing path during a
        #      running session. -Once+RepetitionInterval is the standard
        #      "every N minutes forever" pattern. Avoid
        #      [TimeSpan]::MaxValue on RepetitionDuration — some Windows
        #      builds silently reject it. 9999 days = effectively forever.
        #   2. Boot/logon trigger — guarantees the task fires shortly
        #      after every reboot. Empirically the time-based repetition
        #      does NOT always re-engage after reboot when the original
        #      -At time is in the past; the boot/logon trigger fixes that
        #      without depending on Windows' (buggy) StartWhenAvailable
        #      catch-up semantics. Uses -AtStartup for SYSTEM tasks (boot
        #      time, no user session needed) and -AtLogOn for S4U tasks
        #      (user-context, fires on each interactive logon of TargetUser).
        $triggerInterval = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes($IntervalMinutes) `
                            -RepetitionInterval (New-TimeSpan -Minutes $IntervalMinutes) `
                            -RepetitionDuration (New-TimeSpan -Days 9999)
        if ($RunAsSystem) {
            $triggerBoot = New-ScheduledTaskTrigger -AtStartup
        } else {
            $triggerBoot = New-ScheduledTaskTrigger -AtLogOn -User $TargetUser
        }
        $triggers = @($triggerInterval, $triggerBoot)

        $settings = New-ScheduledTaskSettingsSet `
                    -AllowStartIfOnBatteries `
                    -DontStopIfGoingOnBatteries `
                    -StartWhenAvailable `
                    -Hidden

        if ($RunAsSystem) {
            $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
        } else {
            $principal = New-ScheduledTaskPrincipal -UserId $TargetUser -LogonType S4U -RunLevel Highest
        }

        if ($CategoryId -eq 'usbHistory') {
            $description = if ($ManagedByAutoSet) {
                'System Maintenance scheduled cleanup v4 (managed)'
            } else {
                'System Maintenance scheduled cleanup v4'
            }
        } else {
            $description = if ($ManagedByAutoSet) {
                'System Maintenance scheduled cleanup v4 (managed)'
            } else {
                'System Maintenance scheduled cleanup v4'
            }
        }
        Register-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $taskName -Action $action -Trigger $triggers `
                               -Principal $principal -Settings $settings -Description $description -Force -ErrorAction Stop | Out-Null

        @{
            status          = 'enabled'
            categoryId      = $CategoryId
            taskName        = $taskName
            intervalMinutes = $IntervalMinutes
            runAsSystem     = [bool]$RunAsSystem
            targetUser      = $TargetUser
        }
    }
    catch {
        @{ error = $true; message = "Failed to set auto-erase schedule for '$CategoryId': $($_.Exception.Message)" }
    }
}

# Set-MultiUserAutoEraseSchedule: create one scheduled task per target user
# per category. Each task runs as that user's S4U context so $env:APPDATA
# and HKCU resolve correctly without any hive loading.
# TargetUsers: array of usernames. Empty array = all non-system accounts on
# this machine. Accounts resolving to the same SID use the same task name.
function Set-MultiUserAutoEraseSchedule {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string]$CategoryId,
        [Parameter(Mandatory)] [int]$IntervalMinutes,
        [bool]$RunAsSystem = $false,
        # Comma-separated string from the backend router (or real array from PS callers)
        [string]$TargetUsers = ''
    )
    Assert-AutoEraseAdmin

    # Split comma-separated string into array; empty = all non-system accounts
    $targetArr = if ($TargetUsers -and $TargetUsers.Trim() -ne '') {
        $TargetUsers -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' }
    } else { @() }

    if ($targetArr.Count -eq 0) {
        $sysDrive = $env:SystemDrive
        $targetArr = Get-CimInstance -ClassName Win32_UserProfile -ErrorAction SilentlyContinue |
            Where-Object { -not $_.Special -and $_.SID -match '^S-1-5-21-' -and $_.LocalPath -like "$sysDrive\Users\*" } |
            ForEach-Object { Split-Path $_.LocalPath -Leaf }
    }

    # SYSTEM-context categories are machine-wide. Registering one task per
    # selected user only overwrote the same canonical task repeatedly and made
    # the result count dishonest.
    if ($RunAsSystem) {
        $r = Set-AutoEraseSchedule `
            -CategoryId      $CategoryId `
            -IntervalMinutes $IntervalMinutes `
            -RunAsSystem     $true `
            -TargetUser      'SYSTEM'
        return @{ status = 'ok'; results = @($r); total = 1 }
    }

    $results = @()
    foreach ($user in $targetArr) {
        $r = Set-AutoEraseSchedule `
            -CategoryId      $CategoryId `
            -IntervalMinutes $IntervalMinutes `
            -RunAsSystem     $RunAsSystem `
            -TargetUser      $user
        $results += $r
    }
    @{ status = 'ok'; results = $results; total = $results.Count }
}

# Remove-MultiUserAutoEraseSchedule: remove per-user tasks created by
# Set-MultiUserAutoEraseSchedule. Pass the same TargetUsers list used when
# creating; empty = remove for all users (finds all matching task names).
function Remove-MultiUserAutoEraseSchedule {
    [CmdletBinding()]
    param([Parameter(Mandatory)] [string]$CategoryId, [string]$TargetUsers = '')
    Assert-AutoEraseAdmin
    try {
        $owners = @($TargetUsers -split ',' | Where-Object { $_.Trim() } |
            ForEach-Object { Resolve-AutoEraseSid $_.Trim() })
        $removed = @()
        foreach ($task in @(Get-ScheduledTask -ErrorAction Stop)) {
            $identity = Get-AutoEraseTaskIdentity $task
            if (-not $identity -or $identity.categoryId -ne $CategoryId) { continue }
            if ($owners.Count -gt 0 -and $identity.ownerSid -notin $owners) { continue }
            Unregister-ScheduledTask -TaskPath $task.TaskPath -TaskName $task.TaskName -Confirm:$false -ErrorAction Stop
            $removed += $task.TaskName
        }
        @{ status = 'disabled'; categoryId = $CategoryId; removed = $removed }
    } catch { @{ error = $true; message = "Failed to remove schedule: $($_.Exception.Message)" } }
}

function Remove-AutoEraseSchedule {
    [CmdletBinding()]
    param([Parameter(Mandatory)] [string]$CategoryId)
    Assert-AutoEraseAdmin
    try {
        $owners = @([Security.Principal.WindowsIdentity]::GetCurrent().User.Value, 'S-1-5-18')
        $result = Remove-MultiUserAutoEraseSchedule -CategoryId $CategoryId -TargetUsers ($owners -join ',')
        if ($result.error) { throw $result.message }
        # A disabled schedule must not leave a catch-up marker behind. Limit
        # cleanup to this caller's base task plus the system scope; explicitly
        # selected other-user schedules have their own remove command/state.
        $scopeKeys = @("user-$env:USERNAME", 'system')
        foreach ($root in @(
            (Join-Path $env:LOCALAPPDATA 'WinCommander\auto-erase'),
            (Join-Path $env:ProgramData 'WinCommander\auto-erase\state')
        )) {
            foreach ($scopeKey in $scopeKeys) {
                Remove-Item -LiteralPath (Join-Path $root "$CategoryId.$scopeKey.state.json") -Force -ErrorAction SilentlyContinue
                Remove-Item -LiteralPath (Join-Path $root "$CategoryId.$scopeKey.last") -Force -ErrorAction SilentlyContinue
            }
        }
        @{ status = 'disabled'; categoryId = $CategoryId }
    }
    catch {
        @{ error = $true; message = "Failed to remove auto-erase schedule for '$CategoryId': $($_.Exception.Message)" }
    }
}

function Get-AutoEraseSchedules {
    try {
        $currentSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $rows = @()
        foreach ($t in @(Get-ScheduledTask -ErrorAction Stop)) {
            $identity = Get-AutoEraseTaskIdentity $t
            if (-not $identity) { continue }
            $info = Get-ScheduledTaskInfo -TaskPath $t.TaskPath -TaskName $t.TaskName -ErrorAction SilentlyContinue
            $repetition = @($t.Triggers | ForEach-Object { $_.Repetition.Interval } | Where-Object { $_ }) | Select-Object -First 1
            $minutes = if ($repetition) { [int][Xml.XmlConvert]::ToTimeSpan($repetition).TotalMinutes } else { 0 }
            $rows += [pscustomobject]@{
                categoryId       = $identity.categoryId
                taskName         = $t.TaskName
                enabled          = ($t.State -ne 'Disabled')
                intervalMinutes  = $minutes
                targetUser       = if ($identity.ownerSid -notin @($currentSid, 'S-1-5-18')) { [string]$t.Principal.UserId } else { $null }
                ownerAccount     = $t.Principal.UserId
                managedByAutoSet = ($t.Description -like 'WinCommander Auto-set scheduled wipe*')
                lastRun          = if ($info) { [string]$info.LastRunTime } else { $null }
                nextRun          = if ($info) { [string]$info.NextRunTime } else { $null }
                lastResult       = if ($info) { $info.LastTaskResult } else { $null }
            }
        }
        @{ schedules = $rows; total = $rows.Count }
    } catch { @{ error = $true; message = "Failed to list auto-erase schedules: $($_.Exception.Message)" } }
}

# Task Scheduler omits default-valued nodes, including Settings/Enabled=true.
# Use DOM nodes so absent settings and the task XML namespace are preserved.
function Set-AutoEraseTaskXmlEnabled {
    param([xml]$Document, [bool]$Value)
    $root = $Document.DocumentElement
    $settings = $root.SelectSingleNode('*[local-name()="Settings"]')
    if ($null -eq $settings) {
        $settings = $Document.CreateElement('Settings', $root.NamespaceURI)
        [void]$root.AppendChild($settings)
    }
    $node = $settings.SelectSingleNode('*[local-name()="Enabled"]')
    $previous = $true
    if ($null -ne $node) {
        $previous = [Xml.XmlConvert]::ToBoolean($node.InnerText)
    } else {
        $node = $Document.CreateElement('Enabled', $root.NamespaceURI)
        [void]$settings.AppendChild($node)
    }
    $node.InnerText = [Xml.XmlConvert]::ToString($Value)
    return $previous
}

# The Windows serializer expands defaults and reorders settings. Compare both
# definitions through that same serializer, not their raw exported spelling.
function ConvertTo-AutoEraseComparableTaskXml {
    param([xml]$Document)
    $service = New-Object -ComObject Schedule.Service
    $service.Connect()
    $definition = $service.NewTask(0)
    $definition.XmlText = $Document.OuterXml
    return [xml]$definition.XmlText
}

# Copy task XML instead of recreating schedules: dates, disabled state, selected
# account, scope and settings belong to the user and survive a naming migration.
function Invoke-AutoEraseMigration {
    [CmdletBinding()]
    param([string]$CategoryId = '')
    Assert-AutoEraseAdmin
    Initialize-SystemMaintenanceTaskFolder
    $migrated = @()
    $errors = @()
    foreach ($task in @(Get-ScheduledTask -ErrorAction Stop)) {
        try {
            $identity = Get-AutoEraseTaskIdentity $task
            if (-not $identity -or ($CategoryId -and $identity.categoryId -ne $CategoryId)) { continue }
            $isUsbHistoryTask = $identity.categoryId -eq 'usbHistory'
            $currentPayload = $task.Description -like 'System Maintenance scheduled cleanup v4*'
            $argument = [string](@($task.Actions)[0].Arguments)
            $upgradePayload = -not ($argument -like '*WinCommander\auto-erase\scripts*' -and $currentPayload)
            if ($task.TaskPath -eq $script:SystemMaintenanceTaskPath -and $task.TaskName -eq $identity.taskName -and -not $upgradePayload) { continue }
            if ([string]$task.State -eq 'Running') { throw 'An active scheduled cleanup must finish before its task can be migrated.' }
            $renaming = $task.TaskName -ne $identity.taskName -or $task.TaskPath -ne $script:SystemMaintenanceTaskPath
            if ($renaming -and (Get-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $identity.taskName -ErrorAction SilentlyContinue)) {
                throw 'The replacement task name is occupied. Both existing tasks were left unchanged.'
            }
            $originalXml = Export-ScheduledTask -TaskPath $task.TaskPath -TaskName $task.TaskName -ErrorAction Stop
            [xml]$xml = $originalXml
            if ($upgradePayload) {
                $repetition = @($task.Triggers | ForEach-Object { $_.Repetition.Interval } | Where-Object { $_ }) | Select-Object -First 1
                if (-not $repetition) { throw 'The existing schedule interval could not be checked.' }
                $minutes = [int][Xml.XmlConvert]::ToTimeSpan($repetition).TotalMinutes
                if ($minutes -lt 1) { throw 'The existing schedule interval is invalid.' }
                $owner = [string]$task.Principal.UserId
                $scope = if ($identity.ownerSid -eq 'S-1-5-18') { 'system' } else { "user-$owner" }
                $info = Get-ScheduledTaskInfo -TaskPath $task.TaskPath -TaskName $task.TaskName -ErrorAction Stop
                $firstDue = if ($info.NextRunTime -gt [DateTime]::MinValue) { $info.NextRunTime.ToUniversalTime() } else { [DateTime]::UtcNow.AddMinutes($minutes) }
                $revision = if ($isUsbHistoryTask) { 3 } else { 2 }
                $xml.Task.Actions.Exec.Arguments = ConvertTo-AutoEraseTaskArgument -CategoryId $identity.categoryId -IntervalMinutes $minutes -Script $script:AutoEraseScripts[$identity.categoryId] -ExecutionScope $scope -FirstDueUtc $firstDue.ToString('o') -PayloadRevision $revision
                $xml.Task.RegistrationInfo.Description = if ($task.Description -like '*managed*') { 'System Maintenance scheduled cleanup v4 (managed)' } else { 'System Maintenance scheduled cleanup v4' }
            }
            if ($xml.Task.RegistrationInfo.URI) { $xml.Task.RegistrationInfo.URI = "\$($identity.taskName)" }
            $enabled = Set-AutoEraseTaskXmlEnabled -Document $xml -Value $false
            $expected = ConvertTo-AutoEraseComparableTaskXml $xml
            # Register disabled so overdue or registration triggers cannot launch a
            # second wipe while the original task still exists.
            $register = @{ TaskName = $identity.taskName; TaskPath = $script:SystemMaintenanceTaskPath; Xml = $expected.OuterXml; ErrorAction = 'Stop' }
            if (-not $renaming) { $register.Force = $true }
            Register-ScheduledTask @register | Out-Null
            try {
                [xml]$readback = Export-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $identity.taskName -ErrorAction Stop
                $readback = ConvertTo-AutoEraseComparableTaskXml $readback
                foreach ($section in @('Actions', 'Triggers', 'Principals', 'Settings')) {
                    if ($readback.Task.$section.OuterXml -ne $expected.Task.$section.OuterXml) {
                        throw "The migrated task's $section could not be verified."
                    }
                }
                if ($renaming) {
                    Unregister-ScheduledTask -TaskPath $task.TaskPath -TaskName $task.TaskName -Confirm:$false -ErrorAction Stop
                }
                if ($enabled) { Enable-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $identity.taskName -ErrorAction Stop | Out-Null }
            } catch {
                # Restore the exact original before removing a staged replacement.
                Register-ScheduledTask -TaskPath $task.TaskPath -TaskName $task.TaskName -Xml $originalXml -Force -ErrorAction Stop | Out-Null
                if ($renaming) { Unregister-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $identity.taskName -Confirm:$false -ErrorAction Stop }
                throw
            }
            $migrated += $identity.categoryId
        } catch { $errors += $_.Exception.Message }
    }
    if ($errors.Count) { return @{ error = $true; status = 'partial'; migrated = $migrated; message = ($errors -join ' '); errors = $errors } }
    @{ status = 'ok'; migrated = $migrated }
}
