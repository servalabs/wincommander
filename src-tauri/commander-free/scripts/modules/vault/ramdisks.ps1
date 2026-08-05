# RAM DISK MODULE
# ============================================================================
# Wraps the underlying RAM-disk engine's CLI to create/list/remove virtual-
# memory backed drives. Mirrors the encrypted-volume module's shape so the
# Vault panel can manage both surfaces with one mental model.
#
# The choice of underlying engine is an implementation detail; nothing in
# the public function contract or returned error messages references the
# engine name.

function Get-ImDiskExe {
    # Engine binary detection. Checks PATH first so user-installed copies
    # outside Program Files are honoured, then the standard install paths.
    $cmd = Get-Command imdisk.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    $candidates = @(
        "${env:ProgramW6432}\ImDisk\imdisk.exe",
        "${env:ProgramFiles}\ImDisk\imdisk.exe",
        "${env:ProgramFiles(x86)}\ImDisk\imdisk.exe",
        "${env:SystemRoot}\System32\imdisk.exe"
    ) | Where-Object { $_ -and $_.Trim() -ne '' }
    foreach ($c in $candidates) { if (Test-Path -LiteralPath $c) { return $c } }
    return $null
}

function Test-RamDiskInstalled {
    return $null -ne (Get-ImDiskExe)
}

function Install-RamDiskEngine {
    # One-click installer. Downloads the upstream IExpress self-extracting
    # installer, validates it, strips Mark-of-the-Web (otherwise SmartScreen
    # blocks execution with "file is corrupted and unreadable"), extracts
    # its payload without launching the GUI, then runs the underlying
    # install.cmd with the same args the GUI would have passed.
    #
    # Why this layout: the upstream installer is an IExpress SFX whose
    # manifest declares ExecuteParameters = "/64 /hide .\install.cmd " —
    # i.e. the GUI's only job is to confirm and then invoke install.cmd.
    # We invoke install.cmd directly so the user never sees a dialog.

    if (Test-RamDiskInstalled) {
        return @{ status = 'already_installed' }
    }

    # Driver install needs admin.
    if (Get-Command Assert-IsAdmin -ErrorAction SilentlyContinue) {
        Assert-IsAdmin
    }

    # Direct download from upstream maintainer's static host. This URL is
    # stable, redirects through one hop to a CDN, and serves a real PE
    # binary (no SourceForge interstitial / ZIP wrapper issue).
    $urls = @(
        'https://www.ltr-data.se/files/imdiskinst.exe',
        'http://www.ltr-data.se/files/imdiskinst.exe'
    )
    $tmpExe = Join-Path $env:TEMP "rde_$([Guid]::NewGuid().ToString('N').Substring(0,8)).exe"
    $tmpDir = Join-Path $env:TEMP "rde_x_$([Guid]::NewGuid().ToString('N').Substring(0,8))"

    try {
        [Net.ServicePointManager]::SecurityProtocol = `
            [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13
    } catch {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    }

    $downloaded = $false
    $lastError = $null

    foreach ($url in $urls) {
        if (Test-Path $tmpExe) { Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue }

        # Attempt A — IWR with a wget UA. Some upstream hosts gate
        # downloads behind a UA check; wget is the safe lowest-common-
        # denominator.
        try {
            $oldPref = $ProgressPreference
            $ProgressPreference = 'SilentlyContinue'
            try {
                Invoke-WebRequest -Uri $url -OutFile $tmpExe `
                    -UseBasicParsing -MaximumRedirection 15 `
                    -UserAgent 'Wget/1.21.3' `
                    -TimeoutSec 600 `
                    -ErrorAction Stop
            } finally { $ProgressPreference = $oldPref }
        } catch { $lastError = $_.Exception.Message }

        # Attempt B — BITS fallback for restrictive proxies.
        if (-not (Test-Path $tmpExe) -or (Get-Item $tmpExe).Length -lt 100000) {
            try {
                if (Test-Path $tmpExe) { Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue }
                Import-Module BitsTransfer -ErrorAction Stop
                Start-BitsTransfer -Source $url -Destination $tmpExe -ErrorAction Stop
            } catch { $lastError = $_.Exception.Message; continue }
        }

        if (-not (Test-Path $tmpExe)) { continue }
        if ((Get-Item $tmpExe).Length -lt 100000) {
            $lastError = "Downloaded file too small ($((Get-Item $tmpExe).Length) bytes)"
            continue
        }
        try {
            $fs = [System.IO.File]::OpenRead($tmpExe)
            $b0 = $fs.ReadByte(); $b1 = $fs.ReadByte()
            $fs.Close()
            if ($b0 -ne 0x4D -or $b1 -ne 0x5A) {
                $lastError = 'Downloaded file is not a valid Windows executable.'
                continue
            }
        } catch {
            $lastError = "Could not verify installer integrity: $($_.Exception.Message)"
            continue
        }

        $downloaded = $true
        break
    }

    if (-not $downloaded) {
        if (Test-Path $tmpExe) { Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue }
        return @{
            status = 'error'
            error  = "Could not download the RAM Disk Engine installer. Check internet connectivity or proxy settings.$(if ($lastError) { ' (' + $lastError + ')' })"
        }
    }

    # Strip Mark-of-the-Web. Without this, Windows SmartScreen blocks
    # execution with the cryptic "The file or directory is corrupted and
    # unreadable" message.
    try {
        Unblock-File -LiteralPath $tmpExe -ErrorAction SilentlyContinue
        $adsPath = $tmpExe + ':Zone.Identifier'
        if (Test-Path -LiteralPath $adsPath) {
            Remove-Item -LiteralPath $adsPath -Force -ErrorAction SilentlyContinue
        }
    } catch {}

    # IExpress self-extract without running the embedded command.
    #   /T:<dir>  — extract destination
    #   /C        — extract only, don't execute the manifest's Run command
    #   /Q        — quiet (no extraction progress UI)
    # If extraction succeeds, install.cmd will be in $tmpDir.
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
    $extractedOk = $false
    try {
        $extractProc = Start-Process -FilePath $tmpExe `
            -ArgumentList @("/T:$tmpDir", '/C', '/Q') `
            -Wait -PassThru -WindowStyle Hidden -ErrorAction Stop
        if ($extractProc.ExitCode -eq 0 -and (Test-Path (Join-Path $tmpDir 'install.cmd'))) {
            $extractedOk = $true
        }
    } catch {
        $lastError = "Extraction failed: $($_.Exception.Message)"
    }

    if ($extractedOk) {
        # Run the install command directly with the args the GUI would have
        # used. /64 picks the 64-bit driver, /hide suppresses the install
        # confirmation window.
        $installCmd = Join-Path $tmpDir 'install.cmd'
        $cmdExitCode = $null
        try {
            $cmdProc = Start-Process -FilePath 'cmd.exe' `
                -ArgumentList @('/c', "`"$installCmd`"", '/64', '/hide') `
                -WorkingDirectory $tmpDir `
                -Wait -PassThru -WindowStyle Hidden -ErrorAction Stop
            $cmdExitCode = $cmdProc.ExitCode
        } catch {
            $lastError = "install.cmd failed to launch: $($_.Exception.Message)"
        }

        if ($cmdExitCode -ne 0) {
            $lastError = "install.cmd exited with code $cmdExitCode"
            $extractedOk = $false
        }
    }

    # Cleanup extracted payload regardless of success.
    if (Test-Path $tmpDir) { Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue }

    # Fallback: if the silent extract-and-run path failed for any reason,
    # launch the installer directly. The upstream installer shows a single
    # small confirmation dialog and completes in seconds — still effectively
    # one click for the user.
    if (-not $extractedOk) {
        try {
            $proc = Start-Process -FilePath $tmpExe -Wait -PassThru -ErrorAction Stop
            if ($proc.ExitCode -ne 0) {
                Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue
                return @{ status = 'error'; error = "Installer exited with code $($proc.ExitCode).$(if ($lastError) { ' Previous: ' + $lastError })" }
            }
        } catch {
            Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue
            return @{ status = 'error'; error = "Installer could not start: $($_.Exception.Message)" }
        }
    }

    Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue

    # Driver registration takes a moment; poll briefly so the immediate
    # status refresh shows the install.
    for ($i = 0; $i -lt 20; $i++) {
        if (Test-RamDiskInstalled) {
            return @{ status = 'installed'; message = 'RAM Disk Engine installed.' }
        }
        Start-Sleep -Milliseconds 300
    }

    return @{
        status = 'error'
        error  = 'Install completed but the engine was not detected. A restart may be required.'
    }
}

function _Invoke-ImDisk {
    # Internal helper: runs the engine CLI synchronously, captures stdout
    # and stderr together so callers can grep either stream uniformly.
    #
    # Uses ProcessStartInfo with a pre-built command-line string instead
    # of Start-Process -ArgumentList. WinPS 5.1 does not reliably quote
    # array elements that contain spaces, which silently corrupted the
    # `-p "/fs:ntfs /y /q /v:LABEL"` format-parameters block — imdisk
    # then printed its help banner and exited -1.
    param([Parameter(Mandatory)] [string[]]$Arguments)

    $exe = Get-ImDiskExe
    if (-not $exe) { return @{ ok = $false; output = ''; exitCode = -1; error = 'Engine not installed' } }

    # Build a CreateProcess-style command line. Any arg containing a
    # space (notably the -p format-parameters string) gets wrapped in
    # double quotes; embedded quotes are doubled per Windows convention.
    $cmdLine = ($Arguments | ForEach-Object {
        if ($_ -match '\s') { '"' + ($_ -replace '"', '""') + '"' } else { $_ }
    }) -join ' '

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName               = $exe
    $psi.Arguments              = $cmdLine
    $psi.UseShellExecute        = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError  = $true
    $psi.CreateNoWindow         = $true

    try {
        $proc = [System.Diagnostics.Process]::Start($psi)
        $stdout = $proc.StandardOutput.ReadToEnd()
        $stderr = $proc.StandardError.ReadToEnd()
        $proc.WaitForExit()
        return @{
            ok       = ($proc.ExitCode -eq 0)
            output   = "$stdout`n$stderr"
            exitCode = $proc.ExitCode
        }
    } catch {
        return @{ ok = $false; output = ''; exitCode = -1; error = $_.Exception.Message }
    }
}

function _Parse-ImDiskDeviceDetails {
    # Parses the verbose `engine -l -n <num>` output into a hashtable.
    # Real output varies slightly between versions; the line that carries
    # both the drive letter and the size looks like one of:
    #     R:  Size: 1073741824 bytes (1024 MB).
    #     R: \\?\GLOBALROOT\Device\ImDisk0  Size: 1073741824 bytes (1024 MB).
    # The strict "letter then Size:" regex used to miss the second form,
    # which is why a mounted RAM disk showed "No RAM disks mounted" in the
    # tab while the drive was clearly present in Explorer.
    #
    # New approach: detect each field independently per line, so neither
    # the letter nor the size has to live in any particular position.
    param([string]$Text)

    $info = @{
        deviceNumber = $null
        letter       = $null
        sizeBytes    = 0
        sizeText     = $null
        type         = 'Unknown'
        imageFile    = $null
        properties   = $null
        isRam        = $false
    }

    foreach ($line in ($Text -split "`r?`n")) {
        $l = $line.Trim()
        if (-not $l) { continue }

        if ($l -match '^Device\s+(\d+):') { $info.deviceNumber = [int]$Matches[1]; continue }

        # Drive letter is the first single uppercase letter followed by ':'
        # at the start of a non-header line. Don't require anything after.
        if (-not $info.letter -and $l -match '^([A-Z]):(\s|$)') {
            $info.letter = "$($Matches[1]):"
        }

        # Size can appear anywhere on the line.
        if ($info.sizeBytes -eq 0 -and $l -match 'Size:\s+(\d+)\s+bytes(?:\s*\(([^)]+)\))?') {
            $info.sizeBytes = [int64]$Matches[1]
            if ($Matches[2]) { $info.sizeText = $Matches[2].Trim() }
        }

        if ($l -match '^Disk image type:\s+(.+)$') {
            $info.type = $Matches[1].Trim()
            # Match "Virtual memory file", "Virtual Memory File", "VM" etc.
            if ($info.type -match '(?i)virtual\s*memory|^vm\b|\bvm\b') { $info.isRam = $true }
            continue
        }
        if ($l -match '^Disk image file:\s+(.+)$') { $info.imageFile = $Matches[1].Trim(); continue }
        if ($l -match '^Properties:\s+(.+)$')      { $info.properties = $Matches[1].Trim(); continue }
    }
    return $info
}

function Get-RamDiskStatus {
    $exe = Get-ImDiskExe
    if (-not $exe) {
        return @{ installed = $false; disks = @() }
    }

    $disks = @()
    $seenLetters = New-Object 'System.Collections.Generic.HashSet[string]'

    # ── Path 1: engine text output ──────────────────────────────────────
    # `imdisk -l -n` lists device numbers; each one is then queried for
    # type + drive letter + size. Only VM-backed devices are surfaced.
    # This works when the disk was created via our backend; it misses
    # disks created by the toolkit's tray helper which sometimes uses a
    # separate enumeration path.
    $list = _Invoke-ImDisk -Arguments @('-l', '-n')
    if ($list.ok -and $list.output) {
        $deviceNumbers = @()
        foreach ($line in ($list.output -split "`r?`n")) {
            $t = $line.Trim()
            if ($t -match '^\d+$') { $deviceNumbers += [int]$t }
        }

        foreach ($n in $deviceNumbers) {
            $details = _Invoke-ImDisk -Arguments @('-l', '-n', "$n")
            if (-not $details.ok) { continue }
            $info = _Parse-ImDiskDeviceDetails -Text $details.output
            if (-not $info.isRam) { continue }
            if (-not $info.letter) { continue }
            [void]$seenLetters.Add($info.letter)
            $disks += @{
                deviceNumber = $info.deviceNumber
                letter       = $info.letter
                sizeBytes    = $info.sizeBytes
                size         = $info.sizeText
                type         = 'RAM Disk'
                properties   = 'Volatile RAM'
            }
        }
    }

    # ── Path 2: kernel QueryDosDevice ───────────────────────────────────
    # Authoritative: walk every drive letter A-Z, ask the kernel which NT
    # device path it maps to, and match any that resolve to \Device\ImDisk*.
    # Same approach the VeraCrypt detection uses in vault/volumes.ps1.
    # This catches RAM disks regardless of how they were created — tray
    # helper, command line, mountvol, manual subst — because the NT path
    # is set by the driver itself.
    try {
        if (-not ([System.Management.Automation.PSTypeName]'RdkDosDevices').Type) {
            Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class RdkDosDevices {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern uint QueryDosDevice(string lpDeviceName, StringBuilder lpTargetPath, int ucchMax);
}
'@ -ErrorAction Stop
        }

        $sysDrive = $env:SystemDrive
        foreach ($letter in 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.ToCharArray()) {
            $dl = "${letter}:"
            if ($dl -eq $sysDrive) { continue }
            if ($seenLetters.Contains($dl)) { continue }
            if (-not (Test-Path "${dl}\")) { continue }

            $sb  = New-Object System.Text.StringBuilder 1024
            $len = [RdkDosDevices]::QueryDosDevice($dl, $sb, 1024)
            if ($len -le 0) { continue }
            $ntPath = $sb.ToString()
            if ($ntPath -notmatch '(?i)\\Device\\ImDisk\d+') { continue }

            # We have an ImDisk-backed drive. Try to enrich with size via
            # WMI; fall back to "unknown" if WMI doesn't have it.
            $sizeBytes = 0
            $sizeText  = $null
            try {
                $ld = Get-CimInstance -ClassName Win32_LogicalDisk `
                    -Filter "DeviceID='$dl'" -ErrorAction SilentlyContinue
                if ($ld -and $ld.Size) {
                    $sizeBytes = [int64]$ld.Size
                    $sizeMB    = [math]::Round($sizeBytes / 1MB)
                    $sizeText  = if ($sizeMB -ge 1024) {
                        "{0:N2} GB" -f ($sizeMB / 1024)
                    } else { "$sizeMB MB" }
                }
            } catch {}

            # Pull device number out of the NT path (\Device\ImDisk7 → 7)
            $devNum = $null
            if ($ntPath -match '\\Device\\ImDisk(\d+)') { $devNum = [int]$Matches[1] }

            [void]$seenLetters.Add($dl)
            $disks += @{
                deviceNumber = $devNum
                letter       = $dl
                sizeBytes    = $sizeBytes
                size         = $sizeText
                type         = 'RAM Disk'
                properties   = 'Volatile RAM'
            }
        }
    } catch {}

    return @{
        installed = $true
        disks     = @($disks)
    }
}

function Get-SystemRamInfo {
    # Physical memory totals so the UI can cap the size slider safely.
    try {
        $cs  = Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction Stop
        $os  = Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop
        $totalBytes = [int64]$cs.TotalPhysicalMemory
        $freeKB     = [int64]$os.FreePhysicalMemory
        $freeBytes  = $freeKB * 1024
        return @{
            totalBytes = $totalBytes
            totalMB    = [math]::Round($totalBytes / 1MB)
            freeBytes  = $freeBytes
            freeMB     = [math]::Round($freeBytes / 1MB)
        }
    } catch {
        return @{ totalBytes = 0; totalMB = 0; freeBytes = 0; freeMB = 0 }
    }
}

function New-RamDisk {
    param(
        [string]$SizeMB,
        [Alias('Letter')] [string]$DriveLetter,
        [string]$Filesystem = 'NTFS',
        [string]$Label = 'RAMDISK',
        [bool]$ReadOnly = $false,
        [bool]$Quick = $true
    )

    $exe = Get-ImDiskExe
    if (-not $exe) { return @{ status = 'error'; error = 'RAM Disk Engine not installed' } }

    if ([string]::IsNullOrWhiteSpace($SizeMB))      { return @{ status = 'error'; error = 'SizeMB is required' } }
    if ([string]::IsNullOrWhiteSpace($DriveLetter)) { return @{ status = 'error'; error = 'DriveLetter is required' } }

    $sizeInt = 0
    if (-not [int]::TryParse($SizeMB, [ref]$sizeInt)) {
        return @{ status = 'error'; error = "Invalid SizeMB: '$SizeMB'" }
    }
    if ($sizeInt -lt 256) {
        return @{ status = 'error'; error = 'RAM disks must be at least 256 MB' }
    }

    # Cap: total RAM minus a 3 GB headroom reserved for Windows itself and
    # resident processes. Anything tighter starves the system.
    $sysRam = Get-SystemRamInfo
    $headroomMB = 3072
    $capMB = [int]($sysRam.totalMB - $headroomMB)
    if ($capMB -lt 64) { $capMB = 64 }
    if ($sysRam.totalMB -gt 0 -and $sizeInt -gt $capMB) {
        return @{
            status = 'error'
            error  = "RAM disk size ($sizeInt MB) exceeds the cap ($capMB MB = total $($sysRam.totalMB) MB - 3 GB headroom)"
        }
    }

    $letter = $DriveLetter.TrimEnd('\','/',':').ToUpper()
    if ($letter.Length -ne 1) { return @{ status = 'error'; error = "Invalid DriveLetter: '$DriveLetter'" } }

    $fsMap = @{ 'NTFS' = 'ntfs'; 'FAT32' = 'fat32'; 'EXFAT' = 'exfat'; 'FAT' = 'fat' }
    $fsKey = $Filesystem.ToUpper()
    if (-not $fsMap.ContainsKey($fsKey)) {
        return @{ status = 'error'; error = "Unsupported filesystem: '$Filesystem'" }
    }
    $fmtArgs = @("/fs:$($fsMap[$fsKey])", '/y')
    if ($Quick) { $fmtArgs += '/q' }
    if ($Label) {
        $safeLabel = ($Label -replace '[^A-Za-z0-9_-]', '').Substring(0, [Math]::Min($Label.Length, 32))
        if ($safeLabel) { $fmtArgs += "/v:$safeLabel" }
    }
    $formatString = $fmtArgs -join ' '

    # imdisk's -o flag is comma-separated (`-o opt1,opt2`); passing two
    # separate `-o` switches loses the earlier one. Build a single list.
    $oFlags = @('rem')
    if ($ReadOnly) { $oFlags += 'ro' }
    $imArgs = @('-a', '-t', 'vm', '-s', "${sizeInt}M", '-m', "${letter}:", '-o', ($oFlags -join ','))
    $imArgs += @('-p', $formatString)

    $res = _Invoke-ImDisk -Arguments $imArgs
    if (-not $res.ok) {
        return @{ status = 'error'; error = "Attach failed (exit $($res.exitCode)): $($res.output.Trim())" }
    }

    for ($i = 0; $i -lt 15; $i++) {
        if (Test-Path "${letter}:\") { break }
        Start-Sleep -Milliseconds 200
    }

    @{ status = 'created'; drive = "${letter}:"; sizeMB = $sizeInt; filesystem = $Filesystem }
}

function Remove-RamDisk {
    # Detaches a RAM disk by drive letter. Tries multiple imdisk dismount
    # forms because the right one depends on how the disk was registered:
    #   1. `-D -m R:`        — force-detach by mount point (the usual case)
    #   2. `-d -m R:`        — graceful detach by mount point
    #   3. `-D -u <devNum>`  — force-detach by device number, when the
    #                          mount-point form fails (toolkit tray disks
    #                          are sometimes only addressable by unit #)
    # Returns success if ANY attempt clears the drive letter.
    param([Alias('Letter')] [string]$DriveLetter)

    $exe = Get-ImDiskExe
    if (-not $exe) { return @{ status = 'error'; error = 'RAM Disk Engine not installed' } }

    $letter = $DriveLetter.TrimEnd('\','/',':').ToUpper()
    if ($letter.Length -ne 1) { return @{ status = 'error'; error = "Invalid DriveLetter: '$DriveLetter'" } }

    # Look up the device number from QueryDosDevice so we have it as a
    # fallback if the mount-point dismount path fails.
    $devNum = $null
    try {
        if (-not ([System.Management.Automation.PSTypeName]'RdkDosDevices').Type) {
            Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class RdkDosDevices {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern uint QueryDosDevice(string lpDeviceName, StringBuilder lpTargetPath, int ucchMax);
}
'@ -ErrorAction SilentlyContinue
        }
        $sb = New-Object System.Text.StringBuilder 1024
        $len = [RdkDosDevices]::QueryDosDevice("${letter}:", $sb, 1024)
        if ($len -gt 0 -and $sb.ToString() -match '\\Device\\ImDisk(\d+)') {
            $devNum = [int]$Matches[1]
        }
    } catch {}

    $attempts = @(
        @{ args = @('-D', '-m', "${letter}:"); desc = 'force by letter' }
        @{ args = @('-d', '-m', "${letter}:"); desc = 'graceful by letter' }
    )
    if ($null -ne $devNum) {
        $attempts += @{ args = @('-D', '-u', "$devNum"); desc = "force by unit $devNum" }
        $attempts += @{ args = @('-d', '-u', "$devNum"); desc = "graceful by unit $devNum" }
    }

    $lastError = $null
    foreach ($a in $attempts) {
        $res = _Invoke-ImDisk -Arguments $a.args
        # Wait briefly for the drive letter to clear regardless of exit code —
        # imdisk sometimes reports non-zero while still successfully detaching.
        for ($i = 0; $i -lt 15; $i++) {
            if (-not (Test-Path "${letter}:\")) { break }
            Start-Sleep -Milliseconds 200
        }
        if (-not (Test-Path "${letter}:\")) {
            return @{ status = 'dismounted'; drive = "${letter}:" }
        }
        if (-not $res.ok) { $lastError = "$($a.desc): exit $($res.exitCode) $($res.output.Trim())" }
    }

    return @{ status = 'error'; error = "Detach failed. Last: $lastError" }
}

function Remove-AllRamDisks {
    $status = Get-RamDiskStatus
    if (-not $status.installed) { return @{ status = 'error'; error = 'RAM Disk Engine not installed' } }

    $detached = @()
    foreach ($d in $status.disks) {
        $r = Remove-RamDisk -DriveLetter $d.letter
        if ($r.status -eq 'dismounted') { $detached += $d.letter }
    }
    @{ status = 'all_dismounted'; drives = @($detached) }
}

function Open-RamDisk {
    param([string]$DriveLetter)
    $cleanLetter = $DriveLetter.Replace(':', '')
    $path = "${cleanLetter}:\"
    if (Test-Path $path) {
        Start-Process 'explorer.exe' -ArgumentList $path
        @{ status = 'opened'; drive = $DriveLetter }
    } else {
        @{ status = 'error'; error = "Drive ${cleanLetter}: not found" }
    }
}
