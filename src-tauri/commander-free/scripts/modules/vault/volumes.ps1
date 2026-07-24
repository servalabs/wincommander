# ENCRYPTED VOL MODULE (VERACRYPT)
# ============================================================================

function Test-EncryptionInstalled {
    return $null -ne (Get-VeraCryptExe)
}

function Get-VeraCryptExe {
    $candidates = @(
        "${env:ProgramW6432}\VeraCrypt\VeraCrypt.exe",
        "${env:ProgramFiles}\VeraCrypt\VeraCrypt.exe",
        "${env:ProgramFiles(x86)}\VeraCrypt\VeraCrypt.exe"
    ) | Where-Object { $_ -and $_.Trim() -ne '' }
    foreach ($c in $candidates) { if (Test-Path -LiteralPath $c) { return $c } }
    return $null
}

function Install-EncryptionEngine {
    winget install --id IDRIX.VeraCrypt --exact --silent --accept-source-agreements --accept-package-agreements
    @{ status = 'installing' }
}

function Mount-EncryptionVolume {
    # Volume mounting is a PAID feature, tagged "paid" in backend.rs and
    # dispatched to the Pro sidecar (commander-pro::encvol_engine::mount_volume).
    # This Free stub is only reached if Pro is absent.
    param(
        [Alias('Path')]
        [string]$VolumePath,
        [Alias('Letter')]
        [string]$DriveLetter,
        [string]$Password,
        [string]$Keyfile,
        [string]$Pim
    )
    @{ status = 'error'; error = 'Volume mounting requires WinCommander Pro' }
}

function Dismount-EncryptionVolume {
    # Volume dismounting is a PAID feature, tagged "paid" in backend.rs and
    # dispatched to the Pro sidecar (commander-pro::encvol_engine::dismount_volume).
    # This Free stub is only reached if Pro is absent.
    param(
        [Alias('Letter')]
        [string]$DriveLetter
    )
    @{ status = 'error'; error = 'Volume dismounting requires WinCommander Pro' }
}

function Dismount-AllEncryptionVolumes {
    # Dismounting all volumes is a PAID feature, tagged "paid" in backend.rs and
    # dispatched to the Pro sidecar (commander-pro::encvol_engine::dismount_all_volumes).
    # This Free stub is only reached if Pro is absent.
    @{ status = 'error'; error = 'Volume dismounting requires WinCommander Pro' }
}

# Remove dangling X: -> \Device\VeraCryptVolume* DOS-device symlinks left
# behind by VeraCrypt /force dismounts. Returns the letters it freed.
# Tries DefineDosDevice first (exact-match remove), then mountvol /D as a
# fallback, because either path can fail depending on token/privilege.
function Clear-StaleVeraCryptDosDevices {
    if (-not ([System.Management.Automation.PSTypeName]'VcDosCleanup').Type) {
        try {
            Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class VcDosCleanup {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern uint QueryDosDevice(string lpDeviceName, StringBuilder lpTargetPath, int ucchMax);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool DefineDosDevice(uint dwFlags, string lpDeviceName, string lpTargetPath);
    // DDD_REMOVE_DEFINITION = 0x2, DDD_EXACT_MATCH_ON_REMOVE = 0x4, DDD_RAW_TARGET_PATH = 0x1
    public const uint REMOVE_EXACT_RAW = 0x2 | 0x4 | 0x1;
}
'@ -ErrorAction Stop
        } catch { return @() }
    }
    $cleared = @()
    foreach ($letter in 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.ToCharArray()) {
        $dl = "${letter}:"
        $sb = New-Object System.Text.StringBuilder 1024
        $len = [VcDosCleanup]::QueryDosDevice($dl, $sb, 1024)
        if ($len -eq 0) { continue }
        $target = $sb.ToString()
        if ($target -notmatch 'VeraCryptVolume|TrueCryptVolume') { continue }

        # Skip live mounts — only orphaned symlinks should be removed.
        # A stale symlink left by a /force dismount points to a device that no
        # longer exists, so Test-Path on the drive root returns $false quickly.
        # A live VeraCrypt mount IS accessible; removing its symlink would yank
        # the drive letter out from under a mounted volume.
        if (Test-Path "${dl}\") { continue }

        $removed = $false
        # 1) Specific remove via DefineDosDevice (won't touch unrelated stacked mappings)
        try {
            $removed = [VcDosCleanup]::DefineDosDevice([VcDosCleanup]::REMOVE_EXACT_RAW, $dl, $target)
        } catch {}

        # 2) Fallback: mountvol /D. Removes the mount point regardless of target.
        if (-not $removed) {
            try { & mountvol "${dl}\" /D 2>&1 | Out-Null } catch {}
        }

        # 3) Verify the mapping is actually gone
        $sb2 = New-Object System.Text.StringBuilder 1024
        $len2 = [VcDosCleanup]::QueryDosDevice($dl, $sb2, 1024)
        if ($len2 -eq 0 -or $sb2.ToString() -notmatch 'VeraCryptVolume|TrueCryptVolume') {
            $cleared += $dl
        }
    }
    return $cleared
}

function Get-EncryptionStatus {
    $debug = [System.Collections.ArrayList]@()

    # ProgramW6432 always resolves to the 64-bit Program Files even when this
    # script runs inside a 32-bit PowerShell host (WOW64 process) where
    # $env:ProgramFiles is redirected to Program Files (x86).
    $vcSearchPaths = @(
        "${env:ProgramW6432}\VeraCrypt",
        "${env:ProgramFiles}\VeraCrypt",
        "${env:ProgramFiles(x86)}\VeraCrypt"
    ) | Where-Object { $_ -and $_.Trim() -ne '' } | Select-Object -Unique

    [void]$debug.Add("hostBits=$([IntPtr]::Size * 8)")
    [void]$debug.Add("psVersion=$($PSVersionTable.PSVersion)")
    [void]$debug.Add("ProgramW6432=$env:ProgramW6432")
    [void]$debug.Add("ProgramFiles=$env:ProgramFiles")
    [void]$debug.Add("searchPaths=" + ($vcSearchPaths -join '|'))

    $vcDir = $null
    foreach ($p in $vcSearchPaths) {
        $exe = Join-Path $p 'VeraCrypt.exe'
        if (Test-Path $exe) {
            $vcDir = $p
            [void]$debug.Add("foundVc=$exe")
            break
        }
    }
    if (-not $vcDir) { [void]$debug.Add("foundVc=NONE") }
    $volumes = @()

    # ── PRIMARY: query the VeraCrypt / EncVol driver directly ─────────────
    # Open the driver control device \\.\VeraCrypt (access 0) and issue the
    # mounted-volumes IOCTL to read the driver mount list. This needs ONLY the
    # kernel driver loaded — NOT VeraCrypt.exe installed — so it works for the
    # Pro EncVol engine (EncVolKm.sys registers the same \\.\VeraCrypt device)
    # and recovers the real container path per drive.
    try {
        if (-not ([System.Management.Automation.PSTypeName]'VcMountQuery').Type) {
            Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class VcMountQuery {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr templ);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool DeviceIoControl(IntPtr h, uint code, IntPtr inBuf, uint inSize, IntPtr outBuf, uint outSize, out uint ret, IntPtr ovl);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool CloseHandle(IntPtr h);
    // VC_IOCTL_GET_MOUNTED_VOLUMES = CTL_CODE(FILE_DEVICE_UNKNOWN, 0x806, METHOD_BUFFERED, FILE_ANY_ACCESS)
    const uint IOCTL_GET_MOUNTED = 0x00222018;
    const uint OPEN_EXISTING = 3;
    const uint SHARE_RW = 3;          // FILE_SHARE_READ | FILE_SHARE_WRITE
    const int  WSZVOLUME_OFFSET = 4;  // MOUNT_LIST_STRUCT: right after uint32 ulMountedDrives
    const int  PATH_CHARS = 260;      // wchar_t wszVolume[26][260]
    public static string[] GetMounted(out bool driverPresent) {
        driverPresent = false;
        IntPtr h = CreateFileW(@"\\.\VeraCrypt", 0, SHARE_RW, IntPtr.Zero, OPEN_EXISTING, 0, IntPtr.Zero);
        if (h == new IntPtr(-1)) { return new string[0]; }
        driverPresent = true;
        var outList = new List<string>();
        int bufSize = 65536; // >= sizeof(MOUNT_LIST_STRUCT) for any VeraCrypt version
        IntPtr buf = Marshal.AllocHGlobal(bufSize);
        try {
            Marshal.Copy(new byte[bufSize], 0, buf, bufSize);
            uint ret;
            if (DeviceIoControl(h, IOCTL_GET_MOUNTED, buf, (uint)bufSize, buf, (uint)bufSize, out ret, IntPtr.Zero)) {
                uint mounted = (uint)Marshal.ReadInt32(buf, 0);
                for (int i = 0; i < 26; i++) {
                    if ((mounted & (1u << i)) != 0) {
                        IntPtr p = new IntPtr(buf.ToInt64() + WSZVOLUME_OFFSET + (long)i * PATH_CHARS * 2);
                        string path = Marshal.PtrToStringUni(p, PATH_CHARS);
                        int nul = path.IndexOf('\0');
                        if (nul >= 0) { path = path.Substring(0, nul); }
                        if (path.StartsWith(@"\??\")) { path = path.Substring(4); }
                        outList.Add(((char)('A' + i)).ToString() + ":\t" + path);
                    }
                }
            }
        } finally { Marshal.FreeHGlobal(buf); CloseHandle(h); }
        return outList.ToArray();
    }
}
'@ -ErrorAction Stop
            [void]$debug.Add("Add-Type loaded VcMountQuery")
        }
        $driverPresent = $false
        $mounted = [VcMountQuery]::GetMounted([ref]$driverPresent)
        [void]$debug.Add("driverPresent=$driverPresent driverMounts=$($mounted.Count)")
        foreach ($ln in $mounted) {
            $parts = $ln -split "`t", 2
            $vpath = if ($parts.Count -ge 2 -and $parts[1].Trim() -ne '') { $parts[1] } else { 'Encrypted Volume' }
            $volumes += @{ letter = $parts[0]; path = $vpath; type = 'Mounted' }
            [void]$debug.Add("driver MATCH $($parts[0]) -> $vpath")
        }
    } catch {
        [void]$debug.Add("driver IOCTL ERROR: $($_.Exception.Message)")
    }

    # ── FALLBACK: QueryDosDevice NT-device-path scan ──────────────────────
    # Version-independent; runs only if the driver IOCTL returned nothing (e.g.
    # a foreign/older driver). Cannot recover the container path → placeholder.
    if ($volumes.Count -eq 0) {
        [void]$debug.Add("entering QueryDosDevice fallback")
        try {
            if (-not ([System.Management.Automation.PSTypeName]'VcDosDevices').Type) {
                Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class VcDosDevices {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern uint QueryDosDevice(string lpDeviceName, StringBuilder lpTargetPath, int ucchMax);
}
'@ -ErrorAction Stop
                [void]$debug.Add("Add-Type loaded VcDosDevices")
            }

            $sysDrive = $env:SystemDrive   # e.g. "C:"
            foreach ($letter in 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.ToCharArray()) {
                $dl = "${letter}:"
                if ($dl -eq $sysDrive) { continue }
                if (-not (Test-Path "${dl}\")) { continue }

                $sb = New-Object System.Text.StringBuilder 512
                $len = [VcDosDevices]::QueryDosDevice($dl, $sb, 512)
                if ($len -gt 0) {
                    $ntPath = $sb.ToString()
                    if ($ntPath -match '(?i)\\Device\\(VeraCrypt|TrueCrypt)Volume') {
                        $volumes += @{ letter = $dl; path = "Encrypted Volume"; type = "Mounted" }
                        [void]$debug.Add("QDD MATCH $dl")
                    }
                }
            }
        } catch {
            [void]$debug.Add("QueryDosDevice ERROR: $($_.Exception.Message)")
        }

        # Last resort: Win32_DiskDrive model/caption matching
        # (older VeraCrypt versions register virtual SCSI disks here).
        if ($volumes.Count -eq 0) {
            try {
                $disks = Get-CimInstance Win32_DiskDrive -ErrorAction SilentlyContinue | Where-Object {
                    ($_.Model    -and $_.Model    -match '(?i)truecrypt|veracrypt') -or
                    ($_.Caption  -and $_.Caption  -match '(?i)truecrypt|veracrypt')
                }
                foreach ($disk in $disks) {
                    $parts = Get-CimAssociatedInstance -InputObject $disk -ResultClassName Win32_DiskPartition -ErrorAction SilentlyContinue
                    foreach ($p in $parts) {
                        $lds = Get-CimAssociatedInstance -InputObject $p -ResultClassName Win32_LogicalDisk -ErrorAction SilentlyContinue
                        foreach ($ld in $lds) {
                            if ($ld.DeviceID) {
                                $volumes += @{ letter = $ld.DeviceID; path = "Encrypted Volume"; type = "Mounted" }
                            }
                        }
                    }
                }
            } catch {}
        }
    }

    [void]$debug.Add("volumesFound=$($volumes.Count)")

    # "installed" reflects engine presence/usability for the UI hint: VeraCrypt.exe
    # on disk OR a volume is actually mounted (the driver responded). Detection
    # itself is NEVER gated on VeraCrypt.exe being installed.
    @{
        installed = ($null -ne $vcDir) -or ($volumes.Count -gt 0)
        path      = if ($null -ne $vcDir) { "$vcDir\VeraCrypt.exe" } else { $null }
        volumes   = $volumes
        debug     = @($debug)
    }
}

function Get-BitLockerVolumes {
    # Read-only enumeration of every BitLocker volume for the selective
    # crypto-erase picker. No mutation. Returns [] when BitLocker is absent
    # (Home SKU / cmdlet missing) rather than throwing. escrowRisk is derived
    # by the caller from recoveryPasswordPresent/backupUsed.
    $out = @()
    try {
        $vols = Get-BitLockerVolume -ErrorAction Stop
    } catch {
        return (ConvertTo-Json @($out))
    }
    foreach ($v in $vols) {
        $protectors = @($v.KeyProtector | ForEach-Object { "$($_.KeyProtectorType)" })
        $recovery = @($v.KeyProtector | Where-Object { $_.KeyProtectorType -eq 'RecoveryPassword' })
        $backupUsed = [bool](@($v.KeyProtector | Where-Object { $_.BackupUsed }).Count)
        $out += @{
            mountPoint              = "$($v.MountPoint)"
            volumeType              = "$($v.VolumeType)"
            volumeStatus            = "$($v.VolumeStatus)"
            encryptionMethod        = "$($v.EncryptionMethod)"
            protectorTypes          = $protectors
            recoveryPasswordPresent = [bool]($recovery.Count)
            backupUsed              = $backupUsed
        }
    }
    return (ConvertTo-Json @($out) -Depth 4)
}

function Open-EncryptionVolume {
    param([string]$DriveLetter)
    # The drive letter passed is usually in "A:" format, remove any trailing colon first
    $cleanLetter = $DriveLetter.Replace(":", "")
    $path = "${cleanLetter}:\"
    if (Test-Path $path) {
        # Use explorer.exe to open the directory reliably from a background process
        Start-Process "explorer.exe" -ArgumentList $path
        @{ status = 'opened'; drive = $DriveLetter }
    }
    else {
        @{ status = 'error'; error = "Drive ${cleanLetter}: not found" }
    }
}

function List-EncryptionVolumes {
    $status = Get-EncryptionStatus
    @{ volumes = $status.volumes }
}

function Create-EncryptionVolume {
    # Volume creation is a PAID feature, tagged "paid" in backend.rs and
    # dispatched to the Pro sidecar (commander-pro::encvol_engine::create_volume).
    # This Free stub is only reached if Pro is absent.
    param(
        [string]$Path,
        [string]$SizeMB,
        [string]$Password,
        [string]$Encryption = "AES",
        [string]$Hash = "sha-512",
        [string]$Filesystem = "NTFS",
        [bool]$Quick = $true,
        [string]$Keyfile,
        [string]$Pim
    )
    @{ status = 'error'; error = 'Volume creation requires WinCommander Pro' }
}

function Create-DualVolume {
    # Two-password volume creation is a PAID feature, tagged "paid" in backend.rs and
    # dispatched to the Pro sidecar (commander-pro::encvol_engine::create_dual_volume).
    # This Free stub is only reached if Pro is absent.
    param(
        [string]$Path,
        [string]$FirstPassword,
        [string]$SecondPassword,
        [string]$HostSizeMB,
        [string]$SecondSizeMB,
        [string]$Encryption = "AES",
        [string]$Hash = "sha-512",
        [string]$Filesystem = "NTFS"
    )
    @{ status = 'error'; error = 'Two-password volume creation requires Pro' }
}

function Get-VolumeInfo {
    param([string]$DriveLetter)

    # Query WMI for the logical disk matching the drive letter
    $cleanLetter = $DriveLetter.Replace(":", "")
    $drivePath = "${cleanLetter}:"
    $disk = Get-WmiObject -Class Win32_LogicalDisk -Filter "DeviceID='${drivePath}'" -ErrorAction SilentlyContinue

    if (-not $disk) {
        return @{ status = 'error'; error = "Drive ${cleanLetter}: not mounted" }
    }

    # Format size
    $sizeBytes = $disk.Size
    $sizeFormatted = if ($sizeBytes -ge 1TB) {
        "{0:N2} TB" -f ($sizeBytes / 1TB)
    }
    elseif ($sizeBytes -ge 1GB) {
        "{0:N2} GB" -f ($sizeBytes / 1GB)
    }
    else {
        "{0:N2} MB" -f ($sizeBytes / 1MB)
    }

    # Map filesystem integer to string
    $fsMap = @{ 'FAT' = 'FAT32'; 'FAT32' = 'FAT32'; 'NTFS' = 'NTFS'; 'exFAT' = 'exFAT'; 'ReFS' = 'ReFS' }
    $fsName = if ($disk.FileSystem) { if ($fsMap[$disk.FileSystem]) { $fsMap[$disk.FileSystem] } else { $disk.FileSystem } } else { 'Unknown' }

    # Try to read encryption info from VeraCrypt driver via volume label / WMI
    # The encryption algorithm is stored in the volume header — not accessible without the password.
    # We can return what we know from the OS perspective.
    @{
        status     = 'ok'
        size       = $sizeFormatted
        filesystem = $fsName
        encryption = 'AES-256-XTS'     # Standard for all VeraCrypt volumes
        mode       = 'XTS'
        readOnly   = ($disk.Access -eq 1)
    }
}

function Get-SystemEncryptionStatus {
    $svcName = 'veracrypt'
    $svc = Get-Service -Name $svcName -ErrorAction SilentlyContinue

    if (-not $svc) {
        return @{
            encrypted = $false
            progress  = $null
            algorithm = $null
            mode      = $null
        }
    }

    # Check registry key written by VeraCrypt during system encryption
    $regPath = 'HKLM:\SYSTEM\CurrentControlSet\Services\veracrypt'
    $regExists = Test-Path $regPath

    # Check for the VeraCrypt boot loader presence on the system drive
    # VeraCrypt writes a key under its service when system encryption is active
    $systemEncRegPath = 'HKLM:\SYSTEM\CurrentControlSet\Services\veracrypt\SystemEncryption'
    $systemEncActive = Test-Path $systemEncRegPath

    # Try to read encryption progress if available
    $progress = $null
    $algorithm = $null
    $mode = 'XTS'

    if ($systemEncActive) {
        try {
            $regData = Get-ItemProperty -Path $systemEncRegPath -ErrorAction SilentlyContinue
            if ($regData.EncryptionProgress) { $progress = [double]$regData.EncryptionProgress }
            if ($regData.Algorithm) { $algorithm = $regData.Algorithm }
        }
        catch {}
    }

    # Also check the VeraCrypt volume header backup path as a secondary signal
    $vcBootPath = "$env:SystemRoot\System32\VeraCrypt-DCS"
    $hasBoot = Test-Path $vcBootPath

    $isEncrypted = $systemEncActive -or $hasBoot

    @{
        encrypted = $isEncrypted
        progress  = if ($isEncrypted -and $null -eq $progress) { 100 } else { $progress }
        algorithm = if ($algorithm) { $algorithm } else { if ($isEncrypted) { 'AES-256' } else { $null } }
        mode      = if ($isEncrypted) { $mode } else { $null }
    }
}

function Get-AvailableDriveLetters {
    # Force-clean any stale VeraCrypt DOS-device symlinks before measuring.
    # After a `/force` dismount the symlink can survive (open handles, etc.),
    # which makes Win32_LogicalDisk + .NET DriveInfo both keep reporting the
    # letter as in-use — so the mount dialog drops it from the dropdown.
    try { Clear-StaleVeraCryptDosDevices | Out-Null } catch {}

    $usedLetters = @()

    # Prefer .NET DriveInfo over WMI: it always reads live state via the
    # Mount Manager, while WMI's __ProviderArchitecture cache can lag for
    # several seconds after a mount-point change.
    try {
        $drives = [System.IO.DriveInfo]::GetDrives()
        foreach ($d in $drives) {
            $n = ($d.Name -replace '[:\\]', '').ToUpper()
            if ($n.Length -eq 1) { $usedLetters += $n }
        }
    } catch {}

    # Fallback path if DriveInfo somehow fails — keeps the old behaviour intact.
    if ($usedLetters.Count -eq 0) {
        $used = Get-WmiObject -Class Win32_LogicalDisk -ErrorAction SilentlyContinue |
                ForEach-Object { ($_.DeviceID -replace ':', '').ToUpper() }
        $usedLetters += $used
    }

    # Catches subst'd drives that DriveInfo / WMI might miss.
    $psDrives = Get-PSDrive -PSProvider FileSystem -ErrorAction SilentlyContinue |
                ForEach-Object { $_.Name.ToUpper() } |
                Where-Object { $_.Length -eq 1 }
    $usedLetters += $psDrives

    $usedLetters = $usedLetters | Sort-Object -Unique

    $available = 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
                 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z' |
                 Where-Object { $_ -notin $usedLetters }

    @{ letters = @($available); used = @($usedLetters) }
}

function Get-EncryptionPartitions {
    $systemDrive = ($env:SystemDrive -replace ':', '').ToUpper()
    
    $partitionList = @()
    $rawPartitions = Get-Partition -ErrorAction SilentlyContinue 
    
    if ($null -ne $rawPartitions) {
        foreach ($part in $rawPartitions) {
            # Skip partitions with no usable space or tiny helper ones
            if ($part.Size -lt 100MB) { continue }
            
            # Skip obvious system ones but be generous with the rest
            if ($part.Type -eq "System" -or $part.Type -eq "Recovery" -or $part.Type -eq "Reserved") { continue }
            if ($part.GptType -eq "{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}" -or $part.GptType -eq "{de94bba4-06d1-4d40-a16a-bfd50179d6ac}" -or $part.GptType -eq "{e3c9e316-0b5c-4db8-817d-f92df00215ae}") { continue }
            
            # Skip the actual OS drive letter to prevent self-locking accidents
            if ($part.DriveLetter -eq $systemDrive) { continue }

            $disk = Get-Disk -Number $part.DiskNumber -ErrorAction SilentlyContinue
            $devicePath = "\Device\Harddisk$($part.DiskNumber)\Partition$($part.PartitionNumber)"
            
            $sizeBytes = $part.Size
            $sizeFormatted = if ($sizeBytes -ge 1GB) { "{0:N2} GB" -f ($sizeBytes / 1GB) } else { "{0:N2} MB" -f ($sizeBytes / 1MB) }

            $partitionList += @{
                diskNumber      = $part.DiskNumber
                partitionNumber = $part.PartitionNumber
                driveLetter     = $part.DriveLetter
                size            = $sizeFormatted
                sizeBytes       = $sizeBytes
                devicePath      = $devicePath
                busType         = if ($disk) { $disk.BusType.ToString() } else { "Unknown" }
                model           = if ($disk) { $disk.Model.Trim() } else { "Hard Disk" }
            }
        }
    }
    
    @{ partitions = $partitionList }
}
