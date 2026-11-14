# ============================================================================
# PRIVACY SHIELD - LAUNCHER
# Python-based privacy shield
# ============================================================================


function Resolve-PythonPath {
    # mediapipe requires Python <= 3.12. This function only returns a 3.12.x interpreter.
    # Refresh PATH to catch new installations
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")
    
    # Helper: returns $true only if exe reports Python 3.12.x
    function Test-IsPy312 { param([string]$Exe)
        if (-not (Test-Path $Exe -ErrorAction SilentlyContinue)) { return $false }
        if ($Exe -match 'WindowsApps') { return $false }
        try { $v = & $Exe --version 2>&1 | Out-String; return ($v -match 'Python 3\.12\.') } catch { return $false }
    }

    # 1. Well-known Python 3.12 directories (highest priority)
    $candidates = @(
        "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe",
        "$env:ProgramFiles\Python312\python.exe",
        "$env:ProgramFiles(x86)\Python312\python.exe",
        "C:\Python312\python.exe"
    )
    foreach ($c in $candidates) { if (Test-IsPy312 $c) { return $c } }

    # 2. Registry (HKLM then HKCU, 3.12 only)
    foreach ($root in @("HKLM:\SOFTWARE\Python\PythonCore", "HKCU:\SOFTWARE\Python\PythonCore")) {
        $regPath = "$root\3.12\InstallPath"
        if (Test-Path $regPath) {
            $installDir = (Get-ItemProperty -Path $regPath -ErrorAction SilentlyContinue).'(default)'
            if ($installDir) {
                $exe = Join-Path $installDir 'python.exe'
                if (Test-IsPy312 $exe) { return $exe }
            }
        }
    }

    # 3. Walk PATH - skip anything that isn't 3.12
    foreach ($dir in ($env:Path -split ';')) {
        if ($dir -match 'WindowsApps') { continue }
        $exe = Join-Path $dir 'python.exe'
        if (Test-IsPy312 $exe) { return $exe }
    }

    return $null
}



function Install-AIDependencies {
    param([string]$Target = $null)
    # Delegate to centralized dependencies module
    if (Get-Command "Install-PrivacyShieldAI" -ErrorAction SilentlyContinue) {
        return Install-PrivacyShieldAI -Target $Target
    }
    return @{ error = $true; message = "Dependencies module not loaded." }
}

function Get-PrivacyShieldVideoSourceSummary {
    # Enumerates source metadata only: it does not open a camera or request permission.
    try {
        Add-Type -AssemblyName System.Runtime.WindowsRuntime -ErrorAction Stop
        $null = [Windows.Devices.Enumeration.DeviceInformation,Windows.Devices.Enumeration,ContentType=WindowsRuntime]
        $type = [Windows.Devices.Enumeration.DeviceInformationCollection,Windows.Devices.Enumeration,ContentType=WindowsRuntime]
        $method = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
            $_.Name -eq 'AsTask' -and $_.IsGenericMethodDefinition -and $_.GetParameters().Count -eq 1 -and
            $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
        } | Select-Object -First 1
        $operation = [Windows.Devices.Enumeration.DeviceInformation]::FindAllAsync([Windows.Devices.Enumeration.DeviceClass]::VideoCapture)
        $task = $method.MakeGenericMethod($type).Invoke($null, @($operation))
        if (-not $task.Wait(4000)) { throw 'Camera enumeration timed out' }
        $count = @($task.Result).Count
        $access = 'Unknown'
        try {
            $null = [Windows.Devices.Enumeration.DeviceAccessInformation,Windows.Devices.Enumeration,ContentType=WindowsRuntime]
            $access = [string]([Windows.Devices.Enumeration.DeviceAccessInformation]::CreateFromDeviceClass([Windows.Devices.Enumeration.DeviceClass]::VideoCapture).CurrentStatus)
        } catch {}
        return @{ known = $true; count = $count; access = $access }
    } catch {
        return @{ known = $false; count = 0; access = 'Unknown' }
    }
}

function Get-PrivacyShieldRemoteSession {
    try {
        Add-Type -AssemblyName System.Windows.Forms -ErrorAction Stop
        return [bool][System.Windows.Forms.SystemInformation]::TerminalServerSession
    } catch { return $null }
}

function Get-PrivacyShieldCameraAvailability {
    try {
        $devices = @()
        $uncertain = $false
        $isRemote = Get-PrivacyShieldRemoteSession
        if ($null -eq $isRemote) { $uncertain = $true }
        # ProductType 2/3 means Windows Server. Fleet receives only this
        # coarse capability when Privacy Shield cannot use a camera there.
        $isWindowsServer = $false
        try {
            $os = Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop
            $isWindowsServer = [int]$os.ProductType -ne 1
        } catch {}

        foreach ($className in @("Camera", "Image")) {
            try {
                $found = @(Get-CimInstance -ClassName Win32_PnPEntity `
                    -Filter "PNPClass='$className'" `
                    -ErrorAction Stop)
                foreach ($item in $found) {
                    $cameraLike = $className -eq 'Camera' -or $item.Name -match '(?i)camera|webcam' -or $item.CompatibleID -match '(?i)USB\\Class_0E|VideoCapture'
                    if (-not $cameraLike) { $uncertain = $true; continue }
                    if ($item.Present -ne $true -or $item.Status -ne 'OK' -or $isRemote -ne $false) { $uncertain = $true; continue }
                    # Do not send device names or IDs to the status consumer.
                    $devices += 'Camera source'
                }
            } catch { $uncertain = $true }
        }

        $names = @($devices | Sort-Object -Unique)
        $video = if ($names.Count -eq 0) { Get-PrivacyShieldVideoSourceSummary } else { $null }
        if ($video -and $video.known -and $video.count -gt 0) { $names = @('Camera source') }
        if ($names.Count -eq 0 -and ($isRemote -ne $false -or -not $video -or -not $video.known -or $video.access -ne 'Allowed')) { $uncertain = $true }
        if ($names.Count -eq 0 -and -not $uncertain) {
            # Generic media/software devices may expose a localized or virtual
            # camera through a path not represented by the two CIM classes.
            try {
                $ambiguous = @(Get-PnpDevice -PresentOnly -ErrorAction Stop | Where-Object { $_.Class -in @('Media','SoftwareDevice') })
                if ($ambiguous.Count -gt 0) { $uncertain = $true }
            } catch { $uncertain = $true }
        }
        $policyBlockers = @()
        try {
            if ((Get-ItemProperty -Path 'HKCU:\SOFTWARE\Policies\Microsoft\Camera' -Name 'AllowCamera' -ErrorAction SilentlyContinue).AllowCamera -eq 0) {
                $policyBlockers += 'Your user Camera policy disables camera access (HKCU AllowCamera=0).'
            }
            if ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Camera' -Name 'AllowCamera' -ErrorAction SilentlyContinue).AllowCamera -eq 0) {
                $policyBlockers += 'The device Camera policy disables camera access (HKLM AllowCamera=0).'
            }
            if ((Get-ItemProperty -Path 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy' -Name 'LetAppsAccessCamera' -ErrorAction SilentlyContinue).LetAppsAccessCamera -eq 2) {
                $policyBlockers += 'Your user App Privacy policy forces apps to be denied camera access (HKCU LetAppsAccessCamera=2).'
            }
            if ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy' -Name 'LetAppsAccessCamera' -ErrorAction SilentlyContinue).LetAppsAccessCamera -eq 2) {
                $policyBlockers += 'The device App Privacy policy forces apps to be denied camera access (HKLM LetAppsAccessCamera=2).'
            }
            if ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\DeviceAccess\Global\{E5323777-F976-4f5b-9B55-B94699C46E44}' -Name 'Value' -ErrorAction SilentlyContinue).Value -eq 'Deny') {
                $policyBlockers += 'The Windows device camera master switch is set to Deny.'
            }
        } catch {}
        $blockedByPolicy = $policyBlockers.Count -gt 0
        # This is deliberately a closed, content-free vocabulary.  The Fleet
        # path may use it for operator guidance, but must never receive camera
        # names, registry values, frames, or application details.
        $cameraStatus = if ($policyBlockers -match 'App Privacy') {
            'app_camera_permission_denied'
        } elseif ($blockedByPolicy) {
            'windows_camera_policy_denied'
        } elseif ($names.Count -eq 0 -and $uncertain) {
            'unknown'
        } elseif ($names.Count -eq 0) {
            'hardware_unavailable'
        } else {
            'available'
        }
        $policyScope = if ($policyBlockers -match 'Your user') {
            'user'
        } elseif ($blockedByPolicy) {
            'device'
        } else {
            $null
        }
        @{
            available = if ($cameraStatus -eq 'unknown') { $null } else { ($names.Count -gt 0) -and -not $blockedByPolicy }
            devices   = $names
            message   = if ($blockedByPolicy) { "Camera is blocked by Windows policy: " + ($policyBlockers -join ' ') } elseif ($names.Count -gt 0) { "Camera available." } elseif ($cameraStatus -eq 'unknown') { 'Camera status could not be confirmed. Check camera access and refresh.' } else { "No usable camera detected." }
            blockedByPolicy = $blockedByPolicy
            policyBlockers = @($policyBlockers)
            cameraStatus = $cameraStatus
            cameraPolicyScope = $policyScope
            isWindowsServer = $isWindowsServer
        }
    }
    catch {
        # Registry/WMI probe failures are not evidence that the hardware is
        # absent.  Keep the diagnosis honest and do not echo exception text.
        @{ available = $null; devices = @(); message = 'Camera status could not be read.'; cameraStatus = 'unknown'; cameraPolicyScope = $null; isWindowsServer = $false }
    }
}

function Get-PrivacyShieldPidMarkerPath {
    $root = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:TEMP }
    Join-Path $root "WinCommander\logs\privacy_shield.pid"
}

function Get-PrivacyShieldPidFromMarker {
    $path = Get-PrivacyShieldPidMarkerPath
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    try {
        $candidatePid = [int](Get-Content -LiteralPath $path -Raw -ErrorAction Stop).Trim()
        if ($candidatePid -gt 0) {
            $candidate = Get-Process -Id $candidatePid -ErrorAction SilentlyContinue
            if ($candidate -and $candidate.SessionId -eq [System.Diagnostics.Process]::GetCurrentProcess().SessionId) {
                # PID markers are only hints: Windows can reuse the PID for an
                # unrelated process. Verify its identity before reporting or
                # stopping it. Unreadable identity is unknown, not stopped.
                try {
                    $details = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId=$candidatePid" -ErrorAction Stop
                    if ($null -eq $details -or [string]::IsNullOrWhiteSpace($details.CommandLine)) { return -1 }
                    if ($details.SessionId -eq $candidate.SessionId -and
                        $details.Name -in @('python.exe', 'pythonw.exe') -and
                        $details.CommandLine -match '(?:^|\s)--wc-privacy-shield(?=\s|$)') { return $candidatePid }
                } catch { return -1 }
                return $null
            }
            if ($candidate) { return $null }
        }
    } catch {}
    Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
    return $null
}

function Clear-PrivacyShieldPidMarker {
    Remove-Item -LiteralPath (Get-PrivacyShieldPidMarkerPath) -Force -ErrorAction SilentlyContinue
}

function Get-PrivacyShieldNativeOptions {
    param([string]$CommandLine)

    # Parse only the fixed flags emitted by Start-PrivacyShield. A missing,
    # duplicate, or malformed value means the native tuning is unknown.
    if (-not $CommandLine -or $CommandLine.Length -gt 8192) { return $null }
    $options = @{}
    $flags = @{
        'check-gaze' = 'checkGaze'; 'check-faces' = 'checkFaces'; 'check-phone' = 'checkPhone'
        'blur-gaze' = 'blurGaze'; 'blur-faces' = 'blurFaces'; 'blur-phone' = 'blurPhone'
        'capture-on-device' = 'captureOnDevice'; 'capture-on-multi-face' = 'captureOnMultiFace'
    }
    foreach ($name in $flags.Keys) {
        $matches = [regex]::Matches($CommandLine, "(?:^|\s)--$name(?=\s|$)")
        if ($matches.Count -gt 1) { return $null }
        $options[$flags[$name]] = ($matches.Count -eq 1)
    }
    $videoOnlyCount = [regex]::Matches($CommandLine, '(?:^|\s)--fleet-video-only(?=\s|$)').Count
    if ($videoOnlyCount -gt 1) { return $null }
    if ($videoOnlyCount -eq 1) { $options.fleetVideoOnly = $true }
    $explicitCount = [regex]::Matches($CommandLine, '(?:^|\s)--detector-options-explicit(?=\s|$)').Count
    if ($explicitCount -gt 1) { return $null }
    if ($explicitCount -eq 0 -and -not ($options.checkGaze -or $options.checkFaces -or $options.checkPhone)) {
        $options.checkGaze = $true
    }
    $names = @{
        'model-level' = 'modelLevel'; 'confidence' = 'confidence'
        'overlay-opacity' = 'overlayOpacity'; 'wake-delay-ms' = 'wakeDelayMs'
        'device-wake-multiplier' = 'deviceWakeMultiplier'
        'multi-face-wake-multiplier' = 'multiFaceWakeMultiplier'
        'buffer-frames' = 'bufferFrames'; 'capture-speed' = 'captureSpeed'
        'look-away-delay-seconds' = 'lookAwayDelaySeconds'
    }
    foreach ($name in $names.Keys) {
        $flagCount = [regex]::Matches($CommandLine, "(?:^|\s)--$name(?=\s|$)").Count
        # Older running detectors do not report this additive option.
        if ($name -eq 'look-away-delay-seconds' -and $flagCount -eq 0) { continue }
        if ($flagCount -ne 1) { return $null }
        $matches = [regex]::Matches($CommandLine, "(?:^|\s)--$name\s+([^\s`"']+)(?=\s|$)")
        if ($matches.Count -ne 1) { return $null }
        $raw = $matches[0].Groups[1].Value
        if ($name -eq 'model-level') {
            if ($raw -cnotmatch '^(nano|small|medium|large)$') { return $null }
            $options[$names[$name]] = $raw
        } else {
            if ($raw -cnotmatch '^(0|[1-9][0-9]{0,5})(\.[0-9]{1,4})?$') { return $null }
            $number = 0.0
            if (-not [double]::TryParse($raw, [System.Globalization.NumberStyles]::AllowDecimalPoint,
                [System.Globalization.CultureInfo]::InvariantCulture, [ref]$number)) { return $null }
            $options[$names[$name]] = if ($name -in @('confidence', 'device-wake-multiplier', 'multi-face-wake-multiplier')) {
                $number
            } elseif ($number -eq [math]::Truncate($number)) {
                [int]$number
            } else {
                return $null
            }
        }
    }
    return $options
}

function Test-PrivacyShieldProcessReady {
    param([string]$CommandLine)
    $switches = [regex]::Matches($CommandLine, '(?:^|\s)--run-id(?=\s|$)').Count
    if ($switches -eq 0) { return $true } # Compatibility with an already-running older detector.
    $values = [regex]::Matches($CommandLine, '(?:^|\s)--run-id\s+([0-9a-f]{32})(?=\s|$)')
    if ($switches -ne 1 -or $values.Count -ne 1) { return $false }
    $logPath = Join-Path (Split-Path (Get-PrivacyShieldPidMarkerPath) -Parent) 'privacy_shield.log'
    $tail = Get-PrivacyShieldLaunchLogTail -Path $logPath -RunId $values[0].Groups[1].Value
    return ($tail -match '(?m)\] Status: Shield Active\r?$')
}

function Get-PrivacyShieldStatus {
    param([switch]$SkipCameraProbe)
    try {
        $running = $false
        $processId = $null
        $activeMode = $null
        $nativeOptions = $null
        $cameraIndex = $null
        $processRunning = $false
        $processStartTicks = $null
        $currentSessionId = [System.Diagnostics.Process]::GetCurrentProcess().SessionId

        # Get-CimInstance works reliably on both PowerShell 5.1 (Windows
        # default) AND PowerShell 7+. The previous Get-WmiObject path
        # silently returns nothing under PS 7 because Get-WmiObject was
        # removed -- that broke the status read entirely and the button
        # stayed stuck on "Activate Shield" while Python kept running.
        # Filter at the CIM level (-Filter) instead of pipe-Where so we
        # don't materialise the whole process list just to drop most of
        # it.
        $cimUnavailable = $false
        try {
            $procs = Get-CimInstance -ClassName Win32_Process `
                -Filter "Name='python.exe' OR Name='pythonw.exe'" `
                -ErrorAction Stop
        } catch {
            $cimUnavailable = $true
            $procs = @()
        }
        foreach ($proc in $procs) {
            if ($proc.SessionId -eq $currentSessionId -and $proc.CommandLine -match '(?:^|\s)--wc-privacy-shield(?=\s|$)') {
                $running = $true
                $processRunning = $true
                $processId = $proc.ProcessId
                $running = Test-PrivacyShieldProcessReady -CommandLine $proc.CommandLine
                # The process arguments are the detector's applied state.
                # Do not infer mode from a delayed settings sync: that made a
                # no-blur Notify only process appear as Blur + Notify locally.
                $explicitModes = [regex]::Matches($proc.CommandLine, '(?:^|\s)--mode\s+(\S+)(?=\s|$)')
                $modeSwitchCount = [regex]::Matches($proc.CommandLine, '(?:^|\s)--mode(?=\s|$)').Count
                $activeMode = if ($modeSwitchCount -ne $explicitModes.Count) {
                    $null
                } elseif ($explicitModes.Count -eq 1) {
                    if ($explicitModes[0].Groups[1].Value -cin @('blur_notify', 'notify_only')) {
                        $explicitModes[0].Groups[1].Value
                    } else { $null }
                } elseif ($explicitModes.Count -gt 1) {
                    $null
                } elseif ($proc.CommandLine -match '(?:^|\s)--blur-(?:gaze|faces|phone)(?:\s|$)') {
                    'blur_notify'
                } else {
                    'notify_only'
                }
                try { $processStartTicks = (Get-Process -Id $proc.ProcessId -ErrorAction Stop).StartTime.ToUniversalTime().Ticks.ToString() } catch {}
                $nativeOptions = Get-PrivacyShieldNativeOptions -CommandLine $proc.CommandLine
                # Local recovery needs the exact camera index. Keep this out
                # of Fleet observations and fail closed on missing/duplicate
                # switches rather than guessing the default camera.
                $cameraSwitches = [regex]::Matches($proc.CommandLine, '(?:^|\s)--camera(?=\s|$)').Count
                $cameraValues = [regex]::Matches($proc.CommandLine, '(?:^|\s)--camera\s+([0-9]+)(?=\s|$)')
                if ($cameraSwitches -eq 1 -and $cameraValues.Count -eq 1) {
                    $parsedCamera = 0
                    if ([int]::TryParse($cameraValues[0].Groups[1].Value, [ref]$parsedCamera) -and $parsedCamera -ge 0 -and $parsedCamera -le 32) {
                        $cameraIndex = $parsedCamera
                    }
                }
                break
            }
        }

        # A marker is only a hint. If process identity cannot be read, report
        # unknown instead of guessing from a PID that Windows may have reused.
        if (-not $processRunning) {
            $markerPid = Get-PrivacyShieldPidFromMarker
            if ($markerPid -eq -1) {
                return @{ error = $true; message = 'Privacy Shield process identity could not be verified.'; cameraStatus = 'unknown'; shieldLifecycle = 'unknown' }
            }
            if ($markerPid) {
                $markerProcess = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId=$markerPid" -ErrorAction Stop
                if (-not $markerProcess -or $markerProcess.SessionId -ne $currentSessionId -or
                    $markerProcess.Name -notin @('python.exe', 'pythonw.exe') -or
                    $markerProcess.CommandLine -notmatch '(?:^|\s)--wc-privacy-shield(?=\s|$)') {
                    return @{ error = $true; message = 'Privacy Shield process identity could not be verified.'; cameraStatus = 'unknown'; shieldLifecycle = 'unknown' }
                }
                $running = Test-PrivacyShieldProcessReady -CommandLine $markerProcess.CommandLine
                $processRunning = $true
                $processId = $markerPid
                # Mode is intentionally unknown rather than guessed from cache.
            }
        }
        if ($cimUnavailable -and -not $processRunning) {
            return @{ error = $true; message = 'Privacy Shield status could not be read.'; cameraStatus = 'unknown'; shieldLifecycle = 'unknown' }
        }

        # NOTE: the old "last-ditch fallback" that grabbed the first
        # python.exe via Get-Process was removed. It didn't check the
        # --wc-privacy-shield marker and therefore produced false positives
        # on any machine that had an unrelated Python process (e.g. Windows
        # Server 2025 with a monitoring agent). CIM works on both PS 5.1 and
        # PS 7, so the fallback is not needed.

        $camera = if ($SkipCameraProbe -or ($processRunning -and -not $running)) {
            @{ available = $null; devices = @(); message = ''; cameraStatus = 'unknown'; cameraPolicyScope = $null; isWindowsServer = $false }
        } elseif ($running) {
            # A ready detector has opened and sampled the actual camera.
            # Re-enumerating PnP on every poll adds delay and can contradict it.
            @{ available = $true; devices = @('Camera source'); message = ''; cameraStatus = 'available'; cameraPolicyScope = $null; isWindowsServer = $false }
        } else { Get-PrivacyShieldCameraAvailability }
        @{
            running         = $running
            detectorProcessRunning = $processRunning
            processId       = $processId
            activeMode      = $activeMode
            processStartTicks = $processStartTicks
            nativeOptions   = $nativeOptions
            cameraIndex     = $cameraIndex
            # Fleet requires an explicit true before changing a managed
            # session. The separate status preserves an unknown diagnosis.
            cameraAvailable = [bool]$camera.available
            cameraDevices   = @($camera.devices)
            cameraMessage   = $camera.message
            cameraPolicyBlockers = @($camera.policyBlockers)
            cameraStatus = $camera.cameraStatus
            cameraPolicyScope = $camera.cameraPolicyScope
            shieldLifecycle = if ($running) { 'running' } elseif ($processRunning) { 'starting' } else { 'stopped' }
            isWindowsServer = [bool]$camera.isWindowsServer
        }
    }
    catch {
        # A status-read failure is different from a stopped shield.  The
        # console must not show an invented lifecycle or hardware diagnosis.
        @{ error = $true; message = 'Privacy Shield status could not be read.'; cameraStatus = 'unknown'; shieldLifecycle = 'unknown' }
    }
}

function Get-PrivacyShieldStartFailure {
    param([string]$LogTail)

    # Do not return the log tail: it can contain implementation-specific
    # details.  These short messages and codes are safe for the local UI and
    # Fleet receipt, while keeping a camera busy distinct from missing
    # hardware or a policy denial.
    if ($LogTail -imatch 'device or resource busy|camera.*(?:already|currently).*in use|camera.*busy') {
        return @{ code = 'camera_busy'; message = 'Camera is currently being used by another application.' }
    }
    if ($LogTail -imatch 'black frames|camera feed unavailable') {
        return @{ code = 'camera_feed_unavailable'; message = 'Camera feed is black - open its privacy shutter or close another camera app.' }
    }
    if ($LogTail -imatch 'no camera|webcam|videocapture|camera not found|camera timed out|camera.*(?:open|initializ)') {
        # An open failure alone cannot distinguish missing hardware from a
        # busy camera, privacy restriction, driver failure or timeout.
        return @{ code = 'camera_start_unverified'; message = 'Camera could not be opened. Check Windows camera access and close other camera apps, then try again.' }
    }
    if ($LogTail -imatch 'missing python dependency|dependency import failed|importerror|module not found|no module named') {
        return @{ code = 'runtime_dependency_missing'; message = 'Privacy Shield AI runtime is unavailable.' }
    }
    if ($LogTail -imatch 'model preparation|failed to download|model download') {
        return @{ code = 'model_download_failed'; message = 'Privacy Shield model preparation failed.' }
    }
    if ($LogTail -imatch 'detector init|flatbuffer|not a valid') {
        return @{ code = 'model_invalid'; message = 'Privacy Shield model could not be loaded.' }
    }
    return @{ code = 'shield_start_failed'; message = 'Privacy Shield could not start.' }
}

function Get-PrivacyShieldLaunchLogTail {
    param([string]$Path, [string]$RunId)
    if (-not $Path -or $RunId -cnotmatch '^[0-9a-f]{32}$' -or -not (Test-Path -LiteralPath $Path)) { return '' }
    try {
        # The file can contain older or other-session runs. Only lines tagged
        # by this launched process can contribute to its failure diagnosis.
        return @((Get-Content -LiteralPath $Path -Tail 100 -ErrorAction Stop) | Where-Object { $_.Contains("[$RunId]") }) -join "`n"
    } catch { return '' }
}

function Wait-PrivacyShieldReady {
    param($Process, [string]$LogPath, [string]$RunId, [int]$TimeoutMs = 30000)
    $deadline = [System.Diagnostics.Stopwatch]::StartNew()
    while ($deadline.ElapsedMilliseconds -lt $TimeoutMs) {
        if ($Process.HasExited) { return $false }
        $tail = Get-PrivacyShieldLaunchLogTail -Path $LogPath -RunId $RunId
        if ($tail -match '(?m)\] Status: Shield Active\r?$' -and -not $Process.HasExited) { return $true }
        Start-Sleep -Milliseconds 100
    }
    return $false
}

function Write-PrivacyShieldDebugStage {
    param([ValidateSet('router', 'start', 'status', 'runtime', 'dependencies', 'launch', 'ready')][string]$Stage)
    if ($env:WINCMD_SHIELD_DEBUG_TRACE -ne '1' -or -not $env:LOCALAPPDATA) { return }
    try {
        $path = Join-Path $env:LOCALAPPDATA 'WinCommander\logs\privacy_shield.log'
        Add-Content -LiteralPath $path -Value "Shield debug stage: $Stage" -ErrorAction Stop
    } catch {}
}

function Start-PrivacyShield {
    param(
        [int]$Camera = 0,
        [switch]$CheckGaze,
        [switch]$CheckFaces,
        [switch]$CheckPhone,
        # Fleet can monitor every detector while these independent controls
        # decide which detected conditions blur the endpoint displays.
        [switch]$BlurGaze,
        [switch]$BlurFaces,
        [switch]$BlurPhone,
        [switch]$CaptureOnDevice,
        [switch]$CaptureOnMultiFace,
        [switch]$FleetVideoOnly,
        [ValidateSet('blur_notify', 'notify_only')][string]$Mode = '',
        [string]$ModelLevel = "nano",
        [float]$Confidence = 0.5,
        [int]$OverlayOpacity = 200,
        [int]$WakeDelayMs = 150,
        [ValidateRange(0, 300)][int]$LookAwayDelaySeconds = 0,
        [int]$DeviceWakeMultiplier = 2,
        [int]$MultiFaceWakeMultiplier = 2,
        [int]$BufferFrames = 2,
        [int]$CaptureSpeed = 1
    )
    
    try {
        Write-PrivacyShieldDebugStage 'start'
        $status = Get-PrivacyShieldStatus
        Write-PrivacyShieldDebugStage 'status'
        if ($status.error -or $status.shieldLifecycle -eq 'unknown') {
            return @{ error = $true; message = 'Privacy Shield status could not be confirmed. Refresh and try again.'; cameraStatus = 'unknown'; shieldLifecycle = 'unknown'; diagnosticCode = 'status_unavailable' }
        }
        if ($status.running -or $status.detectorProcessRunning) {
            return @{ error = $true; message = "Shield is already running or starting."; cameraStatus = $status.cameraStatus; shieldLifecycle = $status.shieldLifecycle; diagnosticCode = 'already_running' }
        }
        # Enumeration can be inconclusive for virtual or redirected cameras.
        # In that case the detector's real open/read test below is authoritative.
        # Never bypass an explicit policy denial or a confirmed unavailable camera.
        if ($status.cameraAvailable -ne $true -and $status.cameraStatus -ne 'unknown') {
            return @{
                error = $true
                message = if ($status.cameraMessage) { $status.cameraMessage } else { "No camera detected - Privacy Shield requires a webcam." }
                cameraAvailable = $false
                cameraDevices = @()
                cameraStatus = if ($status.cameraStatus) { $status.cameraStatus } else { 'unknown' }
                cameraPolicyScope = $status.cameraPolicyScope
                shieldLifecycle = 'stopped'
                diagnosticCode = if ($status.cameraStatus) { $status.cameraStatus } else { 'unknown' }
            }
        }

        $pythonExe = Resolve-PythonPath
        Write-PrivacyShieldDebugStage 'runtime'
        if (-not $pythonExe) {
            return @{ error = $true; message = "Privacy Shield AI runtime is unavailable."; cameraStatus = $status.cameraStatus; shieldLifecycle = 'stopped'; diagnosticCode = 'runtime_unavailable' }
        }

        # Installation belongs to the explicit AI-runtime installer. Starting
        # protection must not block its policy mutex on hidden pip downloads.
        & $pythonExe -c 'import mediapipe, cv2, PyQt6, numpy, PIL' *>$null
        Write-PrivacyShieldDebugStage 'dependencies'
        if ($LASTEXITCODE -ne 0) {
            return @{ error = $true; message = 'Privacy Shield AI runtime is incomplete. Install or repair the AI runtime, then try again.'; cameraStatus = $status.cameraStatus; shieldLifecycle = 'stopped'; diagnosticCode = 'runtime_dependency_missing' }
        }

        # Keep the core script in-memory to avoid writing source code to AppData.
        $shieldProcessMarker = "--wc-privacy-shield"
        $embeddedScript = @'
"""
Unified Privacy Shield Module
=============================
Combines:
1. Gaze Detection (Blur on look away)
2. Multi-Face Detection (Blur on >1 face)
3. Object Detection (Blur on Phone/Camera)

Optimized for low-overhead running on CPU efficiently.
"""

import sys
import os
if sys.platform == "win32":
    # The launcher owns temporary drain pipes; keep native library output off
    # those pipes after it exits so later writes cannot hit a closed reader.
    _null_fd = os.open(os.devnull, os.O_WRONLY)
    os.dup2(_null_fd, 1)
    os.dup2(_null_fd, 2)
    os.close(_null_fd)
import json
import time
import argparse
import threading
import traceback
import ctypes
import uuid
import urllib.request
import uuid
from collections import deque
from ctypes import wintypes
from dataclasses import dataclass

# --- Basic Logging ---
LOG_FILE = os.path.join(os.environ.get("LOCALAPPDATA", ""), "WinCommander", "logs", "privacy_shield.log")
CAPTURES_DIR = os.path.join(os.environ.get("LOCALAPPDATA", ""), "WinCommander", "shield_captures")
# Durable NDJSON sidecar of look-state transitions. The PowerShell wrapper
# exits ~8s after spawning us, so stdout is not a reliable channel back to
# WinCommander; this file survives the wrapper and is tailed live by the
# Rust backend (backend.rs shield event reader).
EVENTS_FILE = os.path.join(os.environ.get("LOCALAPPDATA", ""), "WinCommander", "logs", "privacy_shield_events.ndjson")
PID_FILE = os.path.join(os.environ.get("LOCALAPPDATA", ""), "WinCommander", "logs", "privacy_shield.pid")
STOP_FILE = PID_FILE + "." + str(os.getpid()) + ".stop"
RUN_ID = os.environ.get("WINCMD_SHIELD_RUN_ID", "")
os.makedirs(os.path.dirname(LOG_FILE), exist_ok=True)

def log(msg):
    try:
        timestamp = time.strftime("%Y-%m-%d %H:%M:%S")
        with open(LOG_FILE, "a", encoding="utf-8") as handle:
            launch_id = os.environ.get("WINCMD_SHIELD_LAUNCH_ID", "unknown")
            handle.write(f"[{timestamp}] [{RUN_ID}] [launch:{launch_id}] {msg}\n")
    except Exception:
        pass

def emit_event(name, reason="", capture_ids=None):
    # One JSON object per line (NDJSON). Appended + flushed on every Qt
    # look-state change so the backend reader sees transitions promptly.
    try:
        line = json.dumps({"event": name, "reason": reason, "ts": time.time(), "capture_ids": capture_ids or {}})
        with open(EVENTS_FILE, "a", encoding="utf-8") as handle:
            handle.write(line + "\n")
            handle.flush()
    except Exception:
        pass

def write_pid_marker():
    try:
        if os.path.isfile(STOP_FILE):
            os.remove(STOP_FILE)
        temporary = PID_FILE + ".tmp"
        with open(temporary, "w", encoding="ascii") as handle:
            handle.write(str(os.getpid()))
        os.replace(temporary, PID_FILE)
    except Exception as e:
        log(f"Could not write detector PID marker: {e}")

def clear_pid_marker():
    try:
        with open(PID_FILE, encoding="ascii") as handle:
            owned = handle.read().strip() == str(os.getpid())
        if owned:
            os.remove(PID_FILE)
    except FileNotFoundError:
        pass
    except Exception as e:
        log(f"Could not remove detector PID marker: {e}")
    try:
        os.remove(STOP_FILE)
    except FileNotFoundError:
        pass
    except OSError:
        log("Could not remove detector stop request")

def owner_process_is_alive(pid):
    """Check the trusted WinCommander Free owner, never the PS launcher."""
    if not pid or pid <= 0 or sys.platform != "win32":
        return False
    SYNCHRONIZE = 0x00100000
    WAIT_TIMEOUT = 0x00000102
    try:
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        handle = kernel32.OpenProcess(SYNCHRONIZE, False, pid)
        if not handle:
            return False
        try:
            return kernel32.WaitForSingleObject(handle, 0) == WAIT_TIMEOUT
        finally:
            kernel32.CloseHandle(handle)
    except Exception:
        # We cannot verify ownership, so do not keep using the camera.
        return False


# --- Dependency Imports ---
try:
    import cv2
    import numpy as np
    from PyQt6.QtWidgets import QApplication, QWidget, QLabel, QVBoxLayout
    from PyQt6.QtCore import Qt, QTimer, pyqtSignal, QObject, QThread, QPoint
    from PyQt6.QtGui import QColor, QFont, QPainter, QImage, QPixmap
except ImportError as e:
    # Ensure dependencies are installed by wrapper first
    log(f"Dependency import failed: {e}")
    sys.exit(1)

# --- Configuration & Constants ---
import socket
socket.setdefaulttimeout(15)  # Prevent urlretrieve from hanging forever

MODELS_DIR = os.path.join(os.environ.get("LOCALAPPDATA", ""), "WinCommander", "models")
FACE_MODEL_PATH = os.path.join(MODELS_DIR, "face_landmarker.task")

# EfficientDet-Lite Models (Object Detection)
# Mapping 'nano' -> nano.pt, 'medium' -> medium.pt, etc
OBJ_MODELS = {
    "nano": ("nano.pt", "https://storage.googleapis.com/mediapipe-models/object_detector/efficientdet_lite0/float16/1/efficientdet_lite0.tflite"),
    "small": ("small.pt", "https://storage.googleapis.com/mediapipe-models/object_detector/efficientdet_lite0/float16/1/efficientdet_lite0.tflite"), 
    "medium": ("medium.pt", "https://storage.googleapis.com/mediapipe-models/object_detector/efficientdet_lite2/float16/1/efficientdet_lite2.tflite"),
    "large": ("large.pt", "https://storage.googleapis.com/mediapipe-models/object_detector/efficientdet_lite2/float16/1/efficientdet_lite2.tflite")
}

FACE_MODEL_URL = "https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task"
MIN_MODEL_SIZE = 50_000  # 50 KB minimum - guards against empty/corrupt/partial downloads

def classify_gaze_with_reason(landmarks):
    try:
        nose, left, right = landmarks[1], landmarks[234], landmarks[454]
        width = right.x - left.x
        if width <= 0.01:
            return "blackout", "invalid"
        yaw = abs((nose.x - (left.x + right.x) / 2.0) / width)
        if not np.isfinite(yaw):
            return "blackout", "invalid"
        clipped = left.x <= 0.02 or right.x >= 0.98 or getattr(landmarks[10], "y", 0.5) <= 0.02 or getattr(landmarks[152], "y", 0.5) >= 0.98
        if clipped:
            return "blur", "frame_edge"
        if yaw >= 0.46:
            return "blackout", "full_turn"
        if yaw >= 0.12:
            return "blur", "yaw"
        return "clear", "clear"
    except (IndexError, AttributeError, TypeError, ValueError):
        return "blackout", "invalid"


def classify_gaze(landmarks):
    return classify_gaze_with_reason(landmarks)[0]


def overlay_presentation(is_clear, reason):
    if is_clear:
        return "clear"
    if reason == "LOOK AWAY":
        return "blur"
    return "blackout"


def write_capture_result(path, result):
    temporary = path + "." + uuid.uuid4().hex + ".tmp"
    try:
        with open(temporary, "x", encoding="utf-8") as handle:
            json.dump(result, handle)
        os.replace(temporary, path)
    finally:
        if os.path.isfile(temporary):
            os.remove(temporary)


class GazeState:
    def __init__(self, buffer_frames):
        self.buffer_frames = max(1, buffer_frames)
        self.state = "clear"
        self.pending = "clear"
        self.streak = 0

    def observe(self, state):
        self.streak = self.streak + 1 if state == self.pending else 1
        self.pending = state
        required = max(3, self.buffer_frames) if state == "blackout" else self.buffer_frames
        if self.streak >= required:
            self.state = state
        return self.state


def set_process_dpi_aware():
    try: ctypes.windll.shcore.SetProcessDpiAwareness(1)
    except: pass

# --- Worker Thread ---
class ContinuousLookAway:
    """Delay only a continuous, actually observed look-away episode."""
    def __init__(self, delay_seconds):
        self.delay_seconds = max(0, min(300, int(delay_seconds)))
        self.since = None
        self.last_observed = None

    def reset(self):
        self.since = None
        self.last_observed = None

    def observe(self, away, now):
        if not away:
            self.reset()
            return False
        if (self.since is None or self.last_observed is None or
                now < self.last_observed or now - self.last_observed > 2.0):
            self.since = now
        self.last_observed = now
        return now - self.since >= self.delay_seconds

class ShieldWorker(QThread):
    state_changed = pyqtSignal(bool, str, object)
    status_msg = pyqtSignal(str)
    init_failed = pyqtSignal(str)

    def __init__(self, camera_idx=0, check_gaze=True, check_faces=False, check_phone=False,
                 capture_on_device=False,
                 capture_on_multi_face=False, fleet_video_only=False,
                 model_level="nano", confidence=0.5, cam_width=640, cam_height=480,
                 wake_delay_ms=150, device_wake_multiplier=2, multi_face_wake_multiplier=2,
                 buffer_frames=2, capture_speed=1, look_away_delay_seconds=0):
        super().__init__()
        self.camera_idx = camera_idx
        self.check_gaze = check_gaze
        self.check_faces = check_faces
        self.check_phone = check_phone
        self.capture_on_device = capture_on_device
        self.capture_on_multi_face = capture_on_multi_face
        self.fleet_video_only = fleet_video_only
        self._capture_ids = {}
        self.model_level = model_level
        self.confidence = confidence
        self.cam_width = cam_width
        self.cam_height = cam_height
        self.wake_delay_ms = max(50, int(wake_delay_ms))
        self.device_wake_multiplier = max(1, min(20, int(device_wake_multiplier)))
        self.multi_face_wake_multiplier = max(1, min(20, int(multi_face_wake_multiplier)))
        self.buffer_frames = max(1, int(buffer_frames))
        self.capture_speed = max(1, min(4, int(capture_speed)))
        self._look_away = ContinuousLookAway(look_away_delay_seconds)

        self._running = True
        self._is_locked = True
        
        # Hysteresis timers
        self._attentive_ms = 0.0
        self._distracted_ms = 0.0
        self._lock_reason = ""
        self._captured_for_device = False
        self._captured_for_multi_face = False
        self._capture_retry_at = {"device": 0.0, "multi_face": 0.0}
        self._gaze = GazeState(self.buffer_frames)
        self._debug_gaze_reason = None
        self._debug_gaze_events = 0
        self._device_detected_streak = 0
        self._multi_face_detected_streak = 0
        # Presence is noisier than the other detectors: a single missed
        # landmark frame must never create a look-away episode.  Keep the
        # loss latched until a sustained face recovery too, so a flickering
        # feed cannot alternate look_away/look_back and flood notifications.
        self._no_face_detected_streak = 0
        self._face_recovery_streak = 0
        self._presence_loss_active = False
        self._frame_buffer = deque(maxlen=30)
        self._frame_times = deque(maxlen=30)
        self._active_recordings = []

        self.mp_face = None
        self.mp_obj = None
        self.face_detector = None
        self.obj_detector = None

    def _observe_missing_face(self):
        self._no_face_detected_streak += 1
        self._face_recovery_streak = 0
        if self.check_gaze and self._no_face_detected_streak >= max(6, self.buffer_frames * 2):
            self._presence_loss_active = True
        presence_lost = self.check_gaze and self._presence_loss_active
        hold_gaze_overlay = self.check_gaze and not presence_lost and self._is_locked and (
            "LOOK AWAY" in self._lock_reason or "PRESENCE LOST" in self._lock_reason
        )
        return presence_lost, hold_gaze_overlay

    def _is_model_valid(self, path: str) -> bool:
        """Return True only if the file exists and is large enough to be a real model.
        Deletes the file if it exists but is suspiciously small (corrupt / partial download)."""
        if not os.path.exists(path):
            return False
        size = os.path.getsize(path)
        if size < MIN_MODEL_SIZE:
            log(f"Model file is too small ({size} bytes), likely corrupt - deleting for re-download: {path}")
            try:
                os.remove(path)
            except Exception as e:
                log(f"Could not remove corrupt model file: {e}")
            return False
        return True

    def _ensure_models(self):
        os.makedirs(MODELS_DIR, exist_ok=True)
        if (self.check_gaze or self.check_faces) and not self._is_model_valid(FACE_MODEL_PATH):
            try:
                urllib.request.urlretrieve(FACE_MODEL_URL, FACE_MODEL_PATH)
            except Exception as e:
                log(f"Failed to download face model: {e}")
                return False
        
        if self.check_phone:
            # Determine which model to download based on level
            model_info = OBJ_MODELS.get(self.model_level, OBJ_MODELS["nano"])
            filename, url = model_info
            path = os.path.join(MODELS_DIR, filename)
            
            # Store path for loading
            self.obj_model_path = path
            
            if not self._is_model_valid(path):
                try:
                    urllib.request.urlretrieve(url, path)
                except Exception as e:
                    log(f"Failed to download object model ({self.model_level}): {e}")
                    return False
        return True

    def _init_detectors(self):
        try:
            import mediapipe as mp
            from mediapipe.tasks import python
            from mediapipe.tasks.python import vision

            if self.check_gaze or self.check_faces:
                base_options = python.BaseOptions(model_asset_path=FACE_MODEL_PATH)
                options = vision.FaceLandmarkerOptions(
                    base_options=base_options,
                    output_face_blendshapes=True,
                    num_faces=5 if self.check_faces else 1,
                    min_face_detection_confidence=0.5
                )
                self.face_detector = vision.FaceLandmarker.create_from_options(options)

            if self.check_phone:
                base_options = python.BaseOptions(model_asset_path=self.obj_model_path)
                # Max results = 5 objects
                options = vision.ObjectDetectorOptions(
                    base_options=base_options,
                    score_threshold=self.confidence,
                    max_results=5,
                    category_allowlist = ["cell phone", "remote"] 
                )
                self.obj_detector = vision.ObjectDetector.create_from_options(options)
                
            self.mp = mp
            return True
        except Exception as e:
            err_str = str(e)
            if "flatbuffer" in err_str.lower() or "not a valid" in err_str.lower():
                # Cached model files are corrupt - delete them so next start re-downloads cleanly
                log(f"Corrupt model files detected (Flatbuffer error), removing cache for re-download")
                for p in [FACE_MODEL_PATH] + [os.path.join(MODELS_DIR, v[0]) for v in OBJ_MODELS.values()]:
                    if os.path.exists(p):
                        try:
                            os.remove(p)
                            log(f"Removed corrupt model: {p}")
                        except Exception as re:
                            log(f"Could not remove {p}: {re}")
            log(f"Detector init failed: {e}")
            self.status_msg.emit(f"Init Error: {e}")
            return False

    def _check_gaze(self, landmarks):
        state, reason = classify_gaze_with_reason(landmarks)
        if (os.environ.get("WINCMD_SHIELD_DEBUG_TRACE") == "1" and
                reason != self._debug_gaze_reason and self._debug_gaze_events < 32):
            log(f"Gaze debug category: {reason}")
            self._debug_gaze_reason = reason
            self._debug_gaze_events += 1
        return state

    def _start_webcam_recording(self, prefix: str, reason_key: str, post_roll_sec: float = 5.0):
        """Start recording webcam. Records while detection is active + post_roll_sec after it ends."""
        out = None
        try:
            if not self._frame_buffer:
                return False
            if any(rec["reason_key"] == reason_key for rec in self._active_recordings):
                return True
            final_path = os.path.join(CAPTURES_DIR, f"{prefix}_webcam.avi")
            video_path = os.path.join(CAPTURES_DIR, f"{prefix}_pending.avi") if self.fleet_video_only else final_path
            first = self._frame_buffer[0]
            h, w = first.shape[:2]
            fourcc = cv2.VideoWriter_fourcc(*'MJPG')
            elapsed = self._frame_times[-1] - self._frame_times[0] if len(self._frame_times) > 1 else 0.0
            measured_fps = (len(self._frame_times) - 1) / elapsed if elapsed > 0 else 30.0
            output_fps = max(1.0, min(60.0, measured_fps)) * self.capture_speed
            out = cv2.VideoWriter(video_path, fourcc, output_fps, (w, h))
            if not out.isOpened():
                out.release()
                log("Incident webcam writer could not be opened")
                return False
            for f in self._frame_buffer:
                out.write(f)
            now = time.monotonic()
            self._active_recordings.append({
                "out": out, "path": video_path, "reason_key": reason_key,
                "record_until": now + post_roll_sec, "in_post_roll": False,
                "metadata_path": os.path.join(CAPTURES_DIR, prefix + "_capture.json"),
                "final_path": final_path if self.fleet_video_only else None,
                "max_until": now + 30.0 if self.fleet_video_only else float("inf")
            })
            return True
        except Exception as e:
            if out is not None:
                out.release()
            log("Incident webcam recording could not be started")
            return False

    def _capture_incident(self, frame, reason: str, reason_key: str):
        """Capture screen (PNG) + start webcam video. Video records while detection active + 5s post-roll."""
        screen_saved = False
        try:
            os.makedirs(CAPTURES_DIR, exist_ok=True)
        except OSError:
            self.status_msg.emit("Incident capture failed: capture folder is unavailable")
            return False
        incident_id = str(uuid.uuid4()) if self.fleet_video_only else uuid.uuid4().hex
        prefix = "clip_" + incident_id if self.fleet_video_only else time.strftime("%Y%m%d_%H%M%S") + "_" + reason_key + "_" + incident_id
        if not self.fleet_video_only:
            try:
                from PIL import ImageGrab
                with ImageGrab.grab(all_screens=True) as img:
                    img.save(os.path.join(CAPTURES_DIR, prefix + "_screen.png"))
                screen_saved = True
            except Exception:
                log("Incident screenshot could not be saved")
        video_started = self._start_webcam_recording(prefix, reason_key)
        if video_started and self.fleet_video_only:
            self._capture_ids[reason_key] = incident_id
        status = "recording" if video_started else "partial" if screen_saved else "failed"
        self.status_msg.emit("Incident capture " + status)
        capture_binding = os.environ.get("WINCMD_SHIELD_CAPTURE_BINDING", "")
        if len(capture_binding) != 64 or any(char not in "0123456789abcdef" for char in capture_binding):
            capture_binding = ""
        try:
            write_capture_result(os.path.join(CAPTURES_DIR, prefix + "_capture.json"),
                                 {"id": incident_id, "reason": reason_key, "status": status, "screenSaved": screen_saved,
                                  "webcamStarted": video_started, "webcamSaved": False, "ts": time.time(),
                                  "binding": capture_binding,
                                  "screenFile": prefix + "_screen.png" if screen_saved else None,
                                  "webcamFile": prefix + "_webcam.avi" if video_started else None})
        except OSError:
            log("Incident capture result could not be saved")
        return screen_saved or video_started

    def _capture_if_needed(self, frame, triggered, enabled, reason_key, reason, now):
        attribute = "_captured_for_" + reason_key
        if not triggered:
            setattr(self, attribute, False)
            self._capture_retry_at[reason_key] = 0.0
        elif enabled and not getattr(self, attribute) and now >= self._capture_retry_at[reason_key]:
            recording_active = any(rec["reason_key"] == reason_key for rec in self._active_recordings)
            setattr(self, attribute, recording_active or self._capture_incident(frame, reason, reason_key))
            self._capture_retry_at[reason_key] = now + 5.0

    def _finish_recording(self, rec):
        saved = False
        try:
            rec["out"].release()
            size = os.path.getsize(rec["path"]) if os.path.isfile(rec["path"]) else 0
            saved = not rec.get("failed") and size > 0 and (not rec.get("final_path") or size <= 50 * 1024 * 1024)
            if saved and rec.get("final_path"):
                os.rename(rec["path"], rec["final_path"])
        except Exception:
            saved = False
        try:
            with open(rec["metadata_path"], encoding="utf-8") as handle:
                result = json.load(handle)
            result["webcamSaved"] = saved
            result["status"] = "complete" if saved and (result["screenSaved"] or self.fleet_video_only) else "partial" if saved or result["screenSaved"] else "failed"
            write_capture_result(rec["metadata_path"], result)
            self.status_msg.emit("Incident capture " + result["status"])
        except (OSError, ValueError, KeyError):
            self.status_msg.emit("Incident webcam recording saved" if saved else "Incident webcam recording failed")

    def _update_recordings(self, frame, device_triggered, multi_face_triggered, now):
        still_active = []
        for rec in self._active_recordings:
            triggered = device_triggered if rec["reason_key"] == "device" else multi_face_triggered
            if triggered:
                rec["in_post_roll"] = False
                rec["record_until"] = now + 5.0
            elif not rec["in_post_roll"]:
                rec["in_post_roll"] = True
                rec["record_until"] = now + 5.0
            if now < rec["record_until"] and now < rec["max_until"]:
                try:
                    rec["out"].write(frame)
                    still_active.append(rec)
                    continue
                except Exception:
                    rec["failed"] = True
                    self.status_msg.emit("Incident webcam recording failed")
            self._finish_recording(rec)
        self._active_recordings = still_active

    def run(self):
        if not self._ensure_models():
            log("Model preparation failed")
            self.init_failed.emit("Model preparation failed")
            return
        if not self._init_detectors():
            log("Detector initialization failed")
            self.init_failed.emit("Detector initialization failed")
            return

        # Open the camera in a background thread with a hard timeout.
        # cv2.VideoCapture()+DirectShow can hang indefinitely on headless
        # or no-camera systems (servers, VMs, remote-desktop sessions),
        # which keeps the process alive and makes the UI show "Running"
        # even though no detection ever starts.
        _cam_result = [None]
        _cam_done = threading.Event()
        def _try_open_cam():
            try:
                c = cv2.VideoCapture(self.camera_idx, cv2.CAP_DSHOW)
                if not c.isOpened():
                    # Release the failed DirectShow handle before asking the
                    # default backend to open the same camera.
                    c.release()
                    c = cv2.VideoCapture(self.camera_idx)
                _cam_result[0] = c
            except Exception as exc:
                log(f"Camera open exception: {exc}")
            finally:
                _cam_done.set()
        threading.Thread(target=_try_open_cam, daemon=True).start()
        if not _cam_done.wait(timeout=5.0):
            log("Camera initialization timed out")
            self.init_failed.emit("Camera could not be opened. Check camera access and try again.")
            return
        cap = _cam_result[0]
        if not cap or not cap.isOpened():
            self.status_msg.emit("Camera Failed")
            log("Camera could not be opened")
            self.init_failed.emit("Camera could not be opened. Check camera access and close other camera apps, then try again.")
            return

        # A device can open successfully while returning only black frames,
        # commonly because its physical shutter is closed or the camera is
        # reserved by another application. Treat that as an unavailable feed;
        # otherwise every frame becomes a false NO FACE alert and blacks out
        # the user's display.
        black_frames = 0
        sampled_frames = 0
        for _ in range(20):
            ok, sample = cap.read()
            if not ok or sample is None:
                continue
            sampled_frames += 1
            if float(sample.mean()) < 5.0:
                black_frames += 1
        if sampled_frames == 0 or black_frames >= 16:
            cap.release()
            self.status_msg.emit("Camera feed is black")
            log(f"Camera feed unavailable: {black_frames}/{sampled_frames} sampled frames were black")
            self.init_failed.emit("Camera is delivering black frames - open its privacy shutter or close another camera app")
            return

        self.status_msg.emit("Shield Active")
        
        last_ts = time.monotonic()
        while self._running:
            now_ts = time.monotonic()
            # A stalled capture is not proof that the user was attentive.
            delta_ms = min(250.0, max(0.0, (now_ts - last_ts) * 1000.0))
            last_ts = now_ts
            ret, frame = cap.read()
            if not ret:
                self._look_away.reset()
                self._attentive_ms = 0.0
                time.sleep(0.5)
                last_ts = time.monotonic()
                continue

            self._frame_buffer.append(frame.copy())
            self._frame_times.append(now_ts)

            # Convert frame for MediaPipe
            rgb_frame = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
            mp_image = self.mp.Image(image_format=self.mp.ImageFormat.SRGB, data=rgb_frame)
            
            is_clear = True
            reason = ""
            
            try:
                device_triggered = False
                multi_face_triggered = False
                gaze_triggered = False
                gaze_level = "clear"
                raw_look_away = False
                # A sustained missing face is a distinct presence-lost event.
                # It is debounced and recovered independently, so a camera
                # dropout cannot create an alert storm.
                presence_unknown = False
                presence_lost_triggered = False

                # 1. Check Phone/Object - require buffer_frames consecutive detections
                if self.check_phone and self.obj_detector:
                    obj_results = self.obj_detector.detect(mp_image)
                    raw_device = any(
                        d.categories[0].category_name in ["cell phone", "remote"]
                        for d in obj_results.detections
                    )
                    if raw_device:
                        self._device_detected_streak += 1
                    else:
                        self._device_detected_streak = 0
                    if self._device_detected_streak >= self.buffer_frames:
                        device_triggered = True

                # 2. Check Faces (Count & Gaze) - always run so we capture for both when both detected
                if (self.check_faces or self.check_gaze) and self.face_detector:
                    face_results = self.face_detector.detect(mp_image)
                    landmarks = face_results.face_landmarks

                    if not landmarks:
                        # Keep the previous gaze overlay through the short
                        # no-face debounce, then use the existing black
                        # presence-loss overlay. A single missed frame is not
                        # a new alert or a reason to briefly reveal the screen.
                        self._look_away.reset()
                        presence_lost_triggered, presence_unknown = self._observe_missing_face()
                    else:
                        self._no_face_detected_streak = 0
                        if self._presence_loss_active:
                            self._face_recovery_streak += 1
                            if self._face_recovery_streak >= self.buffer_frames:
                                self._presence_loss_active = False
                                self._face_recovery_streak = 0
                            else:
                                # Hold unknown presence until a real recovery;
                                # a lone detected frame must not clear a real
                                # alert episode.
                                presence_unknown = True
                        else:
                            self._face_recovery_streak = 0
                        raw_multi = self.check_faces and len(landmarks) > 1
                        if raw_multi:
                            self._multi_face_detected_streak += 1
                        else:
                            self._multi_face_detected_streak = 0
                        if self._multi_face_detected_streak >= self.buffer_frames:
                            multi_face_triggered = True
                        if not multi_face_triggered and self.check_gaze:
                            gaze_level = self._gaze.observe(self._check_gaze(landmarks[0]))
                            raw_look_away = gaze_level == "blur"
                            delayed_look_away = self._look_away.observe(raw_look_away, now_ts)
                            gaze_triggered = gaze_level == "blackout" or delayed_look_away
                        else:
                            self._look_away.reset()

                # Combine only explicit, configured threats. Presence loss is
                # independent of gaze and uses the same explicit blur flag.
                if device_triggered or multi_face_triggered or gaze_triggered or presence_lost_triggered:
                    is_clear = False
                    parts = []
                    if device_triggered:
                        parts.append("PHONE DETECTED")
                    if multi_face_triggered:
                        parts.append("MULTIPLE FACES")
                    if gaze_triggered:
                        parts.append("LOOK AWAY (FULL TURN)" if gaze_level == "blackout" else "LOOK AWAY")
                    if presence_lost_triggered:
                        parts.append("PRESENCE LOST")
                    reason = " & ".join(parts)

            except Exception as e:
                self._look_away.reset()
                presence_unknown = True
                log(f"Detection loop error: {e}")

            # Capture for each triggered reason (both device and multi-face when both detected)
            self._capture_if_needed(frame, device_triggered, self.capture_on_device, "device", "PHONE DETECTED", now_ts)
            self._capture_if_needed(frame, multi_face_triggered, self.capture_on_multi_face, "multi_face", "MULTIPLE FACES", now_ts)

            # Hysteresis Smoothing
            recovery_unknown = presence_unknown or raw_look_away
            if recovery_unknown and is_clear:
                # A partially recovered face, or a look-away that has not
                # yet reached its trigger delay, is not attentive recovery.
                # Actual independent threats still accrue distracted time.
                self._attentive_ms = 0.0
                self._distracted_ms = 0.0
            elif is_clear:
                self._attentive_ms += delta_ms
                self._distracted_ms = 0.0
            else:
                self._distracted_ms += delta_ms
                self._attentive_ms = 0.0
            
            # Transition Logic
            if recovery_unknown and is_clear:
                # Preserve a genuine alert state through a short unknown
                # camera interval, but never emit/blur solely for absence.
                pass
            elif self._is_locked:
                device_in_reason = "PHONE DETECTED" in self._lock_reason
                multi_in_reason = "MULTIPLE FACES" in self._lock_reason
                device_clear = self.wake_delay_ms * self.device_wake_multiplier if device_in_reason else 0
                multi_clear = self.wake_delay_ms * self.multi_face_wake_multiplier if multi_in_reason else 0
                required_clear_ms = max(device_clear, multi_clear, self.wake_delay_ms)

                # Every trigger uses the configured continuous recovery time.
                # Clearing a gaze/presence lock on one good frame caused the
                # overlay to blink when the next frame lost the face again.
                if is_clear and self._attentive_ms >= required_clear_ms:
                    self._is_locked = False
                    self._lock_reason = ""
                    self._captured_for_device = False
                    self._captured_for_multi_face = False
                    self._capture_ids.clear()
                    self.state_changed.emit(True, "", {})
                elif reason and reason != self._lock_reason:
                    # A new condition can arrive while a prior condition is
                    # still active (for example look-away followed by phone
                    # detection). Emit the new reason so its independent blur
                    # toggle and Fleet alert are evaluated immediately.
                    self._lock_reason = reason
                    self.state_changed.emit(False, reason, dict(self._capture_ids))
            else:
                buffer_ms = self.buffer_frames * 30.0
                if self._distracted_ms >= buffer_ms:
                    self._is_locked = True
                    self._lock_reason = reason
                    self.state_changed.emit(False, reason, dict(self._capture_ids))

            self._update_recordings(frame, device_triggered, multi_face_triggered, now_ts)

            time.sleep(0.03)

        for rec in self._active_recordings:
            self._finish_recording(rec)
        self._active_recordings.clear()
        cap.release()
        if self.face_detector: self.face_detector.close()
        if self.obj_detector: self.obj_detector.close()

# --- UI Overlay ---
class ShieldOverlay(QWidget):
    def __init__(self, screen, opacity=200):
        super().__init__()
        self.setWindowFlags(Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint | Qt.WindowType.Tool | Qt.WindowType.WindowTransparentForInput | Qt.WindowType.WindowDoesNotAcceptFocus)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        self.setObjectName("shieldOverlay")
        # One overlay per physical display is reliable on mixed-DPI and
        # negative-coordinate monitor layouts.
        self.setGeometry(screen.geometry())
        self.screen_source = screen
        self.presentation = "clear"
        self.blurred_screen = None
        
        # Message Label
        self.label = QLabel(self)
        self.label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.label.setStyleSheet("color: rgba(255, 50, 50, 200); background: transparent; font-weight: bold; font-family: Consolas; font-size: 48px;")
        self.label.setWordWrap(True)
        self.label.hide()
        
        layout = QVBoxLayout(self)
        layout.addWidget(self.label)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        
        self.opacity_val = opacity

    def prepare_protection(self):
        # Keep only a blurred in-memory snapshot while protected. Native acrylic
        # becomes an opaque tint when Windows transparency is disabled.
        self.blurred_screen = None
        try:
            snapshot = self.screen_source.grabWindow(0).toImage().convertToFormat(QImage.Format.Format_RGBA8888)
            if snapshot.isNull():
                return
            snapshot = snapshot.scaled(max(1, snapshot.width() // 4), max(1, snapshot.height() // 4),
                                       Qt.AspectRatioMode.IgnoreAspectRatio, Qt.TransformationMode.SmoothTransformation)
            bits = snapshot.bits()
            bits.setsize(snapshot.sizeInBytes())
            pixels = np.frombuffer(bits, dtype=np.uint8).reshape(snapshot.height(), snapshot.bytesPerLine())
            rgba = pixels[:, :snapshot.width() * 4].reshape(snapshot.height(), snapshot.width(), 4)
            blurred = cv2.GaussianBlur(rgba, (0, 0), 8)
            image = QImage(blurred.data, snapshot.width(), snapshot.height(), blurred.strides[0], QImage.Format.Format_RGBA8888).copy()
            self.blurred_screen = QPixmap.fromImage(image)
        except Exception:
            log("Screen blur snapshot unavailable; using protective blackout")

    def paintEvent(self, event):
        painter = QPainter(self)
        if self.presentation == "blur" and self.blurred_screen is not None:
            painter.drawPixmap(self.rect(), self.blurred_screen)
            painter.fillRect(self.rect(), QColor(0, 0, 0, min(80, max(0, self.opacity_val // 4))))
        else:
            painter.fillRect(self.rect(), QColor(0, 0, 0))

    def update_state(self, is_clear, reason):
        self.presentation = overlay_presentation(is_clear, reason)
        if is_clear:
            self.hide()
            self.label.hide()
            self.blurred_screen = None
        else:
            if self.presentation == "blackout" or self.blurred_screen is None:
                self.label.setText(reason.replace(" (FULL TURN)", ""))
                self.label.show()
            else:
                self.label.hide()
            self.show()
            self.update()

class ShieldOverlays:
    def __init__(self, app, opacity=200):
        self.overlays = [ShieldOverlay(screen, opacity) for screen in app.screens()]

    def update_state(self, is_clear, reason):
        if not is_clear:
            # Capture every display before any overlay is shown, and never
            # recapture our own blackout during a full-to-partial transition.
            for overlay in self.overlays:
                if overlay.presentation == "clear":
                    overlay.prepare_protection()
        for overlay in self.overlays:
            overlay.update_state(is_clear, reason)

# --- Main App ---
class ShieldApp(QObject):
    def __init__(self):
        super().__init__()
        self.app = QApplication(sys.argv)
        set_process_dpi_aware()
        
        parser = argparse.ArgumentParser()
        parser.add_argument('--camera', type=int, default=0)
        parser.add_argument('--check-gaze', action='store_true')
        parser.add_argument('--detector-options-explicit', action='store_true')
        parser.add_argument('--check-faces', action='store_true')
        parser.add_argument('--check-phone', action='store_true')
        parser.add_argument('--blur-gaze', action='store_true')
        parser.add_argument('--blur-faces', action='store_true')
        parser.add_argument('--blur-phone', action='store_true')
        parser.add_argument('--capture-on-device', action='store_true')
        parser.add_argument('--capture-on-multi-face', action='store_true')
        parser.add_argument('--fleet-video-only', action='store_true')
        
        # Granular Controls
        parser.add_argument('--model-level', type=str, default='nano', help='nano, small, medium, large')
        parser.add_argument('--confidence', type=float, default=0.5)
        parser.add_argument('--overlay-opacity', type=int, default=200)
        parser.add_argument('--wake-delay-ms', type=int, default=150)
        parser.add_argument('--look-away-delay-seconds', type=int, default=0, choices=range(0, 301))
        parser.add_argument('--device-wake-multiplier', type=int, default=2)
        parser.add_argument('--multi-face-wake-multiplier', type=int, default=2)
        parser.add_argument('--buffer-frames', type=int, default=6)
        parser.add_argument('--capture-speed', type=int, default=1, help='Video playback speed: 1=real-time, 2=2x, 3=3x, 4=4x')
        # Supplied only by WinCommander Free, never the transient PS wrapper.
        parser.add_argument('--owner-pid', type=int, default=0)
        # Ignored for now but kept for API compat if needed
        parser.add_argument('--mode', type=str, default='')
        self.args, _ = parser.parse_known_args()

        self._owner_watchdog = None
        if self.args.owner_pid and not owner_process_is_alive(self.args.owner_pid):
            log("Shield owner is unavailable; refusing unmanaged start")
            clear_pid_marker()
            raise RuntimeError("WinCommander owner process is unavailable")

        write_pid_marker()

        log(f"Shield starting: camera={self.args.camera}, gaze={self.args.check_gaze}, faces={self.args.check_faces}, phone={self.args.check_phone}, model={self.args.model_level}, wake_delay_ms={self.args.wake_delay_ms}, device_multiplier={self.args.device_wake_multiplier}, multi_face_multiplier={self.args.multi_face_wake_multiplier}, buffer_frames={self.args.buffer_frames}")

        # L9: retention purge — captures older than 7d are deleted at session start
        try:
            if os.path.isdir(CAPTURES_DIR):
                _now = time.time()
                for _fname in os.listdir(CAPTURES_DIR):
                    _fpath = os.path.join(CAPTURES_DIR, _fname)
                    try:
                        if os.path.isfile(_fpath) and (_now - os.stat(_fpath).st_mtime) > 7 * 86400:
                            os.remove(_fpath)
                    except Exception:
                        pass
        except Exception:
            pass

        # Defaults if no flags set (default to basic eye tracker behavior)
        if not self.args.detector_options_explicit and not (self.args.check_gaze or self.args.check_faces or self.args.check_phone):
            self.args.check_gaze = True

        # Fresh event sidecar per session so the Rust reader never replays
        # stale look-away/look-back transitions from a previous run.
        try:
            with open(EVENTS_FILE, "w", encoding="utf-8") as _ef:
                _ef.write("")
        except Exception:
            pass

        self.overlay = ShieldOverlays(self.app, opacity=self.args.overlay_opacity)
        # Pass new configurations
        self.worker = ShieldWorker(
            camera_idx=self.args.camera, 
            check_gaze=self.args.check_gaze, 
            check_faces=self.args.check_faces, 
            check_phone=self.args.check_phone,
            capture_on_device=self.args.capture_on_device,
            capture_on_multi_face=self.args.capture_on_multi_face,
            fleet_video_only=self.args.fleet_video_only,
            model_level=self.args.model_level,
            confidence=self.args.confidence,
            wake_delay_ms=self.args.wake_delay_ms,
            look_away_delay_seconds=self.args.look_away_delay_seconds,
            device_wake_multiplier=self.args.device_wake_multiplier,
            multi_face_wake_multiplier=self.args.multi_face_wake_multiplier,
            buffer_frames=self.args.buffer_frames,
            capture_speed=max(1, min(4, self.args.capture_speed))
        )
        self.worker.state_changed.connect(self._handle_state)
        self.worker.status_msg.connect(lambda msg: log(f"Status: {msg}"))
        self.worker.init_failed.connect(self.handle_init_fail)
        self.worker.finished.connect(self.app.quit)
        self.worker.start()
        self._stop_watchdog = QTimer(self)
        self._stop_watchdog.setInterval(250)
        self._stop_watchdog.timeout.connect(self._stop_when_requested)
        self._stop_watchdog.start()
        self.app.aboutToQuit.connect(clear_pid_marker)
        if self.args.owner_pid:
            self._owner_watchdog = QTimer(self)
            self._owner_watchdog.setInterval(1000)
            self._owner_watchdog.timeout.connect(self._stop_when_owner_exits)
            self._owner_watchdog.start()
        log("Shield worker started")

    def _stop_when_owner_exits(self):
        if owner_process_is_alive(self.args.owner_pid):
            return
        log("Shield owner exited; stopping managed detector")
        clear_pid_marker()
        self.worker._running = False
        self.overlay.update_state(True, "")

    def _stop_when_requested(self):
        try:
            if not os.path.isfile(STOP_FILE) or os.path.getsize(STOP_FILE) > 128:
                return
            with open(STOP_FILE, encoding="utf-8-sig") as handle:
                request = json.load(handle)
            if request.get("pid") != os.getpid():
                return
        except (OSError, ValueError, AttributeError):
            return
        self._stop_watchdog.stop()
        self.worker._running = False
        self.overlay.update_state(True, "")

    def _handle_state(self, is_clear, reason, capture_ids):
        # Fleet/flow receives every transition. The local blur toggles only
        # control visual enforcement; an unchecked toggle never silences an
        # attention event.
        self._emit_look_event(is_clear, reason, capture_ids)
        # `is_clear` means the user is attentive. It must clear the overlay,
        # not satisfy a blur condition; otherwise every successful camera
        # startup immediately blacks out the screen.
        should_blur = (not is_clear) and (
            ((("LOOK AWAY" in reason) or ("PRESENCE LOST" in reason)) and self.args.blur_gaze)
            or (("MULTIPLE FACES" in reason) and self.args.blur_faces)
            or (("PHONE DETECTED" in reason) and self.args.blur_phone)
        )
        self.overlay.update_state(is_clear or not should_blur, reason)

    def _emit_look_event(self, is_clear, reason, capture_ids):
        # Qt look-state change -> NDJSON sidecar. `is_clear` True means the
        # user is present/attentive (look_back); False means the overlay
        # engaged (look_away), carrying the trigger reason for context.
        ids = {}
        if not is_clear and self.args.fleet_video_only:
            if "PHONE DETECTED" in reason and "device" in capture_ids:
                ids["phone_detected"] = capture_ids["device"]
            if "MULTIPLE FACES" in reason and "multi_face" in capture_ids:
                ids["multiple_faces"] = capture_ids["multi_face"]
        emit_event("look_back" if is_clear else "look_away", "" if is_clear else reason, ids)

    def handle_init_fail(self, msg):
        log(f"Shield init failed: {msg}")
        clear_pid_marker()
        self.app.quit()
        sys.exit(1)
        
    def run(self):
        sys.exit(self.app.exec())

if __name__ == "__main__":
    try:
        ShieldApp().run()
    except Exception:
        log(traceback.format_exc())
'@
        # Secure cleanup of legacy plaintext copies from previous builds.
        if ($env:LOCALAPPDATA) {
            Remove-ItemSecure -Path (Join-Path $env:LOCALAPPDATA "WinCommander\privacy_shield.py") -Force -ErrorAction SilentlyContinue
        }
        if ($env:APPDATA) {
            Remove-ItemSecure -Path (Join-Path $env:APPDATA "WinCommander\privacy_shield.py") -Force -ErrorAction SilentlyContinue
        }

        # The Python process is intentionally detached from this short-lived
        # PowerShell wrapper. Carry Free's identity into it instead so it can
        # stop if Free is ended, even when Windows cannot assign its Job.
        $ownerPid = 0
        if ($env:WINCMD_SHIELD_OWNER_PID -match '^[1-9][0-9]*$') {
            $ownerPid = [int]$env:WINCMD_SHIELD_OWNER_PID
        }

        $runId = [guid]::NewGuid().ToString('N')
        $pythonArgs = @(
            "-",
            "$shieldProcessMarker",
            "--run-id", "$runId",
            "--camera", "$Camera",
            "--model-level", "$ModelLevel",
            "--confidence", "$Confidence",
            "--overlay-opacity", "$OverlayOpacity",
            "--wake-delay-ms", "$WakeDelayMs",
            "--look-away-delay-seconds", "$LookAwayDelaySeconds",
            "--device-wake-multiplier", "$DeviceWakeMultiplier",
            "--multi-face-wake-multiplier", "$MultiFaceWakeMultiplier",
            "--buffer-frames", "$BufferFrames",
            "--capture-speed", "$CaptureSpeed"
        )
        if ($CheckGaze) { $pythonArgs += "--check-gaze" }
        if ($CheckFaces) { $pythonArgs += "--check-faces" }
        if ($CheckPhone) { $pythonArgs += "--check-phone" }
        if ($PSBoundParameters.ContainsKey('CheckGaze') -or $PSBoundParameters.ContainsKey('CheckFaces') -or $PSBoundParameters.ContainsKey('CheckPhone')) {
            $pythonArgs += "--detector-options-explicit"
        }
        if ($BlurGaze) { $pythonArgs += "--blur-gaze" }
        if ($BlurFaces) { $pythonArgs += "--blur-faces" }
        if ($BlurPhone) { $pythonArgs += "--blur-phone" }
        if ($CaptureOnDevice) { $pythonArgs += "--capture-on-device" }
        if ($CaptureOnMultiFace) { $pythonArgs += "--capture-on-multi-face" }
        if ($FleetVideoOnly) { $pythonArgs += "--fleet-video-only" }
        if ($Mode) { $pythonArgs += "--mode"; $pythonArgs += $Mode }
        if ($ownerPid -gt 0) { $pythonArgs += "--owner-pid"; $pythonArgs += "$ownerPid" }

        # Use resolved python executable (must be python.exe, NOT pythonw.exe)
        # pythonw.exe often blocks OpenCV/Media Foundation from connecting to the camera.
        $pythonCmd = $pythonExe

        $startInfo = New-Object System.Diagnostics.ProcessStartInfo
        $startInfo.EnvironmentVariables['WINCMD_SHIELD_RUN_ID'] = $runId
        $startInfo.FileName = $pythonCmd
        # Correlate failures with this launch; earlier log lines can describe
        # a different camera, runtime, or detector attempt.
        $launchId = [guid]::NewGuid().ToString('N')
        $startInfo.EnvironmentVariables['WINCMD_SHIELD_LAUNCH_ID'] = $launchId
        $startInfo.Arguments = $pythonArgs -join " "
        $startInfo.UseShellExecute = $false
        $startInfo.CreateNoWindow = $true
        $startInfo.RedirectStandardInput = $true
        # The detector is deliberately long-lived.  Do not let it inherit this
        # one-shot command runner's stdout/stderr handles: Rust waits for the
        # runner's output EOF, and inherited handles kept a Fleet Start
        # lifecycle stuck at "applying" until the detector was stopped.
        # Drain natively: PowerShell scriptblock event handlers run without a
        # runspace on callback threads and can terminate the launcher.
        $startInfo.RedirectStandardOutput = $true
        $startInfo.RedirectStandardError = $true
        
        $process = [System.Diagnostics.Process]::Start($startInfo)
        Write-PrivacyShieldDebugStage 'launch'
        if ($process -and -not $process.HasExited) {
            $null = $process.StandardOutput.BaseStream.CopyToAsync([IO.Stream]::Null)
            $null = $process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
            $process.StandardInput.Write($embeddedScript)
            $process.StandardInput.Close()
        }
        $logRoot = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:TEMP }
        $logPath = if ($logRoot) { Join-Path $logRoot "WinCommander\logs\privacy_shield.log" } else { $null }
        if (-not (Wait-PrivacyShieldReady -Process $process -LogPath $logPath -RunId $runId)) {
            # Keep this Process handle while killing: a reused PID must never
            # terminate unrelated work, nor may a timed-out launch run later.
            if (-not $process.HasExited) {
                $process.Kill()
                [void]$process.WaitForExit(5000)
            }
            $logTail = Get-PrivacyShieldLaunchLogTail -Path $logPath -RunId $runId
            $failure = Get-PrivacyShieldStartFailure -LogTail $logTail
            $cameraStatus = if ($failure.code -in @('camera_busy', 'camera_feed_unavailable', 'hardware_unavailable')) {
                $failure.code
            } elseif ($failure.code -eq 'camera_start_unverified') {
                'unknown'
            } else {
                # A dependency/model failure does not resolve an unknown preflight.
                if ($status.cameraStatus) { $status.cameraStatus } else { 'unknown' }
            }
            return @{ error = $true; message = $failure.message; cameraStatus = $cameraStatus; shieldLifecycle = 'stopped'; diagnosticCode = $failure.code }
        }

        Write-PrivacyShieldDebugStage 'ready'
        $startTicks = $null
        try { $startTicks = $process.StartTime.ToUniversalTime().Ticks.ToString() } catch {}
        @{ success = $true; processId = $process.Id; processStartTicks = $startTicks; cameraStatus = 'available'; shieldLifecycle = 'running'; ready = $true; activeMode = if ($Mode) { $Mode } elseif ($BlurGaze -or $BlurFaces -or $BlurPhone) { 'blur_notify' } else { 'notify_only' } }
    }
    catch {
        @{ error = $true; message = 'Privacy Shield could not start.'; cameraStatus = 'unknown'; shieldLifecycle = 'unknown'; diagnosticCode = 'shield_start_failed' }
    }
}

function Request-PrivacyShieldGracefulStop {
    param([int]$DetectorPid)
    $path = (Get-PrivacyShieldPidMarkerPath) + ".$DetectorPid.stop"
    $temporary = $path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    try {
        [IO.File]::WriteAllText($temporary, (@{ pid = $DetectorPid } | ConvertTo-Json -Compress))
        if (Test-Path -LiteralPath $path) { [IO.File]::Replace($temporary, $path, $null) }
        else { [IO.File]::Move($temporary, $path) }
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue }
    }
}

function Stop-PrivacyShield {
    $detector = $null
    try {
        $status = Get-PrivacyShieldStatus -SkipCameraProbe
        if ($status -isnot [System.Collections.IDictionary] -or $status['error'] -eq $true -or $status['running'] -isnot [bool]) {
            return @{ error = $true; message = 'Privacy Shield status could not be verified.' }
        }
        if ($status['running'] -eq $true -or $status['detectorProcessRunning'] -eq $true) {
            try {
                $detector = Get-Process -Id $status['processId'] -ErrorAction Stop
                # The PID can exit between the status read and opening its
                # handle. A fresh stopped read is success; an unreadable or
                # still-running process remains an error.
                $null = $detector.Handle
            } catch {
                $afterExit = Get-PrivacyShieldStatus -SkipCameraProbe
                if ($afterExit -is [System.Collections.IDictionary] -and $afterExit['error'] -ne $true -and
                    $afterExit['running'] -eq $false -and $afterExit['detectorProcessRunning'] -ne $true) {
                    return @{ success = $true; message = 'Stopped.'; running = $false; shieldLifecycle = 'stopped' }
                }
                return @{ error = $true; message = 'Privacy Shield process could not be opened safely.' }
            }
            # Hold the kernel object across validation/request/termination so
            # a recycled numeric PID cannot target an unrelated process.
            $verified = Get-CimInstance -ClassName Win32_Process -Filter ("ProcessId=" + $status['processId']) -ErrorAction Stop
            if (-not $verified -or $verified.SessionId -ne [System.Diagnostics.Process]::GetCurrentProcess().SessionId -or
                $verified.Name -notin @('python.exe', 'pythonw.exe') -or
                $verified.CommandLine -notmatch '(?:^|\s)--wc-privacy-shield(?=\s|$)') {
                return @{ error = $true; message = 'Privacy Shield process identity changed before stop.' }
            }
            try { Request-PrivacyShieldGracefulStop -DetectorPid $status['processId'] } catch {}
            if (-not $detector.WaitForExit(2000)) {
                try { $detector.Kill() } catch {
                    # Kill may race with a graceful exit. Verify the resulting
                    # state below instead of reporting a failed Stop.
                }
                if (-not $detector.WaitForExit(5000)) {
                    return @{ error = $true; message = 'Privacy Shield stop could not be confirmed.' }
                }
            }
            # CIM and the marker can lag the process exit briefly. Never claim
            # stopped until a fresh status sees no detector in this session.
            $confirmed = $false
            for ($attempt = 0; $attempt -lt 5; $attempt++) {
                $after = Get-PrivacyShieldStatus -SkipCameraProbe
                if ($after -is [System.Collections.IDictionary] -and $after['error'] -ne $true -and
                    $after['running'] -eq $false -and $after['detectorProcessRunning'] -ne $true) {
                    $confirmed = $true
                    break
                }
                if ($attempt -lt 4) { Start-Sleep -Milliseconds 150 }
            }
            if (-not $confirmed) { return @{ error = $true; message = 'Privacy Shield stop could not be confirmed.' } }
        }
        # Status removes stale markers itself. Do not remove a marker for a
        # detector still running in another session of the same profile.
        return @{ success = $true; message = "Stopped."; running = $false; shieldLifecycle = 'stopped' }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
    finally {
        if ($detector) { $detector.Dispose() }
    }
}
