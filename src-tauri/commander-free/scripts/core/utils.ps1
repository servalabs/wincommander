# ============================================================================
# CORE UTILITIES
# Shared functions used across all modules
# ============================================================================

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Assert-IsAdmin {
    # KT: Must NOT emit anything to the pipeline on success — bare callers like
    # `Assert-IsAdmin` (used at the top of nearly every module function) would
    # otherwise leak `$true` into the function's pipeline output, and the
    # router serializes everything emitted as `[true, {...}]`. Frontend then
    # sees `data` as an array and `data.totals` / `data.rows` come back
    # undefined. Throw on failure (router's outer catch turns it into the
    # standard error envelope), return void on success.
    if (-not (Test-IsAdmin)) {
        throw "Administrator privileges required."
    }
}

# Registry-write helper used across modules. Previously lived in
# privacy/telemetry.ps1 — but tweaks/system.ps1, tweaks/security.ps1
# and privacy/cleanup.ps1 also call it. Since modules are loaded one
# at a time per command, callers from those other modules saw
#   "The term 'Set-RegistryValueSafe' is not recognized..."
# (e.g. Enable-SvcHostSplit / Disable-SvcHostSplit). Defining it in
# core/utils.ps1 (which is always loaded) fixes the breakage for every
# call site without touching the callers themselves.
function Set-RegistryValueSafe {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][AllowNull()]$Value,
        [ValidateSet("DWord", "String", "QWord", "Binary", "MultiString", "ExpandString")]
        [string]$Type = "DWord"
    )
    if (!(Test-Path $Path)) { New-Item -Path $Path -Force | Out-Null }
    Set-ItemProperty -Path $Path -Name $Name -Value $Value -Type $Type -Force
}

function Get-ParentProcessPath {
    try {
        $parentPid = (Get-CimInstance Win32_Process -Filter "ProcessId = $PID").ParentProcessId
        return (Get-Process -Id $parentPid).MainModule.FileName
    }
    catch {
        return $null
    }
}

function ConvertTo-JsonOutput {
    param([object]$InputObject)
    
    if ($null -eq $InputObject) {
        return "{}"
    }
    
    try {
        $json = $InputObject | ConvertTo-Json -Depth 10 -Compress
        Write-Output $json
    }
    catch {
        $fallback = @{
            error   = $true
            message = "JSON conversion failed: $($_.Exception.Message)"
            raw     = $InputObject.ToString()
        }
        Write-Output ($fallback | ConvertTo-Json -Compress)
    }
}

function Split-ListParam {
    param([object]$Value)
    if ($null -eq $Value) { return @() }
    if ($Value -is [System.Array]) { return $Value }
    $stringValue = [string]$Value
    if ([string]::IsNullOrWhiteSpace($stringValue)) { return @() }
    return $stringValue -split '\s*,\s*' | Where-Object { $_ -ne '' }
}

# Everything's Winget package installs the GUI/service only.  The search UI
# talks to the publisher's separate ES command-line executable, which has no
# Winget package.  Keep it in WinCommander's shared binary folder rather than
# pretending `Voidtools.Everything.Cli` exists.
function Install-EverythingSearchCli {
    $existing = Join-Path $env:ProgramData 'WinCommander\bin\es.exe'
    if (Test-Path -LiteralPath $existing -PathType Leaf) { return $existing }

    $arch = switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) {
        'Arm64' { 'ARM64' }
        'X86'   { 'x86' }
        default { 'x64' }
    }
    $version = '1.1.0.37'
    $uri = "https://www.voidtools.com/ES-$version.$arch.zip"
    $scratch = Join-Path ([System.IO.Path]::GetTempPath()) "WinCommander-ES-$PID-$([Guid]::NewGuid().ToString('N'))"
    $archive = "$scratch.zip"
    $targetDir = Split-Path -Parent $existing

    try {
        New-Item -ItemType Directory -Path $targetDir -Force -ErrorAction Stop | Out-Null
        Invoke-WebRequest -Uri $uri -OutFile $archive -UseBasicParsing -ErrorAction Stop
        Expand-Archive -LiteralPath $archive -DestinationPath $scratch -Force -ErrorAction Stop
        $cli = Get-ChildItem -LiteralPath $scratch -Filter 'es.exe' -File -Recurse -ErrorAction Stop | Select-Object -First 1
        if (-not $cli) { throw 'The official Everything CLI archive did not contain es.exe.' }
        Copy-Item -LiteralPath $cli.FullName -Destination $existing -Force -ErrorAction Stop
        if (-not (Test-Path -LiteralPath $existing -PathType Leaf)) { throw 'Everything CLI installation did not complete.' }
        return $existing
    }
    finally {
        Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# ── Local application-icon resolution ─────────────────────────────────────
# Debloat must show the icon Windows actually associates with an installed
# package/program, without using a network icon service.  These helpers are in
# core because the AppX and BCU inventory modules run independently.
function Get-WcIconCachePath {
    param([Parameter(Mandatory = $true)][string]$Key)
    $base = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { [System.IO.Path]::GetTempPath() }
    $dir = Join-Path $base 'WinCommander\icon-cache'
    New-Item -ItemType Directory -Force -Path $dir -ErrorAction SilentlyContinue | Out-Null
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($Key)
        $digest = ($sha.ComputeHash($bytes) | ForEach-Object { $_.ToString('x2') }) -join ''
        return (Join-Path $dir "$digest.png")
    }
    finally {
        $sha.Dispose()
    }
}

function Get-WcImageDataUri {
    param([Parameter(Mandatory = $true)][string]$ImagePath)
    try {
        if (-not (Test-Path -LiteralPath $ImagePath -PathType Leaf -ErrorAction SilentlyContinue)) { return $null }
        $mime = switch ([System.IO.Path]::GetExtension($ImagePath).ToLowerInvariant()) {
            '.png'  { 'image/png' }
            '.jpg'  { 'image/jpeg' }
            '.jpeg' { 'image/jpeg' }
            '.gif'  { 'image/gif' }
            '.webp' { 'image/webp' }
            default { $null }
        }
        if (-not $mime) { return $null }
        $bytes = [System.IO.File]::ReadAllBytes($ImagePath)
        return "data:$mime;base64,$([Convert]::ToBase64String($bytes))"
    }
    catch { return $null }
}

function Get-WcExecutableIconData {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath,
        [string]$CacheKey = $SourcePath
    )
    try {
        $path = [Environment]::ExpandEnvironmentVariables($SourcePath.Trim())
        if ($path.StartsWith([string][char]34)) {
            $end = $path.IndexOf([char]34, 1)
            if ($end -gt 1) { $path = $path.Substring(1, $end - 1) }
        }
        else {
            $path = $path -replace ',\s*-?\d+\s*$', ''
        }
        $path = $path.Trim([char]34, [char]32)
        if (-not (Test-Path -LiteralPath $path -PathType Leaf -ErrorAction SilentlyContinue)) { return $null }

        # Some programs register a PNG directly; preserve that exact source.
        $image = Get-WcImageDataUri -ImagePath $path
        if ($image) { return $image }

        $cache = Get-WcIconCachePath -Key $CacheKey
        if (Test-Path -LiteralPath $cache -PathType Leaf -ErrorAction SilentlyContinue) {
            return Get-WcImageDataUri -ImagePath $cache
        }

        Add-Type -AssemblyName System.Drawing -ErrorAction SilentlyContinue
        $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($path)
        if ($null -eq $icon -and [System.IO.Path]::GetExtension($path).ToLowerInvariant() -eq '.ico') {
            $icon = New-Object System.Drawing.Icon($path)
        }
        if ($null -eq $icon) { return $null }
        try {
            $bitmap = $icon.ToBitmap()
            try { $bitmap.Save($cache, [System.Drawing.Imaging.ImageFormat]::Png) }
            finally { $bitmap.Dispose() }
        }
        finally { $icon.Dispose() }
        return Get-WcImageDataUri -ImagePath $cache
    }
    catch { return $null }
}

function Get-WcAppxIconData {
    param([Parameter(Mandatory = $true)]$Package)
    try {
        $installLocation = [string]$Package.InstallLocation
        if ([string]::IsNullOrWhiteSpace($installLocation) -or -not (Test-Path -LiteralPath $installLocation -PathType Container -ErrorAction SilentlyContinue)) { return $null }
        $manifestPath = Join-Path $installLocation 'AppxManifest.xml'
        if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf -ErrorAction SilentlyContinue)) { return $null }
        [xml]$manifest = Get-Content -LiteralPath $manifestPath -Raw -ErrorAction Stop
        $visual = $manifest.SelectSingleNode("/*[local-name()='Package']/*[local-name()='Applications']/*[local-name()='Application']/*[local-name()='VisualElements']")
        if (-not $visual) { return $null }
        $logo = @('Square44x44Logo', 'Square150x150Logo', 'Logo') |
            ForEach-Object { $visual.GetAttribute($_) } |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            Select-Object -First 1
        if (-not $logo) { return $null }

        $relative = $logo -replace '/', '\\'
        $direct = Join-Path $installLocation $relative.TrimStart('\\')
        $selected = if (Test-Path -LiteralPath $direct -PathType Leaf -ErrorAction SilentlyContinue) { Get-Item -LiteralPath $direct } else { $null }
        if (-not $selected) {
            $dir = Split-Path -Parent $direct
            $stem = [System.IO.Path]::GetFileNameWithoutExtension($direct)
            if (Test-Path -LiteralPath $dir -PathType Container -ErrorAction SilentlyContinue) {
                $selected = Get-ChildItem -LiteralPath $dir -File -ErrorAction SilentlyContinue |
                    Where-Object { $_.BaseName -like "$stem*" -and $_.Extension -match '^\.(png|jpg|jpeg|gif|webp)$' } |
                    Sort-Object @{ Expression = { if ($_.Name -match 'scale-200') { 0 } elseif ($_.Name -match 'scale-150') { 1 } else { 2 } } }, Length -Descending |
                    Select-Object -First 1
            }
        }
        if (-not $selected) { return $null }
        return Get-WcImageDataUri -ImagePath $selected.FullName
    }
    catch { return $null }
}

function Resolve-DataPath {
    param([string]$RelativePath)
    
    # Build candidates from PSScriptRoot
    $scriptDir = $PSScriptRoot
    $candidates = @()

    # Direct path from script location (skip if $PSScriptRoot is empty — stdin execution)
    if (-not [string]::IsNullOrWhiteSpace($scriptDir)) {
        $candidates += (Join-Path $scriptDir $RelativePath)
    }
    
    # Walk up the directory tree (up to 5 levels)
    $currentParent = if ([string]::IsNullOrWhiteSpace($scriptDir)) { $null } else { $scriptDir }
    for ($i = 0; $i -lt 5; $i++) {
        if ([string]::IsNullOrWhiteSpace($currentParent)) { break }
        
        try {
            $parent = Split-Path -Parent $currentParent -ErrorAction Stop
            if (-not $parent) { break }
            $currentParent = $parent
        }
        catch { break }

        $candidates += (Join-Path $currentParent $RelativePath)
        $candidates += (Join-Path $currentParent "resources\$RelativePath")
        $candidates += (Join-Path $currentParent "bin\$RelativePath")
        $candidates += (Join-Path $currentParent "public\$RelativePath")
    }

    # Also check relative to the main process executable (Release build handling)
    try {
        $parentProc = Get-ParentProcessPath
        if ($parentProc) {
            $exeDir = Split-Path -Parent $parentProc
            $candidates += (Join-Path $exeDir $RelativePath)
            $candidates += (Join-Path $exeDir "resources\$RelativePath")
            $candidates += (Join-Path $exeDir "_up_\resources\$RelativePath")
        }
    }
    catch {}
    
    # Check each candidate
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate -ErrorAction SilentlyContinue) {
            return (Resolve-Path $candidate).Path
        }
    }
    
    return $null
}


function Restart-Explorer {
    # KT: Explorer is a per-user desktop shell. Killing it by image name also
    # kills every RDS user's taskbar, while a process started by an updater or
    # service cannot restore those other sessions. Refresh shell associations
    # instead; changes that require a new shell apply on the user's next logon.
    try {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class WinCommanderShellRefresh {
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(int wEventId, uint uFlags, IntPtr item1, IntPtr item2);
}
'@ -ErrorAction SilentlyContinue
        [WinCommanderShellRefresh]::SHChangeNotify(0x08000000, 0x1000, [IntPtr]::Zero, [IntPtr]::Zero)
        return @{ status = 'refresh_requested'; requiresSignOut = $true }
    }
    catch {
        return @{ status = 'refresh_deferred'; requiresSignOut = $true; warning = $_.Exception.Message }
    }
}

function Schedule-SSDOptimization {
    <#
    .SYNOPSIS
        Issues TRIM (ReTrim) on the specified drive, immediately or deferred via a Scheduled Task.
    .DESCRIPTION
        When -Immediate is specified (the interactive/lockdown erase path), runs
        Optimize-Volume -ReTrim synchronously on SSD/NVMe volumes so freed blocks
        are flushed to the controller before the session ends — closing the
        forensic window that existed with the old deferred approach.
        When -Immediate is not specified (legacy/background callers), falls back to
        the original deferred Scheduled Task at the next 30-minute boundary.
        Either path is a no-op for HDD/Unknown media types.
    #>
    param(
        [Parameter(Mandatory = $true)]
        [string]$DriveLetter,
        # KT: Immediate=$true = synchronous ReTrim now (interactive erase path).
        # Immediate=$false = deferred Scheduled Task (preserved for any callers
        # that explicitly want deferred behaviour; the auto-erase scheduled path
        # does not call this function at all, so its semantics are unaffected).
        [switch]$Immediate
    )

    $drive = $DriveLetter.TrimEnd(':').TrimEnd('\')

    # Media-type gate: only TRIM SSD/NVMe; skip HDD/Unknown (ReTrim is pointless there).
    $isSSD = $false
    try {
        $vol = Get-Volume -DriveLetter $drive -ErrorAction SilentlyContinue
        if ($vol) {
            $partition = Get-Partition -DriveLetter $drive -ErrorAction SilentlyContinue
            if ($partition) {
                $disk = Get-Disk -Number $partition.DiskNumber -ErrorAction SilentlyContinue
                if ($disk) {
                    $physDisk = Get-PhysicalDisk -ErrorAction SilentlyContinue |
                        Where-Object { $_.DeviceId -eq $disk.Number }
                    if ($physDisk -and $physDisk.MediaType -in @('SSD', 'NVMe')) {
                        $isSSD = $true
                    }
                }
            }
        }
    } catch {}
    if (-not $isSSD) { return }

    if ($Immediate) {
        # Synchronous ReTrim — blocks until the controller has processed the
        # hint, so freed blocks are unrecoverable before this call returns.
        try {
            Optimize-Volume -DriveLetter $drive -ReTrim -ErrorAction SilentlyContinue
        } catch {}
        return
    }

    # Deferred path (legacy behaviour): schedule at next 30-minute boundary.
    $taskName = "WinCommander_SSDOptimize_$drive"
    $now = Get-Date
    if ($now.Minute -lt 30) {
        $targetTime = $now.Date.AddHours($now.Hour).AddMinutes(30)
    } else {
        $targetTime = $now.Date.AddHours($now.Hour + 1)
    }

    $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-NoProfile -WindowStyle Hidden -Command `"Optimize-Volume -DriveLetter $drive -ReTrim`""
    $trigger = New-ScheduledTaskTrigger -Once -At ($targetTime.ToString("HH:mm"))
    $principal = New-ScheduledTaskPrincipal -UserId "NT AUTHORITY\SYSTEM" -LogonType ServiceAccount -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -DeleteExpiredTaskAfter (New-TimeSpan -Minutes 1)

    try {
        $existing = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
        if ($existing) { return }
        Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force -ErrorAction Stop | Out-Null
    } catch {}
}

function _WC-EnsureTokenPrivileges {
    # Admin tokens carry SeTakeOwnership / SeRestore / SeBackup but Windows
    # leaves them disabled. A plain Get-Acl / Set-Acl on an HKLM\SYSTEM key
    # that grants admins only ReadKey will fail with "unauthorized
    # operation" until these are turned on. Idempotent; the C# type is
    # cached and the enable calls are no-ops after the first invocation.
    # Duplicates the helper in privacy/telemetry.ps1 so privacy/cleanup
    # can take ownership without depending on the telemetry module being
    # loaded.
    if ($Script:WC_TOKEN_PRIVILEGES_ENABLED) { return }

    if (-not ('WC.TokenPriv' -as [type])) {
        Add-Type -ErrorAction Stop -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
namespace WC {
    public static class TokenPriv {
        [StructLayout(LayoutKind.Sequential, Pack = 1)]
        public struct TokPriv1Luid { public int Count; public long Luid; public int Attr; }
        const int SE_PRIVILEGE_ENABLED    = 0x00000002;
        const int TOKEN_QUERY             = 0x00000008;
        const int TOKEN_ADJUST_PRIVILEGES = 0x00000020;
        [DllImport("kernel32.dll", ExactSpelling = true)]
        static extern IntPtr GetCurrentProcess();
        [DllImport("advapi32.dll", ExactSpelling = true, SetLastError = true)]
        static extern bool OpenProcessToken(IntPtr h, int acc, ref IntPtr phtok);
        [DllImport("advapi32.dll", SetLastError = true)]
        static extern bool LookupPrivilegeValue(string host, string name, ref long pluid);
        [DllImport("advapi32.dll", ExactSpelling = true, SetLastError = true)]
        static extern bool AdjustTokenPrivileges(IntPtr htok, bool disall, ref TokPriv1Luid newst, int len, IntPtr prev, IntPtr relen);
        public static bool Enable(string privilege) {
            IntPtr htok = IntPtr.Zero;
            if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, ref htok)) return false;
            TokPriv1Luid tp = new TokPriv1Luid { Count = 1, Luid = 0, Attr = SE_PRIVILEGE_ENABLED };
            if (!LookupPrivilegeValue(null, privilege, ref tp.Luid)) return false;
            return AdjustTokenPrivileges(htok, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero);
        }
    }
}
"@
    }

    [WC.TokenPriv]::Enable('SeTakeOwnershipPrivilege') | Out-Null
    [WC.TokenPriv]::Enable('SeRestorePrivilege')       | Out-Null
    [WC.TokenPriv]::Enable('SeBackupPrivilege')        | Out-Null

    $Script:WC_TOKEN_PRIVILEGES_ENABLED = $true
}

function _WC-RemoveRegKeyOwnershipAware {
    # Secure-erase + delete a registry subtree, taking ownership where admins
    # only have ReadKey (e.g. HKLM\SYSTEM\...\bam\State\UserSettings\<SID>).
    # Fast path first — most keys don't need the ACL dance.
    # NOTE: does NOT handle PnP device-instance keys whose hidden Properties
    # subkey is kernel-protected (USBSTOR\*, USB\*, BTHENUM\*). Use
    # `pnputil /remove-device` for those.
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path $Path)) { return $true }

    try {
        Invoke-7Erase -Path $Path -Type Registry
        if (-not (Test-Path $Path)) { return $true }
    } catch {}

    _WC-EnsureTokenPrivileges
    $admins = New-Object System.Security.Principal.SecurityIdentifier 'S-1-5-32-544'

    # Walk the subtree, take ownership + grant FullControl on every key
    # (deepest-first so the root takeover doesn't get blocked by a child
    # the kernel re-locked while we were enumerating).
    $all = @()
    try {
        $all = @(Get-ChildItem -LiteralPath $Path -Recurse -ErrorAction SilentlyContinue | ForEach-Object { $_.PSPath })
    } catch {}
    $all += $Path
    $all = $all | Sort-Object -Property { ($_ -split '\\').Count } -Descending

    foreach ($p in $all) {
        try {
            $acl = Get-Acl -Path $p -ErrorAction Stop
            $acl.SetOwner($admins)
            Set-Acl -Path $p -AclObject $acl -ErrorAction Stop
            $aclGrant = Get-Acl -Path $p
            $rule = New-Object System.Security.AccessControl.RegistryAccessRule(
                $admins, 'FullControl',
                'ContainerInherit,ObjectInherit', 'None', 'Allow')
            $aclGrant.AddAccessRule($rule)
            Set-Acl -Path $p -AclObject $aclGrant -ErrorAction Stop
        } catch {}
    }

    try { Invoke-7Erase -Path $Path -Type Registry } catch {}
    return (-not (Test-Path $Path))
}

function Remove-ItemSecure {
    <#
    .SYNOPSIS
        A secure drop-in replacement for Remove-Item and Remove-ItemProperty that uses Invoke-7Erase.
    .DESCRIPTION
        This function identifies if the target is a file, folder, registry key, or registry property
        and routes it to the secure Invoke-7Erase function.
    #>
    param(
        [Parameter(Mandatory = $true, ValueFromPipeline = $true, ValueFromPipelineByPropertyName = $true)]
        [string[]]$Path,
        [string]$Name, # For registry properties
        [switch]$Recurse,
        [switch]$Force,
        [string]$Filter,
        [string]$Include,
        [string]$Exclude,
        [Parameter(ValueFromRemainingArguments=$true)]
        $RemainingArgs
    )

    foreach ($p in $Path) {
        if ($Name) {
            # Registry property erase
            Invoke-7Erase -Path $p -Type "RegistryProperty" -Name $Name
            continue
        }

        $item = Get-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue
        if (-not $item) { continue }

        $type = "File"
        if ($item.PSIsContainer) {
            if ($item.PSProvider.Name -eq "Registry") { $type = "Registry" }
            else { $type = "File" } # Invoke-7Erase handles directories under "File" type
        }
        elseif ($item.PSProvider.Name -eq "Registry") {
            $type = "Registry"
        }

        # Use original input path ($p) — $item.FullName is empty for registry keys
        Invoke-7Erase -Path $p -Type $type
    }
}

# ── Shred policy (script-scope) ────────────────────────────────────────────
# Set-ShredPolicy writes these; Invoke-7Erase reads them.
# Defaults: 1 pass, media-aware OFF (so multi-pass runs on SSD by default).
if ($null -eq $script:WC_SHRED_PASSES)        { $script:WC_SHRED_PASSES        = 1     }
if ($null -eq $script:WC_SHRED_MEDIA_AWARE)   { $script:WC_SHRED_MEDIA_AWARE   = $false }

function Set-ShredPolicy {
    <#
    .SYNOPSIS
        Persist the shred-pass count and media-aware flag for Invoke-7Erase.
    .DESCRIPTION
        Called at PS startup when shredPasses / shredMediaAwareEnabled are configured.
        Both values live as script-scoped vars so every subsequent Invoke-7Erase call
        in the same session honours the configured policy without extra args.
    .PARAMETER Passes
        Number of overwrite passes (1–7). Default 1.
    .PARAMETER MediaAware
        When $true, forces Passes=1 for SSD/NVMe targets (multi-pass is wear without gain).
    #>
    param(
        [ValidateRange(1, 7)]
        [int]$Passes = 1,
        [bool]$MediaAware = $false
    )
    $script:WC_SHRED_PASSES      = $Passes
    $script:WC_SHRED_MEDIA_AWARE = $MediaAware
    return @{ status = "ok"; passes = $Passes; mediaAware = $MediaAware }
}

function Invoke-7Erase {
    <#
    .SYNOPSIS
        Securely clears files, registry keys, or registry properties using configurable passes.
    .DESCRIPTION
        Secure-erase with a configurable RNG-overwrite pass count and MFT/directory-entry
        obfuscation. Default is a SINGLE durable RNG-overwrite pass (NIST SP 800-88: one
        pass clears conventional magnetic media; flash storage defeats extra passes via
        FTL indirection regardless of count) — not a multi-pass DoD 5220.22-M protocol.
        - Files: RNG overwrite (Passes count, default 1) + GUID rename before deletion
        - Directories: Recursive subdirectory renaming + file clearing + parent GUID rename
        - Registry: Native type-aware erasing using GetValueKind() for precision
        TRIM behaviour: at the outermost call boundary, each touched SSD/NVMe drive letter
        is ReTrimmed exactly once, synchronously, before the function returns.
        Pass count: defaults to $script:WC_SHRED_PASSES (set by Set-ShredPolicy; default 1,
        user-configurable up to 7 — extra passes buy nothing on flash and are wear only).
        Media-aware: when $script:WC_SHRED_MEDIA_AWARE is $true AND the target volume is
        SSD/NVMe, Passes is forced to 1 — multi-pass on flash is wear without forensic gain.
        NOTE: the -Passes parameter overrides the script-scope default for callers that
        need a one-off count (e.g. the registry property path below).
    #>
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $false)]
        [ValidateSet("File", "Registry", "RegistryProperty", "Folder")]
        [string]$Type = "File",
        [string]$Name, # Only for RegistryProperty
        [ValidateRange(1, 7)]
        [int]$Passes = $script:WC_SHRED_PASSES
    )

    # Track recursion depth so we issue TRIM exactly once per outermost erase
    # operation — not once per file in a directory walk.
    if ($null -eq $script:WC_7ERASE_DEPTH) { $script:WC_7ERASE_DEPTH = 0 }
    $isOutermost = ($script:WC_7ERASE_DEPTH -eq 0)
    if ($isOutermost) { $script:WC_TRIM_DRIVES = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase) }
    $script:WC_7ERASE_DEPTH++

    # Collect affected drive letters for synchronous TRIM at outermost return
    if ($Type -eq "File" -or $Type -eq "Folder") {
        $drive = (Split-Path -Path $Path -Qualifier)
        if ($drive) { [void]$script:WC_TRIM_DRIVES.Add($drive) }
    }

    # Media-aware gate: when enabled, cap passes to 1 on SSD/NVMe volumes.
    # Multi-pass overwrite on flash storage only wears the cells — the
    # controller's FTL means the old sectors may not be overwritten at all,
    # so additional passes give no forensic benefit while burning P/E cycles.
    if ($script:WC_SHRED_MEDIA_AWARE -and ($Type -eq "File" -or $Type -eq "Folder")) {
        $driveLetter = (Split-Path -Path $Path -Qualifier).TrimEnd(':').TrimEnd('\')
        if ($driveLetter) {
            try {
                $vol = Get-Volume -DriveLetter $driveLetter -ErrorAction SilentlyContinue
                if ($vol) {
                    $part = Get-Partition -DriveLetter $driveLetter -ErrorAction SilentlyContinue
                    if ($part) {
                        $disk = Get-Disk -Number $part.DiskNumber -ErrorAction SilentlyContinue
                        if ($disk) {
                            $phys = Get-PhysicalDisk -ErrorAction SilentlyContinue |
                                Where-Object { $_.DeviceId -eq $disk.Number }
                            if ($phys -and $phys.MediaType -in @('SSD', 'NVMe')) {
                                $Passes = 1
                            }
                        }
                    }
                }
            } catch {}
        }
    }

    # Wrap erase body so depth counter and TRIM flush run on every exit path
    # (return, throw, or fall-through).
    try {
        # Convert to provider path — use -LiteralPath so brackets/wildcards in
        # folder names are not expanded, causing silent early return with no deletion.
        $resolved = Resolve-Path -LiteralPath $Path -ErrorAction SilentlyContinue
        if (-not $resolved) { throw "Path not found: $Path" }
        $Path = $resolved.ProviderPath

        $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()

        if ($Type -eq "File") {
            $file = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
            if ($null -eq $file) { throw "Item not found: $Path" }

            if ($file.PSIsContainer) {
                # PHASE 1: Erase all files recursively
                Get-ChildItem -LiteralPath $Path -Recurse -Force | Where-Object { -not $_.PSIsContainer } | ForEach-Object {
                    Invoke-7Erase -Path $_.FullName -Type "File"
                }

                # PHASE 2: Obfuscate subdirectory names (deepest first to avoid path conflicts)
                Get-ChildItem -LiteralPath $Path -Recurse -Force -Directory |
                Sort-Object -Property FullName -Descending |
                ForEach-Object {
                    $guid = [Guid]::NewGuid().ToString()
                    try {
                        Rename-Item -LiteralPath $_.FullName -NewName $guid -Force -ErrorAction SilentlyContinue
                    }
                    catch {}
                }

                # PHASE 3: Obfuscate parent directory name and delete
                $guid = [Guid]::NewGuid().ToString()
                $parent = Split-Path $Path
                if (Rename-Item -LiteralPath $Path -NewName $guid -Force -ErrorAction SilentlyContinue -PassThru) {
                    $Path = Join-Path $parent $guid
                }

                # Release the file handles opened by the per-file wipe above
                # before attempting removal — otherwise the first Remove-Item
                # races the .NET finalizers and fails intermittently.
                [GC]::Collect(); [GC]::WaitForPendingFinalizers(); Start-Sleep -Milliseconds 150

                # Escalating retry loop: Remove-Item -> .NET Delete (extended
                # \\?\ path beats the 260-char MAX_PATH limit) -> cmd rd. Up to
                # 4 rounds absorbs transient Defender/Explorer locks. Only throw
                # if the folder is STILL on disk after every fallback, so the UI
                # never shows a green checkmark over an un-shredded folder.
                $clean = $Path.TrimEnd('\')
                $long  = if ($clean -like '\\?\*') { $clean } else { '\\?\' + $clean }
                $tries = 0
                while ((Test-Path -LiteralPath $Path) -and ($tries -lt 4)) {
                    $tries++
                    try { Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop } catch {}
                    if (Test-Path -LiteralPath $Path) { try { [System.IO.Directory]::Delete($long, $true) } catch {} }
                    if (Test-Path -LiteralPath $Path) { & cmd /c "rd /s /q `"$clean`"" *>$null }
                    if (Test-Path -LiteralPath $Path) { Start-Sleep -Milliseconds 250 }
                }
                if (Test-Path -LiteralPath $Path) {
                    throw "Could not delete folder '$Path' after $tries attempts (locked file, deny ACL, or in-use handle)."
                }
                return @{ status = "ok"; deleted = $true; type = "folder" }
            }

            $size = $file.Length
            if ($size -gt 0) {
                $filestream = $null
                try {
                    $filestream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
                    for ($i = 0; $i -lt $Passes; $i++) {
                        $buffer = New-Object byte[] 65536
                        $written = 0
                        while ($written -lt $size) {
                            $toWrite = [Math]::Min($buffer.Length, $size - $written)
                            $rng.GetBytes($buffer)
                            $filestream.Write($buffer, 0, $toWrite)
                            $written += $toWrite
                        }
                        $filestream.Position = 0
                        # Flush($true) = FlushFileBuffers so the overwrite bytes
                        # actually reach the disk before the GUID-rename + delete.
                        # A bare .Flush() only empties the .NET buffer to the OS
                        # cache, which may coalesce passes and never physically
                        # write — defeating the point of the overwrite.
                        $filestream.Flush($true)
                    }
                }
                catch {}
                finally {
                    if ($null -ne $filestream) {
                        $filestream.Close()
                        $filestream.Dispose()
                    }
                }
            }

            # Obfuscate MFT entry: Rename to GUID before deletion
            $randomName = [Guid]::NewGuid().ToString()
            $parent = Split-Path $Path
            if (Rename-Item -LiteralPath $Path -NewName $randomName -Force -ErrorAction SilentlyContinue -PassThru) {
                $Path = Join-Path $parent $randomName
            }
            try {
                Remove-Item -LiteralPath $Path -Force -ErrorAction Stop
            } catch {
                throw "Could not delete file '$Path': $($_.Exception.Message)"
            }
            return @{ status = "ok"; deleted = $true; type = "file" }
        }
        elseif ($Type -eq "Registry") {
            $item = Get-Item -LiteralPath $Path -ErrorAction SilentlyContinue
            if ($null -eq $item) { return }

            # Overwrite all values in this key
            foreach ($valName in $item.GetValueNames()) {
                Invoke-7Erase -Path $Path -Type "RegistryProperty" -Name $valName
            }

            # Recursively erase subkeys
            Get-ChildItem -LiteralPath $Path | ForEach-Object {
                Invoke-7Erase -Path $_.PSPath -Type "Registry"
            }

            Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue
        }
        elseif ($Type -eq "RegistryProperty") {
            if (-not $Name) { return }

            try {
                # Use native GetValueKind() for absolute type precision
                $regKey = $null
                try {
                    $regKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey(($Path -replace '^HKCU:\\', ''), $true)
                    if (-not $regKey) {
                        $regKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey(($Path -replace '^HKLM:\\', ''), $true)
                    }
                    if (-not $regKey) { return }

                    $valueKind = $regKey.GetValueKind($Name)

                    for ($i = 0; $i -lt $Passes; $i++) {
                        $randomValue = $null
                        switch ($valueKind) {
                            'DWord' {
                                $bytes = [Byte[]]::new(4)
                                $rng.GetBytes($bytes)
                                $randomValue = [BitConverter]::ToInt32($bytes, 0)
                            }
                            'QWord' {
                                $bytes = [Byte[]]::new(8)
                                $rng.GetBytes($bytes)
                                $randomValue = [BitConverter]::ToInt64($bytes, 0)
                            }
                            'Binary' {
                                $randomValue = New-Object byte[] 64
                                $rng.GetBytes($randomValue)
                            }
                            'MultiString' {
                                $randomValue = @([Guid]::NewGuid().ToString(), [Guid]::NewGuid().ToString())
                            }
                            Default {
                                # String, ExpandString, or Unknown
                                $randomValue = [Guid]::NewGuid().ToString()
                            }
                        }
                        Set-ItemProperty -LiteralPath $Path -Name $Name -Value $randomValue -ErrorAction SilentlyContinue
                    }
                }
                finally {
                    if ($regKey) { $regKey.Close() }
                }

                Remove-ItemProperty -LiteralPath $Path -Name $Name -Force -ErrorAction SilentlyContinue
            }
            catch {}
        }
    }
    finally {
        $script:WC_7ERASE_DEPTH--
        # At the outermost call boundary, synchronously TRIM every touched
        # SSD/NVMe drive exactly once — closing the forensic window.
        if ($isOutermost) {
            foreach ($d in @($script:WC_TRIM_DRIVES)) {
                Schedule-SSDOptimization -DriveLetter $d -Immediate
            }
            $script:WC_TRIM_DRIVES = $null
            $script:WC_7ERASE_DEPTH = $null
        }
    }
}
