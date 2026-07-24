# ============================================================================
# PRIVACY - CLEANUP & HYGIENE MODULE
# Automated erases, history clearing, and cache management
# ============================================================================
#
# NOTE: the per-card auto-erase scheduler (Set-AutoEraseSchedule,
# Remove-AutoEraseSchedule, Get-AutoEraseSchedules,
# Get-AutoEraseSupportedCategories, Invoke-AutoEraseMigration) is NOT
# defined here. It lives in the shared module at
# commander-shared/scripts/auto-erase.ps1 — single source of truth for
# both commander-free and commander-pro. The Free backend prepends that
# module's contents to this one when loading "privacy/cleanup", so
# calling Set-AutoEraseSchedule from this module's runtime context works
# transparently.

# --- Manual Cleaning Actions ---

function Clear-Clipboard {
    try {
        Set-Clipboard -Value $null -ErrorAction SilentlyContinue
        Restart-Service cbdhsvc_* -Force -ErrorAction SilentlyContinue
        @{ status = 'cleared' }
    }
    catch {
        @{ error = $true; message = "Failed to clear clipboard: $($_.Exception.Message)" }
    }
}

function Get-ClipboardHistoryStatus {
    try {
        $cb    = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Clipboard" -EA SilentlyContinue
        # Also detect if an external policy (GPO/MDM we didn't set) is blocking things.
        $polH  = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" -Name "AllowClipboardHistory"    -EA SilentlyContinue
        $polC  = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" -Name "AllowCrossDeviceClipboard" -EA SilentlyContinue

        $histDisabled  = ($cb.EnableClipboardHistory -eq 0 -or $polH.AllowClipboardHistory    -eq 0)
        $cloudDisabled = ($cb.EnableCloudClipboard   -eq 0 -or $polC.AllowCrossDeviceClipboard -eq 0)

        # Enumerate actual clipboard history items via WinRT (Windows 10 1809+).
        # Falls back to an empty array gracefully if the API is unavailable.
        $historyItems = @()
        if (-not $histDisabled) {
            try {
                Add-Type -AssemblyName System.Runtime.WindowsRuntime -EA SilentlyContinue
                $null = [Windows.ApplicationModel.DataTransfer.Clipboard, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
                $null = [Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
                $null = [Windows.ApplicationModel.DataTransfer.StandardDataFormats, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]

                $asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
                    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.IsGenericType
                } | Select-Object -First 1

                if ($asTask) {
                    $asyncOp   = [Windows.ApplicationModel.DataTransfer.Clipboard]::GetHistoryItemsAsync()
                    $resultTy  = [Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult]
                    $task      = $asTask.MakeGenericMethod($resultTy).Invoke($null, @($asyncOp))

                    if ($task.Wait(2000) -and $task.Result -and $task.Result.Items) {
                        $sdfText = [Windows.ApplicationModel.DataTransfer.StandardDataFormats]::Text
                        foreach ($item in $task.Result.Items) {
                            try {
                                $dpv = $item.Content
                                if ($dpv.Contains($sdfText)) {
                                    $textTask = $asTask.MakeGenericMethod([string]).Invoke($null, @($dpv.GetTextAsync()))
                                    if ($textTask.Wait(500)) {
                                        $raw     = $textTask.Result
                                        $preview = ($raw -replace '[\r\n\t]+', ' ').Trim()
                                        if ($preview.Length -gt 80) { $preview = $preview.Substring(0, 80) + '…' }
                                        $historyItems += @{ type = 'text'; preview = $preview; charCount = $raw.Length }
                                    }
                                } else {
                                    $historyItems += @{ type = 'other'; preview = '[Image or other content]'; charCount = 0 }
                                }
                            } catch {}
                        }
                    }
                }
            } catch {}
        }

        @{
            clipboardHistoryDisabled = $histDisabled
            cloudClipboardDisabled   = $cloudDisabled
            historyItems             = $historyItems
        }
    }
    catch {
        @{ error = $true; message = "Failed to read clipboard history status: $($_.Exception.Message)" }
    }
}

function Restart-ClipboardService {
    # cbdhsvc caches EnableClipboardHistory at startup; restart so it re-reads the registry.
    Get-Service -Name "cbdhsvc*" -ErrorAction SilentlyContinue | Restart-Service -Force -ErrorAction SilentlyContinue
}

function Disable-ClipboardHistory {
    Assert-IsAdmin
    try {
        # User-preference keys turn the feature off at the OS level.
        foreach ($p in @("HKCU:\Software\Microsoft\Clipboard", "HKLM:\Software\Microsoft\Clipboard")) {
            if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
            Set-ItemProperty -Path $p -Name "EnableClipboardHistory" -Value 0 -Type DWord -Force
        }
        # Policy keys (HKCU + HKLM, value 0) show "Managed by your organization"
        # in Windows Settings and lock the toggle so the user can't re-enable it.
        # Enable-ClipboardHistory removes these keys so the banner disappears on re-enable.
        foreach ($p in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\System", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System")) {
            if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
            Set-ItemProperty -Path $p -Name "AllowClipboardHistory" -Value 0 -Type DWord -Force
        }
        Restart-ClipboardService
        @{ status = 'disabled' }
    }
    catch {
        @{ error = $true; message = "Failed to disable clipboard history: $($_.Exception.Message)" }
    }
}

function Enable-ClipboardHistory {
    Assert-IsAdmin
    try {
        foreach ($p in @("HKCU:\Software\Microsoft\Clipboard", "HKLM:\Software\Microsoft\Clipboard")) {
            if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
            Set-ItemProperty -Path $p -Name "EnableClipboardHistory" -Value 1 -Type DWord -Force
        }
        # Remove policy keys — any policy key present (even value=1) shows "Managed by organization".
        foreach ($p in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\System", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System")) {
            Remove-ItemProperty -Path $p -Name "AllowClipboardHistory" -ErrorAction SilentlyContinue
        }
        Restart-ClipboardService
        @{ status = 'enabled' }
    }
    catch {
        @{ error = $true; message = "Failed to enable clipboard history: $($_.Exception.Message)" }
    }
}

function Disable-CloudClipboardSync {
    Assert-IsAdmin
    try {
        $p = "HKCU:\Software\Microsoft\Clipboard"
        if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
        Set-ItemProperty -Path $p -Name "EnableCloudClipboard" -Value 0 -Type DWord -Force
        # Policy keys lock the toggle and show "Managed by organization".
        foreach ($pp in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\System", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System")) {
            if (!(Test-Path $pp)) { New-Item -Path $pp -Force | Out-Null }
            Set-ItemProperty -Path $pp -Name "AllowCrossDeviceClipboard" -Value 0 -Type DWord -Force
        }
        @{ status = 'disabled' }
    }
    catch {
        @{ error = $true; message = "Failed to disable cloud clipboard sync: $($_.Exception.Message)" }
    }
}

function Enable-CloudClipboardSync {
    Assert-IsAdmin
    try {
        $p = "HKCU:\Software\Microsoft\Clipboard"
        if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
        Set-ItemProperty -Path $p -Name "EnableCloudClipboard" -Value 1 -Type DWord -Force
        foreach ($pp in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\System", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System")) {
            Remove-ItemProperty -Path $pp -Name "AllowCrossDeviceClipboard" -ErrorAction SilentlyContinue
        }
        @{ status = 'enabled' }
    }
    catch {
        @{ error = $true; message = "Failed to enable cloud clipboard sync: $($_.Exception.Message)" }
    }
}

function Get-PSHistory {
    try {
        # PSReadLine is unavailable in non-interactive sessions - use known default path.
        # Use [System.IO.File]::ReadAllLines for speed (avoids O(n^2) PS array += loop).
        $appData = [Environment]::GetFolderPath('ApplicationData')
        $historyPath = Join-Path $appData 'Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt'
        $entries = @()
        if (Test-Path $historyPath) {
            $lines = [System.IO.File]::ReadAllLines($historyPath)
            # Return last 300 (most recent), reversed so newest is first
            $start = [Math]::Max(0, $lines.Length - 300)
            $slice = $lines[$start..($lines.Length - 1)]
            [Array]::Reverse($slice)
            $idx = 0
            foreach ($line in $slice) {
                $trimmed = $line.Trim()
                if ($trimmed -ne '') {
                    $entries += @{ id = $idx; command = $trimmed }
                    $idx++
                }
            }
        }
        @{ entries = $entries; total = $entries.Count }
    }
    catch {
        @{ error = $true; message = "Failed to read PowerShell history: $($_.Exception.Message)" }
    }
}

function Get-EventLogSummary {
    try {
        $summaries = @()
        $logNames = [System.Diagnostics.Eventing.Reader.EventLogSession]::GlobalSession.GetLogNames()
        foreach ($name in $logNames) {
            try {
                $logInfo = [System.Diagnostics.Eventing.Reader.EventLogSession]::GlobalSession.GetLogInformation($name, [System.Diagnostics.Eventing.Reader.PathType]::LogName)
                $count = $logInfo.RecordCount
                if ($count -gt 0) {
                    $summaries += @{
                        name    = $name
                        count   = [long]$count
                        newest  = if ($logInfo.LastWriteTime) { $logInfo.LastWriteTime.ToString('o') } else { $null }
                        sizeMb  = if ($logInfo.FileSize) { [math]::Round($logInfo.FileSize / 1MB, 2) } else { $null }
                    }
                }
            } catch {}
        }
        # Sort by record count descending
        $sorted = @($summaries | Sort-Object { [long]$_.count } -Descending)
        @{ logs = $sorted; total = $sorted.Count }
    }
    catch {
        @{ error = $true; message = "Failed to read event log summary: $($_.Exception.Message)" }
    }
}

function Get-SRUMData {
    try {
        $entries = @()
        # SRUM full read requires parsing the SRUDB.dat ESE database, which is
        # locked while the service is running. As a useful proxy we surface
        # the current process roster (top resource consumers) — same data SRUM
        # tracks over time. Get-CimInstance replaces Get-WmiObject which is
        # removed in PowerShell 7+; the old code silently returned no rows.
        try {
            $processes = Get-CimInstance -ClassName Win32_Process -ErrorAction Stop
            if ($processes) {
                foreach ($p in $processes) {
                    try {
                        $name = if ($p.Name) { $p.Name -replace '\.exe$','' } else { 'Unknown' }
                        $path = if ($p.ExecutablePath) { $p.ExecutablePath } else { '' }
                        $pid_ = $p.ProcessId
                        $owner = ''
                        try {
                            $ownerResult = Invoke-CimMethod -InputObject $p -MethodName GetOwner -ErrorAction SilentlyContinue
                            if ($ownerResult -and $ownerResult.ReturnValue -eq 0) {
                                $owner = "$($ownerResult.Domain)\$($ownerResult.User)"
                            }
                        } catch {}
                        $entries += @{
                            name        = $name
                            pid         = $pid_
                            path        = $path
                            owner       = $owner
                            cpuTime     = [int64]($p.KernelModeTime + $p.UserModeTime)
                            memoryKB    = [math]::Round(($p.WorkingSetSize / 1KB))
                            threadCount = $p.ThreadCount
                        }
                    } catch {}
                }
            }
        } catch {
            # CIM unavailable — fall back to Get-Process so the dialog never
            # appears empty on a healthy machine just because CIM/WMI is sick.
            $procs = Get-Process -ErrorAction SilentlyContinue
            foreach ($p in $procs) {
                try {
                    $entries += @{
                        name        = $p.ProcessName
                        pid         = $p.Id
                        path        = $p.Path
                        owner       = ''
                        cpuTime     = if ($p.CPU) { [int64]($p.CPU * 10000000) } else { 0 }
                        memoryKB    = [math]::Round(($p.WorkingSet64 / 1KB))
                        threadCount = $p.Threads.Count
                    }
                } catch {}
            }
        }

        # Sort by CPU time descending, take top 50
        $sorted = $entries | Sort-Object { $_.cpuTime } -Descending | Select-Object -First 50

        # Also return SRUM file size if available
        $srumPath = "$env:SystemRoot\System32\sru\SRUDB.dat"
        $srumSizeMb = $null
        if (Test-Path $srumPath) {
            try { $srumSizeMb = [math]::Round((Get-Item $srumPath).Length / 1MB, 2) } catch {}
        }

        @{ entries = @($sorted); srumSizeMb = $srumSizeMb; total = $entries.Count }
    }
    catch {
        @{ error = $true; message = "Failed to read SRUM data: $($_.Exception.Message)" }
    }
}

function Get-ConnectivityHistory {
    try {
        $entries = @()
        $roots = @(
            @{
                source = 'Network Profiles'
                type = 'Profile'
                path = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Profiles'
            },
            @{
                source = 'Unmanaged Signatures'
                type = 'Signature'
                path = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Signatures\Unmanaged'
            },
            @{
                source = 'Managed Signatures'
                type = 'Signature'
                path = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Signatures\Managed'
            }
        )

        foreach ($root in $roots) {
            if (!(Test-Path $root.path)) { continue }

            Get-ChildItem -Path $root.path -ErrorAction SilentlyContinue | ForEach-Object {
                try {
                    $props = Get-ItemProperty -Path $_.PSPath -ErrorAction SilentlyContinue
                    $name = $props.ProfileName
                    if ([string]::IsNullOrWhiteSpace($name)) { $name = $props.Description }
                    if ([string]::IsNullOrWhiteSpace($name)) { $name = $props.DnsSuffix }
                    if ([string]::IsNullOrWhiteSpace($name)) { $name = $_.PSChildName }

                    $entries += @{
                        source      = $root.source
                        type        = $root.type
                        name        = $name
                        key         = $_.PSChildName
                        description = $props.Description
                        dnsSuffix   = $props.DnsSuffix
                        category    = $props.Category
                    }
                } catch {}
            }
        }

        @{
            entries = $entries
            total   = $entries.Count
        }
    }
    catch {
        @{ error = $true; message = "Failed to read connectivity history: $($_.Exception.Message)" }
    }
}

function Get-USBDeviceHistory {
    try {
        $devices = @()
        $root = 'HKLM:\SYSTEM\CurrentControlSet\Enum\USBSTOR'
        if (Test-Path $root) {
            Get-ChildItem $root -ErrorAction SilentlyContinue | ForEach-Object {
                $deviceClass = $_
                Get-ChildItem $deviceClass.PSPath -ErrorAction SilentlyContinue | ForEach-Object {
                    $props = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
                    $devices += @{
                        deviceId     = $_.PSChildName
                        friendlyName = $props.FriendlyName
                        manufacturer = $props.Mfg
                        className    = $props.Class
                    }
                }
            }
        }
        @{ devices = $devices }
    }
    catch {
        @{ error = $true; message = "Failed to read USB history: $($_.Exception.Message)" }
    }
}

function Get-BluetoothDevices {
    try {
        # Running as SYSTEM to access BTHPORT keys which are often locked to Admin
        $scriptContent = @"
`$devices = @()
`$btPath = 'HKLM:\SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices'
if (Test-Path `$btPath) {
    Get-ChildItem `$btPath -ErrorAction SilentlyContinue | ForEach-Object {
        `$mac = `$_.PSChildName
        `$props = Get-ItemProperty `$_.PSPath -ErrorAction SilentlyContinue
        `$name = if (`$props.Name) { 
            [System.Text.Encoding]::ASCII.GetString(`$props.Name).Trim([char]0) 
        } else { "Unknown" }
        
        `$lastSeen = "Unknown"
        if (`$props.LastSeen) {
            `$lastSeen = [DateTime]::FromFileTime(`$props.LastSeen).ToString("o")
        }

        `$devices += @{
            id = `$mac
            name = `$name
            lastSeen = `$lastSeen
        }
    }
}
`$devices | ConvertTo-Json -Compress
"@
        
        $tempScript = "$env:TEMP\System_BtGet.ps1"
        $outputFile = "$env:TEMP\System_BtOut.json"
        
        # Wrapper to capture output
        $wrapper = @"
`$res = & { $scriptContent }
`$res | Out-File -FilePath '$outputFile' -Encoding ASCII -Force
"@
        $wrapper | Out-File -FilePath $tempScript -Encoding ASCII -Force

        $taskName = "WinCommander_BtScan_System"
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
        
        $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-ExecutionPolicy Bypass -WindowStyle Hidden -File `"$tempScript`""
        $principal = New-ScheduledTaskPrincipal -UserId "NT AUTHORITY\SYSTEM" -LogonType ServiceAccount -RunLevel Highest
        Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
        
        Start-ScheduledTask -TaskName $taskName
        
        # Wait for completion
        $timeout = 10
        while ($timeout -gt 0) {
            $task = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
            if (-not $task -or ($task.State -ne 'Running' -and $task.State -ne 'Queued')) { break }
            Start-Sleep -Seconds 1
            $timeout--
        }
        
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
        Invoke-7Erase -Path $tempScript -Type File

        $devices = @()
        if (Test-Path $outputFile) {
            $json = Get-Content $outputFile -Raw -ErrorAction SilentlyContinue
            if ($json) {
                $devices = $json | ConvertFrom-Json
            }
            Invoke-7Erase -Path $outputFile -Type File
        }

        # Ensure array
        if ($devices -is [PSCustomObject]) { $devices = @($devices) }

        @{ devices = $devices }
    }
    catch {
        @{ error = $true; message = "Failed to get Bluetooth devices: $($_.Exception.Message)" }
    }
}

function Get-NetworkDrives {
    try {
        $drivesList = @()
        
        # 1. Active Mapped Drives
        $psDrives = Get-PSDrive -PSProvider FileSystem | Where-Object { $null -ne $_.DisplayRoot }
        foreach ($d in $psDrives) {
            $drivesList += @{
                Name        = $d.Name
                DisplayRoot = $d.DisplayRoot
            }
        }
        
        # 2. Remembered Drives (Registry)
        $mapNetDrv = 'HKCU:\Network'
        if (Test-Path $mapNetDrv) {
            Get-ChildItem $mapNetDrv -ErrorAction SilentlyContinue | ForEach-Object {
                $driveLetter = $_.PSChildName
                $props = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
                if ($props.RemotePath) {
                    $remotePath = $props.RemotePath
                    # Avoid duplicates if active (case insensitive check)
                    $exists = $false
                    foreach ($existing in $drivesList) {
                        if ($existing.Name -eq $driveLetter) { $exists = $true; break }
                    }
                    
                    if (-not $exists) {
                        $drivesList += @{
                            Name        = $driveLetter
                            DisplayRoot = $remotePath
                        }
                    }
                }
            }
        }

        # Force array return to avoid JSON object issues
        @{ drives = @($drivesList) }
    }
    catch {
        @{ error = $true; message = "Failed to get network drives: $($_.Exception.Message)" }
    }
}

function Get-DnsCacheEntries {
    try {
        $entries = @()
        $blocklistDomains = @{}
        $hostsPath = 'C:\Windows\System32\drivers\etc\hosts'
        if (Test-Path $hostsPath) {
            $inVaultSection = $false
            Get-Content $hostsPath -ErrorAction SilentlyContinue | ForEach-Object {
                $line = $_.Trim()
                if ($line -match '^#\s*WINCOMMANDER_BLOCKLIST_START') { $inVaultSection = $true; return }
                if ($line -match '^#\s*WINCOMMANDER_BLOCKLIST_END') { $inVaultSection = $false; return }
                if (-not $inVaultSection) { return }
                if ($line -match '^(0\.0\.0\.0|127\.0\.0\.1)\s+([^\s#]+)') {
                    $domain = $Matches[2].ToLowerInvariant().TrimEnd('.')
                    if (-not $blocklistDomains.ContainsKey($domain)) { $blocklistDomains[$domain] = $true }
                }
            }
        }

        $isBlockedDomain = {
            param([string]$Name)
            if ([string]::IsNullOrWhiteSpace($Name)) { return $false }
            $normalized = $Name.ToLowerInvariant().TrimEnd('.')
            if ($blocklistDomains.ContainsKey($normalized)) { return $true }
            $current = $normalized
            while ($current -match '\.') {
                $current = $current.Substring($current.IndexOf('.') + 1)
                if ($blocklistDomains.ContainsKey($current)) { return $true }
            }
            return $false
        }

        # DNS record-type numeric codes → human-readable names. PowerShell's
        # Get-DnsClientCache returns these as integers; without translation
        # the detail dialog showed cryptic numbers like 1, 28, 5.
        $recordTypeMap = @{
            1 = 'A'; 2 = 'NS'; 5 = 'CNAME'; 6 = 'SOA'; 12 = 'PTR'; 13 = 'HINFO';
            15 = 'MX'; 16 = 'TXT'; 17 = 'RP'; 18 = 'AFSDB'; 24 = 'SIG'; 25 = 'KEY';
            28 = 'AAAA'; 29 = 'LOC'; 33 = 'SRV'; 35 = 'NAPTR'; 36 = 'KX';
            37 = 'CERT'; 39 = 'DNAME'; 41 = 'OPT'; 42 = 'APL'; 43 = 'DS';
            44 = 'SSHFP'; 45 = 'IPSECKEY'; 46 = 'RRSIG'; 47 = 'NSEC'; 48 = 'DNSKEY';
            49 = 'DHCID'; 50 = 'NSEC3'; 51 = 'NSEC3PARAM'; 52 = 'TLSA';
            53 = 'SMIMEA'; 55 = 'HIP'; 59 = 'CDS'; 60 = 'CDNSKEY';
            61 = 'OPENPGPKEY'; 62 = 'CSYNC'; 63 = 'ZONEMD'; 64 = 'SVCB';
            65 = 'HTTPS'; 99 = 'SPF'; 257 = 'CAA';
        }
        $statusMap = @{
            0 = 'Success'; 9003 = 'NXDomain'; 9501 = 'No Records';
            9701 = 'Bad Packet'; 9702 = 'No Packet';
        }

        $cache = Get-DnsClientCache -ErrorAction SilentlyContinue
        foreach ($c in $cache) {
            $name = ([string]$c.Entry).Trim()
            if (& $isBlockedDomain $name) { continue }

            # Exclude reverse lookups and localhost/null artifacts
            if ($name.EndsWith(".in-addr.arpa", [System.StringComparison]::InvariantCultureIgnoreCase) -or
                $name -eq "0.0.0.0" -or
                $name -eq ".") { continue }

            $rtNum = $null
            try { $rtNum = [int]$c.RecordType } catch {}
            $rtName = if ($rtNum -ne $null -and $recordTypeMap.ContainsKey($rtNum)) {
                $recordTypeMap[$rtNum]
            } else {
                "$($c.RecordType)"
            }

            $stNum = $null
            try { $stNum = [int]$c.Status } catch {}
            $stName = if ($stNum -ne $null -and $statusMap.ContainsKey($stNum)) {
                $statusMap[$stNum]
            } else {
                "$($c.Status)"
            }

            $section = "$($c.Section)"
            $dataClean = ($c.Data | Out-String).Trim()

            $entries += @{
                name        = $name
                data        = $dataClean
                recordType  = $rtName
                recordTypeRaw = $rtNum
                status      = $stName
                ttl         = $c.TimeToLive
                section     = $section
                dataLength  = $c.DataLength
                timeStamp   = "$($c.TimeStamp)"
            }
        }
        @{ entries = $entries }
    }
    catch {
        @{ error = $true; message = "Failed to read DNS cache: $($_.Exception.Message)" }
    }
}

function Get-ProcessIntelligence {
    try {
        $entries = @()
        $procs = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Id -gt 0 }
        foreach ($p in $procs) {
            $path = $null
            $sigStatus = "UnknownError"
            $signer = $null
            try {
                $path = $p.MainModule.FileName
            }
            catch {}
            if ($path -and (Test-Path -LiteralPath $path -ErrorAction SilentlyContinue)) {
                try {
                    $sig = Get-AuthenticodeSignature -LiteralPath $path -ErrorAction SilentlyContinue
                    if ($sig) {
                        $sigStatus = $sig.Status.ToString()
                        if ($sig.Status -eq 'Valid' -and $sig.SignerCertificate) {
                            $signer = $sig.SignerCertificate.Subject
                        }
                    }
                }
                catch {
                    $sigStatus = "UnknownError"
                }
            }
            $elevated = "No"
            try {
                # Get-WmiObject is gone in PowerShell 7+; Get-CimInstance + Invoke-CimMethod
                # is the modern path. Silently fallback if neither cooperates.
                $cimProc = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId=$($p.Id)" -ErrorAction SilentlyContinue
                if ($cimProc) {
                    $owner = Invoke-CimMethod -InputObject $cimProc -MethodName GetOwner -ErrorAction SilentlyContinue
                    if ($owner -and $owner.User -eq 'SYSTEM') { $elevated = 'Yes' }
                }
            }
            catch {}
            $entries += @{
                name     = $p.ProcessName
                pid      = $p.Id
                path     = if ($path) { $path } else { "" }
                signed   = $sigStatus
                signer   = $signer
                elevated = $elevated
            }
        }
        @{ processes = $entries }
    }
    catch {
        @{ error = $true; message = "Failed to get process intelligence: $($_.Exception.Message)"; processes = @() }
    }
}

function Get-ExecutionCache {
    try {
        $entries = @()
        $seen = @{}
        $storePath = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store'
        $persistPath = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Persisted'
        $userAssistRoot = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\UserAssist'
        $muiCachePath = 'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache'
        $recentAppsPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Search\RecentApps'

        $addEntry = {
            param([string]$Path, [string]$Source)
            if ([string]::IsNullOrWhiteSpace($Path)) { return }
            if (-not $seen.ContainsKey($Path)) {
                $seen[$Path] = $true
                $entries += @{
                    path   = $Path
                    source = $Source
                }
            }
        }

        foreach ($path in @($storePath, $persistPath)) {
            if (Test-Path $path) {
                # Add logic to handle different registry structures if needed
                $props = Get-ItemProperty -Path $path -ErrorAction SilentlyContinue 
                if ($props) {
                    $props.PSObject.Properties | ForEach-Object {
                        if ($_.Name -notin @('PSPath', 'PSParentPath', 'PSChildName', 'PSDrive', 'PSProvider')) {
                            . $addEntry $_.Name (Split-Path -Leaf $path)
                        }
                    }
                }
            }
        }

        # BAM omitted: kernel-protected, cannot be cleared; only clearable sources shown so Clear leaves list empty
        # if (Test-Path $bamRoot) { ... }

        if (Test-Path $userAssistRoot) {
            Get-ChildItem $userAssistRoot -ErrorAction SilentlyContinue | ForEach-Object {
                $countPath = Join-Path $_.PSPath 'Count'
                if (Test-Path $countPath) {
                    $props = Get-ItemProperty -Path $countPath -ErrorAction SilentlyContinue | Select-Object -Property *
                    $props.PSObject.Properties | ForEach-Object {
                        if ($_.Name -notmatch '^PS(.*)') {
                            $decoded = ($_.Name.ToCharArray() | ForEach-Object {
                                    $c = [int][char]$_
                                    if ($c -ge 65 -and $c -le 90) { [char](((($c - 65) + 13) % 26) + 65) }
                                    elseif ($c -ge 97 -and $c -le 122) { [char](((($c - 97) + 13) % 26) + 97) }
                                    else { [char]$c }
                                }) -join ''
                            $decoded = $decoded -replace '^\\\?\\', ''
                            . $addEntry $decoded 'UserAssist'
                        }
                    }
                }
            }
        }

        if (Test-Path $muiCachePath) {
            $props = Get-ItemProperty -Path $muiCachePath -ErrorAction SilentlyContinue | Select-Object -Property *
            $props.PSObject.Properties | ForEach-Object {
                if ($_.Name -notmatch '^PS(.*)' -and $_.Name -ne 'LangID') {
                    . $addEntry $_.Name 'MuiCache'
                }
            }
        }

        if (Test-Path $recentAppsPath) {
            Get-ChildItem $recentAppsPath -ErrorAction SilentlyContinue | ForEach-Object {
                $appPath = $_.GetValue('AppPath')
                if ($appPath) { . $addEntry $appPath 'RecentApps' }
            }
        }
        
        # Check ShimCache (AppCompatCache) existence (just counting as one entry if present, or listing size)
        $shimCachePath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\AppCompatCache'
        if (Test-Path $shimCachePath) {
            $shimData = Get-ItemProperty -Path $shimCachePath -Name 'AppCompatCache' -ErrorAction SilentlyContinue
            if ($shimData -and $shimData.AppCompatCache) {
                . $addEntry "ShimCache (Binary Blob - $($shimData.AppCompatCache.Length) bytes)" "ShimCache"
            }
        }

        @{ entries = $entries }
    }
    catch {
        @{ error = $true; message = "Failed to read execution cache: $($_.Exception.Message)" }
    }
}

# --- Privacy Protection Logic ---

function Get-PrivacyProtectionStatus {
    try {
        $memPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management'
        $explorerPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
        $prefetchPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
        
        $pagefile = (Get-ItemProperty -Path $memPath -Name 'PagingFiles' -ErrorAction SilentlyContinue).PagingFiles
        $pagefileDisabled = [string]::IsNullOrWhiteSpace($pagefile)
        
        $prefetch = (Get-ItemProperty -Path $prefetchPath -Name 'EnablePrefetcher' -ErrorAction SilentlyContinue).EnablePrefetcher
        $prefetchDisabled = ($prefetch -eq 0)
        
        $trackDocs = (Get-ItemProperty -Path $explorerPath -Name 'Start_TrackDocs' -ErrorAction SilentlyContinue).Start_TrackDocs
        $recentFilesDisabled = ($trackDocs -eq 0)
        
        $trackProgs = (Get-ItemProperty -Path $explorerPath -Name 'Start_TrackProgs' -ErrorAction SilentlyContinue).Start_TrackProgs
        $jumpListsDisabled = ($trackProgs -eq 0)
        
        $thumbCache = (Get-ItemProperty -Path $explorerPath -Name 'DisableThumbnailCache' -ErrorAction SilentlyContinue).DisableThumbnailCache
        $thumbCacheDisabled = ($thumbCache -eq 1)
        
        @{
            enabled        = (@($pagefileDisabled, $prefetchDisabled, $recentFilesDisabled, $jumpListsDisabled, $thumbCacheDisabled) | Where-Object { $_ } | Measure-Object).Count -ge 3
            pagefile       = $pagefileDisabled
            prefetch       = $prefetchDisabled
            recentFiles    = $recentFilesDisabled
            jumpLists      = $jumpListsDisabled
            thumbnailCache = $thumbCacheDisabled
        }
    }
    catch {
        @{ enabled = $false; error = $_.Exception.Message }
    }
}

function Enable-PrivacyProtection {
    try {
        Disable-Pagefile | Out-Null
        Disable-Prefetch | Out-Null
        Disable-RecentFilesTracking | Out-Null
        Disable-JumpLists | Out-Null
        Disable-ThumbnailCache | Out-Null
        Get-PrivacyProtectionStatus
    }
    catch {
        @{ error = $true; message = "Failed to enable privacy protection: $($_.Exception.Message)" }
    }
}

function Disable-PrivacyProtection {
    try {
        Enable-Pagefile | Out-Null
        Enable-Prefetch | Out-Null
        Enable-RecentFilesTracking | Out-Null
        Enable-JumpLists | Out-Null
        Enable-ThumbnailCache | Out-Null
        Get-PrivacyProtectionStatus
    }
    catch {
        @{ error = $true; message = "Failed to disable privacy protection: $($_.Exception.Message)" }
    }
}

function Disable-Pagefile {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' -Name 'PagingFiles' -Value '' -Type MultiString -Force
    Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' -Name 'ClearPageFileAtShutdown' -Value 1 -Type DWord -Force
    return @{ status = 'disabled' }
}

function Enable-Pagefile {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' -Name 'PagingFiles' -Value '?:\pagefile.sys' -Type MultiString -Force
    return @{ status = 'enabled' }
}

function Disable-Prefetch {
    Assert-IsAdmin
    $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name 'EnablePrefetcher' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'EnableSuperfetch' -Value 0 -Type DWord -Force -ErrorAction SilentlyContinue
    return @{ status = 'disabled' }
}

function Enable-Prefetch {
    Assert-IsAdmin
    $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
    Set-ItemProperty -Path $path -Name 'EnablePrefetcher' -Value 3 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'EnableSuperfetch' -Value 3 -Type DWord -Force -ErrorAction SilentlyContinue
    return @{ status = 'enabled' }
}

function Disable-RecentFilesTracking {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'Start_TrackDocs' -Value 0 -Type DWord -Force
    return @{ status = 'disabled' }
}

function Enable-RecentFilesTracking {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'Start_TrackDocs' -Value 1 -Type DWord -Force
    return @{ status = 'enabled' }
}

function Disable-JumpLists {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'Start_TrackProgs' -Value 0 -Type DWord -Force
    return @{ status = 'disabled' }
}

function Enable-JumpLists {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'Start_TrackProgs' -Value 1 -Type DWord -Force
    return @{ status = 'enabled' }
}

# ────────────────────────────────────────────────────────────────────
# Phase E hide-recent toggles
# ────────────────────────────────────────────────────────────────────
# All HKCU writes — no admin required. Each toggle's "disabled" sense
# means "history surface is hidden / not recorded". Convention:
#   Disable-X → set the registry value that hides the history
#   Enable-X  → restore default (history visible / recorded)

# --- Quick Access "Recent files" group in File Explorer Home view
function Disable-QuickAccessRecent {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowRecent' -Value 0 -Type DWord -Force
    return @{ status = 'disabled' }
}
function Enable-QuickAccessRecent {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowRecent' -Value 1 -Type DWord -Force
    return @{ status = 'enabled' }
}
function Get-QuickAccessRecentStatus {
    $v = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowRecent' -ErrorAction SilentlyContinue
    return @{ disabled = ($v.ShowRecent -eq 0) }
}

# --- Quick Access "Frequent folders" group in File Explorer Home view
function Disable-QuickAccessFrequent {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowFrequent' -Value 0 -Type DWord -Force
    return @{ status = 'disabled' }
}
function Enable-QuickAccessFrequent {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowFrequent' -Value 1 -Type DWord -Force
    return @{ status = 'enabled' }
}
function Get-QuickAccessFrequentStatus {
    $v = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowFrequent' -ErrorAction SilentlyContinue
    return @{ disabled = ($v.ShowFrequent -eq 0) }
}

# --- Win+R "Run" dialog history (the dropdown of recently typed cmds)
function Disable-RunMRU {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer'
    if (-not (Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name 'NoRunMRU' -Value 1 -Type DWord -Force
    # Also erase the existing RunMRU keys so historical entries vanish.
    Remove-Item -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU' -Recurse -Force -ErrorAction SilentlyContinue
    return @{ status = 'disabled' }
}
function Enable-RunMRU {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer'
    Remove-ItemProperty -Path $path -Name 'NoRunMRU' -ErrorAction SilentlyContinue
    return @{ status = 'enabled' }
}
function Get-RunMRUStatus {
    $v = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer' -Name 'NoRunMRU' -ErrorAction SilentlyContinue
    return @{ disabled = ($v.NoRunMRU -eq 1) }
}

# --- Windows Search box history ("recent searches")
function Disable-SearchHistory {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\SearchSettings'
    if (-not (Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name 'IsDeviceSearchHistoryEnabled' -Value 0 -Type DWord -Force
    return @{ status = 'disabled' }
}
function Enable-SearchHistory {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\SearchSettings' -Name 'IsDeviceSearchHistoryEnabled' -Value 1 -Type DWord -Force
    return @{ status = 'enabled' }
}
function Get-SearchHistoryStatus {
    $v = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\SearchSettings' -Name 'IsDeviceSearchHistoryEnabled' -ErrorAction SilentlyContinue
    return @{ disabled = ($v.IsDeviceSearchHistoryEnabled -eq 0) }
}

function Disable-ThumbnailCache {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
    Set-ItemProperty -Path $path -Name 'DisableThumbnailCache' -Value 1 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'DisableThumbsDBOnNetworkFolders' -Value 1 -Type DWord -Force
    # Best-effort shred of any existing thumbnail-cache DBs. Expand the wildcard
    # HERE: Invoke-7Erase resolves with -LiteralPath (so it never expands globs),
    # and the files may not exist at all (fresh profile / already cleared). A
    # missing cache is NOT an error — the registry keys above are the real
    # disable, so we iterate the matched files and skip cleanly when there are none.
    Get-ChildItem -Path "$env:LOCALAPPDATA\Microsoft\Windows\Explorer\thumbcache_*.db" -Force -ErrorAction SilentlyContinue |
        ForEach-Object { Invoke-7Erase -Path $_.FullName -Type File }
    return @{ status = 'disabled' }
}

function Enable-ThumbnailCache {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
    Set-ItemProperty -Path $path -Name 'DisableThumbnailCache' -Value 0 -Type DWord -Force
    Remove-ItemSecure -Path $path -Name 'DisableThumbsDBOnNetworkFolders' -ErrorAction SilentlyContinue
    return @{ status = 'enabled' }
}

# --- High-Intensity Privacy Clean ---

function Get-ShellBags {
    try {
        $entries = @()
        $seen = @{}
        # Use only primary BagMRU (Local Settings) - the second path often duplicates or is empty
        $mruRoots = @("HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU")
        foreach ($root in $mruRoots) {
            if (-not (Test-Path $root)) { continue }
            $rootItem = Get-Item $root -ErrorAction SilentlyContinue
            if (-not $rootItem) { continue }
            $stack = @(@{ key = $rootItem; path = "" })
            while ($stack.Count -gt 0) {
                $cur = $stack[-1]
                if ($stack.Count -eq 1) { $stack = @() } else { $stack = $stack[0..($stack.Count - 2)] }
                $key = $cur.key
                $relPath = $cur.path
                $fullPath = if ($relPath) { $relPath } else { "(root)" }
                $lastMod = if ($key.LastWriteTime) { $key.LastWriteTime.ToString("o") } else { "" }
                $id = $key.PSPath
                if (-not $seen.ContainsKey($id)) {
                    $seen[$id] = $true
                    $pathHint = $fullPath
                    try {
                        $props = Get-ItemProperty -Path $key.PSPath -ErrorAction SilentlyContinue
                        if ($props -and $props.PSObject.Properties["NodeSlot"]) {
                            $nodeSlot = $props.NodeSlot
                            $bagsPaths = @(
                                "HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags",
                                "HKCU:\Software\Microsoft\Windows\Shell\Bags"
                            )
                            foreach ($bp in $bagsPaths) {
                                if (Test-Path $bp) {
                                    $bagKeys = Get-ChildItem $bp -Recurse -ErrorAction SilentlyContinue | Where-Object { [string]$_.PSChildName -eq [string]$nodeSlot }
                                    foreach ($bk in $bagKeys) {
                                        $itemPos = (Get-ItemProperty -Path $bk.PSPath -Name "ItemPos" -ErrorAction SilentlyContinue).ItemPos
                                        if ($itemPos -and [string]::IsNullOrWhiteSpace([string]$itemPos) -eq $false) {
                                            $pathHint = [string]$itemPos
                                            break
                                        }
                                    }
                                    if ($pathHint -ne $fullPath) { break }
                                }
                            }
                        }
                    }
                    catch {}
                    # Use friendly labels for internal registry structure
                    $displayPath = if ($pathHint -eq "(root)") { "ShellBag root" } elseif ($pathHint -match '^\d+(\\\d+)*$') { "Folder view ($pathHint)" } else { $pathHint }
                    $entries += @{ path = $displayPath; lastModified = $lastMod; source = "ShellBags" }
                }
                Get-ChildItem $key.PSPath -ErrorAction SilentlyContinue | ForEach-Object {
                    $childPath = if ($relPath) { "$relPath\$($_.PSChildName)" } else { $_.PSChildName }
                    $stack += @{ key = $_; path = $childPath }
                }
            }
        }
        @{ entries = $entries }
    }
    catch {
        @{ error = $true; message = "Failed to get ShellBags: $($_.Exception.Message)" }
    }
}

function Get-WlanProfiles {
    try {
        $profiles = @()
        $netshOutput = netsh wlan show profiles
        $profileNames = $netshOutput | Select-String "All User Profile" | ForEach-Object { ($_.ToString().Split(':'))[1].Trim() }
        
        foreach ($name in $profileNames) {
            $passOutput = netsh wlan show profile name="$name" key=clear
            $password = $null
            $passLine = $passOutput | Select-String "Key Content"
            if ($passLine) {
                $password = ($passLine.ToString().Split(':'))[1].Trim()
            }
            
            $profiles += @{
                name     = $name
                password = $password
            }
        }
        @{ profiles = $profiles }
    }
    catch {
        @{ error = $true; message = "Failed to get WLAN profiles: $($_.Exception.Message)" }
    }
}

function Get-BluetoothDevices {
    $debugLog = @()
    $devices = @()
    $seenNames = @{}

    # PHASE 1: Direct Read (User Context)
    $debugLog += "Phase 1: Direct Scan"
    try {
        # Helper to check if a name is a generic system component
        $isSystem = {
            param($n)
            $generic = @("Microsoft Bluetooth", "RFCOMM", "Personal Area Network", "BTHBRB", "BTHLE", "Generic Bluetooth", "Bluetooth LE")
            foreach ($g in $generic) { if ($n -match $g) { return $true } }
            return $false
        }

        # Helper to extract MAC or clean ID
        $cleanId = {
            param($id)
            if ($id -match 'BluetoothDevice_([0-9A-Fa-f]{12})') { return $Matches[1].ToUpper() }
            if ($id -match '_([0-9A-Fa-f]{12})_') { return $Matches[1].ToUpper() }
            if ($id -match '^([0-9A-Fa-f]{12})$') { return $Matches[1].ToUpper() } # Direct MAC
            return $id
        }

        # 1. Check BTHPORT (Best Names for Paired Devices)
        $btPath = 'HKLM:\SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices'
        if (Test-Path $btPath) {
            Get-ChildItem $btPath -ErrorAction SilentlyContinue | ForEach-Object {
                $id = $_.PSChildName
                $props = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
                $name = if ($props.Name) { try { [System.Text.Encoding]::ASCII.GetString($props.Name).Trim([char]0) } catch { "Unknown Device" } } else { "Unknown Device" }
                
                if (-not (&$isSystem $name) -and -not $seenNames[$name]) {
                    $lastSeen = if ($props.LastSeen) { try { [DateTime]::FromFileTime($props.LastSeen).ToString("yyyy-MM-dd HH:mm:ss") } catch { "Registry Traces" } } else { "Registry Traces" }
                    $devices += @{ id = (&$cleanId $id); name = $name; lastSeen = $lastSeen }
                    $seenNames[$name] = $true
                }
            }
        }

        # 2. Check Enum History (Traces)
        $enumPaths = @('HKLM:\SYSTEM\CurrentControlSet\Enum\BTHENUM', 'HKLM:\SYSTEM\CurrentControlSet\Enum\BTH')
        foreach ($p in $enumPaths) {
            if (Test-Path $p) {
                Get-ChildItem $p -ErrorAction SilentlyContinue | ForEach-Object {
                    $devClass = $_
                    Get-ChildItem $devClass.PSPath -ErrorAction SilentlyContinue | ForEach-Object {
                        $sub = $_
                        $props = Get-ItemProperty $sub.PSPath -ErrorAction SilentlyContinue
                        $name = $props.FriendlyName
                        if (-not $name) { $name = $props.DeviceDesc }
                        if ($name -match ';') { $name = $name.Split(';')[-1] }
                        if (-not $name -or $name -eq $sub.PSChildName) { return } # Skip if no meaningful name

                        if (-not (&$isSystem $name) -and -not $seenNames[$name]) {
                            $devices += @{ id = (&$cleanId $sub.PSChildName); name = $name; lastSeen = "Registry Trace" }
                            $seenNames[$name] = $true
                        }
                    }
                }
            }
        }
    }
    catch {
        $debugLog += "Phase 1 Error: $($_.Exception.Message)"
    }

    # PHASE 2: SYSTEM Task (If Phase 1 found nothing or for escalation)
    if ($devices.Count -eq 0) {
        $debugLog += "Phase 2: SYSTEM Task Escalation"
        try {
            $scriptContent = @'
$res = @()
$seenN = @{}
$isSys = { param($n); $gen = @("Microsoft Bluetooth", "RFCOMM", "Personal Area Network", "BTHBRB", "BTHLE", "Generic Bluetooth", "Bluetooth LE"); foreach ($g in $gen) { if ($n -match $g) { return $true } }; return $false }
$btPath = 'HKLM:\SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices'
if (Test-Path $btPath) {
    Get-ChildItem $btPath -ErrorAction SilentlyContinue | ForEach-Object {
        $id = $_.PSChildName; $pr = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
        $nm = if ($pr.Name) { try { [System.Text.Encoding]::ASCII.GetString($pr.Name).Trim([char]0) } catch { "Unknown Device" } } else { "Unknown Device" }
        if (-not (&$isSys $nm) -and -not $seenN[$nm]) {
            $ls = if ($pr.LastSeen) { try { [DateTime]::FromFileTime($pr.LastSeen).ToString("yyyy-MM-dd HH:mm:ss") } catch { "Registry Traces" } } else { "Registry Traces" }
            $res += @{ id = $id; name = $nm; lastSeen = $ls }; $seenN[$nm] = $true
        }
    }
}
$enumPaths = @('HKLM:\SYSTEM\CurrentControlSet\Enum\BTHENUM', 'HKLM:\SYSTEM\CurrentControlSet\Enum\BTH')
foreach ($p in $enumPaths) {
    if (Test-Path $p) {
        Get-ChildItem $p -ErrorAction SilentlyContinue | ForEach-Object {
            $c = $_; Get-ChildItem $c.PSPath -ErrorAction SilentlyContinue | ForEach-Object {
                $sub = $_; $pr = Get-ItemProperty $sub.PSPath -ErrorAction SilentlyContinue
                $nm = $pr.FriendlyName; if (-not $nm) { $nm = $pr.DeviceDesc }; if ($nm -match ';') { $nm = $nm.Split(';')[-1] }
                if ($nm -and -not (&$isSys $nm) -and -not $seenN[$nm]) {
                    $res += @{ id = $sub.PSChildName; name = $nm; lastSeen = "Registry Trace" }; $seenN[$nm] = $true
                }
            }
        }
    }
}
$res | ConvertTo-Json -Compress
'@
            $tempScript = "$env:TEMP\System_BtGet.ps1"
            $outputFile = "$env:TEMP\System_BtOut.json"
            $escapedScript = $scriptContent.Replace("'", "''")
            $wrapper = "[scriptblock]::Create('$escapedScript').Invoke() | Out-File -FilePath '$outputFile' -Encoding UTF8 -Force"
            $wrapper | Out-File -FilePath $tempScript -Encoding UTF8 -Force

            $taskName = "WinCommander_BtScan_System"
            Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
            $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-ExecutionPolicy Bypass -WindowStyle Hidden -File `"$tempScript`""
            $principal = New-ScheduledTaskPrincipal -UserId "NT AUTHORITY\SYSTEM" -LogonType ServiceAccount -RunLevel Highest
            Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
            Start-ScheduledTask -TaskName $taskName
            
            $timeout = 10
            while ($timeout -gt 0) {
                if (Test-Path $outputFile) { break }
                Start-Sleep -Seconds 1
                $timeout--
            }

            if (Test-Path $outputFile) {
                $json = Get-Content $outputFile -Raw
                if ($json) {
                    $sysDevices = $json | ConvertFrom-Json
                    if ($sysDevices -is [PSCustomObject]) { $sysDevices = @($sysDevices) }
                    foreach ($d in $sysDevices) {
                        if (-not $seenNames[$d.name]) {
                            $d.id = (&$cleanId $d.id) # Apply cleanId to the ID from the SYSTEM task
                            $devices += $d
                            $seenNames[$d.name] = $true
                        }
                    }
                    $debugLog += "Phase 2 Success: Found $($sysDevices.Count) devices"
                }
                else {
                    $debugLog += "Phase 2 Error: Output file was empty"
                }
                Remove-ItemSecure $outputFile -ErrorAction SilentlyContinue
            }
            else {
                $debugLog += "Phase 2 Error: Task timeout or output file missing"
            }
            Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
            Invoke-7Erase -Path $tempScript -Type File
        }
        catch {
            $debugLog += "Phase 2 Error: $($_.Exception.Message)"
        }
    }

    if ($devices -is [PSCustomObject]) { $devices = @($devices) }
    @{ 
        devices   = $devices
        debugInfo = $debugLog -join "`n"
        error     = if ($devices.Count -eq 0) { $true } else { $false }
        message   = if ($devices.Count -eq 0) { "No user devices found." } else { $null }
    }
}

function Get-NetworkDrives {
    try {
        $drives = Get-PSDrive -PSProvider FileSystem | Where-Object { $null -ne $_.DisplayRoot } | Select-Object Name, DisplayRoot
        @{ drives = $drives }
    }
    catch {
        @{ error = $true; message = "Failed to get network drives: $($_.Exception.Message)" }
    }
}

function Get-RecentFiles {
    try {
        $recentPath = [Environment]::GetFolderPath('Recent')
        $entries = @()
        if (Test-Path $recentPath) {
            Get-ChildItem "$recentPath\*" -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $target = $null
                try {
                    $shell = New-Object -ComObject WScript.Shell
                    $lnk = $shell.CreateShortcut($_.FullName)
                    $target = $lnk.TargetPath
                } catch {}
                $entries += @{
                    name         = $_.BaseName
                    extension    = $_.Extension
                    target       = $target
                    lastModified = $_.LastWriteTime.ToString("o")
                    sizeBytes    = $_.Length
                }
            }
        }
        @{ entries = $entries; total = $entries.Count; path = $recentPath }
    }
    catch {
        @{ error = $true; message = "Failed to get recent files: $($_.Exception.Message)" }
    }
}

function Get-RDPHistory {
    try {
        $entries = @()

        # MRU entries (most recently used hosts)
        $defaultPath = 'HKCU:\Software\Microsoft\Terminal Server Client\Default'
        if (Test-Path $defaultPath) {
            $props = Get-ItemProperty -Path $defaultPath -ErrorAction SilentlyContinue
            if ($props) {
                $mruKeys = $props.PSObject.Properties | Where-Object { $_.Name -match '^MRU\d+$' }
                foreach ($k in $mruKeys) {
                    $entries += @{ type = 'MRU'; host = $k.Value; key = $k.Name }
                }
            }
        }

        # Per-server entries
        $serversPath = 'HKCU:\Software\Microsoft\Terminal Server Client\Servers'
        if (Test-Path $serversPath) {
            Get-ChildItem $serversPath -ErrorAction SilentlyContinue | ForEach-Object {
                $hostname = $_.PSChildName
                $props = Get-ItemProperty -Path $_.PSPath -ErrorAction SilentlyContinue
                $username = if ($props -and $props.UsernameHint) { $props.UsernameHint } else { $null }
                $entries += @{ type = 'Server'; host = $hostname; username = $username; key = $_.PSPath }
            }
        }

        # Saved credentials (cmdkey)
        $cmdkeyOutput = cmdkey /list 2>&1
        foreach ($line in $cmdkeyOutput) {
            if ($line -like "*target=TERMSRV/*") {
                if ($line -match 'target=(\S+)') {
                    $entries += @{ type = 'SavedCredential'; host = $Matches[1]; key = $Matches[1] }
                }
            }
        }

        # Default.rdp file presence
        $docPath = [Environment]::GetFolderPath('MyDocuments')
        $rdpFile = "$docPath\Default.rdp"
        if (Test-Path $rdpFile) {
            $fi = Get-Item $rdpFile -Force -ErrorAction SilentlyContinue
            $entries += @{ type = 'DefaultRDP'; host = 'Default.rdp'; key = $rdpFile; lastModified = $fi.LastWriteTime.ToString("o") }
        }

        @{ entries = $entries; total = $entries.Count }
    }
    catch {
        @{ error = $true; message = "Failed to get RDP history: $($_.Exception.Message)" }
    }
}

function Clear-RDPHistory {
    # Clears all four locations that Windows stores outgoing RDP connection history:
    #   1. Terminal Server Client Default MRU keys (MRU0–MRUn) — the dropdown list in mstsc
    #   2. Per-server subkeys under Servers\ — stores UsernameHint per host
    #   3. Default.rdp in Documents — persists last-used connection settings
    # Saved credentials (TERMSRV/) are a separate store — use Clear-RDPPasswords for those.
    try {
        $cleared = @()

        # 1. MRU entries
        $defaultPath = 'HKCU:\Software\Microsoft\Terminal Server Client\Default'
        if (Test-Path $defaultPath) {
            $props = Get-ItemProperty -Path $defaultPath -ErrorAction SilentlyContinue
            if ($props) {
                $mruKeys = $props.PSObject.Properties |
                    Where-Object { $_.Name -match '^MRU\d+$' } |
                    Select-Object -ExpandProperty Name
                foreach ($k in $mruKeys) {
                    Remove-ItemProperty -Path $defaultPath -Name $k -Force -ErrorAction SilentlyContinue
                    $cleared += "MRU:$k"
                }
            }
        }

        # 2. Per-server subkeys (UsernameHint stored per hostname)
        $serversPath = 'HKCU:\Software\Microsoft\Terminal Server Client\Servers'
        if (Test-Path $serversPath) {
            Get-ChildItem $serversPath -ErrorAction SilentlyContinue | ForEach-Object {
                Remove-Item -Path $_.PSPath -Recurse -Force -ErrorAction SilentlyContinue
                $cleared += "Server:$($_.PSChildName)"
            }
        }

        # 3. Default.rdp file in Documents
        $docPath = [Environment]::GetFolderPath('MyDocuments')
        $rdpFile = "$docPath\Default.rdp"
        if (Test-Path $rdpFile) {
            Remove-Item -Path $rdpFile -Force -ErrorAction SilentlyContinue
            $cleared += "File:Default.rdp"
        }

        @{ success = $true; cleared = $cleared; total = $cleared.Count }
    }
    catch {
        @{ success = $false; error = $_.Exception.Message }
    }
}

function Clear-RDPPasswords {
    # Removes all saved RDP credentials (TERMSRV/* entries) from Windows
    # Credential Manager. Uses cmdkey /delete for each entry found.
    try {
        $removed = @()
        $errors  = @()

        $cmdkeyOutput = cmdkey /list 2>&1
        $targets = @()
        foreach ($line in $cmdkeyOutput) {
            if ($line -like '*target=TERMSRV/*') {
                if ($line -match 'target=(\S+)') { $targets += $Matches[1] }
            }
        }

        foreach ($target in $targets) {
            $result = cmdkey /delete:$target 2>&1
            if ($LASTEXITCODE -eq 0) {
                $removed += $target
            } else {
                $errors += $target
            }
        }

        @{ success = $true; removed = $removed; errors = $errors; total = $removed.Count }
    }
    catch {
        @{ success = $false; error = $_.Exception.Message }
    }
}

function Get-JumpLists {
    try {
        $entries = @()
        $autoPath = "$env:APPDATA\Microsoft\Windows\Recent\AutomaticDestinations"
        $customPath = "$env:APPDATA\Microsoft\Windows\Recent\CustomDestinations"

        if (Test-Path $autoPath) {
            Get-ChildItem $autoPath -ErrorAction SilentlyContinue | ForEach-Object {
                $entries += @{
                    name         = $_.Name
                    type         = 'Automatic'
                    sizeKB       = [math]::Round($_.Length / 1KB, 1)
                    lastModified = $_.LastWriteTime.ToString("o")
                }
            }
        }
        if (Test-Path $customPath) {
            Get-ChildItem $customPath -ErrorAction SilentlyContinue | ForEach-Object {
                $entries += @{
                    name         = $_.Name
                    type         = 'Custom'
                    sizeKB       = [math]::Round($_.Length / 1KB, 1)
                    lastModified = $_.LastWriteTime.ToString("o")
                }
            }
        }

        @{ entries = $entries; total = $entries.Count }
    }
    catch {
        @{ error = $true; message = "Failed to get jump lists: $($_.Exception.Message)" }
    }
}

function Get-BrowserFootprints {
    # Uses Get-InstalledBrowsers from the security module (loaded as additional dependency)
    try {
        $chromiumArtifacts = @('History','Cookies','Login Data','Top Sites','Favicons','Web Data','Visited Links','Shortcuts','Cache','Code Cache','IndexedDB','Local Storage','Session Storage','Network','Download Metadata')
        $geckoArtifacts    = @('places.sqlite','cookies.sqlite','logins.json','favicons.sqlite','formhistory.sqlite','cache2','storage','sessionstore.jsonlz4')

        $helper_measurePath = {
            param($p)
            $sz = 0
            try {
                if ((Get-Item $p -Force -ErrorAction Stop).PSIsContainer) {
                    $sz = (Get-ChildItem $p -Recurse -Force -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum -ErrorAction SilentlyContinue).Sum
                } else {
                    $sz = (Get-Item $p -Force -ErrorAction Stop).Length
                }
            } catch {}
            if (-not $sz) { $sz = 0 }
            return $sz
        }

        $installed = Get-InstalledBrowsers
        $browsers  = @()

        foreach ($b in $installed) {
            $udd = $b.UserDataDir
            if (-not $udd -or -not (Test-Path $udd)) { continue }

            $artifacts  = @()
            $totalBytes = 0

            if ($b.Engine -eq 'Chromium') {
                # Chromium: profile is UserDataDir\Default
                $profileDir = Join-Path $udd 'Default'
                if (-not (Test-Path $profileDir)) { continue }
                foreach ($art in $chromiumArtifacts) {
                    $p = Join-Path $profileDir $art
                    if (Test-Path $p) {
                        $sz = & $helper_measurePath $p
                        $totalBytes += $sz
                        $artifacts += @{ name = $art; sizeKB = [math]::Round($sz / 1KB, 1) }
                    }
                }
            } else {
                # Gecko: each profile is a subdirectory under UserDataDir\Profiles
                # Read profiles.ini to find active profiles, fallback to scanning Profiles dir
                $profileDirs = @()
                $profilesIni = Join-Path $udd 'profiles.ini'
                if (Test-Path $profilesIni) {
                    $ini = Get-Content $profilesIni -ErrorAction SilentlyContinue
                    $currentPath = $null
                    $isRelative  = $true
                    foreach ($line in $ini) {
                        if ($line -match '^\[Profile') { $currentPath = $null; $isRelative = $true }
                        elseif ($line -match '^Path=(.+)$') { $currentPath = $Matches[1].Trim() }
                        elseif ($line -match '^IsRelative=(\d)') { $isRelative = $Matches[1] -eq '1' }
                        elseif ($line -match '^\[' -and $currentPath) {
                            $resolved = if ($isRelative) { Join-Path $udd ($currentPath -replace '/', '\') } else { $currentPath -replace '/', '\' }
                            if (Test-Path $resolved) { $profileDirs += $resolved }
                            $currentPath = $null
                        }
                    }
                    # Flush last entry
                    if ($currentPath) {
                        $resolved = if ($isRelative) { Join-Path $udd ($currentPath -replace '/', '\') } else { $currentPath -replace '/', '\' }
                        if (Test-Path $resolved) { $profileDirs += $resolved }
                    }
                }
                # Fallback: scan Profiles subdir
                if ($profileDirs.Count -eq 0) {
                    $pfDir = Join-Path $udd 'Profiles'
                    if (Test-Path $pfDir) {
                        $profileDirs = Get-ChildItem $pfDir -Directory -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
                    }
                }
                if ($profileDirs.Count -eq 0) { continue }

                foreach ($profDir in $profileDirs) {
                    foreach ($art in $geckoArtifacts) {
                        $p = Join-Path $profDir $art
                        if (Test-Path $p) {
                            $sz = & $helper_measurePath $p
                            $totalBytes += $sz
                            $existing = $artifacts | Where-Object { $_.name -eq $art }
                            if ($existing) {
                                $existing.sizeKB += [math]::Round($sz / 1KB, 1)
                            } else {
                                $artifacts += @{ name = $art; sizeKB = [math]::Round($sz / 1KB, 1) }
                            }
                        }
                    }
                }
            }

            if ($artifacts.Count -gt 0) {
                $browsers += @{
                    browser     = $b.Name
                    profilePath = $udd
                    artifacts   = $artifacts
                    totalSizeKB = [math]::Round($totalBytes / 1KB, 1)
                }
            }
        }

        @{ browsers = $browsers; totalBrowsers = $browsers.Count }
    }
    catch {
        @{ error = $true; message = "Failed to get browser footprints: $($_.Exception.Message)" }
    }
}

function Clear-BrowserFootprints {
    [CmdletBinding()]
    param(
        [string]$Browser = '',
        [string]$ProfilePath = ''
    )
    try {
        $chromiumArtifacts = @('History','Cookies','Login Data','Top Sites','Favicons','Web Data','Visited Links','Shortcuts','Cache','Code Cache','IndexedDB','Local Storage','Session Storage','Network','Download Metadata')
        $geckoArtifacts    = @('places.sqlite','cookies.sqlite','logins.json','favicons.sqlite','formhistory.sqlite','cache2','storage','sessionstore.jsonlz4')
        $removed = @()
        $errors  = @()

        $resolveGeckoProfileDirs = {
            param($UserDataDir)
            $profileDirs = @()
            $profilesIni = Join-Path $UserDataDir 'profiles.ini'
            if (Test-Path $profilesIni) {
                $ini = Get-Content $profilesIni -ErrorAction SilentlyContinue
                $currentPath = $null
                $isRelative  = $true
                foreach ($line in $ini) {
                    if ($line -match '^\[Profile') {
                        $currentPath = $null
                        $isRelative = $true
                    }
                    elseif ($line -match '^Path=(.+)$') {
                        $currentPath = $Matches[1].Trim()
                    }
                    elseif ($line -match '^IsRelative=(\d)') {
                        $isRelative = $Matches[1] -eq '1'
                    }
                    elseif ($line -match '^\[' -and $currentPath) {
                        $resolved = if ($isRelative) { Join-Path $UserDataDir ($currentPath -replace '/', '\') } else { $currentPath -replace '/', '\' }
                        if (Test-Path $resolved) { $profileDirs += $resolved }
                        $currentPath = $null
                    }
                }
                if ($currentPath) {
                    $resolved = if ($isRelative) { Join-Path $UserDataDir ($currentPath -replace '/', '\') } else { $currentPath -replace '/', '\' }
                    if (Test-Path $resolved) { $profileDirs += $resolved }
                }
            }

            if ($profileDirs.Count -eq 0) {
                $pfDir = Join-Path $UserDataDir 'Profiles'
                if (Test-Path $pfDir) {
                    $profileDirs = Get-ChildItem $pfDir -Directory -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
                }
            }

            return $profileDirs
        }

        $installed = Get-InstalledBrowsers
        if (-not $installed -or @($installed).Count -eq 0) {
            return @{ success = $true; removed = @(); errors = @(); total = 0 }
        }

        $targets = $installed
        if ($Browser) {
            $targets = $targets | Where-Object { $_.Name -ieq $Browser }
        }
        if ($ProfilePath) {
            $targets = $targets | Where-Object { $_.UserDataDir -ieq $ProfilePath }
        }

        foreach ($b in $targets) {
            $udd = $b.UserDataDir
            if (-not $udd -or -not (Test-Path $udd)) { continue }

            if ($b.Engine -eq 'Chromium') {
                $profileDir = Join-Path $udd 'Default'
                if (-not (Test-Path $profileDir)) { continue }
                foreach ($art in $chromiumArtifacts) {
                    $p = Join-Path $profileDir $art
                    if (-not (Test-Path $p)) { continue }
                    try {
                        Remove-ItemSecure -Path $p
                        $removed += "$($b.Name):$art"
                    }
                    catch {
                        $errors += "$($b.Name):$art"
                    }
                }
            }
            else {
                $profileDirs = & $resolveGeckoProfileDirs $udd
                if ($profileDirs.Count -eq 0) { continue }

                foreach ($profDir in $profileDirs) {
                    foreach ($art in $geckoArtifacts) {
                        $p = Join-Path $profDir $art
                        if (-not (Test-Path $p)) { continue }
                        try {
                            Remove-ItemSecure -Path $p
                            $removed += "$($b.Name):$art"
                        }
                        catch {
                            $errors += "$($b.Name):$art"
                        }
                    }
                }
            }
        }

        @{ success = $true; removed = $removed; errors = $errors; total = $removed.Count }
    }
    catch {
        @{ success = $false; error = $_.Exception.Message }
    }
}

function Get-PrefetchFiles {
    try {
        $prefetchPath = "$env:SystemRoot\Prefetch"
        $entries = @()
        $accessDenied = $false
        if (Test-Path $prefetchPath) {
            $files = $null
            # Attempt 1: native PS listing
            try {
                $files = Get-ChildItem "$prefetchPath\*.pf" -Force -ErrorAction Stop
            } catch [System.UnauthorizedAccessException] {
                $accessDenied = $true
            } catch {}
            # Attempt 2: .NET IO (sometimes bypasses PS provider restrictions)
            if (-not $files -and -not $accessDenied) {
                try {
                    $netFiles = [System.IO.Directory]::GetFiles($prefetchPath, '*.pf')
                    $files = $netFiles | ForEach-Object { try { [System.IO.FileInfo]::new($_) } catch { $null } } | Where-Object { $_ }
                } catch [System.UnauthorizedAccessException] {
                    $accessDenied = $true
                } catch {}
            }
            # Attempt 3: cmd /c dir (last resort)
            if (-not $files -and -not $accessDenied) {
                $cmdOut = cmd /c "dir /b `"$prefetchPath\*.pf`"" 2>&1
                if (($cmdOut -join ' ') -match 'Access is denied|Zugriff verweigert|access.denied') {
                    $accessDenied = $true
                } else {
                    $files = $cmdOut | Where-Object { $_ -match '\.pf$' } | ForEach-Object {
                        [PSCustomObject]@{
                            Name = $_
                            BaseName = [System.IO.Path]::GetFileNameWithoutExtension($_)
                            Length = 0
                            LastWriteTime = [datetime]::MinValue
                            CreationTime  = [datetime]::MinValue
                        }
                    }
                }
            }
            if ($files) {
                $files | Sort-Object LastWriteTime -Descending | ForEach-Object {
                    $exeName = $_.BaseName -replace '-[0-9A-Fa-f]{8}$', ''
                    $entries += @{
                        name         = $exeName
                        fileName     = $_.Name
                        sizeKB       = if ($_.Length -gt 0) { [math]::Round($_.Length / 1KB, 1) } else { 0 }
                        lastRun      = if ($_.LastWriteTime -ne [datetime]::MinValue) { $_.LastWriteTime.ToString("o") } else { $null }
                        created      = if ($_.CreationTime -ne [datetime]::MinValue) { $_.CreationTime.ToString("o") } else { $null }
                    }
                }
            }
        }
        # Check EnablePrefetcher registry value
        $enablePrefetcher = 3  # default = enabled
        try {
            $pfParams = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters' -Name EnablePrefetcher -ErrorAction Stop
            $enablePrefetcher = [int]$pfParams.EnablePrefetcher
        } catch {}
        @{ entries = $entries; total = $entries.Count; path = $prefetchPath; accessDenied = $accessDenied; enablePrefetcher = $enablePrefetcher }
    }
    catch {
        @{ error = $true; message = "Failed to get prefetch files: $($_.Exception.Message)" }
    }
}

function Get-ShadowCopies {
    try {
        $copies = @()
        # Try CIM first (preferred), fall back to WMI
        $shadows = $null
        try { $shadows = Get-CimInstance -ClassName Win32_ShadowCopy -ErrorAction Stop } catch {}
        if (-not $shadows) {
            try { $shadows = Get-WmiObject Win32_ShadowCopy -ErrorAction Stop } catch {}
        }
        # Also try vssadmin to count even if CIM/WMI fails
        if (-not $shadows) {
            $vssOut = & vssadmin list shadows 2>&1
            $vssStr = ($vssOut | ForEach-Object { $_.ToString() }) -join "`n"
            if ($vssStr -match 'Shadow Copy ID') {
                $ids = [regex]::Matches($vssStr, 'Shadow Copy ID:\s*\{([^}]+)\}')
                foreach ($m in $ids) {
                    $copies += @{ id = $m.Groups[1].Value; drive = '?'; deviceObject = ''; created = ''; stateStr = 'Listed'; persistent = $false; clientAccessible = $false; originatingMachine = ''; serviceMachine = '' }
                }
            }
        } else {
            $shadowList = if ($shadows -is [System.Array]) { $shadows } else { @($shadows) }
            foreach ($s in $shadowList) {
                $stateVal = try { [int]$s.State } catch { 0 }
                $copies += @{
                    id           = [string]$s.ID
                    drive        = [string]$s.VolumeName
                    deviceObject = [string]$s.DeviceObject
                    created      = [string]$s.InstallDate
                    originatingMachine = [string]$s.OriginatingMachine
                    serviceMachine     = [string]$s.ServiceMachine
                    clientAccessible   = [bool]$s.ClientAccessible
                    persistent         = [bool]$s.Persistent
                    stateStr           = switch ($stateVal) {
                        1 { 'Prepare' } 2 { 'Processing Prepare' } 3 { 'Prepared' }
                        4 { 'Processing Precopy' } 5 { 'Precopy' } 6 { 'Processing Create' }
                        8 { 'Processing Commit' } 9 { 'Processing Postcommit' }
                        10 { 'Committed' } 12 { 'Deleted' } 13 { 'Count' }
                        default { 'Unknown' }
                    }
                }
            }
        }
        # Check VSS service status
        $vssRunning = $false
        try {
            $vssSvc = Get-Service -Name VSS -ErrorAction Stop
            $vssRunning = ($vssSvc.Status -eq 'Running')
        } catch {}
        @{ copies = $copies; total = $copies.Count; vssRunning = $vssRunning }
    }
    catch {
        @{ error = $true; message = "Failed to get shadow copies: $($_.Exception.Message)" }
    }
}

function Get-NTFSJournals {
    try {
        $journals = @()
        $drives = Get-PSDrive -PSProvider FileSystem -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^[A-Z]$' }
        foreach ($drive in $drives) {
            $driveLetter = "$($drive.Name):"
            try {
                $raw = & fsutil usn queryjournal $driveLetter 2>&1
                # fsutil returns an array of lines; join for regex matching
                $outputStr = ($raw | ForEach-Object { $_.ToString() }) -join "`n"
                $hasJournal = $outputStr -match '(?i)Journal\s*ID'
                $isError    = $outputStr -match '(?i)(not found|does not have|error|invalid|access denied)'
                if ($hasJournal -and -not $isError) {
                    # Extract Journal ID - format: "Journal ID :   0x01234567890abcde"
                    $journalId = $null
                    if ($outputStr -match '(?i)Journal\s*ID\s*[=:]\s*(0x[0-9a-fA-F]+|[0-9a-fA-F]+)') {
                        $journalId = $Matches[1]
                    }
                    # Extract Max Size
                    $maxSizeVal = $null
                    if ($outputStr -match '(?i)Maximum\s*Size\s*[=:]\s*([0-9]+\s*(?:bytes|kb|mb|gb)?)') {
                        $maxSizeVal = $Matches[1].Trim()
                    } elseif ($outputStr -match '(?i)Maximum\s*Size\s*[=:]\s*(0x[0-9a-fA-F]+)') {
                        $maxSizeVal = $Matches[1]
                    }
                    $journals += @{
                        drive     = $driveLetter
                        journalId = $journalId
                        maxSize   = $maxSizeVal
                        present   = $true
                    }
                } else {
                    $journals += @{ drive = $driveLetter; present = $false; journalId = $null; maxSize = $null }
                }
            } catch {
                $journals += @{ drive = $driveLetter; present = $false; journalId = $null; maxSize = $null }
            }
        }
        @{ journals = $journals; total = ($journals | Where-Object { $_.present }).Count }
    }
    catch {
        @{ error = $true; message = "Failed to get NTFS journals: $($_.Exception.Message)" }
    }
}

# ============================================================================
# ADVANCED PRIVACY CLEAN - GROUP I-A: Hardcore Privacy Clean
# ============================================================================

function Get-VirtualMemoryStatus {
    try {
        $hiberStatus = & powercfg /query SCHEME_CURRENT SUB_SLEEP HIBERNATE 2>&1
        $hiberEnabled = $hiberStatus -notmatch 'hibernate.*off'
        try {
            $pfParam = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' `
                -Name 'ClearPageFileAtShutdown' -ErrorAction Stop
            $clearOnShutdown = [int]$pfParam.ClearPageFileAtShutdown -eq 1
        } catch { $clearOnShutdown = $false }
        $hiberFile = Test-Path "$env:SystemDrive\hiberfil.sys"
        $swapFile = Test-Path "$env:SystemDrive\swapfile.sys"
        @{ hiberEnabled = $hiberEnabled; hiberFileExists = $hiberFile; swapFileExists = $swapFile; clearPageFileAtShutdown = $clearOnShutdown }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# GROUP I-A: STATUS VIEWERS (Get-* companions for each clear function)
# ============================================================================

function Get-AmcacheEntries {
    # Returns Amcache hive file info + live registry entry counts per category
    try {
        $entries = @()
        $hveFile = "$env:SystemRoot\AppCompat\Programs\Amcache.hve"
        # Amcache.hve is ACL-protected on standard-user sessions.  File.Exists
        # returns false for inaccessible files without a provider error.
        $hveSize = if ([System.IO.File]::Exists($hveFile)) { [math]::Round(([System.IO.FileInfo]::new($hveFile)).Length / 1MB, 2) } else { 0 }

        # Categories accessible as live registry (mounted hive)
        $cats = @(
            @{ name = 'InventoryApplication';        path = 'HKLM:\AppCompatCache\Amcache\InventoryApplication' },
            @{ name = 'InventoryApplicationFile';    path = 'HKLM:\AppCompatCache\Amcache\InventoryApplicationFile' },
            @{ name = 'InventoryApplicationShortcut';path = 'HKLM:\AppCompatCache\Amcache\InventoryApplicationShortcut' },
            @{ name = 'InventoryDeviceContainer';    path = 'HKLM:\AppCompatCache\Amcache\InventoryDeviceContainer' },
            @{ name = 'InventoryDriverBinary';       path = 'HKLM:\AppCompatCache\Amcache\InventoryDriverBinary' }
        )
        foreach ($cat in $cats) {
            $count = 0
            $sample = @()
            if (Test-Path $cat.path -ErrorAction SilentlyContinue) {
                $children = Get-ChildItem $cat.path -ErrorAction SilentlyContinue
                $count = ($children | Measure-Object).Count
                $sample = $children | Select-Object -First 3 | ForEach-Object {
                    $props = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
                    @{
                        id   = $_.PSChildName
                        name = $props.Name -or $props.ProductName -or $props.BinFileVersion -or ''
                        path = $props.LowerCaseLongPath -or $props.LongPathHash -or ''
                    }
                }
            }
            $entries += @{ category = $cat.name; count = $count; sample = $sample }
        }
        @{ entries = $entries; hveFileSizeMb = $hveSize; hveFileExists = ($hveSize -gt 0); total = ($entries | Measure-Object -Property count -Sum).Sum }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-RecycleBinInfo {
    # Read-only viewer. Each deleted item in a volume's $Recycle.Bin has a
    # paired metadata file ($I...) recording the ORIGINAL path, deletion
    # time and size; the $R... file holds the data. This parses every $I
    # file across all fixed and removable volumes (and every per-account SID folder we can
    # read) so the operator can see what was deleted without recovering it.
    # Removes nothing — emptying is the paid Clear-RecycleBinMetadata clearer.
    try {
        $items = @()
        $totalSizeKB = 0.0
        $sidNameCache = @{}

        $drives = [System.IO.DriveInfo]::GetDrives() |
            Where-Object { $_.DriveType -in @('Fixed','Removable') -and $_.IsReady }
        foreach ($drv in $drives) {
            $binRoot = Join-Path $drv.RootDirectory.FullName '$Recycle.Bin'
            if (-not (Test-Path -LiteralPath $binRoot -ErrorAction SilentlyContinue)) { continue }

            $sidDirs = Get-ChildItem -LiteralPath $binRoot -Directory -Force -ErrorAction SilentlyContinue
            foreach ($sidDir in $sidDirs) {
                $sid = $sidDir.Name
                if (-not $sidNameCache.ContainsKey($sid)) {
                    $resolved = $sid
                    try {
                        $resolved = (New-Object System.Security.Principal.SecurityIdentifier($sid)).Translate(
                            [System.Security.Principal.NTAccount]).Value
                    } catch {}
                    $sidNameCache[$sid] = $resolved
                }
                $account = $sidNameCache[$sid]

                $iFiles = Get-ChildItem -LiteralPath $sidDir.FullName -Filter '$I*' -Force -ErrorAction SilentlyContinue
                foreach ($iFile in $iFiles) {
                    try {
                        $bytes = [System.IO.File]::ReadAllBytes($iFile.FullName)
                        if ($bytes.Length -lt 24) { continue }
                        $version   = [System.BitConverter]::ToInt64($bytes, 0)
                        $sizeBytes = [System.BitConverter]::ToInt64($bytes, 8)
                        $ft        = [System.BitConverter]::ToInt64($bytes, 16)
                        $deleted   = ''
                        try { $deleted = [System.DateTime]::FromFileTime($ft).ToString('yyyy-MM-dd HH:mm:ss') } catch {}

                        $origPath = ''
                        if ($version -eq 2 -and $bytes.Length -ge 28) {
                            # Win10+: int32 path length (wchars incl. NUL) at 24, UTF-16 path at 28
                            $pathChars = [System.BitConverter]::ToInt32($bytes, 24)
                            $byteLen   = [Math]::Max(0, ($pathChars - 1) * 2)
                            if ($bytes.Length -ge (28 + $byteLen)) {
                                $origPath = [System.Text.Encoding]::Unicode.GetString($bytes, 28, $byteLen)
                            }
                        } else {
                            # Legacy: fixed 520-byte (260 wchar) UTF-16 path at offset 24
                            $avail = [Math]::Min(520, $bytes.Length - 24)
                            if ($avail -gt 0) {
                                $raw = [System.Text.Encoding]::Unicode.GetString($bytes, 24, $avail)
                                $nul = $raw.IndexOf([char]0)
                                $origPath = if ($nul -ge 0) { $raw.Substring(0, $nul) } else { $raw }
                            }
                        }

                        $sizeKB = [Math]::Round($sizeBytes / 1KB, 1)
                        $totalSizeKB += $sizeKB
                        $items += @{
                            originalPath = $origPath
                            deletedTime  = $deleted
                            sizeBytes    = $sizeBytes
                            sizeKB       = $sizeKB
                            account      = $account
                            sid          = $sid
                            drive        = $drv.Name
                        }
                    } catch {}
                }
            }
        }

        @{
            items       = $items
            total       = $items.Count
            totalSizeKB = [Math]::Round($totalSizeKB, 1)
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-NTUserTraces {
    # Returns all RunMRU, TypedPaths, TypedURLs, WordWheelQuery values
    try {
        $sections = @()

        # RunMRU
        $runMruPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU'
        $runMru = @()
        if (Test-Path $runMruPath) {
            $props = Get-ItemProperty $runMruPath -ErrorAction SilentlyContinue
            $order = $props.MRUList
            if ($props) {
                $props.PSObject.Properties | Where-Object { $_.Name -notmatch '^(PS|MRUList)' } | ForEach-Object {
                    $runMru += @{ key = $_.Name; value = ($_.Value -replace '\\1$','') }
                }
            }
        }
        $sections += @{ name = 'RunMRU'; count = $runMru.Count; entries = $runMru }

        # TypedPaths
        $tpPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths'
        $typedPaths = @()
        if (Test-Path $tpPath) {
            Get-ItemProperty $tpPath | ForEach-Object {
                $_.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
                    $typedPaths += @{ key = $_.Name; value = $_.Value }
                }
            }
        }
        $sections += @{ name = 'TypedPaths'; count = $typedPaths.Count; entries = $typedPaths }

        # TypedURLs (IE/Edge legacy)
        $tuPath = 'HKCU:\Software\Microsoft\Internet Explorer\TypedURLs'
        $typedURLs = @()
        if (Test-Path $tuPath) {
            Get-ItemProperty $tuPath | ForEach-Object {
                $_.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
                    $typedURLs += @{ key = $_.Name; value = $_.Value }
                }
            }
        }
        $sections += @{ name = 'TypedURLs'; count = $typedURLs.Count; entries = $typedURLs }

        # WordWheelQuery (Start/Explorer search MRU)
        $wwqPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery'
        $wwq = @()
        if (Test-Path $wwqPath) {
            Get-ItemProperty $wwqPath | ForEach-Object {
                $_.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
                    $wwq += @{ key = $_.Name; value = $_.Value }
                }
            }
        }
        $sections += @{ name = 'WordWheelQuery'; count = $wwq.Count; entries = $wwq }

        $totalCount = ($sections | Measure-Object -Property count -Sum).Sum
        @{ sections = $sections; total = $totalCount }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-NotepadStateFiles {
    # Returns Notepad tab state file listing with sizes
    try {
        $files = @()
        $notepadPkg = Get-ChildItem "$env:LOCALAPPDATA\Packages" -Filter 'Microsoft.WindowsNotepad_*' -Directory -ErrorAction SilentlyContinue | Select-Object -First 1
        $packageFound = $notepadPkg -ne $null
        if ($notepadPkg) {
            $tabDir = Join-Path $notepadPkg.FullName 'LocalState\TabState'
            if (Test-Path $tabDir) {
                Get-ChildItem -Path $tabDir -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
                    $files += @{
                        name    = $_.Name
                        sizeKB  = [math]::Round($_.Length / 1KB, 1)
                        modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    }
                }
            }
        }
        # Also check legacy Notepad recent (non-packaged)
        $legacyPath = "$env:APPDATA\Microsoft\Windows\Recent"
        @{ files = $files; total = $files.Count; packageFound = $packageFound; totalSizeKB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum, 1) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-PCAInfo {
    # Returns PCA database file list from AppCompat\Programs
    try {
        $pcaDir = "$env:SystemRoot\AppCompat\Programs"
        $files = @()
        if (Test-Path $pcaDir) {
            Get-ChildItem -Path $pcaDir -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{
                    name     = $_.Name
                    sizeKB   = [math]::Round($_.Length / 1KB, 1)
                    modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    type     = $_.Extension
                }
            }
        }
        # Check if PcaSvc service is running
        $pcaSvcState = try { (Get-Service PcaSvc -ErrorAction Stop).Status.ToString() } catch { 'Unknown' }
        @{ files = $files; total = $files.Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2); pcaSvcState = $pcaSvcState }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-CrashDumpList {
    # Returns all crash dump files with size, name, and age
    try {
        $dumps = @()
        $sources = @(
            @{ label = 'Minidump';           path = "$env:SystemRoot\Minidump" },
            @{ label = 'CrashDumps';         path = "$env:LOCALAPPDATA\CrashDumps" },
            @{ label = 'WER ReportArchive';  path = "$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportArchive" },
            @{ label = 'WER ReportQueue';    path = "$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportQueue" }
        )
        $memDmpPath = "$env:SystemRoot\MEMORY.DMP"
        if (Test-Path $memDmpPath) {
            $f = Get-Item $memDmpPath
            $dumps += @{ source = 'FullDump'; name = 'MEMORY.DMP'; sizeKB = [math]::Round($f.Length/1KB,1); modified = $f.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
        }
        foreach ($src in $sources) {
            if (Test-Path $src.path) {
                Get-ChildItem -Path $src.path -Recurse -ErrorAction SilentlyContinue | Where-Object { -not $_.PSIsContainer } | ForEach-Object {
                    $dumps += @{
                        source   = $src.label
                        name     = $_.Name
                        sizeKB   = [math]::Round($_.Length/1KB,1)
                        modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    }
                }
            }
        }
        @{ dumps = $dumps; total = $dumps.Count; totalSizeMB = [math]::Round(($dumps | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-SQLiteWALList {
    # Returns all .wal/.shm files found in LOCALAPPDATA / APPDATA (max 100)
    # Uses -Depth 5 to avoid extremely deep recursive scans that can time out
    try {
        $found = @()
        $roots = @($env:LOCALAPPDATA, $env:APPDATA)
        foreach ($root in $roots) {
            if (-not (Test-Path $root)) { continue }
            Get-ChildItem -Path $root -Include '*.wal','*.shm' -Recurse -Depth 5 -Force -ErrorAction SilentlyContinue |
                Select-Object -First 100 |
                ForEach-Object {
                    $found += @{
                        name     = $_.Name
                        sizeKB   = [math]::Round($_.Length/1KB,1)
                        dir      = Split-Path $_.FullName -Parent | Split-Path -Leaf
                        modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    }
                }
        }
        @{ files = $found; total = $found.Count; totalSizeMB = [math]::Round(($found | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-RecallDatabaseInfo {
    # Returns Recall/Timeline/notification DB file listing
    try {
        $dbs = @()
        $sources = @(
            @{ label = 'Recall';    path = "$env:LOCALAPPDATA\CoreAIPlatform.00" },
            @{ label = 'Recall';    path = "$env:LOCALAPPDATA\Microsoft\Windows\Recall" },
            @{ label = 'Timeline';  path = "$env:LOCALAPPDATA\ConnectedDevicesPlatform" },
            @{ label = 'ActionCenter'; path = "$env:LOCALAPPDATA\Microsoft\Windows\ActionCenter" }
        )
        foreach ($src in $sources) {
            if (Test-Path $src.path) {
                Get-ChildItem -Path $src.path -Include '*.db','*.db-wal','*.db-shm','*.db-journal','*.edb' -Recurse -Force -ErrorAction SilentlyContinue |
                    Select-Object -First 100 |
                    ForEach-Object {
                        $dbs += @{
                            source   = $src.label
                            name     = $_.Name
                            sizeKB   = [math]::Round($_.Length/1KB,1)
                            modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                        }
                    }
            }
        }
        @{ databases = $dbs; total = $dbs.Count; totalSizeMB = [math]::Round(($dbs | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-SearchIndexInfo {
    # Returns Windows Search index file info without stopping the service
    try {
        $svcState = try { (Get-Service -Name WSearch -ErrorAction Stop).Status } catch { 'Unknown' }
        $files = @()
        $searchDirs = @(
            @{ label = 'Index DB';  path = "$env:ProgramData\Microsoft\Search\Data\Applications\Windows" },
            @{ label = 'Temp';      path = "$env:ProgramData\Microsoft\Search\Data\Temp" }
        )
        $totalSizeKB = 0
        foreach ($sd in $searchDirs) {
            # Windows Search owns parts of its data tree.  Directory.Exists
            # returns false for inaccessible directories without a provider error.
            if ([System.IO.Directory]::Exists($sd.path)) {
                Get-ChildItem -LiteralPath $sd.path -Recurse -Force -ErrorAction SilentlyContinue | Where-Object { -not $_.PSIsContainer } |
                    ForEach-Object {
                        $kb = [math]::Round($_.Length/1KB, 1)
                        $totalSizeKB += $kb
                        $files += @{
                            label    = $sd.label
                            name     = $_.Name
                            sizeKB   = $kb
                            modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                        }
                    }
            }
        }
        @{
            files        = $files
            total        = $files.Count
            totalSizeMB  = [math]::Round($totalSizeKB / 1KB, 2)
            wsearchState = [string]$svcState
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-PrintSpoolerInfo {
    # Returns pending spool jobs + XPS cache files without touching the service
    try {
        $svcState = try { (Get-Service -Name Spooler -ErrorAction Stop).Status } catch { 'Unknown' }
        $files = @()
        $sources = @(
            @{ label = 'Spool Queue'; path = "$env:SystemRoot\System32\spool\PRINTERS" },
            @{ label = 'XPS Cache';   path = "$env:LOCALAPPDATA\Microsoft\XPS" }
        )
        $totalSizeKB = 0
        foreach ($src in $sources) {
            if (Test-Path $src.path) {
                Get-ChildItem -Path $src.path -Recurse -Force -ErrorAction SilentlyContinue | Where-Object { -not $_.PSIsContainer } |
                    ForEach-Object {
                        $kb = [math]::Round($_.Length/1KB, 1)
                        $totalSizeKB += $kb
                        $files += @{
                            source   = $src.label
                            name     = $_.Name
                            sizeKB   = $kb
                            modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                        }
                    }
            }
        }
        @{
            files         = $files
            total         = $files.Count
            totalSizeMB   = [math]::Round($totalSizeKB / 1KB, 2)
            spoolerState  = [string]$svcState
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-WSLDataInfo {
    # Store WSL distributions retain their virtual disks below the package LocalState directory.
    # Registered distro names are included without opening the distributions or their disks.
    try {
        $files = @()
        $totalBytes = [int64]0
        $packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
        if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $packagesRoot -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $vhdx = Join-Path $_.FullName 'LocalState\ext4.vhdx'
                if (Test-Path $vhdx -PathType Leaf -ErrorAction SilentlyContinue) {
                    $item = Get-Item -LiteralPath $vhdx -Force -ErrorAction SilentlyContinue
                    if ($item) {
                        $files += @{ name = "Store WSL: $($_.Name)\\LocalState\\ext4.vhdx"; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                        $totalBytes += $item.Length
                    }
                }
            }
        }
        $wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
        if ($wsl) {
            foreach ($distro in @(& $wsl.Source -l -q 2>$null)) {
                $name = ([string]$distro).Trim()
                if ($name) { $files += @{ name = "Registered WSL distro: $name"; sizeKB = 0; modified = '' } }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-DockerDesktopDataInfo {
    # Docker Desktop stores both WSL distributions below LocalAppData and its desktop log in AppData.
    try {
        $files = @()
        $totalBytes = [int64]0
        $dockerWsl = Join-Path $env:LOCALAPPDATA 'Docker\wsl'
        if (Test-Path $dockerWsl -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $dockerWsl -Recurse -File -Filter '*.vhdx' -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{ name = "Docker WSL: $($_.FullName.Substring($dockerWsl.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $_.Length
            }
        }
        $dockerLog = Join-Path $env:APPDATA 'Docker\log.txt'
        if (Test-Path $dockerLog -PathType Leaf -ErrorAction SilentlyContinue) {
            $item = Get-Item -LiteralPath $dockerLog -Force -ErrorAction SilentlyContinue
            if ($item) {
                $files += @{ name = 'Docker: log.txt'; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $item.Length
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-VirtualMachineArtifactsInfo {
    # This intentionally inventories only snapshot, log, and recent-item artefacts; it never starts or stops a VM.
    try {
        $files = @()
        $totalBytes = [int64]0
        $vmwareRoot = Join-Path $env:USERPROFILE 'Documents\Virtual Machines'
        if (Test-Path $vmwareRoot -ErrorAction SilentlyContinue) {
            foreach ($pattern in @('*-0000*.vmdk','*.vmsn','vmware*.log')) {
                Get-ChildItem -Path $vmwareRoot -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object {
                    $files += @{ name = "VMware: $($_.FullName.Substring($vmwareRoot.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                    $totalBytes += $_.Length
                }
            }
        }
        $virtualBoxRoot = Join-Path $env:USERPROFILE 'VirtualBox VMs'
        if (Test-Path $virtualBoxRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $virtualBoxRoot -Directory -Recurse -Force -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq 'Snapshots' } | ForEach-Object {
                foreach ($pattern in @('*.vdi','*.vbox-prev')) {
                    Get-ChildItem -Path $_.FullName -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object {
                        $files += @{ name = "VirtualBox snapshot: $($_.FullName.Substring($virtualBoxRoot.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                        $totalBytes += $_.Length
                    }
                }
            }
            foreach ($pattern in @('VBox.log','VBoxSVC.log')) {
                Get-ChildItem -Path $virtualBoxRoot -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object {
                    $files += @{ name = "VirtualBox log: $($_.FullName.Substring($virtualBoxRoot.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                    $totalBytes += $_.Length
                }
            }
        }
        foreach ($hostLog in @(
            (Join-Path $env:ProgramData 'VMware\vmnetdhcp.log'),
            (Join-Path $env:ProgramData 'VMware\vmnetnat.log'),
            (Join-Path $env:USERPROFILE '.VirtualBox\VirtualBox.xml')
        )) {
            if (Test-Path $hostLog -PathType Leaf -ErrorAction SilentlyContinue) {
                $item = Get-Item -LiteralPath $hostLog -Force -ErrorAction SilentlyContinue
                if ($item) {
                    $files += @{ name = "VM configuration/log: $($item.FullName)"; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                    $totalBytes += $item.Length
                }
            }
        }
        $vmwareMru = 'HKCU:\Software\VMware, Inc.\VMware Workstation'
        if (Test-Path $vmwareMru -ErrorAction SilentlyContinue) {
            $mruProperties = (Get-Item -Path $vmwareMru -ErrorAction SilentlyContinue).Property | Where-Object { $_ -match '(?i)mru|recent' }
            foreach ($property in @($mruProperties)) {
                $files += @{ name = "VMware MRU: $property"; sizeKB = 0; modified = '' }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-DeveloperCachesInfo {
    # Configuration files are surfaced by presence only; token contents are never read or returned.
    try {
        $files = @()
        $totalBytes = [int64]0
        $cacheDirs = @(
            @{ label = 'npm cache'; path = (Join-Path $env:APPDATA 'npm-cache') },
            @{ label = 'npm cache'; path = (Join-Path $env:LOCALAPPDATA 'npm-cache') },
            @{ label = 'pip cache'; path = (Join-Path $env:LOCALAPPDATA 'pip\Cache') },
            @{ label = 'cargo registry cache'; path = (Join-Path $env:USERPROFILE '.cargo\registry\cache') },
            @{ label = 'cargo registry source'; path = (Join-Path $env:USERPROFILE '.cargo\registry\src') }
        )
        foreach ($cache in $cacheDirs) {
            if (-not (Test-Path $cache.path -ErrorAction SilentlyContinue)) { continue }
            Get-ChildItem -Path $cache.path -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{ name = "$($cache.label): $($_.FullName.Substring($cache.path.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $_.Length
            }
        }
        foreach ($config in @(
            @{ label = 'npm token configuration present'; path = (Join-Path $env:USERPROFILE '.npmrc') },
            @{ label = 'cargo configuration present'; path = (Join-Path $env:USERPROFILE '.cargo\config.toml') },
            @{ label = 'cargo credentials present'; path = (Join-Path $env:USERPROFILE '.cargo\credentials.toml') }
        )) {
            if (Test-Path $config.path -PathType Leaf -ErrorAction SilentlyContinue) {
                $item = Get-Item -LiteralPath $config.path -Force -ErrorAction SilentlyContinue
                if ($item) {
                    $files += @{ name = $config.label; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                    $totalBytes += $item.Length
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-CredentialManagerInfo {
    # cmdkey exposes target names only; it never returns credential secret values here.
    try {
        $files = @()
        $targets = @{}
        foreach ($line in @(& cmdkey.exe /list 2>$null)) {
            $text = ([string]$line).Trim()
            if ($text -match '(?i)target=(.+)$') {
                $target = $Matches[1].Trim()
                if ($target) { $targets[$target] = $true }
            }
        }
        foreach ($target in $targets.Keys | Sort-Object) {
            $files += @{ name = "Saved credential target: $target"; sizeKB = 0; modified = '' }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-NetworkWizardHistoryInfo {
    try {
        $files = @()
        $networkWizard = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Network\Network Connection Wizard'
        if (Test-Path $networkWizard -ErrorAction SilentlyContinue) {
            $item = Get-Item -Path $networkWizard -ErrorAction SilentlyContinue
            foreach ($property in @($item.Property | Where-Object { $_ -notmatch '^PS' })) {
                $files += @{ name = "Network Connection Wizard value: $property"; sizeKB = 0; modified = '' }
            }
            Get-ChildItem -Path $networkWizard -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{ name = "Network Connection Wizard subkey: $($_.PSChildName)"; sizeKB = 0; modified = '' }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-WERHistoryInfo {
    # Values are reported by name only so the viewer doesn't disclose consent or exclusion contents.
    try {
        $files = @()
        foreach ($root in @('HKCU:\Software\Microsoft\Windows\Windows Error Reporting', 'HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting')) {
            foreach ($suffix in @('Consent', 'ExclusionList')) {
                $path = Join-Path $root $suffix
                if (-not (Test-Path $path -ErrorAction SilentlyContinue)) { continue }
                $item = Get-Item -Path $path -ErrorAction SilentlyContinue
                foreach ($property in @($item.Property | Where-Object { $_ -notmatch '^PS' })) {
                    $files += @{ name = "$root\\$suffix value: $property"; sizeKB = 0; modified = '' }
                }
                Get-ChildItem -Path $path -Force -ErrorAction SilentlyContinue | ForEach-Object {
                    $files += @{ name = "$root\\$suffix subkey: $($_.PSChildName)"; sizeKB = 0; modified = '' }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-InactiveUserProtectionMetadataInfo {
    # DPAPI metadata can reveal which protected material exists; only other users' profiles are inspected.
    try {
        $files = @()
        $totalBytes = [int64]0
        $currentSid = ''
        $currentProfile = [Environment]::GetFolderPath('UserProfile')
        try { $currentSid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value } catch {}
        if (-not $currentSid -and -not $currentProfile) { throw 'Current profile identity unavailable; inactive profile scan skipped.' }
        $profiles = @(Get-CimInstance -ClassName Win32_UserProfile -ErrorAction SilentlyContinue |
            Where-Object {
                -not $_.Special -and -not $_.Loaded -and $_.SID -match '^S-1-5-21-' -and
                $_.SID -ne $currentSid -and $_.LocalPath -and
                $_.LocalPath.TrimEnd('\\') -ne $currentProfile.TrimEnd('\\')
            })
        foreach ($profile in $profiles) {
            $protectRoot = Join-Path $profile.LocalPath "AppData\Roaming\Microsoft\Protect\$($profile.SID)"
            if (-not (Test-Path $protectRoot -ErrorAction SilentlyContinue)) { continue }
            Get-ChildItem -Path $protectRoot -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $relative = $_.FullName.Substring($protectRoot.Length).TrimStart('\\')
                $files += @{ name = "Inactive user DPAPI metadata: $($profile.SID)\\$relative"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $_.Length
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-StickyNotesInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
        if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $packagesRoot -Directory -Filter 'Microsoft.MicrosoftStickyNotes_*' -Force -ErrorAction SilentlyContinue | ForEach-Object {
                foreach ($name in @('plum.sqlite','plum.sqlite-wal','plum.sqlite-shm')) {
                    $path = Join-Path $_.FullName "LocalState\\$name"
                    if (Test-Path $path -PathType Leaf -ErrorAction SilentlyContinue) {
                        $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
                        if ($item) {
                            $files += @{ name = "Sticky Notes: $name"; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                            $totalBytes += $item.Length
                        }
                    }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-OneDriveMetadataInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $settingsRoot = Join-Path $env:LOCALAPPDATA 'Microsoft\\OneDrive\\settings'
        if (Test-Path $settingsRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $settingsRoot -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
                foreach ($pattern in @('*.dat','*.db')) {
                    Get-ChildItem -Path $_.FullName -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object {
                        $files += @{ name = "OneDrive metadata: $($_.FullName.Substring($settingsRoot.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                        $totalBytes += $_.Length
                    }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-SpotlightCacheInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
        if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $packagesRoot -Directory -Filter 'Microsoft.Windows.ContentDeliveryManager_*' -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $assets = Join-Path $_.FullName 'LocalState\\Assets'
                if (Test-Path $assets -ErrorAction SilentlyContinue) {
                    Get-ChildItem -Path $assets -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                        $files += @{ name = "Spotlight asset: $($_.Name)"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                        $totalBytes += $_.Length
                    }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-FontCacheInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $cacheRoots = @(
            (Join-Path $env:WINDIR 'ServiceProfiles\\LocalService\\AppData\\Local\\FontCache'),
            (Join-Path $env:LOCALAPPDATA 'Microsoft\\Windows\\Fonts')
        )
        foreach ($root in $cacheRoots) {
            if (-not (Test-Path $root -ErrorAction SilentlyContinue)) { continue }
            Get-ChildItem -Path $root -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{ name = "Font cache: $($_.FullName.Substring($root.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $_.Length
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-LegacyIconCacheInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $path = Join-Path $env:LOCALAPPDATA 'IconCache.db'
        if (Test-Path $path -PathType Leaf -ErrorAction SilentlyContinue) {
            $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
            if ($item) {
                $files += @{ name = 'IconCache.db'; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $item.Length
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-GameCapturesInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $roots = @((Join-Path $env:USERPROFILE 'Videos\\Captures'))
        $packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
        if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $packagesRoot -Directory -Force -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -like 'Microsoft.XboxGamingOverlay_*' -or $_.Name -like 'Microsoft.GamingApp_*' } |
                ForEach-Object { $roots += Join-Path $_.FullName 'LocalState' }
        }
        foreach ($root in $roots) {
            if (-not (Test-Path $root -ErrorAction SilentlyContinue)) { continue }
            Get-ChildItem -Path $root -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{ name = "Game capture: $($_.FullName.Substring($root.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $_.Length
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-PhotosCacheInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
        if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $packagesRoot -Directory -Filter 'Microsoft.Windows.Photos_*' -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $localState = Join-Path $_.FullName 'LocalState'
                if (Test-Path $localState -ErrorAction SilentlyContinue) {
                    Get-ChildItem -Path $localState -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                        $files += @{ name = "Photos cache: $($_.FullName.Substring($localState.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                        $totalBytes += $_.Length
                    }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-XboxCacheInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
        if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $packagesRoot -Directory -Force -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -like 'Microsoft.GamingApp_*' -or $_.Name -like 'Microsoft.XboxApp_*' } |
                ForEach-Object {
                    $localState = Join-Path $_.FullName 'LocalState'
                    if (Test-Path $localState -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $localState -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                            $files += @{ name = "Xbox cache: $($_.FullName.Substring($localState.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                            $totalBytes += $_.Length
                        }
                    }
                }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-CleanupArtifactFiles {
    param([Parameter(Mandatory)] [array]$Sources)
    $files = @()
    foreach ($source in $Sources) {
        if (-not (Test-Path $source.path -ErrorAction SilentlyContinue)) { continue }
        foreach ($pattern in $source.patterns) {
            Get-ChildItem -Path $source.path -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object {
                try {
                    $relative = $_.FullName.Substring($source.path.Length).TrimStart('\\')
                    $files += @{ name = "$($source.label): $relative"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                } catch {}
            }
        }
    }
    @($files)
}

function Get-CommunicationCachesInfo {
    try {
        $files = @(Get-CleanupArtifactFiles -Sources @(
            @{ label = 'Teams'; path = (Join-Path $env:APPDATA 'Microsoft\\Teams\\Cache'); patterns = @('*') },
            @{ label = 'Teams'; path = (Join-Path $env:APPDATA 'Microsoft\\Teams\\IndexedDB'); patterns = @('*') },
            @{ label = 'Teams'; path = (Join-Path $env:LOCALAPPDATA 'Packages\\MSTeams_8wekyb3d8bbwe\\LocalCache\\Microsoft\\MSTeams\\EBWebView\\Default\\IndexedDB'); patterns = @('*') },
            @{ label = 'Slack'; path = (Join-Path $env:APPDATA 'Slack\\Cache'); patterns = @('*') },
            @{ label = 'Slack'; path = (Join-Path $env:APPDATA 'Slack\\IndexedDB'); patterns = @('*') },
            @{ label = 'Slack'; path = (Join-Path $env:APPDATA 'Slack\\Local Storage\\leveldb'); patterns = @('*') },
            @{ label = 'Zoom'; path = (Join-Path $env:APPDATA 'Zoom\\data'); patterns = @('*') },
            @{ label = 'Zoom'; path = (Join-Path $env:APPDATA 'Zoom\\logs'); patterns = @('*') },
            @{ label = 'Discord'; path = (Join-Path $env:APPDATA 'discord\\Cache'); patterns = @('*') },
            @{ label = 'Discord'; path = (Join-Path $env:APPDATA 'discord\\IndexedDB'); patterns = @('*') },
            @{ label = 'Discord'; path = (Join-Path $env:APPDATA 'discord\\Local Storage\\leveldb'); patterns = @('*') }
        ))
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-EditorHistoryInfo {
    try {
        $sources = @(
            @{ label = 'VS Code'; path = (Join-Path $env:APPDATA 'Code\\User\\globalStorage'); patterns = @('storage.json','state.vscdb') },
            @{ label = 'VS Code local history'; path = (Join-Path $env:APPDATA 'Code\\User\\History'); patterns = @('*') }
        )
        $jetBrainsRoot = Join-Path $env:APPDATA 'JetBrains'
        if (Test-Path $jetBrainsRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $jetBrainsRoot -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $sources += @{ label = 'JetBrains recent projects'; path = (Join-Path $_.FullName 'options'); patterns = @('recentProjects.xml') }
                $sources += @{ label = 'JetBrains local history'; path = (Join-Path $env:LOCALAPPDATA ("JetBrains\\{0}\\LocalHistory" -f $_.Name)); patterns = @('*') }
            }
        }
        $files = @(Get-CleanupArtifactFiles -Sources $sources)
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-GitActivityInfo {
    try {
        $files = @()
        $targets = @{}
        foreach ($line in @(& cmdkey.exe /list 2>$null)) {
            $text = ([string]$line).Trim()
            if ($text -match '(?i)target=(.+)$') {
                $target = $Matches[1].Trim()
                if ($target -match '(?i)git:') { $targets[$target] = $true }
            }
        }
        foreach ($target in $targets.Keys | Sort-Object) { $files += @{ name = "Git credential target: $target"; sizeKB = 0; modified = '' } }
        $gitConfig = Join-Path $env:USERPROFILE '.gitconfig'
        if (Test-Path $gitConfig -PathType Leaf -ErrorAction SilentlyContinue) {
            $safeDirectories = @(& git.exe config --global --get-all safe.directory 2>$null)
            if ($safeDirectories.Count -gt 0) { $files += @{ name = 'Global git safe.directory entries present'; sizeKB = 0; modified = '' } }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-SSHStateInfo {
    try {
        $files = @()
        $knownHosts = Join-Path $env:USERPROFILE '.ssh\\known_hosts'
        if (Test-Path $knownHosts -PathType Leaf -ErrorAction SilentlyContinue) {
            $item = Get-Item -LiteralPath $knownHosts -Force -ErrorAction SilentlyContinue
            if ($item) { $files += @{ name = 'SSH known_hosts'; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') } }
        }
        $sshAdd = Get-Command ssh-add.exe -ErrorAction SilentlyContinue
        if ($sshAdd) {
            $null = & $sshAdd.Source -l 2>$null
            $agentState = if ($LASTEXITCODE -eq 0) { 'SSH agent identities loaded' } elseif ($LASTEXITCODE -eq 1) { 'SSH agent has no identities' } else { 'SSH agent unavailable' }
            $files += @{ name = $agentState; sizeKB = 0; modified = '' }
        }
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-RemoteAccessLogsInfo {
    try {
        $files = @(Get-CleanupArtifactFiles -Sources @(
            @{ label = 'TeamViewer'; path = (Join-Path $env:APPDATA 'TeamViewer'); patterns = @('Connections*.txt','*.trace','*.log') },
            @{ label = 'AnyDesk'; path = (Join-Path $env:APPDATA 'AnyDesk'); patterns = @('Connections*.txt','*.trace','*.log') },
            @{ label = 'AnyDesk'; path = (Join-Path $env:ProgramData 'AnyDesk'); patterns = @('Connections*.txt','*.trace','*.log') },
            @{ label = 'VNC'; path = (Join-Path $env:APPDATA 'RealVNC'); patterns = @('Connections*.txt','*.trace','*.log') },
            @{ label = 'Chrome Remote Desktop'; path = (Join-Path $env:LOCALAPPDATA 'Google\\Chrome Remote Desktop'); patterns = @('Connections*.txt','*.trace','*.log') }
        ))
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-PasswordManagerCachesInfo {
    try {
        $files = @(Get-CleanupArtifactFiles -Sources @(
            @{ label = '1Password'; path = (Join-Path $env:LOCALAPPDATA '1Password\\Cache'); patterns = @('*') },
            @{ label = '1Password'; path = (Join-Path $env:LOCALAPPDATA '1Password\\Local Storage\\leveldb'); patterns = @('*') },
            @{ label = 'Bitwarden'; path = (Join-Path $env:APPDATA 'Bitwarden\\Cache'); patterns = @('*') },
            @{ label = 'Bitwarden'; path = (Join-Path $env:APPDATA 'Bitwarden\\Local Storage\\leveldb'); patterns = @('*') },
            @{ label = 'Bitwarden'; path = (Join-Path $env:APPDATA 'Bitwarden\\Session Storage'); patterns = @('*') },
            @{ label = 'LastPass'; path = (Join-Path $env:APPDATA 'LastPass\\Cache'); patterns = @('*') },
            @{ label = 'LastPass'; path = (Join-Path $env:APPDATA 'LastPass\\Local Storage\\leveldb'); patterns = @('*') },
            @{ label = 'LastPass'; path = (Join-Path $env:APPDATA 'LastPass\\Session Storage'); patterns = @('*') }
        ))
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-GameLauncherLogsInfo {
    try {
        $files = @(Get-CleanupArtifactFiles -Sources @(
            @{ label = 'Steam log'; path = (Join-Path ${env:ProgramFiles(x86)} 'Steam\\logs'); patterns = @('*') },
            @{ label = 'Steam localconfig'; path = (Join-Path ${env:ProgramFiles(x86)} 'Steam\\userdata'); patterns = @('localconfig.vdf') },
            @{ label = 'Epic Games Launcher'; path = (Join-Path $env:LOCALAPPDATA 'EpicGamesLauncher\\Saved\\Logs'); patterns = @('*') },
            @{ label = 'Battle.net'; path = (Join-Path $env:APPDATA 'Battle.net\\Logs'); patterns = @('*') },
            @{ label = 'Battle.net'; path = (Join-Path $env:ProgramData 'Battle.net\\Logs'); patterns = @('*') }
        ))
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-AdobeRecentInfo {
    try {
        $files = @()
        $readerRoot = 'HKCU:\Software\Adobe\Acrobat Reader'
        if (Test-Path $readerRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $readerRoot -Force -ErrorAction SilentlyContinue | ForEach-Object {
                foreach ($suffix in @('AVGeneral\\cRecentFiles','AVGeneral\\cDigitalSignatures')) {
                    $path = Join-Path $_.PSPath $suffix
                    if (Test-Path $path -ErrorAction SilentlyContinue) {
                        $item = Get-Item -Path $path -ErrorAction SilentlyContinue
                        foreach ($property in @($item.Property | Where-Object { $_ -notmatch '^PS' })) {
                            $files += @{ name = "Adobe $suffix value: $property"; sizeKB = 0; modified = '' }
                        }
                    }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-OfficeTempFilesInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $roots = @(
            (Join-Path $env:USERPROFILE 'Documents'),
            (Join-Path $env:USERPROFILE 'Desktop'),
            (Join-Path $env:USERPROFILE 'Downloads')
        )
        foreach ($root in $roots) {
            if (-not (Test-Path $root -ErrorAction SilentlyContinue)) { continue }
            Get-ChildItem -Path $root -Recurse -Depth 2 -File -Include '~$*.doc*','~$*.xls*','~$*.ppt*' -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{ name = "Office temporary file: $($_.FullName.Substring($root.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $_.Length
            }
        }
        $tempRoot = $env:TEMP
        if (Test-Path $tempRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $tempRoot -Recurse -Depth 2 -File -Filter '*.tmp' -Force -ErrorAction SilentlyContinue |
                Sort-Object LastWriteTime -Descending | Select-Object -First 500 | ForEach-Object {
                    $files += @{ name = "Temporary file: $($_.FullName.Substring($tempRoot.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                    $totalBytes += $_.Length
                }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Test-WCCloudProviderInactive {
    param([Parameter(Mandatory)] [ValidateSet('OneDrive','Dropbox','GoogleDrive')] [string]$Provider)
    $processNames = @{ OneDrive = @('OneDrive'); Dropbox = @('Dropbox'); GoogleDrive = @('GoogleDriveFS','GoogleDrive') }
    $installPaths = @{
        OneDrive = @((Join-Path $env:LOCALAPPDATA 'Microsoft\\OneDrive\\OneDrive.exe'), (Join-Path $env:ProgramFiles 'Microsoft OneDrive\\OneDrive.exe'))
        Dropbox = @((Join-Path $env:LOCALAPPDATA 'Dropbox\\bin\\Dropbox.exe'), (Join-Path $env:ProgramFiles 'Dropbox\\Client\\Dropbox.exe'))
        GoogleDrive = @((Join-Path $env:LOCALAPPDATA 'Google\\DriveFS\\GoogleDriveFS.exe'), (Join-Path $env:ProgramFiles 'Google\\Drive File Stream\\GoogleDriveFS.exe'))
    }
    if (@(Get-Process -Name $processNames[$Provider] -ErrorAction SilentlyContinue).Count -gt 0) { return $false }
    foreach ($path in $installPaths[$Provider]) {
        if (Test-Path $path -PathType Leaf -ErrorAction SilentlyContinue) { return $false }
    }
    return $true
}

function Get-WCCloudPlaceholderFiles {
    param([Parameter(Mandatory)] [string]$UserProfilePath)
    $placeholders = @()
    if (-not (Test-Path $UserProfilePath -ErrorAction SilentlyContinue)) { return @() }
    $roots = @()
    Get-ChildItem -Path $UserProfilePath -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
        if ($_.Name -like 'OneDrive*') { $roots += @{ provider = 'OneDrive'; path = $_.FullName } }
        elseif ($_.Name -eq 'Dropbox') { $roots += @{ provider = 'Dropbox'; path = $_.FullName } }
        elseif ($_.Name -eq 'Google Drive') { $roots += @{ provider = 'GoogleDrive'; path = $_.FullName } }
    }
    foreach ($root in $roots) {
        if (-not (Test-WCCloudProviderInactive -Provider $root.provider)) { continue }
        if (-not (Test-Path $root.path -ErrorAction SilentlyContinue)) { continue }
        Get-ChildItem -Path $root.path -Recurse -Force -ErrorAction SilentlyContinue | Where-Object {
            ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
        } | ForEach-Object { $placeholders += @{ provider = $root.provider; item = $_ } }
    }
    @($placeholders)
}

function Get-FirewallLogInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        foreach ($profile in @(Get-NetFirewallProfile -ErrorAction SilentlyContinue)) {
            $path = [string]$profile.LogFileName
            if (-not $path -or -not (Test-Path $path -PathType Leaf -ErrorAction SilentlyContinue)) { continue }
            $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
            if ($item) {
                $files += @{ name = "Firewall $($profile.Name) log: $($item.Name)"; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $item.Length
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-NeighborCacheInfo {
    try {
        $files = @()
        Get-NetNeighbor -ErrorAction SilentlyContinue | ForEach-Object {
            $files += @{ name = "Neighbor: $($_.InterfaceAlias) $($_.IPAddress) ($($_.State))"; sizeKB = 0; modified = '' }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-NetBIOSCacheInfo {
    try {
        $files = @()
        foreach ($line in @(& nbtstat.exe -c 2>$null)) {
            $text = ([string]$line).Trim()
            if ($text -match '<[0-9A-F]{2}>' -and $text -notmatch 'NetBIOS Remote Cache') {
                $files += @{ name = "NetBIOS cache: $text"; sizeKB = 0; modified = '' }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-GeolocationCacheInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $lfSvc = 'C:\ProgramData\Microsoft\Windows\LfSvc'
        if (Test-Path $lfSvc -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $lfSvc -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{ name = "Geolocation cache: $($_.FullName.Substring($lfSvc.Length).TrimStart('\\'))"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $_.Length
            }
        }
        $locationConsent = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location'
        if (Test-Path $locationConsent -ErrorAction SilentlyContinue) {
            (Get-Item -Path $locationConsent -ErrorAction SilentlyContinue).Property | Where-Object { $_ -notmatch '^PS' } | ForEach-Object {
                $files += @{ name = "Location consent metadata: $_"; sizeKB = 0; modified = '' }
            }
            Get-ChildItem -Path $locationConsent -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{ name = "Location consent usage metadata: $($_.PSChildName)"; sizeKB = 0; modified = '' }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-VPNPhonebooksInfo {
    try {
        $files = @(Get-CleanupArtifactFiles -Sources @(
            @{ label = 'User VPN'; path = (Join-Path $env:APPDATA 'Microsoft\\Network\\Connections\\Pbk'); patterns = @('rasphone.pbk','*.log','*.etl') },
            @{ label = 'User VPN'; path = (Join-Path $env:LOCALAPPDATA 'Microsoft\\Network\\Connections'); patterns = @('rasphone.pbk','*.log','*.etl') },
            @{ label = 'System VPN'; path = (Join-Path $env:ProgramData 'Microsoft\\Network\\Connections\\Pbk'); patterns = @('rasphone.pbk','*.log','*.etl') }
        ))
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-ProxyCacheInfo {
    try {
        $files = @()
        $connections = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings\Connections'
        if (Test-Path $connections -ErrorAction SilentlyContinue) {
            (Get-Item -Path $connections -ErrorAction SilentlyContinue).Property | Where-Object { $_ -notmatch '^PS' } | ForEach-Object {
                $files += @{ name = "Proxy connection metadata: $_"; sizeKB = 0; modified = '' }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-CloudPlaceholdersInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        foreach ($entry in @(Get-WCCloudPlaceholderFiles -UserProfilePath $env:USERPROFILE)) {
            $item = $entry.item
            $size = if ($item.PSIsContainer) { 0 } else { [int64]$item.Length }
            $files += @{ name = "Orphaned $($entry.provider) placeholder: $($item.FullName)"; sizeKB = [math]::Round($size / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
            $totalBytes += $size
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-BITSQueueInfo {
    try {
        $files = @()
        try { $transfers = @(Get-BitsTransfer -AllUsers -ErrorAction Stop) } catch { $transfers = @(Get-BitsTransfer -ErrorAction SilentlyContinue) }
        foreach ($transfer in $transfers) {
            $files += @{ name = "BITS job: $($transfer.DisplayName) ($($transfer.JobState))"; sizeKB = 0; modified = '' }
        }
        $downloader = Join-Path $env:ProgramData 'Microsoft\\Network\\Downloader'
        if (Test-Path $downloader -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $downloader -File -Filter 'qmgr*.dat' -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{ name = "BITS queue database: $($_.Name)"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
            }
        }
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-CellularHistoryInfo {
    try {
        $files = @()
        foreach ($line in @(& netsh.exe mbn show interfaces 2>$null)) {
            $text = ([string]$line).Trim()
            if ($text -match '(?i)^(Name|Interface Name)\s*:\s*(.+)$') {
                $files += @{ name = "Cellular interface: $($Matches[2].Trim())"; sizeKB = 0; modified = '' }
            }
        }
        foreach ($line in @(& netsh.exe mbn show profiles 2>$null)) {
            $text = ([string]$line).Trim()
            if ($text -match '(?i)^(Profile|Profile Name)\s*:\s*(.+)$') {
                $files += @{ name = "Cellular profile: $($Matches[2].Trim())"; sizeKB = 0; modified = '' }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-WebCacheInfo {
    # Returns WebCacheV*.dat (ESE database — WebCacheV01.dat historically, WebCacheV24.dat
    # on newer builds) + rotating V01*.log transaction logs
    try {
        $webCacheDir = "$env:LOCALAPPDATA\Microsoft\Windows\WebCache"
        $files = @()
        if (Test-Path $webCacheDir) {
            $targets  = @(Get-ChildItem -Path (Join-Path $webCacheDir 'WebCacheV*.dat') -Force -ErrorAction SilentlyContinue)
            $targets += @(Get-ChildItem -Path (Join-Path $webCacheDir 'V01*.log') -Force -ErrorAction SilentlyContinue)
            foreach ($f in $targets) {
                $files += @{
                    name     = $f.Name
                    sizeKB   = [math]::Round($f.Length / 1KB, 1)
                    modified = $f.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-ThumbnailCacheInfo {
    # Returns Explorer thumbnail + icon cache database file listing
    try {
        $explorerDir = "$env:LOCALAPPDATA\Microsoft\Windows\Explorer"
        $files = @()
        if (Test-Path $explorerDir) {
            $targets  = @(Get-ChildItem -Path (Join-Path $explorerDir 'thumbcache_*.db') -Force -ErrorAction SilentlyContinue)
            $targets += @(Get-ChildItem -Path (Join-Path $explorerDir 'iconcache_*.db') -Force -ErrorAction SilentlyContinue)
            foreach ($f in $targets) {
                $files += @{
                    name     = $f.Name
                    sizeKB   = [math]::Round($f.Length / 1KB, 1)
                    modified = $f.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-NotificationDatabaseInfo {
    # Returns the WPN (Windows Push Notification) database + its WAL/SHM sidecar files
    try {
        $notifDir = "$env:LOCALAPPDATA\Microsoft\Windows\Notifications"
        $files = @()
        if (Test-Path $notifDir) {
            foreach ($name in @('wpndatabase.db', 'wpndatabase.db-wal', 'wpndatabase.db-shm')) {
                $p = Join-Path $notifDir $name
                if (Test-Path $p) {
                    $f = Get-Item -Path $p -Force -ErrorAction SilentlyContinue
                    if ($f) {
                        $files += @{
                            name     = $f.Name
                            sizeKB   = [math]::Round($f.Length / 1KB, 1)
                            modified = $f.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                        }
                    }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-BranchCacheInfo {
    # Returns BranchCache publication cache (PeerDistPub) + hosted-cache reply data (PeerDistRep) file listing
    try {
        $sources = @(
            @{ label = 'PeerDistRep'; path = "$env:SystemRoot\System32\PeerDistRep" },
            @{ label = 'PeerDistPub'; path = "$env:SystemRoot\ServiceProfiles\NetworkService\AppData\Local\PeerDistPub" }
        )
        $files = @()
        foreach ($src in $sources) {
            # PeerDistPub lives under the NetworkService profile whose ACL denies
            # even elevated reads on some systems; -EA SilentlyContinue keeps the
            # access-denied error (a non-terminating error try/catch won't trap)
            # out of the router's output stream.
            if (Test-Path $src.path -ErrorAction SilentlyContinue) {
                Get-ChildItem -Path $src.path -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                    $files += @{
                        source   = $src.label
                        name     = $_.Name
                        sizeKB   = [math]::Round($_.Length / 1KB, 1)
                        modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Phase E: system diagnostic / servicing / defender-history viewers (read-only) ---

function Get-EventTranscriptInfo {
    # Returns the Diagnostic Data Viewer's EventTranscript.db + its rotating
    # ESE transaction logs (*.jrs / *.rbs). System-wide (ProgramData), not per-user.
    try {
        $etDir = "$env:ProgramData\Microsoft\Diagnosis\EventTranscript"
        $files = @()
        if (Test-Path $etDir -ErrorAction SilentlyContinue) {
            $targets  = @(Get-ChildItem -Path (Join-Path $etDir 'EventTranscript.db') -Force -ErrorAction SilentlyContinue)
            $targets += @(Get-ChildItem -Path (Join-Path $etDir '*.jrs') -Force -ErrorAction SilentlyContinue)
            $targets += @(Get-ChildItem -Path (Join-Path $etDir '*.rbs') -Force -ErrorAction SilentlyContinue)
            foreach ($f in $targets) {
                $files += @{
                    source   = 'EventTranscript'
                    name     = $f.Name
                    sizeKB   = [math]::Round($f.Length / 1KB, 1)
                    modified = $f.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-ActivitiesTimelineInfo {
    # Returns Windows Timeline / Activities History database (ActivitiesCache.db)
    # + its WAL/SHM sidecars, recursed across every per-identity subfolder
    # under ConnectedDevicesPlatform.
    try {
        $cdpDir = "$env:LOCALAPPDATA\ConnectedDevicesPlatform"
        $files = @()
        if (Test-Path $cdpDir -ErrorAction SilentlyContinue) {
            $targets = @(Get-ChildItem -Path $cdpDir -Include 'ActivitiesCache.db','*.db-wal','*.db-shm' -Recurse -Force -ErrorAction SilentlyContinue)
            foreach ($f in $targets) {
                $files += @{
                    source   = 'ConnectedDevicesPlatform'
                    name     = $f.Name
                    sizeKB   = [math]::Round($f.Length / 1KB, 1)
                    modified = $f.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-RdpBitmapCacheInfo {
    # Returns RDP client bitmap-cache tile files (thumbnails cached from past
    # outgoing RDP sessions).
    try {
        $rdpCacheDir = "$env:LOCALAPPDATA\Microsoft\Terminal Server Client\Cache"
        $files = @()
        if (Test-Path $rdpCacheDir -ErrorAction SilentlyContinue) {
            $targets  = @(Get-ChildItem -Path (Join-Path $rdpCacheDir '*.bin') -Force -ErrorAction SilentlyContinue)
            $targets += @(Get-ChildItem -Path (Join-Path $rdpCacheDir '*.bmc') -Force -ErrorAction SilentlyContinue)
            foreach ($f in $targets) {
                $files += @{
                    source   = 'Terminal Server Client\Cache'
                    name     = $f.Name
                    sizeKB   = [math]::Round($f.Length / 1KB, 1)
                    modified = $f.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-ServicingLogsInfo {
    # Returns Windows servicing log files: CBS (Component-Based Servicing) + DISM.
    # System paths — Test-Path is guarded because ACLs can deny access on some builds.
    try {
        $sources = @(
            @{ label = 'CBS';  path = "$env:SystemRoot\Logs\CBS" },
            @{ label = 'DISM'; path = "$env:SystemRoot\Logs\DISM" }
        )
        $files = @()
        foreach ($src in $sources) {
            if (Test-Path $src.path -ErrorAction SilentlyContinue) {
                Get-ChildItem -Path $src.path -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                    $files += @{
                        source   = $src.label
                        name     = $_.Name
                        sizeKB   = [math]::Round($_.Length / 1KB, 1)
                        modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-DeviceInstallLogsInfo {
    # Returns device-installation setup logs (setupapi.dev.log, setupapi.app.log, etc.)
    try {
        $infDir = "$env:SystemRoot\INF"
        $files = @()
        if (Test-Path $infDir -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path (Join-Path $infDir 'setupapi.*.log') -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{
                    source   = 'INF'
                    name     = $_.Name
                    sizeKB   = [math]::Round($_.Length / 1KB, 1)
                    modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-UsageTraceLogsInfo {
    # Returns ETW trace logs: SleepStudy (modern-standby diagnostics) + WDI
    # (Windows Diagnostic Infrastructure) + WMI trace logs.
    try {
        $sources = @(
            @{ label = 'SleepStudy'; path = "$env:SystemRoot\System32\SleepStudy" },
            @{ label = 'WDI';        path = "$env:SystemRoot\System32\WDI\LogFiles" },
            @{ label = 'WMI';        path = "$env:SystemRoot\System32\LogFiles\WMI" }
        )
        $files = @()
        foreach ($src in $sources) {
            if (Test-Path $src.path -ErrorAction SilentlyContinue) {
                Get-ChildItem -Path (Join-Path $src.path '*.etl') -Force -ErrorAction SilentlyContinue | ForEach-Object {
                    $files += @{
                        source   = $src.label
                        name     = $_.Name
                        sizeKB   = [math]::Round($_.Length / 1KB, 1)
                        modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-DefenderHistoryInfo {
    # Returns Windows Defender scan-history records + the MpCmdRun.log support log.
    # System path (ProgramData) — ACL-guarded like the other Phase E system viewers.
    try {
        $files = @()
        $historyDir = "$env:ProgramData\Microsoft\Windows Defender\Scans\History\Service"
        if (Test-Path $historyDir -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $historyDir -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $files += @{
                    source   = 'ScanHistory'
                    name     = $_.Name
                    sizeKB   = [math]::Round($_.Length / 1KB, 1)
                    modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                }
            }
        }
        $mpLog = "$env:ProgramData\Microsoft\Windows Defender\Support\MpCmdRun.log"
        if (Test-Path $mpLog -ErrorAction SilentlyContinue) {
            $f = Get-Item -Path $mpLog -Force -ErrorAction SilentlyContinue
            if ($f) {
                $files += @{
                    source   = 'Support'
                    name     = $f.Name
                    sizeKB   = [math]::Round($f.Length / 1KB, 1)
                    modified = $f.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# MULTI-USER CLEANUP CLEANUP
# ============================================================================
#
# These functions extend the per-category cleaners above to operate across
# all Windows user accounts on the machine, not just the current session.
#
# Architecture:
#   Get-UserProfiles           — enumerate non-system user profile dirs
#   Get-LoggedInUsers          — find accounts with active/disconnected sessions
#   Invoke-WithUserHive        — load NTUSER.DAT, run a scriptblock, unload
#   Invoke-CleanupClearForUser — clean specified categories for one account
#   Invoke-CleanupClearAllUsers — orchestrate across all/selected accounts
# ============================================================================

# Returns true for real, user-facing profile folders. Excludes Windows
# defaults plus automation/sandbox profiles that can have S-1-5-21 SIDs but
# are not meaningful accounts for per-user cleanup.
function Test-WCVisibleUserProfile {
    param(
        [string]$Name,
        [string]$DisplayName,
        [string]$Path
    )
    $hiddenExact = @('Default', 'Default User', 'defaultuser0', 'Public', 'All Users', 'Sandbox', 'WDAGUtilityAccount')
    $hiddenFragments = @('sandbox', 'codex', 'defaultuser', 'wdagutilityaccount')
    if ([string]::IsNullOrWhiteSpace($Name)) { return $false }
    if ($hiddenExact -contains $Name -or $hiddenExact -contains $DisplayName) { return $false }
    $probe = "$Name $DisplayName $Path".ToLowerInvariant()
    foreach ($fragment in $hiddenFragments) {
        if ($probe.Contains($fragment)) { return $false }
    }
    return $true
}

# Returns all non-system user profiles as objects with Name and Path.
# Mirrors the user_profiles() function in acquire.rs.
function Get-UserProfiles {
    # Win32_UserProfile is the authoritative Windows list of user profiles.
    # Filter: S-1-5-21-* SIDs = real local/domain user accounts.
    # Excludes S-1-5-82-* (IIS app pool virtual accounts like ".NET v4.5"),
    # S-1-5-80-* (NT Service accounts), S-1-5-18/19/20 (System/Network/Local).
    $sysDrive = $env:SystemDrive
    # Identify the current account by SID, NOT by name. $env:USERNAME is the
    # SAM logon name, which diverges from the profile FOLDER name after an
    # account rename (the C:\Users\<name> folder is created once and never
    # renamed). SID is stable and unambiguous.
    $mySid = ''
    try { $mySid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value } catch {}

    $profiles = Get-CimInstance -ClassName Win32_UserProfile -ErrorAction SilentlyContinue |
        Where-Object {
            -not $_.Special -and
            $_.SID -match '^S-1-5-21-' -and
            $_.LocalPath -like "$sysDrive\Users\*"
        } |
        ForEach-Object {
            $leaf = Split-Path $_.LocalPath -Leaf
            # `name` is the folder leaf — the stable internal key used to target
            # clears/scans. `displayName` is the real (possibly renamed) account
            # name resolved from the SID, falling back to the folder name.
            $display = $leaf
            try { $display = (([Security.Principal.SecurityIdentifier]$_.SID).Translate([Security.Principal.NTAccount]).Value -split '\\')[-1] } catch {}
            if (Test-WCVisibleUserProfile -Name $leaf -DisplayName $display -Path $_.LocalPath) {
                @{ name = $leaf; displayName = $display; path = $_.LocalPath; sid = $_.SID; isCurrent = ($_.SID -eq $mySid) }
            }
        }
    # currentUser is the current SID's profile FOLDER name (so it matches the
    # `name` keys the UI uses), not $env:USERNAME — otherwise selecting your own
    # account would be treated as "another user".
    $myLeaf = (@($profiles) | Where-Object { $_.isCurrent } | Select-Object -First 1).name
    @{ profiles = @($profiles); total = @($profiles).Count; currentUser = $myLeaf; currentSid = $mySid; isAdmin = [bool](Test-IsAdmin) }
}

# Returns usernames of accounts with Active or Disconnected RDP/console sessions.
# These users have live registry hives — reg load would fail or corrupt data.
function Get-LoggedInUsers {
    $raw = & query session 2>&1
    $users = @()
    foreach ($line in $raw) {
        # query session output columns: SESSIONNAME  USERNAME  ID  STATE  ...
        if ($line -match '^\s*(?:>?\s*)(\S+)\s+(\S+)\s+\d+\s+(Active|Disc)') {
            $users += $Matches[2]
        }
    }
    @{ users = @($users | Select-Object -Unique) }
}

# Load an offline NTUSER.DAT hive under a temporary key, run $ScriptBlock
# with -HiveRoot pointing at it, then unload the hive.
# Requires Administrator or SYSTEM context.
# $ScriptBlock receives one named parameter: [string]$HiveRoot — the full
# Registry:: path prefix to use instead of "HKCU:".
function Invoke-WithUserHive {
    param(
        [Parameter(Mandatory)] [string]$UserProfilePath,
        [Parameter(Mandatory)] [scriptblock]$ScriptBlock
    )
    $username  = Split-Path $UserProfilePath -Leaf
    $ntuser    = Join-Path $UserProfilePath 'NTUSER.DAT'
    $mountName = "WC_TempHive_$username"
    $mountPath = "HKEY_USERS\$mountName"

    if (-not (Test-Path $ntuser)) {
        Write-Warning "Invoke-WithUserHive: NTUSER.DAT not found at $ntuser"
        return
    }

    # Check the hive isn't already loaded (e.g. user partially logged off)
    $alreadyLoaded = Test-Path "Registry::$mountPath"
    if (-not $alreadyLoaded) {
        $loadOut = & reg load $mountPath $ntuser 2>&1
        $loadOk  = ($LASTEXITCODE -eq 0) -and (Test-Path "Registry::$mountPath")
        if (-not $loadOk) {
            Write-Warning "Invoke-WithUserHive: reg load failed for $username - $($loadOut -join ' ')"
            return
        }
    }

    try {
        & $ScriptBlock -HiveRoot "Registry::$mountPath"
    } finally {
        # Release .NET registry handles before unloading
        [System.GC]::Collect()
        [System.GC]::WaitForPendingFinalizers()
        if (-not $alreadyLoaded) {
            $null = & reg unload $mountPath 2>&1
        }
    }
}

# Clean specified cleanup categories for a single user account.
# $UserProfilePath: full path to the profile (e.g. C:\Users\Bob)
# $CategoryIds: array of category IDs from cleanupCategories.ts
# $HiveRoot: "HKCU:" for current user, "Registry::HKEY_USERS\WC_TempHive_*" for offline
function Invoke-CleanupClearForUser {
    param(
        [Parameter(Mandatory)] [string]$UserProfilePath,
        [Parameter(Mandatory)] [string[]]$CategoryIds,
        [string]$HiveRoot = 'HKCU:'
    )
    # Secure-erase shims. The original implementation called Erase-OneFile /
    # Erase-Dir, but those are only ever defined INSIDE the $script:EraseFunctions
    # here-string in auto-erase.ps1 (base64-embedded into scheduled tasks) — they
    # are NOT live functions in this dispatch session, so every call silently
    # threw and was swallowed by the per-category catch, making multi-user
    # clears a no-op. Delegate to Remove-ItemSecure (core/utils.ps1, always
    # loaded) which does the same secure single-pass erase for both files and directories.
    function Erase-OneFile($p) { if ($p) { Remove-ItemSecure -Path $p } }
    function Erase-Dir($d)     { if ($d) { Remove-ItemSecure -Path $d } }
    function Get-CleanupTargetFiles([array]$Sources) {
        $targets = @()
        foreach ($source in $Sources) {
            if (-not (Test-Path $source.path -ErrorAction SilentlyContinue)) { continue }
            foreach ($pattern in $source.patterns) {
                $targets += @(Get-ChildItem -Path $source.path -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue |
                    ForEach-Object { $_.FullName })
            }
        }
        @($targets)
    }

    $appData  = Join-Path $UserProfilePath 'AppData\Roaming'
    $localApp = Join-Path $UserProfilePath 'AppData\Local'
    $docs     = Join-Path $UserProfilePath 'Documents'

    # Per-category verification result, returned to the caller so
    # Invoke-CleanupClearAllUsers can report genuine failures instead of a
    # hardcoded 'cleaned' regardless of whether anything was actually removed.
    $catResults = @{}

    foreach ($cat in $CategoryIds) {
        try {
            switch ($cat) {
                'rdpHistory' {
                    foreach ($k in @(
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\Default",
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\Servers",
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\LocalDevices"
                    )) {
                        if (Test-Path $k) { Remove-ItemSecure -Path $k }
                    }
                    Erase-OneFile (Join-Path $docs 'Default.rdp')
                    Erase-OneFile (Join-Path $appData 'Microsoft\Windows\Recent\AutomaticDestinations\1b4dd67f29cb1962.automaticDestinations-ms')
                    Erase-Dir    (Join-Path $localApp 'Microsoft\Terminal Server Client\Cache')
                    # Remove stored TERMSRV credentials (only works for current user context)
                    if ($HiveRoot -eq 'HKCU:') {
                        cmdkey /list 2>$null | Select-String 'Target: Domain:target=TERMSRV/' | ForEach-Object {
                            $t = $_.ToString().Split('=')[1]; cmdkey /delete:$t 2>$null | Out-Null
                        }
                    }
                    $stillThere = @(
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\Default",
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\Servers",
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\LocalDevices"
                    ) | Where-Object { Test-Path $_ }
                    $catResults[$cat] = if ($stillThere.Count -eq 0) { 'cleaned' } else { 'failed' }
                }
                'recentFiles' {
                    $recentDir = Join-Path $appData 'Microsoft\Windows\Recent'
                    Erase-Dir $recentDir
                    $officeRecent = Join-Path $appData 'Microsoft\Office\Recent'
                    if (Test-Path $officeRecent) {
                        Get-ChildItem $officeRecent -Recurse -File -Force -EA SilentlyContinue |
                            ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $recentLeft = if (Test-Path $recentDir) { @(Get-ChildItem $recentDir -File -Force -EA SilentlyContinue).Count } else { 0 }
                    $officeLeft = if (Test-Path $officeRecent) { @(Get-ChildItem $officeRecent -Recurse -File -Force -EA SilentlyContinue).Count } else { 0 }
                    $catResults[$cat] = if (($recentLeft + $officeLeft) -eq 0) { 'cleaned' } else { 'failed' }
                }
                'jumpLists' {
                    $autoDest = Join-Path $appData 'Microsoft\Windows\Recent\AutomaticDestinations'
                    $custDest = Join-Path $appData 'Microsoft\Windows\Recent\CustomDestinations'
                    Erase-Dir $autoDest
                    Erase-Dir $custDest
                    $autoLeft = if (Test-Path $autoDest) { @(Get-ChildItem $autoDest -File -Force -EA SilentlyContinue).Count } else { 0 }
                    $custLeft = if (Test-Path $custDest) { @(Get-ChildItem $custDest -File -Force -EA SilentlyContinue).Count } else { 0 }
                    $catResults[$cat] = if (($autoLeft + $custLeft) -eq 0) { 'cleaned' } else { 'failed' }
                }
                'psHistory' {
                    $hist = Join-Path $appData 'Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt'
                    if (Test-Path $hist) { Erase-OneFile $hist }
                }
                'browserFootprints' {
                    $chromiumRoots = @(
                        (Join-Path $localApp 'Google\Chrome\User Data'),
                        (Join-Path $localApp 'Microsoft\Edge\User Data'),
                        (Join-Path $localApp 'BraveSoftware\Brave-Browser\User Data'),
                        (Join-Path $appData  'Opera Software\Opera Stable'),
                        (Join-Path $localApp 'Vivaldi\User Data')
                    )
                    $chromiumArtifacts = @(
                        'History','History-journal','Cookies','Cookies-journal',
                        'Login Data','Login Data-journal','Top Sites','Top Sites-journal',
                        'Favicons','Favicons-journal','Web Data','Web Data-journal',
                        'Visited Links','Shortcuts','Shortcuts-journal',
                        'Media History','Media History-journal'
                    )
                    foreach ($root in $chromiumRoots) {
                        if (-not (Test-Path $root)) { continue }
                        Get-ChildItem $root -Directory -EA SilentlyContinue |
                            Where-Object { $_.Name -eq 'Default' -or $_.Name -like 'Profile*' } |
                            ForEach-Object {
                                $prof = $_.FullName
                                foreach ($a in $chromiumArtifacts) {
                                    $p = Join-Path $prof $a
                                    if (Test-Path $p -PathType Leaf)      { Erase-OneFile $p }
                                    elseif (Test-Path $p -PathType Container) { Erase-Dir $p }
                                }
                            }
                    }
                    $geckoRoots = @(
                        (Join-Path $appData  'Mozilla\Firefox\Profiles'),
                        (Join-Path $localApp 'LibreWolf\Profiles')
                    )
                    $geckoArtifacts = @(
                        'places.sqlite','places.sqlite-journal','places.sqlite-wal','places.sqlite-shm',
                        'cookies.sqlite','cookies.sqlite-journal','cookies.sqlite-wal','cookies.sqlite-shm',
                        'logins.json','formhistory.sqlite','favicons.sqlite','favicons.sqlite-journal'
                    )
                    foreach ($root in $geckoRoots) {
                        if (-not (Test-Path $root)) { continue }
                        Get-ChildItem $root -Directory -EA SilentlyContinue | ForEach-Object {
                            foreach ($a in $geckoArtifacts) {
                                $p = Join-Path $_.FullName $a
                                if (Test-Path $p) { Erase-OneFile $p }
                            }
                        }
                    }
                    $leftover = 0
                    foreach ($root in $chromiumRoots) {
                        if (-not (Test-Path $root)) { continue }
                        Get-ChildItem $root -Directory -EA SilentlyContinue |
                            Where-Object { $_.Name -eq 'Default' -or $_.Name -like 'Profile*' } |
                            ForEach-Object {
                                foreach ($a in $chromiumArtifacts) {
                                    if (Test-Path (Join-Path $_.FullName $a)) { $leftover++ }
                                }
                            }
                    }
                    foreach ($root in $geckoRoots) {
                        if (-not (Test-Path $root)) { continue }
                        Get-ChildItem $root -Directory -EA SilentlyContinue | ForEach-Object {
                            foreach ($a in $geckoArtifacts) {
                                if (Test-Path (Join-Path $_.FullName $a)) { $leftover++ }
                            }
                        }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'shellBags' {
                    foreach ($k in @(
                        "$HiveRoot\Software\Microsoft\Windows\Shell\Bags",
                        "$HiveRoot\Software\Microsoft\Windows\Shell\BagMRU",
                        "$HiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags",
                        "$HiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU"
                    )) {
                        if (Test-Path $k) { Remove-ItemSecure -Path $k }
                    }
                    $stillThere = @(
                        "$HiveRoot\Software\Microsoft\Windows\Shell\Bags",
                        "$HiveRoot\Software\Microsoft\Windows\Shell\BagMRU",
                        "$HiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags",
                        "$HiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU"
                    ) | Where-Object { Test-Path $_ }
                    $catResults[$cat] = if ($stillThere.Count -eq 0) { 'cleaned' } else { 'failed' }
                }
                'execCache' {
                    # UserAssist, MuiCache, and the user-side PCA Store/Persisted
                    # keys are per-user. ShimCache and the HKLM PCA keys are
                    # system-wide and handled elsewhere.
                    $ua = "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist"
                    if (Test-Path $ua -ErrorAction SilentlyContinue) {
                        Get-ChildItem $ua -EA SilentlyContinue | ForEach-Object {
                            $countPath = Join-Path $_.PSPath 'Count'
                            if (Test-Path $countPath -ErrorAction SilentlyContinue) {
                                Remove-Item -Path $countPath -Recurse -Force -ErrorAction SilentlyContinue
                            }
                        }
                    }

                    $muiCache = "$HiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"
                    if (Test-Path $muiCache -ErrorAction SilentlyContinue) {
                        Remove-Item -Path $muiCache -Recurse -Force -ErrorAction SilentlyContinue
                        New-Item -Path $muiCache -Force -ErrorAction SilentlyContinue | Out-Null
                    }

                    $recentApps = "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Search\RecentApps"
                    if (Test-Path $recentApps -ErrorAction SilentlyContinue) {
                        Get-ChildItem $recentApps -ErrorAction SilentlyContinue | ForEach-Object {
                            Remove-Item -Path $_.PSPath -Recurse -Force -ErrorAction SilentlyContinue
                        }
                    }

                    foreach ($pcaPath in @(
                        "$HiveRoot\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store",
                        "$HiveRoot\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Persisted"
                    )) {
                        if (Test-Path $pcaPath -ErrorAction SilentlyContinue) {
                            Remove-Item -Path $pcaPath -Recurse -Force -ErrorAction SilentlyContinue
                        }
                    }

                    $leftover = 0
                    if (Test-Path $ua -ErrorAction SilentlyContinue) {
                        $leftover += @(Get-ChildItem $ua -EA SilentlyContinue | ForEach-Object {
                            $countPath = Join-Path $_.PSPath 'Count'
                            if (Test-Path $countPath -ErrorAction SilentlyContinue) { $countPath }
                        }).Count
                    }
                    if (Test-Path $muiCache -ErrorAction SilentlyContinue) {
                        $muiItem = Get-Item -Path $muiCache -EA SilentlyContinue
                        $leftover += @($muiItem.Property | Where-Object { $_ -notmatch '^PS' -and $_ -ne 'LangID' }).Count
                        $leftover += @(Get-ChildItem $muiCache -EA SilentlyContinue).Count
                    }
                    if (Test-Path $recentApps -ErrorAction SilentlyContinue) {
                        $leftover += @(Get-ChildItem $recentApps -EA SilentlyContinue).Count
                    }
                    foreach ($pcaPath in @(
                        "$HiveRoot\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store",
                        "$HiveRoot\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Persisted"
                    )) {
                        if (Test-Path $pcaPath -ErrorAction SilentlyContinue) { $leftover++ }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'netDrives' {
                    # Drive mappings are session-specific; only actionable for current user
                    if ($HiveRoot -eq 'HKCU:') {
                        Get-PSDrive -PSProvider FileSystem -EA SilentlyContinue |
                            Where-Object { $_.DisplayRoot -like '\\*' } |
                            ForEach-Object { Remove-PSDrive -Name $_.Name -Force -EA SilentlyContinue }
                        net use * /delete /yes 2>$null | Out-Null
                        $remaining = @(Get-PSDrive -PSProvider FileSystem -EA SilentlyContinue | Where-Object { $_.DisplayRoot -like '\\*' })
                        $catResults[$cat] = if ($remaining.Count -eq 0) { 'cleaned' } else { 'failed' }
                    } else {
                        $catResults[$cat] = 'cleaned'
                    }
                }
                'credentialManager' {
                    $credentialDirs = @(
                        (Join-Path $appData 'Microsoft\Credentials'),
                        (Join-Path $localApp 'Microsoft\Credentials'),
                        (Join-Path $localApp 'Microsoft\Vault')
                    )
                    $currentProfile = [Environment]::GetFolderPath('UserProfile')
                    $isCurrentProfile = $currentProfile -and ($UserProfilePath.TrimEnd('\\') -eq $currentProfile.TrimEnd('\\'))
                    if ($isCurrentProfile) {
                        $targets = @{}
                        foreach ($line in @(& cmdkey.exe /list 2>$null)) {
                            $text = ([string]$line).Trim()
                            if ($text -match '(?i)target=(.+)$') {
                                $target = $Matches[1].Trim()
                                if ($target) { $targets[$target] = $true }
                            }
                        }
                        foreach ($target in $targets.Keys) { & cmdkey.exe "/delete:$target" 2>$null | Out-Null }
                    }
                    foreach ($credentialDir in $credentialDirs) { Erase-Dir $credentialDir }
                    $leftover = @($credentialDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    if ($isCurrentProfile) {
                        $remainingTargets = @(& cmdkey.exe /list 2>$null | Where-Object { ([string]$_).Trim() -match '(?i)target=(.+)$' }).Count
                        $leftover += $remainingTargets
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'networkWizardHistory' {
                    $networkWizard = "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Network\Network Connection Wizard"
                    if (Test-Path $networkWizard -ErrorAction SilentlyContinue) { Remove-ItemSecure -Path $networkWizard }
                    $catResults[$cat] = if (Test-Path $networkWizard -ErrorAction SilentlyContinue) { 'failed' } else { 'cleaned' }
                }
                'ntUserTraces' {
                    foreach ($k in @(
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\OpenSavePidlMRU",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\LastVisitedPidlMRU",
                        "$HiveRoot\Software\Microsoft\Office\16.0\Common\Internet\LocationsMRU",
                        "$HiveRoot\Software\Microsoft\Internet Explorer\TypedURLsTime",
                        "$HiveRoot\Software\Microsoft\Internet Explorer\IntelliForms\Storage2"
                    )) {
                        if (Test-Path $k -ErrorAction SilentlyContinue) {
                            # These locations store history in registry values, not
                            # child keys, so enumerate-and-remove is a silent no-op.
                            Remove-Item -Path $k -Recurse -Force -ErrorAction SilentlyContinue
                        }
                    }
                    $leftover = 0
                    foreach ($k in @(
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\OpenSavePidlMRU",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\LastVisitedPidlMRU",
                        "$HiveRoot\Software\Microsoft\Office\16.0\Common\Internet\LocationsMRU",
                        "$HiveRoot\Software\Microsoft\Internet Explorer\TypedURLsTime",
                        "$HiveRoot\Software\Microsoft\Internet Explorer\IntelliForms\Storage2"
                    )) {
                        if (Test-Path $k -ErrorAction SilentlyContinue) { $leftover++ }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'notepadState' {
                    $tabDirs = @(Get-ChildItem (Join-Path $localApp 'Packages') -Filter 'Microsoft.WindowsNotepad_*' -Directory -Force -EA SilentlyContinue |
                        ForEach-Object { Join-Path $_.FullName 'LocalState\TabState' } | Where-Object { Test-Path $_ })
                    foreach ($tab in $tabDirs) {
                        Get-ChildItem $tab -File -Force -EA SilentlyContinue | ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = 0
                    foreach ($tab in $tabDirs) { $leftover += @(Get-ChildItem $tab -File -Force -EA SilentlyContinue).Count }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'walFiles' {
                    $walFiles = @(Get-ChildItem $appData,$localApp -Include '*.db-wal','*.db-shm','*.sqlite-wal','*.sqlite-shm' `
                        -Recurse -File -Force -EA SilentlyContinue)
                    $walFiles | ForEach-Object { Erase-OneFile $_.FullName }
                    $leftover = @(Get-ChildItem $appData,$localApp -Include '*.db-wal','*.db-shm','*.sqlite-wal','*.sqlite-shm' `
                        -Recurse -File -Force -EA SilentlyContinue).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'crashDumps' {
                    $targets = @(
                        (Join-Path $localApp 'CrashDumps'),
                        "$env:SystemRoot\Minidump",
                        "$env:ProgramData\Microsoft\Windows\WER\ReportArchive",
                        "$env:ProgramData\Microsoft\Windows\WER\ReportQueue",
                        (Join-Path $localApp 'Microsoft\Windows\WER\ReportArchive'),
                        (Join-Path $localApp 'Microsoft\Windows\WER\ReportQueue')
                    )
                    foreach ($t in $targets) {
                        if (Test-Path $t) {
                            Get-ChildItem $t -Recurse -File -Force -EA SilentlyContinue | ForEach-Object { Erase-OneFile $_.FullName }
                            Get-ChildItem $t -Recurse -Directory -Force -EA SilentlyContinue |
                                Sort-Object FullName -Descending | ForEach-Object { Remove-Item $_.FullName -Force -EA SilentlyContinue }
                        }
                    }
                    $leftover = 0
                    foreach ($t in $targets) {
                        if (Test-Path $t) { $leftover += @(Get-ChildItem $t -Recurse -File -Force -EA SilentlyContinue).Count }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'recallDb' {
                    $recallDirs = @(
                        (Join-Path $localApp 'CoreAIPlatform.00\UKP'),
                        (Join-Path $localApp 'ConnectedDevicesPlatform')
                    )
                    foreach ($d in $recallDirs) {
                        if (Test-Path $d) {
                            Get-ChildItem $d -Recurse -File -Force -EA SilentlyContinue | ForEach-Object { Erase-OneFile $_.FullName }
                        }
                    }
                    $leftover = 0
                    foreach ($d in $recallDirs) {
                        if (Test-Path $d) { $leftover += @(Get-ChildItem $d -Recurse -File -Force -EA SilentlyContinue).Count }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'webCache' {
                    $webCacheDir = Join-Path $localApp 'Microsoft\Windows\WebCache'
                    if (Test-Path $webCacheDir) {
                        $targets  = @(Get-ChildItem -Path (Join-Path $webCacheDir 'WebCacheV*.dat') -Force -EA SilentlyContinue)
                        $targets += @(Get-ChildItem -Path (Join-Path $webCacheDir 'V01*.log') -Force -EA SilentlyContinue)
                        $targets | ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = 0
                    if (Test-Path $webCacheDir) {
                        $leftover += @(Get-ChildItem -Path (Join-Path $webCacheDir 'WebCacheV*.dat') -Force -EA SilentlyContinue).Count
                        $leftover += @(Get-ChildItem -Path (Join-Path $webCacheDir 'V01*.log') -Force -EA SilentlyContinue).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'activitiesTimeline' {
                    $cdpDir = Join-Path $localApp 'ConnectedDevicesPlatform'
                    if (Test-Path $cdpDir) {
                        Get-ChildItem -Path $cdpDir -Include 'ActivitiesCache.db*' -Recurse -Force -EA SilentlyContinue |
                            ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = if (Test-Path $cdpDir) {
                        @(Get-ChildItem -Path $cdpDir -Include 'ActivitiesCache.db*' -Recurse -Force -EA SilentlyContinue).Count
                    } else { 0 }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'rdpBitmapCache' {
                    $rdpCacheDir = Join-Path $localApp 'Microsoft\Terminal Server Client\Cache'
                    if (Test-Path $rdpCacheDir) {
                        $targets  = @(Get-ChildItem -Path (Join-Path $rdpCacheDir '*.bin') -Force -EA SilentlyContinue)
                        $targets += @(Get-ChildItem -Path (Join-Path $rdpCacheDir '*.bmc') -Force -EA SilentlyContinue)
                        $targets | ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = 0
                    if (Test-Path $rdpCacheDir) {
                        $leftover += @(Get-ChildItem -Path (Join-Path $rdpCacheDir '*.bin') -Force -EA SilentlyContinue).Count
                        $leftover += @(Get-ChildItem -Path (Join-Path $rdpCacheDir '*.bmc') -Force -EA SilentlyContinue).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'thumbnailDb' {
                    $explorerDir = Join-Path $localApp 'Microsoft\Windows\Explorer'
                    if (Test-Path $explorerDir) {
                        $targets  = @(Get-ChildItem -Path (Join-Path $explorerDir 'thumbcache_*.db') -Force -EA SilentlyContinue)
                        $targets += @(Get-ChildItem -Path (Join-Path $explorerDir 'iconcache_*.db') -Force -EA SilentlyContinue)
                        $targets | ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = 0
                    if (Test-Path $explorerDir) {
                        $leftover += @(Get-ChildItem -Path (Join-Path $explorerDir 'thumbcache_*.db') -Force -EA SilentlyContinue).Count
                        $leftover += @(Get-ChildItem -Path (Join-Path $explorerDir 'iconcache_*.db') -Force -EA SilentlyContinue).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'notificationDb' {
                    $notifDir = Join-Path $localApp 'Microsoft\Windows\Notifications'
                    foreach ($name in @('wpndatabase.db', 'wpndatabase.db-wal', 'wpndatabase.db-shm')) {
                        Erase-OneFile (Join-Path $notifDir $name)
                    }
                    $leftover = 0
                    foreach ($name in @('wpndatabase.db', 'wpndatabase.db-wal', 'wpndatabase.db-shm')) {
                        if (Test-Path (Join-Path $notifDir $name)) { $leftover++ }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'wslData' {
                    $wslVhdx = @()
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        $wslVhdx = @(Get-ChildItem -Path $packagesRoot -Directory -Force -ErrorAction SilentlyContinue |
                            ForEach-Object { Join-Path $_.FullName 'LocalState\ext4.vhdx' } |
                            Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue })
                    }
                    foreach ($vhdx in $wslVhdx) { Erase-OneFile $vhdx }
                    $leftover = @($wslVhdx | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'dockerDesktopData' {
                    $dockerWsl = Join-Path $localApp 'Docker\wsl'
                    $dockerLog = Join-Path $appData 'Docker\log.txt'
                    $vhdx = if (Test-Path $dockerWsl -ErrorAction SilentlyContinue) {
                        @(Get-ChildItem -Path $dockerWsl -Recurse -File -Filter '*.vhdx' -Force -ErrorAction SilentlyContinue)
                    } else { @() }
                    foreach ($item in $vhdx) { Erase-OneFile $item.FullName }
                    Erase-OneFile $dockerLog
                    $leftover = if (Test-Path $dockerWsl -ErrorAction SilentlyContinue) {
                        @(Get-ChildItem -Path $dockerWsl -Recurse -File -Filter '*.vhdx' -Force -ErrorAction SilentlyContinue).Count
                    } else { 0 }
                    if (Test-Path $dockerLog -PathType Leaf -ErrorAction SilentlyContinue) { $leftover++ }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'virtualMachineArtifacts' {
                    $targets = @()
                    $vmwareRoot = Join-Path $docs 'Virtual Machines'
                    $virtualBoxRoot = Join-Path $UserProfilePath 'VirtualBox VMs'
                    if (Test-Path $vmwareRoot -ErrorAction SilentlyContinue) {
                        foreach ($pattern in @('*-0000*.vmdk','*.vmsn','vmware*.log')) {
                            $targets += @(Get-ChildItem -Path $vmwareRoot -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
                        }
                    }
                    if (Test-Path $virtualBoxRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $virtualBoxRoot -Directory -Recurse -Force -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq 'Snapshots' } | ForEach-Object {
                            foreach ($pattern in @('*.vdi','*.vbox-prev')) {
                                $targets += @(Get-ChildItem -Path $_.FullName -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
                            }
                        }
                        foreach ($pattern in @('VBox.log','VBoxSVC.log')) {
                            $targets += @(Get-ChildItem -Path $virtualBoxRoot -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
                        }
                    }
                    $targets += @(
                        (Join-Path $env:ProgramData 'VMware\vmnetdhcp.log'),
                        (Join-Path $env:ProgramData 'VMware\vmnetnat.log')
                    ) | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }
                    $virtualBoxXml = Join-Path $UserProfilePath '.VirtualBox\VirtualBox.xml'
                    $vmwareMru = "$HiveRoot\Software\VMware, Inc.\VMware Workstation"
                    $currentProfile = [Environment]::GetFolderPath('UserProfile')
                    $canAccessProfileHive = ($HiveRoot -ne 'HKCU:') -or ($currentProfile -and ($UserProfilePath.TrimEnd('\\') -eq $currentProfile.TrimEnd('\\')))
                    foreach ($target in $targets) { Erase-OneFile $target }
                    Erase-OneFile $virtualBoxXml
                    if ($canAccessProfileHive -and (Test-Path $vmwareMru -ErrorAction SilentlyContinue)) {
                        (Get-Item -Path $vmwareMru -ErrorAction SilentlyContinue).Property |
                            Where-Object { $_ -match '(?i)mru|recent' } |
                            ForEach-Object { Remove-ItemProperty -Path $vmwareMru -Name $_ -ErrorAction SilentlyContinue }
                    }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    if (Test-Path $virtualBoxXml -PathType Leaf -ErrorAction SilentlyContinue) { $leftover++ }
                    if ($canAccessProfileHive -and (Test-Path $vmwareMru -ErrorAction SilentlyContinue)) {
                        $leftover += @((Get-Item -Path $vmwareMru -ErrorAction SilentlyContinue).Property | Where-Object { $_ -match '(?i)mru|recent' }).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'developerCaches' {
                    $cacheDirs = @(
                        (Join-Path $appData 'npm-cache'),
                        (Join-Path $localApp 'npm-cache'),
                        (Join-Path $localApp 'pip\Cache'),
                        (Join-Path $UserProfilePath '.cargo\registry\cache'),
                        (Join-Path $UserProfilePath '.cargo\registry\src')
                    )
                    $configFiles = @(
                        (Join-Path $UserProfilePath '.npmrc'),
                        (Join-Path $UserProfilePath '.cargo\config.toml'),
                        (Join-Path $UserProfilePath '.cargo\credentials.toml')
                    )
                    foreach ($cacheDir in $cacheDirs) { Erase-Dir $cacheDir }
                    foreach ($configFile in $configFiles) { Erase-OneFile $configFile }
                    $leftover = @($cacheDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $leftover += @($configFiles | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'stickyNotes' {
                    $targets = @()
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $packagesRoot -Directory -Filter 'Microsoft.MicrosoftStickyNotes_*' -Force -ErrorAction SilentlyContinue | ForEach-Object {
                            foreach ($name in @('plum.sqlite','plum.sqlite-wal','plum.sqlite-shm')) {
                                $path = Join-Path $_.FullName "LocalState\\$name"
                                if (Test-Path $path -PathType Leaf -ErrorAction SilentlyContinue) { $targets += $path }
                            }
                        }
                    }
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'oneDriveMetadata' {
                    $settingsRoot = Join-Path $localApp 'Microsoft\\OneDrive\\settings'
                    $targets = @()
                    if (Test-Path $settingsRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $settingsRoot -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
                            foreach ($pattern in @('*.dat','*.db')) {
                                $targets += @(Get-ChildItem -Path $_.FullName -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
                            }
                        }
                    }
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'spotlightCache' {
                    $assetDirs = @()
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        $assetDirs = @(Get-ChildItem -Path $packagesRoot -Directory -Filter 'Microsoft.Windows.ContentDeliveryManager_*' -Force -ErrorAction SilentlyContinue |
                            ForEach-Object { Join-Path $_.FullName 'LocalState\\Assets' } |
                            Where-Object { Test-Path $_ -ErrorAction SilentlyContinue })
                    }
                    foreach ($assetDir in $assetDirs) { Erase-Dir $assetDir }
                    $leftover = @($assetDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'legacyIconCache' {
                    $iconCache = Join-Path $localApp 'IconCache.db'
                    Erase-OneFile $iconCache
                    $catResults[$cat] = if (Test-Path $iconCache -PathType Leaf -ErrorAction SilentlyContinue) { 'failed' } else { 'cleaned' }
                }
                'gameCaptures' {
                    $captureDirs = @((Join-Path $UserProfilePath 'Videos\\Captures'))
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $packagesRoot -Directory -Force -ErrorAction SilentlyContinue |
                            Where-Object { $_.Name -like 'Microsoft.XboxGamingOverlay_*' -or $_.Name -like 'Microsoft.GamingApp_*' } |
                            ForEach-Object { $captureDirs += Join-Path $_.FullName 'LocalState' }
                    }
                    foreach ($captureDir in $captureDirs) { Erase-Dir $captureDir }
                    $leftover = @($captureDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'photosCache' {
                    $photoDirs = @()
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        $photoDirs = @(Get-ChildItem -Path $packagesRoot -Directory -Filter 'Microsoft.Windows.Photos_*' -Force -ErrorAction SilentlyContinue |
                            ForEach-Object { Join-Path $_.FullName 'LocalState' } |
                            Where-Object { Test-Path $_ -ErrorAction SilentlyContinue })
                    }
                    foreach ($photoDir in $photoDirs) { Erase-Dir $photoDir }
                    $leftover = @($photoDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'xboxCache' {
                    $xboxDirs = @()
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        $xboxDirs = @(Get-ChildItem -Path $packagesRoot -Directory -Force -ErrorAction SilentlyContinue |
                            Where-Object { $_.Name -like 'Microsoft.GamingApp_*' -or $_.Name -like 'Microsoft.XboxApp_*' } |
                            ForEach-Object { Join-Path $_.FullName 'LocalState' } |
                            Where-Object { Test-Path $_ -ErrorAction SilentlyContinue })
                    }
                    foreach ($xboxDir in $xboxDirs) { Erase-Dir $xboxDir }
                    $leftover = @($xboxDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'communicationCaches' {
                    $cacheDirs = @(
                        (Join-Path $appData 'Microsoft\\Teams\\Cache'), (Join-Path $appData 'Microsoft\\Teams\\IndexedDB'),
                        (Join-Path $localApp 'Packages\\MSTeams_8wekyb3d8bbwe\\LocalCache\\Microsoft\\MSTeams\\EBWebView\\Default\\IndexedDB'),
                        (Join-Path $appData 'Slack\\Cache'), (Join-Path $appData 'Slack\\IndexedDB'), (Join-Path $appData 'Slack\\Local Storage\\leveldb'),
                        (Join-Path $appData 'Zoom\\data'), (Join-Path $appData 'Zoom\\logs'),
                        (Join-Path $appData 'discord\\Cache'), (Join-Path $appData 'discord\\IndexedDB'), (Join-Path $appData 'discord\\Local Storage\\leveldb')
                    )
                    foreach ($cacheDir in $cacheDirs) { Erase-Dir $cacheDir }
                    $leftover = @($cacheDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'editorHistory' {
                    $targets = @((Join-Path $appData 'Code\\User\\globalStorage\\storage.json'), (Join-Path $appData 'Code\\User\\globalStorage\\state.vscdb'))
                    $historyDirs = @((Join-Path $appData 'Code\\User\\History'))
                    $jetBrainsRoot = Join-Path $appData 'JetBrains'
                    if (Test-Path $jetBrainsRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $jetBrainsRoot -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
                            $targets += Join-Path $_.FullName 'options\\recentProjects.xml'
                            $historyDirs += Join-Path $localApp ("JetBrains\\{0}\\LocalHistory" -f $_.Name)
                        }
                    }
                    foreach ($target in $targets) { Erase-OneFile $target }
                    foreach ($historyDir in $historyDirs) { Erase-Dir $historyDir }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $leftover += @($historyDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'gitActivity' {
                    $gitConfig = Join-Path $UserProfilePath '.gitconfig'
                    $currentProfile = [Environment]::GetFolderPath('UserProfile')
                    $isCurrentProfile = $currentProfile -and ($UserProfilePath.TrimEnd('\\') -eq $currentProfile.TrimEnd('\\'))
                    if ($isCurrentProfile) {
                        foreach ($line in @(& cmdkey.exe /list 2>$null)) {
                            $text = ([string]$line).Trim()
                            if ($text -match '(?i)target=(.+)$' -and $Matches[1].Trim() -match '(?i)git:') {
                                & cmdkey.exe ("/delete:{0}" -f $Matches[1].Trim()) 2>$null | Out-Null
                            }
                        }
                    }
                    if (Test-Path $gitConfig -PathType Leaf -ErrorAction SilentlyContinue) { & git.exe config --file $gitConfig --unset-all safe.directory 2>$null | Out-Null }
                    $leftover = if (Test-Path $gitConfig -PathType Leaf -ErrorAction SilentlyContinue) {
                        @(& git.exe config --file $gitConfig --get-all safe.directory 2>$null).Count
                    } else { 0 }
                    if ($isCurrentProfile) {
                        $leftover += @(& cmdkey.exe /list 2>$null | Where-Object { ([string]$_).Trim() -match '(?i)target=.*git:' }).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'sshState' {
                    $knownHosts = Join-Path $UserProfilePath '.ssh\\known_hosts'
                    Erase-OneFile $knownHosts
                    $currentProfile = [Environment]::GetFolderPath('UserProfile')
                    if ($currentProfile -and ($UserProfilePath.TrimEnd('\\') -eq $currentProfile.TrimEnd('\\'))) {
                        $sshAdd = Get-Command ssh-add.exe -ErrorAction SilentlyContinue
                        if ($sshAdd) { & $sshAdd.Source -D 2>$null | Out-Null }
                    }
                    $catResults[$cat] = if (Test-Path $knownHosts -PathType Leaf -ErrorAction SilentlyContinue) { 'failed' } else { 'cleaned' }
                }
                'remoteAccessLogs' {
                    $targets = @(Get-CleanupTargetFiles @(
                        @{ path = (Join-Path $appData 'TeamViewer'); patterns = @('Connections*.txt','*.trace','*.log') },
                        @{ path = (Join-Path $appData 'AnyDesk'); patterns = @('Connections*.txt','*.trace','*.log') },
                        @{ path = (Join-Path $env:ProgramData 'AnyDesk'); patterns = @('Connections*.txt','*.trace','*.log') },
                        @{ path = (Join-Path $appData 'RealVNC'); patterns = @('Connections*.txt','*.trace','*.log') },
                        @{ path = (Join-Path $localApp 'Google\\Chrome Remote Desktop'); patterns = @('Connections*.txt','*.trace','*.log') }
                    ))
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'passwordManagerCaches' {
                    $cacheDirs = @(
                        (Join-Path $localApp '1Password\\Cache'), (Join-Path $localApp '1Password\\Local Storage\\leveldb'),
                        (Join-Path $appData 'Bitwarden\\Cache'), (Join-Path $appData 'Bitwarden\\Local Storage\\leveldb'), (Join-Path $appData 'Bitwarden\\Session Storage'),
                        (Join-Path $appData 'LastPass\\Cache'), (Join-Path $appData 'LastPass\\Local Storage\\leveldb'), (Join-Path $appData 'LastPass\\Session Storage')
                    )
                    foreach ($cacheDir in $cacheDirs) { Erase-Dir $cacheDir }
                    $leftover = @($cacheDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'gameLauncherLogs' {
                    $steamRoot = ${env:ProgramFiles(x86)}
                    if (-not $steamRoot) { $steamRoot = $env:ProgramFiles }
                    $targets = @(Get-CleanupTargetFiles @(
                        @{ path = (Join-Path $steamRoot 'Steam\\logs'); patterns = @('*') },
                        @{ path = (Join-Path $steamRoot 'Steam\\userdata'); patterns = @('localconfig.vdf') },
                        @{ path = (Join-Path $localApp 'EpicGamesLauncher\\Saved\\Logs'); patterns = @('*') },
                        @{ path = (Join-Path $appData 'Battle.net\\Logs'); patterns = @('*') },
                        @{ path = (Join-Path $env:ProgramData 'Battle.net\\Logs'); patterns = @('*') }
                    ))
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'adobeRecent' {
                    $readerRoot = "$HiveRoot\Software\Adobe\Acrobat Reader"
                    $targets = @()
                    if (Test-Path $readerRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $readerRoot -Force -ErrorAction SilentlyContinue | ForEach-Object {
                            foreach ($suffix in @('AVGeneral\\cRecentFiles','AVGeneral\\cDigitalSignatures')) {
                                $path = Join-Path $_.PSPath $suffix
                                if (Test-Path $path -ErrorAction SilentlyContinue) { $targets += $path }
                            }
                        }
                    }
                    foreach ($target in $targets) { Remove-ItemSecure -Path $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'officeTempFiles' {
                    $roots = @((Join-Path $docs ''), (Join-Path $UserProfilePath 'Desktop'), (Join-Path $UserProfilePath 'Downloads'))
                    $targets = @()
                    foreach ($root in $roots) {
                        if (Test-Path $root -ErrorAction SilentlyContinue) {
                            $targets += @(Get-ChildItem -Path $root -Recurse -Depth 2 -File -Include '~$*.doc*','~$*.xls*','~$*.ppt*' -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
                        }
                    }
                    $tempRoot = Join-Path $localApp 'Temp'
                    if (Test-Path $tempRoot -ErrorAction SilentlyContinue) {
                        $targets += @(Get-ChildItem -Path $tempRoot -Recurse -Depth 2 -File -Filter '*.tmp' -Force -ErrorAction SilentlyContinue |
                            Sort-Object LastWriteTime -Descending | Select-Object -First 500 | ForEach-Object { $_.FullName })
                    }
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'vpnPhonebooks' {
                    $targets = @(Get-CleanupTargetFiles @(
                        @{ path = (Join-Path $appData 'Microsoft\\Network\\Connections\\Pbk'); patterns = @('rasphone.pbk','*.log','*.etl') },
                        @{ path = (Join-Path $localApp 'Microsoft\\Network\\Connections'); patterns = @('rasphone.pbk','*.log','*.etl') },
                        @{ path = (Join-Path $env:ProgramData 'Microsoft\\Network\\Connections\\Pbk'); patterns = @('rasphone.pbk','*.log','*.etl') }
                    ))
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'proxyCache' {
                    $connections = "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Internet Settings\Connections"
                    if (Test-Path $connections -ErrorAction SilentlyContinue) { Remove-ItemSecure -Path $connections }
                    $catResults[$cat] = if (Test-Path $connections -ErrorAction SilentlyContinue) { 'failed' } else { 'cleaned' }
                }
                'cloudPlaceholders' {
                    $targets = @(Get-WCCloudPlaceholderFiles -UserProfilePath $UserProfilePath | ForEach-Object { $_.item.FullName })
                    foreach ($target in $targets) { Remove-ItemSecure -Path $target }
                    $leftover = @((Get-WCCloudPlaceholderFiles -UserProfilePath $UserProfilePath) | ForEach-Object { $_.item.FullName }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                # System-wide categories are intentionally omitted — they are
                # handled by the existing single-user Clear-* functions and do
                # not need per-user iteration (event logs, prefetch, USB, etc.)
            }
        } catch {
            # Record the failure per-category instead of silently swallowing it —
            # one category's exception must not be reported as 'cleaned', and
            # must not abort the rest of the requested categories.
            $catResults[$cat] = 'failed'
        }
    }

    return $catResults
}

# Orchestrate a cleanup clear across all or selected user accounts.
# $CategoryIds: which categories to clean (scopeAware ones; systemWide
#               categories are silently skipped — they affect all users
#               already and should be cleaned via their own Clear-* function)
# $TargetUsers: usernames to target; empty = all non-system accounts
#
# Per-user logic:
#   current user (logged in) → Invoke-CleanupClearForUser with HKCU:
#   other user logged in     → file-system paths only (skip live HKCU)
#   other user offline       → load hive → clean → unload
function Invoke-CleanupClearAllUsers {
    [CmdletBinding()]
    param(
        # Accept either a real array or a comma-separated string — the backend
        # router passes all params as strings so we split here.
        [Parameter(Mandatory)] [string]$CategoryIds,
        [string]$TargetUsers = ''
    )

    $categoryArr = $CategoryIds -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' }
    $targetArr   = if ($TargetUsers -and $TargetUsers.Trim() -ne '') {
        $TargetUsers -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' }
    } else { @() }

    # Filter to only scope-aware categories
    $scopeAwareIds = @(
        'rdpHistory','recentFiles','jumpLists','psHistory','browserFootprints',
        'shellBags','execCache','netDrives','ntUserTraces','notepadState',
        'walFiles','crashDumps','recallDb',
        'webCache','thumbnailDb','notificationDb','activitiesTimeline','rdpBitmapCache',
        'wslData','dockerDesktopData','virtualMachineArtifacts','developerCaches',
        'credentialManager','networkWizardHistory',
        'stickyNotes','oneDriveMetadata','spotlightCache','legacyIconCache',
        'gameCaptures','photosCache','xboxCache',
        'communicationCaches','editorHistory','gitActivity','sshState','remoteAccessLogs',
        'passwordManagerCaches','gameLauncherLogs','adobeRecent','officeTempFiles',
        'vpnPhonebooks','proxyCache','cloudPlaceholders'
    )
    $effectiveCats = $categoryArr | Where-Object { $scopeAwareIds -contains $_ }
    if ($effectiveCats.Count -eq 0) {
        return @{ status = 'ok'; message = 'No scope-aware categories requested'; results = @() }
    }

    # Current account is identified by SID (stable across renames), not by
    # $env:USERNAME — otherwise the logged-in profile would be mistaken for an
    # offline account and we'd try to `reg load` a hive that's already in use.
    $mySid = ''
    try { $mySid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value } catch {}

    # Enumerate target profiles — S-1-5-21-* SIDs only (real user accounts)
    $sysDrive    = $env:SystemDrive
    $allProfiles = Get-CimInstance -ClassName Win32_UserProfile -ErrorAction SilentlyContinue |
        Where-Object { -not $_.Special -and $_.SID -match '^S-1-5-21-' -and $_.LocalPath -like "$sysDrive\Users\*" } |
        ForEach-Object {
            $leaf = Split-Path $_.LocalPath -Leaf
            $display = $leaf
            try { $display = (([Security.Principal.SecurityIdentifier]$_.SID).Translate([Security.Principal.NTAccount]).Value -split '\\')[-1] } catch {}
            if (Test-WCVisibleUserProfile -Name $leaf -DisplayName $display -Path $_.LocalPath) {
                [pscustomobject]@{ Name = $leaf; FullName = $_.LocalPath; SID = $_.SID }
            }
        }

    # Privilege gate — clearing another user's traces requires loading their
    # offline NTUSER.DAT hive and writing under their profile, both of which
    # need Administrator. A non-admin caller is constrained (by SID) to their
    # OWN account so the operation degrades safely instead of erroring or
    # touching accounts it has no right to.
    $isAdmin = Test-IsAdmin
    $profiles = if (-not $isAdmin) {
        $allProfiles | Where-Object { $_.SID -eq $mySid }
    } elseif ($targetArr.Count -gt 0) {
        $allProfiles | Where-Object { $targetArr -contains $_.Name }
    } else { $allProfiles }

    $loggedIn = (Get-LoggedInUsers).users
    $results  = @()

    foreach ($prof in $profiles) {
        $username      = $prof.Name
        $isCurrentUser = ($prof.SID -eq $mySid)
        $isLoggedIn    = ($loggedIn -contains $username)

        if ($isCurrentUser) {
            $catResults = Invoke-CleanupClearForUser -UserProfilePath $prof.FullName `
                -CategoryIds $effectiveCats -HiveRoot 'HKCU:'
            $failedCats = @($catResults.Keys | Where-Object { $catResults[$_] -ne 'cleaned' })
            $results += if ($failedCats.Count -eq 0) {
                @{ user = $username; status = 'cleaned' }
            } else {
                @{
                    user        = $username
                    status      = 'partial'
                    note        = 'Some categories could not be verified as removed'
                    failedCats  = $failedCats
                    cleanedCats = @($catResults.Keys | Where-Object { $catResults[$_] -eq 'cleaned' })
                }
            }

        } elseif ($isLoggedIn) {
            # File paths accessible to admin even for a live session
            $fileOnlyCats = $effectiveCats | Where-Object {
                $_ -in @('recentFiles','jumpLists','psHistory','browserFootprints',
                         'notepadState','walFiles','crashDumps','recallDb','credentialManager',
                         'wslData','dockerDesktopData','virtualMachineArtifacts','developerCaches',
                         'stickyNotes','oneDriveMetadata','spotlightCache','legacyIconCache',
                         'gameCaptures','photosCache','xboxCache','communicationCaches','editorHistory',
                         'gitActivity','sshState','remoteAccessLogs','passwordManagerCaches',
                         'gameLauncherLogs','officeTempFiles','vpnPhonebooks','cloudPlaceholders')
            }
            $registryCats = $effectiveCats | Where-Object {
                $_ -in @('rdpHistory','shellBags','execCache','netDrives','ntUserTraces','networkWizardHistory','adobeRecent','proxyCache')
            }
            $catResults = if ($fileOnlyCats.Count -gt 0) {
                Invoke-CleanupClearForUser -UserProfilePath $prof.FullName `
                    -CategoryIds $fileOnlyCats -HiveRoot 'HKCU:'
            } else { @{} }
            $failedCats = @($catResults.Keys | Where-Object { $catResults[$_] -ne 'cleaned' })
            $results += @{
                user        = $username
                status      = 'partial'
                note        = 'User is logged in - registry categories skipped (live hive)'
                skippedCats = @($registryCats)
                cleanedCats = @($catResults.Keys | Where-Object { $catResults[$_] -eq 'cleaned' })
                failedCats  = $failedCats
            }

        } else {
            # Offline user — load hive, clean, unload
            $capturedProf = $prof
            $capturedCats = $effectiveCats
            $catResults = Invoke-WithUserHive -UserProfilePath $prof.FullName -ScriptBlock {
                param([string]$HiveRoot)
                Invoke-CleanupClearForUser -UserProfilePath $capturedProf.FullName `
                    -CategoryIds $capturedCats -HiveRoot $HiveRoot
            }
            if ($null -eq $catResults) {
                # Invoke-WithUserHive returned early (NTUSER.DAT missing or reg
                # load failed) — nothing was verified, so this is a failure,
                # not a silent 'cleaned'.
                $results += @{
                    user       = $username
                    status     = 'partial'
                    note       = 'Could not load offline hive - categories not verified'
                    failedCats = @($capturedCats)
                }
            } else {
                $failedCats = @($catResults.Keys | Where-Object { $catResults[$_] -ne 'cleaned' })
                $results += if ($failedCats.Count -eq 0) {
                    @{ user = $username; status = 'cleaned' }
                } else {
                    @{
                        user        = $username
                        status      = 'partial'
                        note        = 'Some categories could not be verified as removed'
                        failedCats  = $failedCats
                        cleanedCats = @($catResults.Keys | Where-Object { $catResults[$_] -eq 'cleaned' })
                    }
                }
            }
        }
    }

    @{
        status  = 'ok'
        cleaned = @($results | Where-Object { $_.status -eq 'cleaned' }).Count
        partial = @($results | Where-Object { $_.status -eq 'partial' }).Count
        results = $results
    }
}

# ============================================================================
# PER-USER CLEANUP VIEWER
# ============================================================================
# Get-CleanupSummaryAllUsers: scan every scope-aware category for every
# real user account and return counts + preview items — same shape as the
# per-category Get-* commands the UI already uses, but grouped by user.
#
# File-based categories (recentFiles, psHistory, browserFootprints,
# notepadState, walFiles, crashDumps, recallDb) are read directly from
# the profile path — no special privileges needed beyond admin.
#
# Registry-based categories (rdpHistory, shellBags, execCache, netDrives,
# ntUserTraces) use the live HKU\<SID> hive for logged-in users, and
# Invoke-WithUserHive for offline users (requires admin).
# ============================================================================

function Get-CleanupSummaryAllUsers {
    [CmdletBinding()]
    param(
        # Comma-separated list of category IDs to scan; empty = all scope-aware
        [string]$CategoryIds = '',
        # Comma-separated list of usernames to scan; empty = every account.
        # The UI scans one user at a time (the user switcher), so it normally
        # passes a single name here to avoid the cost of loading every hive.
        [string]$TargetUsers = ''
    )

    $allCats = @('rdpHistory','recentFiles','jumpLists','psHistory','browserFootprints',
                 'shellBags','execCache','ntUserTraces','notepadState','walFiles',
                 'crashDumps','recallDb','netDrives','webCache','thumbnailDb','notificationDb',
                 'activitiesTimeline','rdpBitmapCache')
    $requestedCats = if ($CategoryIds -and $CategoryIds.Trim() -ne '') {
        $CategoryIds -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' -and $_ -in $allCats }
    } else { $allCats }

    $targetArr = if ($TargetUsers -and $TargetUsers.Trim() -ne '') {
        $TargetUsers -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' }
    } else { @() }

    # Current account identified by SID (stable across renames), not name.
    $mySid = ''
    try { $mySid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value } catch {}

    $sysDrive    = $env:SystemDrive
    $allProfiles = Get-CimInstance -ClassName Win32_UserProfile -ErrorAction SilentlyContinue |
        Where-Object { -not $_.Special -and $_.SID -match '^S-1-5-21-' -and $_.LocalPath -like "$sysDrive\Users\*" } |
        ForEach-Object {
            $leaf = Split-Path $_.LocalPath -Leaf
            $display = $leaf
            try { $display = (([Security.Principal.SecurityIdentifier]$_.SID).Translate([Security.Principal.NTAccount]).Value -split '\\')[-1] } catch {}
            if (Test-WCVisibleUserProfile -Name $leaf -DisplayName $display -Path $_.LocalPath) {
                [pscustomobject]@{ Name = $leaf; FullName = $_.LocalPath; SID = $_.SID }
            }
        }

    # Privilege gate — reading another user's offline hive / profile requires
    # Administrator. A non-admin caller can only ever see their own account
    # (matched by SID), regardless of what was requested.
    $isAdmin = Test-IsAdmin
    if (-not $isAdmin) {
        $allProfiles = $allProfiles | Where-Object { $_.SID -eq $mySid }
    } elseif ($targetArr.Count -gt 0) {
        $allProfiles = $allProfiles | Where-Object { $targetArr -contains $_.Name }
    }

    $loggedIn = (Get-LoggedInUsers).users
    $userResults = @()

    foreach ($prof in $allProfiles) {
        $username      = $prof.Name
        $isCurrentUser = ($prof.SID -eq $mySid)
        $isLoggedIn    = ($loggedIn -contains $username)
        $appData       = Join-Path $prof.FullName 'AppData\Roaming'
        $localApp      = Join-Path $prof.FullName 'AppData\Local'

        # Determine registry hive root
        $hiveRoot      = 'HKCU:'   # default — overridden for non-current users
        $hiveLoaded    = $false
        $hiveAvailable = $true     # false when reg load failed or hive unreachable
        $hiveError     = $null     # surfaced to UI as a warning
        $mountName     = "WC_TempHive_$username"
        $mountPath     = "HKEY_USERS\$mountName"

        if (-not $isCurrentUser) {
            # Strategy: try the live SID path in HKEY_USERS FIRST — this works for
            # any user who is logged in (interactive, RDP, disconnected session, etc.)
            # without needing reg load. Only fall back to reg load for truly offline users.
            # This avoids the "NTUSER.DAT locked" error for logged-in users that
            # Get-LoggedInUsers failed to detect.
            $sidRoot = "Registry::HKEY_USERS\$($prof.SID)"
            if (Test-Path $sidRoot) {
                $hiveRoot = $sidRoot
            } else {
                # SID not in HKEY_USERS — user is genuinely offline. Load the hive.
                $ntuser = Join-Path $prof.FullName 'NTUSER.DAT'
                if (Test-Path $ntuser) {
                    $already = Test-Path "Registry::$mountPath"
                    if (-not $already) {
                        $loadOut = & reg load $mountPath $ntuser 2>&1
                        $loadOk  = ($LASTEXITCODE -eq 0) -and (Test-Path "Registry::$mountPath")
                    } else {
                        $loadOk = $true
                    }
                    if ($loadOk) {
                        $hiveRoot   = "Registry::$mountPath"
                        $hiveLoaded = (-not $already)
                    } else {
                        # Truly inaccessible (locked by a session we cannot see).
                        # File-system categories still run; registry ones show 0.
                        $hiveAvailable = $false
                    }
                } else {
                    $hiveAvailable = $false
                }
            }
        }

        # The card grid only shows the first few items, but detail dialogs need
        # enough rows to audit common high-count traces without hiding records.
        $previewMax = 500
        $categories = @()
        foreach ($cat in $requestedCats) {
            $count = 0; $items = @()
            try {
                # For the current user delegate to the same per-category functions the
                # main cleanup panel uses — counts will always be identical.
                # For other users fall through to the file-system + offline-hive path.
                $delegated = $false
                if ($isCurrentUser) {
                    $delegated = $true
                    switch ($cat) {
                        'recentFiles' {
                            $r = Get-RecentFiles
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'psHistory' {
                            $r = Get-PSHistory
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.command }
                        }
                        'jumpLists' {
                            $r = Get-JumpLists
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'browserFootprints' {
                            $r = Get-BrowserFootprints
                            $count = if ($r.totalBrowsers) { [int]$r.totalBrowsers } else { 0 }
                            $items = @($r.browsers) | Select-Object -First $previewMax | ForEach-Object { "$($_.browser): $($_.totalSizeKB) KB" }
                        }
                        'shellBags' {
                            $r = Get-ShellBags
                            $count = @($r.entries).Count
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.path }
                        }
                        'execCache' {
                            $r = Get-ExecutionCache
                            $count = @($r.entries).Count
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.path }
                        }
                        'ntUserTraces' {
                            $r = Get-NTUserTraces
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.sections) | ForEach-Object { "$($_.name): $($_.count)" } | Select-Object -First $previewMax
                        }
                        'rdpHistory' {
                            $r = Get-RDPHistory
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.host }
                        }
                        'notepadState' {
                            $r = Get-NotepadStateFiles
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'crashDumps' {
                            $r = Get-CrashDumpList
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.dumps) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'walFiles' {
                            $r = Get-SQLiteWALList
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.databases) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'recallDb' {
                            $r = Get-RecallDatabaseInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'webCache' {
                            $r = Get-WebCacheInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'thumbnailDb' {
                            $r = Get-ThumbnailCacheInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'notificationDb' {
                            $r = Get-NotificationDatabaseInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'activitiesTimeline' {
                            $r = Get-ActivitiesTimelineInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'rdpBitmapCache' {
                            $r = Get-RdpBitmapCacheInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        default { $delegated = $false }
                    }
                }
                if (-not $delegated) {
                # Other-users path (file-system + offline-hive scanning)
                switch ($cat) {
                    'recentFiles' {
                        $p = Join-Path $appData 'Microsoft\Windows\Recent'
                        if (Test-Path $p) {
                            $files = Get-ChildItem $p -File -Force -EA SilentlyContinue
                            $count = $files.Count
                            $items = $files | Select-Object -First $previewMax | ForEach-Object { $_.BaseName }
                        }
                    }
                    'psHistory' {
                        $p = Join-Path $appData 'Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt'
                        if (Test-Path $p) {
                            # Match Get-PSHistory exactly: filter empty lines, return last 300.
                            $lines = [System.IO.File]::ReadAllLines($p)
                            $nonEmpty = @($lines | Where-Object { $_.Trim() -ne '' })
                            $count = $nonEmpty.Count
                            $start = [Math]::Max(0, $nonEmpty.Count - $previewMax)
                            $items = if ($nonEmpty.Count -gt 0) {
                                $nonEmpty[$start..($nonEmpty.Count - 1)] |
                                    ForEach-Object { $_.Substring(0, [Math]::Min($_.Length, 80)) }
                            } else { @() }
                        }
                    }
                    'jumpLists' {
                        $auto = Join-Path $appData 'Microsoft\Windows\Recent\AutomaticDestinations'
                        $cust = Join-Path $appData 'Microsoft\Windows\Recent\CustomDestinations'
                        $f1 = if (Test-Path $auto) { @(Get-ChildItem $auto -File -Force -EA SilentlyContinue) } else { @() }
                        $f2 = if (Test-Path $cust) { @(Get-ChildItem $cust -File -Force -EA SilentlyContinue) } else { @() }
                        $count = $f1.Count + $f2.Count
                        $items = @($f1 + $f2) | Select-Object -First $previewMax | ForEach-Object { $_.BaseName }
                    }
                    'browserFootprints' {
                        # Count actual artifact files per browser — not just "browser installed".
                        # Matches Get-BrowserFootprints per-category function behaviour.
                        $artifactNames = @('History','Cookies','Login Data','Top Sites','Favicons',
                                           'Web Data','Visited Links','Shortcuts','Download Metadata')
                        $chromiumRoots = @(
                            @{n='Chrome'; p=Join-Path $localApp 'Google\Chrome\User Data'},
                            @{n='Edge';   p=Join-Path $localApp 'Microsoft\Edge\User Data'},
                            @{n='Brave';  p=Join-Path $localApp 'BraveSoftware\Brave-Browser\User Data'},
                            @{n='Opera';  p=Join-Path $appData  'Opera Software\Opera Stable'},
                            @{n='Vivaldi';p=Join-Path $localApp 'Vivaldi\User Data'}
                        )
                        foreach ($r in $chromiumRoots) {
                            if (-not (Test-Path $r.p)) { continue }
                            # Walk all profile directories (Default + Profile 1, 2, …)
                            $profileDirs = @(Get-ChildItem $r.p -Directory -Force -EA SilentlyContinue |
                                Where-Object { $_.Name -eq 'Default' -or $_.Name -match '^Profile \d+$' })
                            foreach ($pd in $profileDirs) {
                                foreach ($a in $artifactNames) {
                                    $f = Join-Path $pd.FullName $a
                                    if (Test-Path $f -PathType Leaf) {
                                        $count++
                                        $items += "$($r.n): $a"
                                    }
                                }
                            }
                        }
                        # Firefox — count places.sqlite (history+bookmarks) and cookies.sqlite per profile
                        $ffRoot = Join-Path $appData 'Mozilla\Firefox\Profiles'
                        if (Test-Path $ffRoot) {
                            Get-ChildItem $ffRoot -Directory -Force -EA SilentlyContinue | ForEach-Object {
                                foreach ($db in @('places.sqlite','cookies.sqlite','formhistory.sqlite','logins.json')) {
                                    if (Test-Path (Join-Path $_.FullName $db)) {
                                        $count++
                                        $items += "Firefox: $db"
                                    }
                                }
                            }
                        }
                    }
                    'notepadState' {
                        $pkg = Get-ChildItem (Join-Path $localApp 'Packages') -Filter 'Microsoft.WindowsNotepad_*' -Directory -Force -EA SilentlyContinue | Select-Object -First 1
                        if ($pkg) {
                            $tab = Join-Path $pkg.FullName 'LocalState\TabState'
                            if (Test-Path $tab) {
                                $files = Get-ChildItem $tab -File -Force -EA SilentlyContinue
                                $count = $files.Count
                                $items = $files | Select-Object -First $previewMax | ForEach-Object { $_.Name }
                            }
                        }
                    }
                    'crashDumps' {
                        # Match Get-CrashDumpList: CrashDumps + Minidump + WER ReportArchive + ReportQueue
                        $dirs = @(
                            Join-Path $localApp 'CrashDumps',
                            "$env:SystemRoot\Minidump",
                            Join-Path $localApp 'Microsoft\Windows\WER\ReportArchive',
                            Join-Path $localApp 'Microsoft\Windows\WER\ReportQueue'
                        )
                        foreach ($d in $dirs) {
                            if (Test-Path $d) {
                                $f = Get-ChildItem $d -Recurse -File -Force -EA SilentlyContinue
                                $count += $f.Count
                                $items += $f | Select-Object -First $previewMax | ForEach-Object { $_.Name }
                            }
                        }
                    }
                    'walFiles' {
                        # Match per-category function: *.wal, *.db-wal, *.sqlite-wal, *.shm, *.db-shm
                        # Depth 5 balances coverage vs scan time (browsers live ≤4 deep in AppData).
                        $f = Get-ChildItem $appData,$localApp `
                            -Include '*.wal','*.db-wal','*.sqlite-wal','*.shm','*.db-shm' `
                            -Recurse -Depth 5 -File -Force -EA SilentlyContinue
                        $count = @($f).Count
                        $items = @($f) | Select-Object -First $previewMax | ForEach-Object { $_.Name }
                    }
                    'recallDb' {
                        # Match Get-RecallDatabaseInfo: Recall + alternate + Timeline + ActionCenter
                        $dirs = @(
                            Join-Path $localApp 'CoreAIPlatform.00\UKP',
                            Join-Path $localApp 'CoreAIPlatform.00',
                            Join-Path $localApp 'ConnectedDevicesPlatform',
                            Join-Path $localApp 'Microsoft\Windows\ActionCenterCache'
                        )
                        foreach ($d in $dirs) {
                            if (Test-Path $d) {
                                $f = Get-ChildItem $d -File -Force -EA SilentlyContinue |
                                    Where-Object { $_.Extension -in @('.db','.db-wal','.db-shm','.db-journal','.edb') }
                                $count += $f.Count
                                $items += $f | Select-Object -First $previewMax | ForEach-Object { $_.Name }
                            }
                        }
                    }
                    'webCache' {
                        # Match Get-WebCacheInfo: WebCacheV*.dat (V01 historically, V24 on newer builds) + rotating V01*.log
                        $p = Join-Path $localApp 'Microsoft\Windows\WebCache'
                        if (Test-Path $p -ErrorAction SilentlyContinue) {
                            $f  = @(Get-ChildItem -Path (Join-Path $p 'WebCacheV*.dat') -Force -EA SilentlyContinue)
                            $f += @(Get-ChildItem -Path (Join-Path $p 'V01*.log') -Force -EA SilentlyContinue)
                            $count = $f.Count
                            $items = $f | Select-Object -First $previewMax | ForEach-Object { $_.Name }
                        }
                    }
                    'thumbnailDb' {
                        # Match Get-ThumbnailCacheInfo: thumbcache_*.db + iconcache_*.db
                        $p = Join-Path $localApp 'Microsoft\Windows\Explorer'
                        if (Test-Path $p) {
                            $f  = @(Get-ChildItem -Path (Join-Path $p 'thumbcache_*.db') -Force -EA SilentlyContinue)
                            $f += @(Get-ChildItem -Path (Join-Path $p 'iconcache_*.db') -Force -EA SilentlyContinue)
                            $count = $f.Count
                            $items = $f | Select-Object -First $previewMax | ForEach-Object { $_.Name }
                        }
                    }
                    'notificationDb' {
                        # Match Get-NotificationDatabaseInfo: wpndatabase.db + -wal + -shm
                        $p = Join-Path $localApp 'Microsoft\Windows\Notifications'
                        foreach ($name in @('wpndatabase.db', 'wpndatabase.db-wal', 'wpndatabase.db-shm')) {
                            $f = Join-Path $p $name
                            if (Test-Path $f) {
                                $count++
                                $items += $name
                            }
                        }
                    }
                    'activitiesTimeline' {
                        # Match Get-ActivitiesTimelineInfo: ActivitiesCache.db + -wal/-shm sidecars
                        $p = Join-Path $localApp 'ConnectedDevicesPlatform'
                        if (Test-Path $p) {
                            $f = Get-ChildItem -Path $p -Include 'ActivitiesCache.db','*.db-wal','*.db-shm' -Recurse -Force -EA SilentlyContinue
                            $count = @($f).Count
                            $items = @($f) | Select-Object -First $previewMax | ForEach-Object { $_.Name }
                        }
                    }
                    'rdpBitmapCache' {
                        # Match Get-RdpBitmapCacheInfo: *.bin + *.bmc
                        $p = Join-Path $localApp 'Microsoft\Terminal Server Client\Cache'
                        if (Test-Path $p) {
                            $f  = @(Get-ChildItem -Path (Join-Path $p '*.bin') -Force -EA SilentlyContinue)
                            $f += @(Get-ChildItem -Path (Join-Path $p '*.bmc') -Force -EA SilentlyContinue)
                            $count = $f.Count
                            $items = $f | Select-Object -First $previewMax | ForEach-Object { $_.Name }
                        }
                    }
                    'rdpHistory' {
                        if ($hiveAvailable) {
                            # Match Get-RDPHistory: Default MRU + per-server UsernameHint entries
                            $defaultK = "$hiveRoot\Software\Microsoft\Terminal Server Client\Default"
                            if (Test-Path $defaultK) {
                                $props = (Get-Item $defaultK -EA SilentlyContinue).Property |
                                    Where-Object { $_ -like 'MR*' }
                                $count += @($props).Count
                                $items += @($props) | Select-Object -First $previewMax |
                                    ForEach-Object { (Get-ItemPropertyValue $defaultK $_ -EA SilentlyContinue) }
                            }
                            $serversK = "$hiveRoot\Software\Microsoft\Terminal Server Client\Servers"
                            if (Test-Path $serversK) {
                                $servers = Get-ChildItem $serversK -EA SilentlyContinue
                                $count  += @($servers).Count
                                $items  += @($servers) | Select-Object -First $previewMax |
                                    ForEach-Object { $_.PSChildName }
                            }
                            # Default.rdp file (profile-relative)
                            $defaultRdp = Join-Path (Split-Path $appData -Parent | Split-Path -Parent) 'Documents\Default.rdp'
                            if (Test-Path $defaultRdp) { $count++ ; $items += 'Default.rdp' }
                        } else { $items = @("[registry unavailable]") }
                    }
                    'shellBags' {
                        if ($hiveAvailable) {
                            # BagMRU stores binary PIDL data — actual folder paths cannot
                            # be decoded from an offline hive without shell COM APIs.
                            # Count the subkeys (each = one folder view slot) and surface
                            # the NodeSlot depth as a minimal indicator of activity.
                            $totalBags = 0
                            foreach ($bagRoot in @(
                                "$hiveRoot\Software\Microsoft\Windows\Shell\BagMRU",
                                "$hiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU"
                            )) {
                                if (Test-Path $bagRoot) {
                                    $totalBags += @(Get-ChildItem $bagRoot -Recurse -EA SilentlyContinue).Count
                                }
                            }
                            $count = $totalBags
                            if ($totalBags -gt 0) { $items = @("$totalBags folder view slots recorded") }
                        } else { $items = @("[registry unavailable]") }
                    }
                    'execCache' {
                        if ($hiveAvailable) {
                            # Mirror Get-ExecutionCache exactly: Store+Persisted (HKLM, all users) +
                            # UserAssist ROT13 decoded paths + MuiCache exe keys + RecentApps.
                            # Deduplicate across all sources the same way.
                            $seen = @{}

                            # HKLM AppCompat Store + Persisted (same for every user, no hive needed)
                            foreach ($hklmP in @(
                                'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store',
                                'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Persisted'
                            )) {
                                if (Test-Path $hklmP) {
                                    $p = Get-ItemProperty $hklmP -EA SilentlyContinue
                                    if ($p) {
                                        $p.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
                                            if ($_.Name -and -not $seen.ContainsKey($_.Name)) { $seen[$_.Name] = $true; $items += $_.Name }
                                        }
                                    }
                                }
                            }

                            # UserAssist — ROT13 decode property names under each GUID\Count subkey
                            $uaK = "$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist"
                            if (Test-Path $uaK) {
                                Get-ChildItem $uaK -EA SilentlyContinue | ForEach-Object {
                                    $cK = "$($_.PSPath)\Count"
                                    if (Test-Path $cK) {
                                        $p = Get-ItemProperty $cK -EA SilentlyContinue | Select-Object -Property *
                                        if ($p) {
                                            $p.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
                                                $d = ($_.Name.ToCharArray() | ForEach-Object {
                                                    $c = [int][char]$_
                                                    if ($c -ge 65 -and $c -le 90) { [char](((($c-65)+13)%26)+65) }
                                                    elseif ($c -ge 97 -and $c -le 122) { [char](((($c-97)+13)%26)+97) }
                                                    else { [char]$c }
                                                }) -join ''
                                                $d = $d -replace '^\\\?\\', ''
                                                if ($d -and -not $seen.ContainsKey($d)) { $seen[$d] = $true; $items += $d }
                                            }
                                        }
                                    }
                                }
                            }

                            # MuiCache — property NAME is the exe path (same logic as Get-ExecutionCache)
                            $muiK = "$hiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"
                            if (Test-Path $muiK) {
                                (Get-Item $muiK -EA SilentlyContinue).Property |
                                    Where-Object { $_ -notmatch '^PS' -and $_ -ne 'LangID' } |
                                    ForEach-Object { if (-not $seen.ContainsKey($_)) { $seen[$_] = $true; $items += $_ } }
                            }

                            # RecentApps — AppId property under each subkey
                            $raK = "$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Search\RecentApps"
                            if (Test-Path $raK) {
                                Get-ChildItem $raK -EA SilentlyContinue | ForEach-Object {
                                    $p2 = Get-ItemProperty $_.PSPath -EA SilentlyContinue
                                    if ($p2 -and $p2.AppId -and -not $seen.ContainsKey($p2.AppId)) {
                                        $seen[$p2.AppId] = $true
                                        $items += $p2.AppId
                                    }
                                }
                            }

                            $count = $seen.Count
                        } else { $items = @("[registry unavailable]") }
                    }
                    'ntUserTraces' {
                        if ($hiveAvailable) {
                            # Match Get-NTUserTraces: RunMRU + TypedPaths + TypedURLs + WordWheelQuery + ComDlg32
                            $traceSuffixes = @(
                                'RunMRU',
                                'TypedPaths',
                                'TypedURLs',
                                'WordWheelQuery',
                                'ComDlg32\OpenSavePidlMRU',
                                'ComDlg32\LastVisitedPidlMRU'
                            )
                            foreach ($kSuffix in $traceSuffixes) {
                                $k = "$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\$kSuffix"
                                if (Test-Path $k) {
                                    $props = (Get-Item $k -EA SilentlyContinue).Property |
                                        Where-Object { $_ -ne '(default)' -and $_ -notmatch '^MRU' }
                                    $count += @($props).Count
                                    foreach ($prop in @($props) | Select-Object -First $previewMax) {
                                        $raw = Get-ItemPropertyValue $k $prop -EA SilentlyContinue
                                        # WordWheelQuery (and some ComDlg32 entries) store
                                        # search terms as REG_BINARY null-terminated Unicode.
                                        if ($raw -is [byte[]]) {
                                            try { $raw = [System.Text.Encoding]::Unicode.GetString($raw).TrimEnd([char]0) } catch { $raw = $null }
                                        }
                                        if ($raw) { $items += "$kSuffix\$prop`: $raw" }
                                    }
                                }
                            }
                        } else { $items = @("[registry unavailable]") }
                    }
                    'netDrives' {
                        if ($hiveAvailable) {
                            $k = "$hiveRoot\Network"
                            if (Test-Path $k) {
                                $drives = Get-ChildItem $k -EA SilentlyContinue
                                $count = @($drives).Count
                                $items = @($drives) | Select-Object -First $previewMax | ForEach-Object { $_.PSChildName }
                            }
                        } else { $items = @("[registry unavailable]") }
                    }
                }
                } # end if (-not $delegated)
            } catch {}
            $categories += @{ id = $cat; count = $count; items = @($items | Select-Object -First $previewMax) }
        }

        # Unload hive if we loaded it
        if ($hiveLoaded) {
            [System.GC]::Collect(); [System.GC]::WaitForPendingFinalizers()
            $null = & reg unload $mountPath 2>&1
        }

        $totalCount = ($categories | Measure-Object -Property count -Sum).Sum
        $entry = @{
            username      = $username
            path          = $prof.FullName
            sid           = $prof.SID
            isCurrentUser = $isCurrentUser
            isLoggedIn    = $isLoggedIn
            total         = $totalCount
            categories    = $categories
            hiveAvailable = $hiveAvailable
        }
        if ($hiveError) { $entry.hiveError = $hiveError }
        $userResults += $entry
    }

    @{ status = 'ok'; users = $userResults; userCount = $userResults.Count; isAdmin = [bool]$isAdmin }
}
