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
function Assert-AutoEraseAncestors {
    param([string]$Path)
    $parent = [IO.Directory]::GetParent([IO.Path]::GetFullPath($Path))
    while ($parent) {
        $item = Get-Item -LiteralPath $parent.FullName -Force -ErrorAction Stop
        if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Reparse-point cleanup ancestors are not supported.'
        }
        $parent = $parent.Parent
    }
}
function Erase-OneFile($p) {
    try { $f = Get-Item -LiteralPath $p -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); return $false }
    catch { $script:AutoEraseFailed++; return $false }
    if (-not $f -or $f.PSIsContainer) { return $false }
    if ($f.Attributes -band [IO.FileAttributes]::ReparsePoint) { $script:AutoEraseFailed++; return $false }
    try { Assert-AutoEraseAncestors $p } catch { $script:AutoEraseFailed++; return $false }
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
        } catch { $script:AutoEraseFailed++; return $false } finally {
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
    try { $null = Get-Item -LiteralPath $target -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); $script:AutoEraseRemoved++; return $true }
    catch { $script:AutoEraseFailed++; return $false }
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
    try {
        $root = Get-Item -LiteralPath $d -Force -ErrorAction Stop
        if ($root.PSIsContainer -eq $false) { throw 'The cleanup directory changed type.' }
        if ($root.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse-point cleanup roots are not supported.' }
        Assert-AutoEraseAncestors $d
    }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); return }
    catch { $script:AutoEraseFailed++; return }
    try {
        $files = @(Get-AutoEraseFiles -Path $d -Recurse)
        $files | ForEach-Object { Erase-OneFile $_.FullName | Out-Null }
        # Only remove empty directories; a failed overwrite must never fall through to an ordinary recursive delete.
        Get-AutoEraseDirectories -Path $d -Recurse | Sort-Object { $_.FullName.Length } -Descending | ForEach-Object {
            if ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse-point cleanup directories are not supported.' }
            if (@(Get-ChildItem -LiteralPath $_.FullName -Force -ErrorAction Stop).Count -eq 0) { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction Stop }
        }
        if (@(Get-ChildItem -LiteralPath $d -Force -ErrorAction Stop).Count -eq 0) { Remove-Item -LiteralPath $d -Force -ErrorAction Stop }
    } catch { $script:AutoEraseFailed++ }
}
function Get-AutoEraseFiles {
    param([string]$Path, [string]$Filter = '*', [switch]$Recurse, [int]$Depth = -1)
    try { $root = Get-Item -LiteralPath $Path -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); return }
    if ($root.PSIsContainer -eq $false) { throw 'The cleanup directory changed type.' }
    if ($root.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse-point cleanup roots are not supported.' }
    Assert-AutoEraseAncestors $Path
    $pending = [Collections.Generic.Queue[object]]::new()
    $pending.Enqueue(@{ path=$Path; depth=0 })
    $files = [Collections.Generic.List[object]]::new()
    while ($pending.Count) {
        $current = $pending.Dequeue()
        foreach ($file in @(Get-ChildItem -LiteralPath $current.path -Filter $Filter -File -Force -ErrorAction Stop)) {
            if ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse-point cleanup files are not supported.' }
            $files.Add($file)
        }
        if ($Recurse -and ($Depth -lt 0 -or $current.depth -lt $Depth)) {
            foreach ($directory in @(Get-ChildItem -LiteralPath $current.path -Directory -Force -ErrorAction Stop)) {
                if ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse-point cleanup directories are not supported.' }
                $pending.Enqueue(@{ path=$directory.FullName; depth=($current.depth + 1) })
            }
        }
    }
    $files.ToArray()
}
function Get-AutoEraseDirectories {
    param([string]$Path, [string]$Filter = '*', [switch]$Recurse, [int]$Depth = -1)
    try { $root = Get-Item -LiteralPath $Path -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); return }
    if ($root.PSIsContainer -eq $false) { throw 'The cleanup directory changed type.' }
    if ($root.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse-point cleanup roots are not supported.' }
    Assert-AutoEraseAncestors $Path
    $pending = [Collections.Generic.Queue[object]]::new()
    $pending.Enqueue(@{ path=$Path; depth=0 })
    $directories = [Collections.Generic.List[object]]::new()
    while ($pending.Count) {
        $current = $pending.Dequeue()
        $arguments = @{ LiteralPath=$current.path; Directory=$true; Force=$true; ErrorAction='Stop' }
        if (-not $Recurse) { $arguments.Filter = $Filter }
        foreach ($directory in @(Get-ChildItem @arguments)) {
            if ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse-point cleanup directories are not supported.' }
            if ($directory.Name -like $Filter) { $directories.Add($directory) }
            if ($Recurse -and ($Depth -lt 0 -or $current.depth -lt $Depth)) { $pending.Enqueue(@{ path=$directory.FullName; depth=($current.depth + 1) }) }
        }
    }
    $directories.ToArray()
}
function Erase-FileSet {
    param([object[]]$Files)
    foreach ($file in @($Files | Sort-Object FullName -Unique)) {
        Erase-OneFile $file.FullName | Out-Null
    }
}
function Get-AutoEraseRegistryItem {
    param([string]$Path)
    try { Get-Item -LiteralPath $Path -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0) }
}
function Erase-RegistryTree {
    param([string]$Path)
    if ($null -ne (Get-AutoEraseRegistryItem $Path)) {
        Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop
        if ($null -ne (Get-AutoEraseRegistryItem $Path)) { throw 'Registry cleanup could not be verified.' }
        $script:AutoEraseRemoved++
    }
}
function Erase-RegistryValues {
    param([string]$Path, [string]$NamePattern = '*', [string[]]$ExcludedNames = @())
    $item = Get-AutoEraseRegistryItem $Path
    if ($null -eq $item) { return }
    foreach ($name in @($item.Property | Where-Object { $_ -like $NamePattern -and $_ -notin $ExcludedNames })) {
        Remove-ItemProperty -LiteralPath $Path -Name $name -Force -ErrorAction Stop
        $readback = Get-AutoEraseRegistryItem $Path
        if ($name -in @($readback.Property)) { throw 'Registry value cleanup could not be verified.' }
        $script:AutoEraseRemoved++
    }
}
function Invoke-AutoEraseWithServices {
    param([string[]]$Names, [scriptblock]$Body)
    $running = @(Get-Service -Name $Names -ErrorAction Stop | Where-Object Status -eq 'Running')
    try {
        foreach ($service in $running) { Stop-Service -Name $service.Name -Force -ErrorAction Stop }
        & $Body
    } finally {
        foreach ($service in $running) { Start-Service -Name $service.Name -ErrorAction Stop }
    }
}
'@

$script:WlanProfileFunctions = @'
function Initialize-WCWlanNative {
    if (([System.Management.Automation.PSTypeName]'WcCleanup.Wlan').Type) { return }
    Add-Type -ErrorAction Stop -ReferencedAssemblies 'System.Xml.dll' -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace WcCleanup {
    public sealed class WlanProfileEntry { public string InterfaceId; public string Name; public uint Flags; }
    public sealed class WlanSnapshot { public int InterfaceCount; public string[] Interfaces; public WlanProfileEntry[] Profiles; }
    public static class Wlan {
        [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
        private struct InterfaceInfo { public Guid InterfaceGuid; [MarshalAs(UnmanagedType.ByValTStr, SizeConst=256)] public string Description; public int State; }
        [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
        private struct ProfileInfo { [MarshalAs(UnmanagedType.ByValTStr, SizeConst=256)] public string Name; public uint Flags; }
        [DllImport("wlanapi.dll", ExactSpelling=true)] private static extern uint WlanOpenHandle(uint requested, IntPtr reserved, out uint negotiated, out IntPtr handle);
        [DllImport("wlanapi.dll", ExactSpelling=true)] private static extern uint WlanCloseHandle(IntPtr handle, IntPtr reserved);
        [DllImport("wlanapi.dll", ExactSpelling=true)] private static extern uint WlanEnumInterfaces(IntPtr handle, IntPtr reserved, out IntPtr list);
        [DllImport("wlanapi.dll", ExactSpelling=true)] private static extern uint WlanGetProfileList(IntPtr handle, ref Guid iface, IntPtr reserved, out IntPtr list);
        [DllImport("wlanapi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] private static extern uint WlanDeleteProfile(IntPtr handle, ref Guid iface, string name, IntPtr reserved);
        [DllImport("wlanapi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] private static extern uint WlanGetProfile(IntPtr handle, ref Guid iface, string name, IntPtr reserved, out IntPtr xml, ref uint flags, out uint access);
        [DllImport("wlanapi.dll", ExactSpelling=true)] private static extern void WlanFreeMemory(IntPtr memory);
        private static void Check(uint result) { if (result != 0) throw new Win32Exception((int)result); }
        private static IntPtr Open() { uint negotiated; IntPtr h; Check(WlanOpenHandle(2, IntPtr.Zero, out negotiated, out h)); return h; }
        public static WlanSnapshot Enumerate() {
            IntPtr h = Open(), interfaces = IntPtr.Zero;
            try {
                Check(WlanEnumInterfaces(h, IntPtr.Zero, out interfaces));
                if (interfaces == IntPtr.Zero) throw new InvalidOperationException("Wi-Fi adapter enumeration returned no result.");
                int count = Marshal.ReadInt32(interfaces);
                if (count <= 0 || count > 1024) throw new InvalidOperationException("Wi-Fi adapters are unavailable or invalid.");
                var profiles = new List<WlanProfileEntry>();
                var adapterIds = new List<string>();
                int ifaceSize = Marshal.SizeOf(typeof(InterfaceInfo));
                int profileSize = Marshal.SizeOf(typeof(ProfileInfo));
                for (int i = 0; i < count; i++) {
                    InterfaceInfo info = (InterfaceInfo)Marshal.PtrToStructure(IntPtr.Add(interfaces, checked(8 + i * ifaceSize)), typeof(InterfaceInfo));
                    adapterIds.Add(info.InterfaceGuid.ToString("D"));
                    IntPtr list = IntPtr.Zero;
                    try {
                        Guid id = info.InterfaceGuid;
                        Check(WlanGetProfileList(h, ref id, IntPtr.Zero, out list));
                        if (list == IntPtr.Zero) throw new InvalidOperationException("Wi-Fi profile enumeration returned no result.");
                        int n = Marshal.ReadInt32(list);
                        if (n < 0 || n > 100000) throw new InvalidOperationException("Wi-Fi profile count is invalid.");
                        for (int j = 0; j < n; j++) {
                            ProfileInfo profile = (ProfileInfo)Marshal.PtrToStructure(IntPtr.Add(list, checked(8 + j * profileSize)), typeof(ProfileInfo));
                            if (String.IsNullOrEmpty(profile.Name)) throw new InvalidOperationException("A Wi-Fi profile has no identity.");
                            profiles.Add(new WlanProfileEntry { InterfaceId = id.ToString("D"), Name = profile.Name, Flags = profile.Flags });
                        }
                    } finally { if (list != IntPtr.Zero) WlanFreeMemory(list); }
                }
                return new WlanSnapshot { InterfaceCount = count, Interfaces = adapterIds.ToArray(), Profiles = profiles.ToArray() };
            } finally { if (interfaces != IntPtr.Zero) WlanFreeMemory(interfaces); WlanCloseHandle(h, IntPtr.Zero); }
        }
        public static void Delete(string interfaceId, string name) {
            Guid id;
            if (!Guid.TryParse(interfaceId, out id) || String.IsNullOrEmpty(name) || name.Length > 255) throw new ArgumentException("Invalid Wi-Fi profile identity.");
            IntPtr h = Open();
            try { Check(WlanDeleteProfile(h, ref id, name, IntPtr.Zero)); }
            finally { WlanCloseHandle(h, IntPtr.Zero); }
        }
        public static string ReadKey(string interfaceId, string name) {
            Guid id;
            if (!Guid.TryParse(interfaceId, out id) || String.IsNullOrEmpty(name) || name.Length > 255) throw new ArgumentException("Invalid Wi-Fi profile identity.");
            IntPtr h = Open(), xml = IntPtr.Zero;
            try {
                uint flags = 4, access;
                Check(WlanGetProfile(h, ref id, name, IntPtr.Zero, out xml, ref flags, out access));
                if (xml == IntPtr.Zero) return null;
                var doc = new System.Xml.XmlDocument();
                doc.XmlResolver = null;
                var settings = new System.Xml.XmlReaderSettings { DtdProcessing = System.Xml.DtdProcessing.Prohibit, XmlResolver = null };
                using (var reader = System.Xml.XmlReader.Create(new System.IO.StringReader(Marshal.PtrToStringUni(xml)), settings)) doc.Load(reader);
                var protection = doc.SelectSingleNode("//*[local-name()='sharedKey']/*[local-name()='protected']");
                if (protection == null || !String.Equals(protection.InnerText, "false", StringComparison.OrdinalIgnoreCase)) return null;
                var node = doc.SelectSingleNode("//*[local-name()='sharedKey']/*[local-name()='keyMaterial']");
                return node == null ? null : node.InnerText;
            } finally { if (xml != IntPtr.Zero) WlanFreeMemory(xml); WlanCloseHandle(h, IntPtr.Zero); }
        }
    }
}
"@
}
function Get-WCWlanSnapshot {
    Initialize-WCWlanNative
    [WcCleanup.Wlan]::Enumerate()
}
function Get-WCWlanKey {
    param([string]$InterfaceId, [string]$Name)
    Initialize-WCWlanNative
    [WcCleanup.Wlan]::ReadKey($InterfaceId, $Name)
}
function Remove-WCWlanEntry {
    param([Parameter(Mandatory)][string]$InterfaceId, [Parameter(Mandatory)][string]$Name)
    Initialize-WCWlanNative
    [WcCleanup.Wlan]::Delete($InterfaceId, $Name)
}
function Assert-WCWlanSnapshot {
    param($Snapshot)
    if ($null -eq $Snapshot -or [int]$Snapshot.InterfaceCount -le 0 -or $null -eq $Snapshot.Interfaces -or $null -eq $Snapshot.Profiles -or
        @($Snapshot.Interfaces).Count -ne [int]$Snapshot.InterfaceCount) { throw 'Wi-Fi profile coverage is unavailable.' }
    $adapterIds = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($interface in @($Snapshot.Interfaces)) {
        $id = [Guid]::Empty
        if (-not [Guid]::TryParse([string]$interface, [ref]$id) -or $id -eq [Guid]::Empty -or -not $adapterIds.Add($id.ToString('D'))) { throw 'Wi-Fi adapter identity is invalid.' }
    }
    foreach ($profile in @($Snapshot.Profiles)) {
        $id = [Guid]::Empty
        if (-not [Guid]::TryParse([string]$profile.InterfaceId, [ref]$id) -or $id -eq [Guid]::Empty -or
            [string]::IsNullOrWhiteSpace([string]$profile.Name) -or ([string]$profile.Name).Length -gt 255 -or
            $null -eq $profile.Flags -or [int64]$profile.Flags -notin @(0,1,2) -or -not $adapterIds.Contains($id.ToString('D'))) { throw 'Wi-Fi profile identity is invalid.' }
    }
}
function Invoke-WCWlanClear {
    param([string]$Name = '')
    $removed = 0; $failed = 0; $targets = @()
    try {
        $initial = Get-WCWlanSnapshot
        Assert-WCWlanSnapshot $initial
        # The previous reader reported all-user profiles only; do not touch
        # group-policy or private per-user profiles (flags 1 and 2).
        $targets = @($initial.Profiles | Where-Object { [int64]$_.Flags -eq 0 -and (-not $Name -or $_.Name -ceq $Name) })
        if ($Name -and $targets.Count -ne 1) { throw 'The selected Wi-Fi profile is unavailable or ambiguous.' }
        foreach ($target in $targets) {
            try {
                $current = Get-WCWlanSnapshot
                Assert-WCWlanSnapshot $current
                $matches = @($current.Profiles | Where-Object { $_.InterfaceId -ieq $target.InterfaceId -and $_.Name -ceq $target.Name })
                # Do not let a name collision or changed profile type widen scope.
                if ($matches.Count -ne 1 -or [int64]$matches[0].Flags -ne 0) { throw 'Wi-Fi profile identity changed or is ambiguous.' }
                Remove-WCWlanEntry -InterfaceId $target.InterfaceId -Name $target.Name
                $after = Get-WCWlanSnapshot
                Assert-WCWlanSnapshot $after
                if ($target.InterfaceId -notin @($after.Interfaces) -or @($after.Profiles | Where-Object { $_.InterfaceId -ieq $target.InterfaceId -and $_.Name -ceq $target.Name }).Count) { throw 'Wi-Fi profile removal was not observed.' }
                $removed++
            } catch { $failed++ }
        }
        $final = Get-WCWlanSnapshot
        Assert-WCWlanSnapshot $final
        if (@($initial.Interfaces | Where-Object { $_ -notin @($final.Interfaces) }).Count) { $failed++ }
        if (@($final.Profiles | Where-Object { [int64]$_.Flags -eq 0 -and (-not $Name -or $_.Name -ceq $Name) }).Count) { $failed++ }
    } catch { $failed++ }
    $ok = $failed -eq 0
    @{ ok = $ok; status = if ($ok) { 'complete' } else { 'failed' }; targeted = $targets.Count; removed = $removed; failed = $failed; stdout = "$(if ($ok) { 'ok' } else { 'fail' }): removed=$removed failed=$failed" }
}
'@
. ([scriptblock]::Create($script:WlanProfileFunctions))

$script:ScheduledCredentialFunctions = @'
function Initialize-WCScheduledCredentials {
    if (([System.Management.Automation.PSTypeName]'WcCleanup.Credentials').Type) { return }
    Add-Type -ErrorAction Stop -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace WcCleanup {
    public sealed class CredentialIdentity { public string Name; public uint Type; public string UserName; }
    [StructLayout(LayoutKind.Sequential)]
    public struct Credential {
        public uint Flags, Type;
        public IntPtr TargetName, Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public uint CredentialBlobSize;
        public IntPtr CredentialBlob;
        public uint Persist, AttributeCount;
        public IntPtr Attributes, TargetAlias, UserName;
    }
    public static class Credentials {
        [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true, EntryPoint="CredEnumerateW")]
        private static extern bool Enumerate(string filter, uint flags, out uint count, out IntPtr credentials);
        [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true, EntryPoint="CredDeleteW")]
        private static extern bool Delete(string target, uint type, uint flags);
        [DllImport("advapi32.dll")] private static extern void CredFree(IntPtr buffer);
        public static CredentialIdentity[] List() {
            uint count; IntPtr entries;
            if (!Enumerate(null, 1, out count, out entries)) {
                int error = Marshal.GetLastWin32Error();
                if (error == 1168) return new CredentialIdentity[0];
                throw new Win32Exception(error);
            }
            try {
                var output = new List<CredentialIdentity>();
                for (int i = 0; i < count; i++) {
                    IntPtr entry = Marshal.ReadIntPtr(entries, checked(i * IntPtr.Size));
                    if (entry == IntPtr.Zero) throw new InvalidOperationException("Invalid credential entry.");
                    Credential credential = (Credential)Marshal.PtrToStructure(entry, typeof(Credential));
                    uint type = credential.Type;
                    string name = Marshal.PtrToStringUni(credential.TargetName);
                    string userName = Marshal.PtrToStringUni(credential.UserName);
                    if (String.IsNullOrEmpty(name) || type == 0) throw new InvalidOperationException("Invalid credential identity.");
                    output.Add(new CredentialIdentity { Name = name, Type = type, UserName = userName });
                }
                return output.ToArray();
            } finally { CredFree(entries); }
        }
        public static void Remove(string name, uint type) {
            if (!Delete(name, type, 0)) throw new Win32Exception(Marshal.GetLastWin32Error());
        }
    }
}
"@
}
function Get-WCScheduledCredentials {
    Initialize-WCScheduledCredentials
    [WcCleanup.Credentials]::List()
}
function Get-WCCredentialDeleteTarget($Name, $Type, $UserName) {
    $target = [string]$Name
    if ([string]::IsNullOrWhiteSpace($target)) { throw 'Invalid credential identity.' }
    if ([uint32]$Type -eq 6) {
        if ([string]::IsNullOrWhiteSpace([string]$UserName) -or $target.Contains('|') -or ([string]$UserName).Contains('|')) { throw 'Extended credential identity is ambiguous.' }
        $target += '|' + [string]$UserName
    }
    return $target
}
function Remove-WCScheduledCredential($Name, $Type, $UserName) {
    $target = Get-WCCredentialDeleteTarget $Name $Type $UserName
    Initialize-WCScheduledCredentials
    [WcCleanup.Credentials]::Remove($target, [uint32]$Type)
}
'@

$script:ScheduledSshFunctions = @'
function Invoke-WCSshProcess {
    param([Parameter(Mandatory)][string]$Executable, [Parameter(Mandatory)][string]$Arguments, [int]$TimeoutMilliseconds = 30000)
    if ($TimeoutMilliseconds -lt 1) { throw 'Invalid SSH process timeout.' }
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable; $start.Arguments = $Arguments
    $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    if (-not $process) { throw 'SSH agent tool could not start.' }
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync(); $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutMilliseconds)) {
            try { $process.Kill() } catch { }
            throw 'SSH agent tool timed out.'
        }
        $stdout.Wait(); $stderr.Wait()
        return $process.ExitCode
    } finally { $process.Dispose() }
}
function Invoke-WCSshAdd {
    param([Parameter(Mandatory)][ValidateSet('-D','-l')][string]$Option)
    $path = Join-Path ([Environment]::GetFolderPath('System')) 'OpenSSH\ssh-add.exe'
    if (-not (Test-Path -LiteralPath $path -PathType Leaf -ErrorAction Stop)) { throw 'Windows OpenSSH agent tooling is unavailable.' }
    Invoke-WCSshProcess -Executable $path -Arguments $Option
}
'@
. ([scriptblock]::Create($script:ScheduledSshFunctions))

$script:ScheduledWslFunctions = @'
function Convert-WCScheduledWslProviderBytes {
    param([byte[]]$Bytes, $Inventory)
    $expected = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in @($Inventory)) { if (-not $expected.Add([string]$entry.Name)) { throw 'Duplicate WSL registry identity.' } }
    $encodings = @()
    if ($Bytes.Length -ge 2 -and $Bytes[0] -eq 0xFF -and $Bytes[1] -eq 0xFE) {
        $encodings = @([Text.UnicodeEncoding]::new($false, $true, $true))
    } elseif ($Bytes.Length -ge 3 -and $Bytes[0] -eq 0xEF -and $Bytes[1] -eq 0xBB -and $Bytes[2] -eq 0xBF) {
        $encodings = @([Text.UTF8Encoding]::new($true, $true))
    } else {
        $encodings = @([Text.UnicodeEncoding]::new($false, $false, $true), [Text.UTF8Encoding]::new($false, $true))
    }
    foreach ($encoding in $encodings) {
        try {
            $text = $encoding.GetString($Bytes).TrimStart([char]0xFEFF)
            $names = [Collections.Generic.List[string]]::new()
            $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
            foreach ($line in @($text -split "`r?`n")) {
                $name = $line.Trim()
                if (-not $name) { continue }
                if ($name -match '[\x00-\x1f"\\]' -or $name.StartsWith('-') -or -not $seen.Add($name)) { throw 'WSL provider inventory is ambiguous.' }
                $names.Add($name)
            }
            if ($seen.Count -ne $expected.Count) { continue }
            $matches = $true
            foreach ($name in $seen) { if (-not $expected.Contains($name)) { $matches = $false; break } }
            if ($matches) { return @($names.ToArray()) }
        } catch { continue }
    }
    throw 'WSL provider and registry inventories differ or could not be decoded.'
}
function Get-WCScheduledWslProviderNames {
    param($Inventory)
    $exe = Join-Path $env:SystemRoot 'System32\wsl.exe'
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf -ErrorAction Stop)) { throw 'The WSL provider is unavailable.' }
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $exe; $start.Arguments = '--list --quiet'
    $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    if (-not $process) { throw 'The WSL provider could not start.' }
    $output = New-Object IO.MemoryStream
    try {
        $copy = $process.StandardOutput.BaseStream.CopyToAsync($output)
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(30000)) {
            try { $process.Kill() } catch { }
            throw 'WSL inventory timed out.'
        }
        $copy.Wait(); $stderr.Wait()
        if ($process.ExitCode -ne 0) { throw 'WSL inventory could not be verified.' }
        Convert-WCScheduledWslProviderBytes -Bytes ($output.ToArray()) -Inventory $Inventory
    } finally { $output.Dispose(); $process.Dispose() }
}
function Assert-WCScheduledWslProviderInventory($Inventory) {
    $names = @(Get-WCScheduledWslProviderNames -Inventory $Inventory)
    $registry = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in @($Inventory)) { $null = $registry.Add([string]$entry.Name) }
    if ($names.Count -ne $registry.Count) { throw 'WSL provider and registry inventories differ.' }
    foreach ($name in $names) { if (-not $registry.Contains($name)) { throw 'WSL provider and registry inventories differ.' } }
}
function Get-WCScheduledWslInventory {
    $root = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss'
    try { $null = Get-Item -LiteralPath $root -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); return @() }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in @(Get-ChildItem -LiteralPath $root -ErrorAction Stop)) {
        $data = Get-ItemProperty -LiteralPath $entry.PSPath -ErrorAction Stop
        $name = [string]$data.DistributionName
        if (-not $name -or $name -ne $name.Trim() -or $name -match '[\x00-\x1f"\\]' -or $name.StartsWith('-') -or -not $seen.Add($name)) {
            throw 'An unsafe or duplicate WSL distribution identity was detected.'
        }
        $base = [string]$data.BasePath
        if ($base.StartsWith('\\?\')) { $base = $base.Substring(4) }
        $disk = if ($base) { Join-Path $base 'ext4.vhdx' } else { '' }
        [pscustomobject]@{ Name=$name; Disk=$disk }
    }
}
function Test-WCScheduledWslDisk {
    param([Parameter(Mandatory)][string]$Path)
    try { $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); return $false }
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'WSL disk identity could not be verified.' }
    return $true
}
function Test-WCScheduledWslOrphans {
    $root = Join-Path $env:LOCALAPPDATA 'Packages'
    try { $rootItem = Get-Item -LiteralPath $root -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); return $false }
    if (-not $rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'WSL package coverage is unavailable.' }
    foreach ($package in @(Get-ChildItem -LiteralPath $root -Directory -Force -ErrorAction Stop)) {
        if ($package.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'WSL package identity could not be verified.' }
        if (Test-WCScheduledWslDisk -Path (Join-Path $package.FullName 'LocalState\ext4.vhdx')) { return $true }
    }
    return $false
}
function Remove-WCScheduledWslDistro($Name) {
    $exe = Join-Path $env:SystemRoot 'System32\wsl.exe'
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'The WSL provider is unavailable.' }
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $exe
    $start.Arguments = '--unregister "' + $Name + '"'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(300000)) {
            try { $process.Kill() } catch { }
            throw 'WSL unregister timed out; its final state must be checked.'
        }
        $stdout.Wait(); $stderr.Wait()
        if ($process.ExitCode -ne 0) { throw 'WSL could not unregister the distribution.' }
    } finally { $process.Dispose() }
}
'@

$script:ScheduledVmFunctions = @'
function Invoke-WCProviderProcess {
    param([Parameter(Mandatory)][string]$FilePath, [Parameter(Mandatory)][string[]]$Arguments, [bool]$Mutation = $false, [int]$TimeoutSeconds = 120)
    if ($TimeoutSeconds -lt 1 -or @($Arguments | Where-Object { $_ -match '[\x00-\x1f"]' }).Count) { throw 'Invalid provider invocation.' }
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $FilePath
    $start.Arguments = (@($Arguments | ForEach-Object { '"' + $_ + '"' }) -join ' ')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    # Mutating provider operations may continue merging snapshots after a timeout.
    # Never interrupt a merge or leave it blocked on a closed redirected pipe.
    $start.RedirectStandardOutput = -not $Mutation
    $start.RedirectStandardError = -not $Mutation
    $process = [Diagnostics.Process]::Start($start)
    if (-not $process) { throw 'The VM provider could not start.' }
    try {
        if (-not $Mutation) {
            $stdout = $process.StandardOutput.ReadToEndAsync()
            $stderr = $process.StandardError.ReadToEndAsync()
        }
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            if (-not $Mutation) { try { $process.Kill() } catch {} }
            throw 'The VM provider did not finish in time; completion is unverified.'
        }
        if ($process.ExitCode -ne 0) { throw 'The VM provider failed.' }
        if (-not $Mutation) {
            $stdout.Wait(); $stderr.Wait()
            return @($stdout.Result -split "`r?`n" | Where-Object { $_ -ne '' })
        }
        return @()
    } finally { $process.Dispose() }
}

function Assert-WCProviderIdle {
    param([Parameter(Mandatory)][string]$Name)
    if (@(Get-Process -ErrorAction Stop | Where-Object { $_.ProcessName -ieq $Name }).Count) { throw 'The VM provider is busy; a previous operation may still be running.' }
}

function Invoke-WCVBox {
    param([Parameter(Mandatory)][string[]]$Arguments)
    $programFiles = [Environment]::GetFolderPath('ProgramFiles')
    $exe = Join-Path $programFiles 'Oracle\VirtualBox\VBoxManage.exe'
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'VirtualBox is unavailable.' }
    Assert-WCProviderIdle -Name 'VBoxManage'
    $mutation = $Arguments.Count -ge 3 -and $Arguments[0] -eq 'snapshot' -and $Arguments[2] -eq 'delete'
    return @(Invoke-WCProviderProcess -FilePath $exe -Arguments $Arguments -Mutation $mutation -TimeoutSeconds $(if ($mutation) { 1800 } else { 120 }))
}

function Get-WCVBoxMachines {
    $identities = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($line in @(Invoke-WCVBox -Arguments @('list', 'vms'))) {
        if ($line -notmatch '\{([0-9a-fA-F-]{36})\}\s*$') { throw 'VirtualBox returned an invalid machine identity.' }
        $id = [guid]::Empty
        if (-not [guid]::TryParse($Matches[1], [ref]$id)) { throw 'VirtualBox returned an invalid machine identity.' }
        if (-not $identities.Add($id.ToString())) { throw 'VirtualBox returned a duplicate machine identity.' }
    }
    return @($identities)
}

function Get-WCVBoxMachineInfo {
    param([Parameter(Mandatory)][string]$Id)
    $uuid = [guid]::Empty
    if (-not [guid]::TryParse($Id, [ref]$uuid)) { throw 'Invalid VirtualBox machine identity.' }
    $path = $null; $state = $null
    foreach ($line in @(Invoke-WCVBox -Arguments @('showvminfo', $uuid.ToString(), '--machinereadable'))) {
        if ($line -match '^CfgFile="([^"]+)"$') { $path = $Matches[1] }
        elseif ($line -match '^VMState="([^"]+)"$') { $state = $Matches[1] }
    }
    if (-not $path -or -not $state) { throw 'VirtualBox machine details are incomplete.' }
    return [pscustomobject]@{ Path = $path; State = $state }
}

function Test-WCVBoxMachineInScope {
    param([Parameter(Mandatory)][string]$ConfigPath)
    $root = [IO.Path]::GetFullPath((Join-Path $env:USERPROFILE 'VirtualBox VMs')).TrimEnd('\')
    $file = [IO.Path]::GetFullPath($ConfigPath)
    if (-not $file.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { return $false }
    $cursor = $file
    while ($cursor.Length -ge $root.Length) {
        $item = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'VirtualBox machine path contains a reparse point.' }
        if ($cursor -ieq $root) { return $true }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
        if (-not $cursor) { throw 'VirtualBox machine path could not be verified.' }
    }
    throw 'VirtualBox machine path could not be verified.'
}

function Get-WCVBoxSnapshots {
    param([Parameter(Mandatory)][string]$MachineId)
    $machine = [guid]::Empty
    if (-not [guid]::TryParse($MachineId, [ref]$machine)) { throw 'Invalid VirtualBox machine identity.' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $nameKeys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $uuidKeys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $lines = @(Invoke-WCVBox -Arguments @('snapshot', $machine.ToString(), 'list', '--machinereadable'))
    foreach ($line in $lines) {
        if ($line -match '^SnapshotName([-0-9]*)="(.*)"$') {
            if (-not $nameKeys.Add($Matches[1])) { throw 'VirtualBox returned a duplicate snapshot.' }
        } elseif ($line -match '^SnapshotUUID([-0-9]*)="([^"]+)"$') {
            $key = $Matches[1]; $value = $Matches[2]
            $id = [guid]::Empty
            if (-not [guid]::TryParse($value, [ref]$id) -or -not $seen.Add($id.ToString()) -or -not $uuidKeys.Add($key)) { throw 'VirtualBox returned an invalid snapshot identity.' }
        } elseif ($line -match '^Snapshot(?:Name|UUID)') { throw 'VirtualBox returned an invalid snapshot identity.' }
    }
    if ($nameKeys.Count -ne $uuidKeys.Count -or @($nameKeys | Where-Object { -not $uuidKeys.Contains($_) }).Count) { throw 'VirtualBox snapshot inventory is incomplete.' }
    if (-not $seen.Count -and @($lines | Where-Object { $_.Trim() }).Count) {
        throw 'VirtualBox snapshot inventory could not be verified.'
    }
    return @($seen)
}

function Remove-WCVBoxSnapshot {
    param([Parameter(Mandatory)][string]$MachineId, [Parameter(Mandatory)][string]$SnapshotId)
    $machine = [guid]::Empty; $snapshot = [guid]::Empty
    if (-not [guid]::TryParse($MachineId, [ref]$machine) -or -not [guid]::TryParse($SnapshotId, [ref]$snapshot)) { throw 'Invalid VirtualBox snapshot identity.' }
    $null = Invoke-WCVBox -Arguments @('snapshot', $machine.ToString(), 'delete', $snapshot.ToString())
}

function Invoke-WCVBoxSnapshotCleanup {
    $removed = 0; $failed = 0
    try { $machines = @(Get-WCVBoxMachines) } catch { return @{ removed = 0; failed = 1 } }
    foreach ($machine in $machines) {
        try {
            $info = Get-WCVBoxMachineInfo $machine
            if (-not (Test-WCVBoxMachineInScope $info.Path)) { continue }
            $before = @(Get-WCVBoxSnapshots $machine)
            if ($before.Count -and $info.State -ne 'poweroff') { $failed++; continue }
            foreach ($snapshot in $before) {
                try {
                    $current = Get-WCVBoxMachineInfo $machine
                    if ($current.State -ne 'poweroff' -or $current.Path -ine $info.Path -or -not (Test-WCVBoxMachineInScope $current.Path)) { throw 'The virtual machine changed state or identity.' }
                    Remove-WCVBoxSnapshot $machine $snapshot
                } catch { $failed++; break }
            }
            $after = @(Get-WCVBoxSnapshots $machine)
            foreach ($snapshot in $before) {
                if ($snapshot -in $after) { $failed++ } else { $removed++ }
            }
            if ($after.Count) { $failed++ }
        } catch { $failed++ }
    }
    return @{ removed = $removed; failed = $failed }
}

function Invoke-WCVmrun {
    param([Parameter(Mandatory)][string[]]$Arguments)
    $paths = @(foreach ($base in @(${env:ProgramFiles(x86)}, $env:ProgramFiles)) {
        if ($base) { Join-Path $base 'VMware\VMware Workstation\vmrun.exe' }
    })
    $exe = @($paths | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1)
    if (-not $exe.Count) { throw 'VMware Workstation is unavailable.' }
    Assert-WCProviderIdle -Name 'vmrun'
    $mutation = $Arguments[0] -eq 'deleteSnapshot'
    return @(Invoke-WCProviderProcess -FilePath $exe[0] -Arguments (@('-T', 'ws') + $Arguments) -Mutation $mutation -TimeoutSeconds $(if ($mutation) { 1800 } else { 120 }))
}

function Get-WCVmwareMachines {
    $root = Join-Path $env:USERPROFILE 'Documents\Virtual Machines'
    if (-not (Get-WCVmPathIfPresent -Path $root -Directory $true)) { return @() }
    return @(Get-AutoEraseFiles -Path $root -Filter '*.vmx' -Recurse | ForEach-Object { $_.FullName })
}

function Test-WCVmwareMachineInScope {
    param([Parameter(Mandatory)][string]$ConfigPath)
    $root = [IO.Path]::GetFullPath((Join-Path $env:USERPROFILE 'Documents\Virtual Machines')).TrimEnd('\')
    $file = [IO.Path]::GetFullPath($ConfigPath)
    if (-not $file.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetExtension($file) -ine '.vmx') { return $false }
    $cursor = $file
    while ($cursor.Length -ge $root.Length) {
        $item = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'VMware machine path contains a reparse point.' }
        if ($cursor -ieq $root) { return $true }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
        if (-not $cursor) { throw 'VMware machine path could not be verified.' }
    }
    throw 'VMware machine path could not be verified.'
}

function Get-WCVmrunInventory {
    param([Parameter(Mandatory)][string[]]$Arguments, [Parameter(Mandatory)][string]$Header)
    $lines = @(Invoke-WCVmrun -Arguments $Arguments)
    if (-not $lines.Count -or $lines[0] -notmatch ('^' + [regex]::Escape($Header) + ':\s*(\d+)\s*$')) { throw 'VMware inventory could not be verified.' }
    $expected = [int]$Matches[1]
    $values = @($lines | Select-Object -Skip 1 | Where-Object { $_.Trim() })
    if ($values.Count -ne $expected) { throw 'VMware inventory count is inconsistent.' }
    return $values
}

function Invoke-WCVmwareSnapshotCleanup {
    $removed = 0; $failed = 0
    try {
        $machines = @(Get-WCVmwareMachines)
        if (-not $machines.Count) { return @{ removed = 0; failed = 0 } }
        $running = @((Get-WCVmrunInventory -Arguments @('list') -Header 'Total running VMs') | ForEach-Object {
            if (-not [IO.Path]::IsPathRooted($_)) { throw 'VMware returned an invalid running machine identity.' }
            [IO.Path]::GetFullPath($_)
        })
    } catch { return @{ removed = 0; failed = 1 } }
    foreach ($machine in $machines) {
        try {
            if (-not (Test-WCVmwareMachineInScope $machine)) { $failed++; continue }
            $before = @(Get-WCVmrunInventory -Arguments @('listSnapshots', $machine) -Header 'Total snapshots')
            $canonicalMachine = [IO.Path]::GetFullPath($machine)
            if ($before.Count -and @($running | Where-Object { $_ -ieq $canonicalMachine }).Count) { $failed++; continue }
            $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
            foreach ($snapshot in $before) {
                if ($snapshot -notmatch '^[A-Za-z0-9_][A-Za-z0-9_. -]{0,119}$' -or -not $seen.Add($snapshot)) {
                    throw 'VMware returned an ambiguous snapshot name.'
                }
            }
            foreach ($snapshot in $before) {
                try {
                    $runningNow = @((Get-WCVmrunInventory -Arguments @('list') -Header 'Total running VMs') | ForEach-Object {
                        if (-not [IO.Path]::IsPathRooted($_)) { throw 'VMware returned an invalid running machine identity.' }
                        [IO.Path]::GetFullPath($_)
                    })
                    if (@($runningNow | Where-Object { $_ -ieq $canonicalMachine }).Count) { throw 'The virtual machine is running.' }
                    $null = Invoke-WCVmrun -Arguments @('deleteSnapshot', $machine, $snapshot)
                } catch { $failed++; break }
            }
            $after = @(Get-WCVmrunInventory -Arguments @('listSnapshots', $machine) -Header 'Total snapshots')
            foreach ($snapshot in $before) {
                if ($snapshot -cin $after) { $failed++ } else { $removed++ }
            }
            if ($after.Count) { $failed++ }
        } catch { $failed++ }
    }
    return @{ removed = $removed; failed = $failed }
}

function Get-WCVmPathIfPresent {
    param([Parameter(Mandatory)][string]$Path, [bool]$Directory)
    try { $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); return $null }
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or [bool]$item.PSIsContainer -ne $Directory) {
        throw 'A virtual-machine artifact path could not be safely verified.'
    }
    return $item
}

function Remove-WCVmwareRecentHistory {
    $path = 'HKCU:\Software\VMware, Inc.\VMware Workstation'
    try { $key = Get-Item -LiteralPath $path -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); return @{ removed=0; failed=0 } }
    $names = @($key.Property | Where-Object { $_ -match '(?i)mru|recent' })
    $failed = 0
    foreach ($name in $names) {
        try { Remove-ItemProperty -LiteralPath $path -Name $name -ErrorAction Stop }
        catch { $failed++ }
    }
    $remaining = @((Get-Item -LiteralPath $path -ErrorAction Stop).Property | Where-Object { $_ -match '(?i)mru|recent' })
    $removed = @($names | Where-Object { $_ -notin $remaining }).Count
    if ($remaining.Count) { $failed++ }
    return @{ removed=$removed; failed=$failed }
}

function Assert-WCVmUserArtifactPath {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Root)
    $profile = [IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\')
    $rootPath = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    $file = [IO.Path]::GetFullPath($Path)
    if (-not $rootPath.StartsWith($profile + '\', [StringComparison]::OrdinalIgnoreCase) -or
        -not $file.StartsWith($rootPath + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'A virtual-machine log is outside the selected profile.' }
    $cursor = $file
    while ($cursor.Length -ge $profile.Length) {
        $item = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'A virtual-machine log path contains a reparse point.' }
        if ($cursor -ieq $profile) { return }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
        if (-not $cursor) { throw 'A virtual-machine log path could not be verified.' }
    }
    throw 'A virtual-machine log path could not be verified.'
}

function Get-WCVmUserLogFiles {
    $sources = @(
        @{ root=(Join-Path $env:USERPROFILE 'Documents\Virtual Machines'); patterns=@('vmware*.log') },
        @{ root=(Join-Path $env:USERPROFILE 'VirtualBox VMs'); patterns=@('VBox.log','VBoxSVC.log') }
    )
    foreach ($source in $sources) {
        if (-not (Get-WCVmPathIfPresent -Path $source.root -Directory $true)) { continue }
        foreach ($pattern in $source.patterns) {
            foreach ($file in @(Get-AutoEraseFiles -Path $source.root -Filter $pattern -Recurse)) {
                Assert-WCVmUserArtifactPath -Path $file.FullName -Root $source.root
                $file.FullName
            }
        }
    }
}

function Invoke-WCVmUserLogCleanup {
    $targets = @(Get-WCVmUserLogFiles)
    if (-not $targets.Count) { return }
    $busyNames = @('VirtualBoxVM','VBoxHeadless','VBoxSVC','VBoxManage','vmware-vmx','vmrun')
    if (@(Get-Process -ErrorAction Stop | Where-Object { $_.ProcessName -in $busyNames }).Count) { throw 'A virtual-machine provider is active; its logs were left unchanged.' }
    foreach ($path in $targets) { Erase-OneFile $path | Out-Null }
    if (@(Get-WCVmUserLogFiles).Count) { throw 'Virtual-machine logs remain after cleanup.' }
}

function Get-WCVirtualMachineArtifactRemainders {
    $count = 0
    $vmwareRoot = Join-Path $env:USERPROFILE 'Documents\Virtual Machines'
    if (Get-WCVmPathIfPresent -Path $vmwareRoot -Directory $true) {
        foreach ($pattern in @('*-0000*.vmdk', '*.vmsn', 'vmware*.log')) {
            $count += @(Get-AutoEraseFiles -Path $vmwareRoot -Filter $pattern -Recurse).Count
        }
    }
    $virtualBoxRoot = Join-Path $env:USERPROFILE 'VirtualBox VMs'
    if (Get-WCVmPathIfPresent -Path $virtualBoxRoot -Directory $true) {
        foreach ($dir in @(Get-AutoEraseDirectories -Path $virtualBoxRoot -Recurse | Where-Object { $_.Name -eq 'Snapshots' })) {
            foreach ($pattern in @('*.vdi', '*.vbox-prev')) {
                $count += @(Get-AutoEraseFiles -Path $dir.FullName -Filter $pattern -Recurse).Count
            }
        }
        foreach ($pattern in @('VBox.log', 'VBoxSVC.log')) {
            $count += @(Get-AutoEraseFiles -Path $virtualBoxRoot -Filter $pattern -Recurse).Count
        }
    }
    foreach ($path in @((Join-Path $env:ProgramData 'VMware\vmnetdhcp.log'), (Join-Path $env:ProgramData 'VMware\vmnetnat.log'), (Join-Path $env:USERPROFILE '.VirtualBox\VirtualBox.xml'))) {
        if (Get-WCVmPathIfPresent -Path $path -Directory $false) { $count++ }
    }
    $mru = 'HKCU:\Software\VMware, Inc.\VMware Workstation'
    try { $key = Get-Item -LiteralPath $mru -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0); $key = $null }
    if ($key) { $count += @($key.Property | Where-Object { $_ -match '(?i)mru|recent' }).Count }
    return $count
}

function Invoke-WCVirtualMachineArtifactCleanup {
    $result = @{ removed = 0; failed = 0 }
    $virtualBoxRoot = Join-Path $env:USERPROFILE 'VirtualBox VMs'
    try {
        if (Get-WCVmPathIfPresent -Path $virtualBoxRoot -Directory $true) {
            $snapshotResult = Invoke-WCVBoxSnapshotCleanup
            $result.removed += [int]$snapshotResult.removed
            $result.failed += [int]$snapshotResult.failed
        }
    } catch { $result.failed++ }
    $vmwareResult = Invoke-WCVmwareSnapshotCleanup
    $result.removed += [int]$vmwareResult.removed
    $result.failed += [int]$vmwareResult.failed
    try {
        $recentResult = Remove-WCVmwareRecentHistory
        $result.removed += [int]$recentResult.removed
        $result.failed += [int]$recentResult.failed
    } catch { $result.failed++ }
    try { Invoke-WCVmUserLogCleanup } catch { $result.failed++ }
    try { if ((Get-WCVirtualMachineArtifactRemainders) -gt 0) { $result.failed++ } }
    catch { $result.failed++ }
    return $result
}
'@


$script:ScheduledDockerFunctions = @'
function New-WCDockerStartInfo {
    param([Parameter(Mandatory)][string]$Executable, [Parameter(Mandatory)][string]$ConfigPath, [Parameter(Mandatory)][string[]]$Arguments, [bool]$Mutation)
    $all = @('--config', $ConfigPath, '--host', 'npipe:////./pipe/dockerDesktopLinuxEngine') + $Arguments
    if (@($all | Where-Object { $_ -match '[\x00-\x1f"]' }).Count) { throw 'Invalid Docker provider invocation.' }
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable
    $start.Arguments = (@($all | ForEach-Object { '"' + $_ + '"' }) -join ' ')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = -not $Mutation
    $start.RedirectStandardError = -not $Mutation
    foreach ($key in @($start.EnvironmentVariables.Keys)) {
        if ($key -match '^(?i:DOCKER_|HTTP_PROXY$|HTTPS_PROXY$|ALL_PROXY$|NO_PROXY$)') { $start.EnvironmentVariables.Remove($key) }
    }
    return $start
}

function Invoke-WCDocker {
    param([Parameter(Mandatory)][string]$ConfigPath, [Parameter(Mandatory)][string[]]$Arguments, [bool]$Mutation = $false)
    $programFiles = [Environment]::GetFolderPath('ProgramFiles')
    $exe = Join-Path $programFiles 'Docker\Docker\resources\bin\docker.exe'
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf -ErrorAction Stop)) { throw 'Docker Desktop CLI is unavailable.' }
    if (@(Get-Process -ErrorAction Stop | Where-Object { $_.ProcessName -ieq 'docker' }).Count) { throw 'A Docker CLI operation is already running.' }
    $start = New-WCDockerStartInfo -Executable $exe -ConfigPath $ConfigPath -Arguments $Arguments -Mutation $Mutation
    $process = [Diagnostics.Process]::Start($start)
    if (-not $process) { throw 'Docker Desktop CLI could not start.' }
    try {
        if (-not $Mutation) {
            $stdout = $process.StandardOutput.ReadToEndAsync()
            $stderr = $process.StandardError.ReadToEndAsync()
        }
        $timeout = if ($Mutation) { 1800000 } else { 120000 }
        if (-not $process.WaitForExit($timeout)) {
            if ($Mutation) { $script:WCDockerMutationTimedOut = $true }
            else { try { $process.Kill() } catch {} }
            throw 'Docker Desktop operation timed out and remains unverified.'
        }
        if ($process.ExitCode -ne 0) { throw 'Docker Desktop operation failed.' }
        if (-not $Mutation) {
            $stdout.Wait(); $stderr.Wait()
            return @($stdout.Result -split "`r?`n" | Where-Object { $_ -ne '' })
        }
        return @()
    } finally { $process.Dispose() }
}

function Get-WCDockerDesktopIdentity {
    param([Parameter(Mandatory)][string]$ConfigPath)
    $lines = @(Invoke-WCDocker -ConfigPath $ConfigPath -Arguments @('info', '--format', '{{json .}}'))
    if ($lines.Count -ne 1) { throw 'Docker Desktop identity was not verified.' }
    try { $info = $lines[0] | ConvertFrom-Json -ErrorAction Stop } catch { throw 'Docker Desktop identity was not verified.' }
    if ([string]$info.OSType -cne 'linux' -or [string]$info.OperatingSystem -cnotin @('Docker Desktop', 'Docker Desktop (containerized)') -or -not [string]$info.ID) {
        throw 'The local Linux Docker Desktop engine was not verified.'
    }
    return [string]$info.ID
}

function Invoke-WCCheckedDocker {
    param([string]$ConfigPath, [string]$Identity, [string[]]$Arguments, [bool]$Mutation = $false)
    if ((Get-WCDockerDesktopIdentity $ConfigPath) -cne $Identity) { throw 'Docker Desktop engine identity changed.' }
    return @(Invoke-WCDocker -ConfigPath $ConfigPath -Arguments $Arguments -Mutation $Mutation)
}

function Get-WCDockerResources {
    param([string]$ConfigPath, [string]$Identity)
    $containers = @(Invoke-WCCheckedDocker $ConfigPath $Identity @('container', 'ls', '--all', '--quiet', '--no-trunc'))
    $images = @(Invoke-WCCheckedDocker $ConfigPath $Identity @('image', 'ls', '--all', '--quiet', '--no-trunc'))
    $volumes = @(Invoke-WCCheckedDocker $ConfigPath $Identity @('volume', 'ls', '--quiet'))
    foreach ($id in $containers) { if ($id -notmatch '^[0-9a-f]{64}$') { throw 'Docker container inventory was invalid.' } }
    foreach ($id in $images) { if ($id -notmatch '^(sha256:)?[0-9a-f]{64}$') { throw 'Docker image inventory was invalid.' } }
    if (@($volumes | Where-Object { -not $_.Trim() -or $_ -match '[\x00-\x1f]' }).Count) { throw 'Docker volume inventory was invalid.' }
    return @{ containers = @($containers | Sort-Object -Unique); images = @($images | Sort-Object -Unique); volumes = @($volumes | Sort-Object -Unique) }
}

function Get-WCDockerArtifactRemainders {
    $count = 0
    $root = Join-Path $env:LOCALAPPDATA 'Docker\wsl'
    $rootItem = $null
    try { $rootItem = Get-Item -LiteralPath $root -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0) }
    if ($rootItem) {
        if (-not $rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Docker disk inventory is unavailable.' }
        foreach ($item in @(Get-ChildItem -LiteralPath $root -Recurse -Force -ErrorAction Stop)) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Docker disk inventory contains a reparse point.' }
            if (-not $item.PSIsContainer -and $item.Name -like '*.vhdx') { $count++ }
        }
    }
    foreach ($fixedDisk in @(
        (Join-Path $env:ProgramData 'DockerDesktop\vm-data\DockerDesktop.vhdx'),
        (Join-Path $env:LOCALAPPDATA 'Docker\vm-data\DockerDesktop.vhdx')
    )) {
        $diskItem = $null
        try { $diskItem = Get-Item -LiteralPath $fixedDisk -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0) }
        if ($diskItem) {
            if ($diskItem.PSIsContainer -or ($diskItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Docker disk inventory is unavailable.' }
            $count++
        }
    }
    $log = Join-Path $env:APPDATA 'Docker\log.txt'
    $logItem = $null
    try { $logItem = Get-Item -LiteralPath $log -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0) }
    if ($logItem) {
        if ($logItem.PSIsContainer -or ($logItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Docker log inventory is unavailable.' }
        $count++
    }
    return $count
}

function Get-WCDockerLocalVolumes {
    param([string]$ConfigPath, [string]$Identity, [string[]]$Names)
    $eligible = [Collections.Generic.List[string]]::new()
    $excluded = 0
    foreach ($name in $Names) {
        if ($name -notmatch '^[A-Za-z0-9][A-Za-z0-9_.-]*$') { throw 'Docker volume identity was invalid.' }
        $lines = @(Invoke-WCCheckedDocker $ConfigPath $Identity @('volume', 'inspect', '--format', '{{json .}}', $name))
        if ($lines.Count -ne 1) { throw 'Docker volume inspection was incomplete.' }
        try { $volume = $lines[0] | ConvertFrom-Json -ErrorAction Stop } catch { throw 'Docker volume inspection was invalid.' }
        if ([string]$volume.Name -cne $name) { throw 'Docker volume identity changed.' }
        $options = if ($null -eq $volume.Options) { 0 } elseif ($volume.Options -is [pscustomobject]) { @($volume.Options.PSObject.Properties).Count } else { -1 }
        $expectedPath = '/var/lib/docker/volumes/' + $name + '/_data'
        if ([string]$volume.Driver -ceq 'local' -and [string]$volume.Scope -ceq 'local' -and $options -eq 0 -and [string]$volume.Mountpoint -ceq $expectedPath) {
            $eligible.Add($name)
        } else { $excluded++ }
    }
    return @{ eligible = @($eligible); excluded = $excluded }
}

function Get-WCDockerBuildCacheCount {
    param([string]$ConfigPath, [string]$Identity)
    $lines = @(Invoke-WCCheckedDocker $ConfigPath $Identity @('system', 'df', '--format', '{{json .}}'))
    $rows = @($lines | ForEach-Object { $_ | ConvertFrom-Json -ErrorAction Stop } | Where-Object { $_.Type -ceq 'Build Cache' })
    if ($rows.Count -ne 1 -or [string]$rows[0].TotalCount -notmatch '^\d+$') { throw 'Docker build cache readback is unavailable.' }
    return [int64]$rows[0].TotalCount
}

function Invoke-WCDockerDesktopCleanup {
    $result = @{ removed = 0; failed = 0 }
    $script:WCDockerMutationTimedOut = $false
    $config = Join-Path ([IO.Path]::GetTempPath()) ('wincommander-docker-cli-' + [guid]::NewGuid())
    try {
        $null = New-Item -ItemType Directory -Path $config -ErrorAction Stop
        $identity = Get-WCDockerDesktopIdentity $config
        $before = Get-WCDockerResources $config $identity
        $volumes = Get-WCDockerLocalVolumes $config $identity $before.volumes
        foreach ($id in $before.containers) {
            try { $null = Invoke-WCCheckedDocker $config $identity @('container', 'rm', '--force', $id) $true }
            catch { $result.failed++; if ($script:WCDockerMutationTimedOut) { throw } }
        }
        try { $null = Invoke-WCCheckedDocker $config $identity @('system', 'prune', '--all', '--force') $true }
        catch { $result.failed++; if ($script:WCDockerMutationTimedOut) { throw } }
        foreach ($name in $volumes.eligible) {
            try {
                $verified = Get-WCDockerLocalVolumes $config $identity @($name)
                if ($verified.eligible.Count -ne 1) { $result.failed++; continue }
                $null = Invoke-WCCheckedDocker $config $identity @('volume', 'rm', $name) $true
            } catch { $result.failed++; if ($script:WCDockerMutationTimedOut) { throw } }
        }
        if ($volumes.excluded) { $result.failed++ }
        # This is the same per-user Docker log included in the scan and manual
        # cleanup. Erase-OneFile rejects reparse points and records failures.
        Erase-OneFile (Join-Path $env:APPDATA 'Docker\log.txt') | Out-Null
        $after = Get-WCDockerResources $config $identity
        $result.removed = [Math]::Max(0, $before.containers.Count - $after.containers.Count) + [Math]::Max(0, $before.images.Count - $after.images.Count) + [Math]::Max(0, $before.volumes.Count - $after.volumes.Count)
        if ($after.containers.Count -or $after.images.Count -or $after.volumes.Count -or (Get-WCDockerBuildCacheCount $config $identity) -gt 0 -or (Get-WCDockerArtifactRemainders) -gt 0) { $result.failed++ }
        # The supported CLI cannot verify whether a custom disk image remains.
        $result.failed++
    } catch { $result.failed++ }
    finally {
        if (-not $script:WCDockerMutationTimedOut) {
            $absolute = [IO.Path]::GetFullPath($config)
            $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
            if ($absolute.StartsWith($temp, [StringComparison]::OrdinalIgnoreCase) -and [IO.Path]::GetFileName($absolute) -like 'wincommander-docker-cli-*' -and (Test-Path -LiteralPath $absolute -PathType Container)) {
                try { [IO.Directory]::Delete($absolute, $false) } catch { }
            }
        }
    }
    return $result
}
'@


$script:AutoEraseScripts = @{
    'dockerDesktopData' = @'
try {
    $result = Invoke-WCDockerDesktopCleanup
    $script:AutoEraseRemoved += [int]$result.removed
    $script:AutoEraseFailed += [int]$result.failed
} catch { $script:AutoEraseFailed++ }
'@
    'virtualMachineArtifacts' = @'
try {
    $result = Invoke-WCVirtualMachineArtifactCleanup
    $script:AutoEraseRemoved += [int]$result.removed
    $script:AutoEraseFailed += [int]$result.failed
} catch { $script:AutoEraseFailed++ }
'@
    'wslData' = @'
try {
    $before = @(Get-WCScheduledWslInventory)
    Assert-WCScheduledWslProviderInventory $before
    $dockerDistros = @($before | Where-Object { $_.Name -in @('docker-desktop', 'docker-desktop-data') })
    $eligible = @($before | Where-Object { $_.Name -notin @('docker-desktop', 'docker-desktop-data') })
    if ($dockerDistros.Count -gt 0) { $script:AutoEraseFailed++ }
    foreach ($distro in $eligible) {
        try { Remove-WCScheduledWslDistro $distro.Name }
        catch { $script:AutoEraseFailed++ }
    }
    $after = @(Get-WCScheduledWslInventory)
    Assert-WCScheduledWslProviderInventory $after
    foreach ($distro in $eligible) {
        $registered = @($after | Where-Object { $_.Name -eq $distro.Name }).Count -gt 0
        $diskPresent = $distro.Disk -and (Test-WCScheduledWslDisk -Path $distro.Disk)
        if (-not $registered -and -not $diskPresent) { $script:AutoEraseRemoved++ }
        else { $script:AutoEraseFailed++ }
    }
    if ($after.Count -gt 0 -or (Test-WCScheduledWslOrphans)) { $script:AutoEraseFailed++ }
} catch { $script:AutoEraseFailed++ }
'@
    'credentialManager' = @'
try {
    $before = @(Get-WCScheduledCredentials)
    foreach ($credential in $before) {
        try { Remove-WCScheduledCredential $credential.Name $credential.Type $credential.UserName }
        catch { $script:AutoEraseFailed++ }
    }
    $after = @(Get-WCScheduledCredentials)
    foreach ($credential in $before) {
        $stillPresent = @($after | Where-Object { $_.Name -eq $credential.Name -and $_.Type -eq $credential.Type -and ($_.Type -ne 6 -or $_.UserName -eq $credential.UserName) }).Count -gt 0
        if (-not $stillPresent) { $script:AutoEraseRemoved++ }
    }
    if ($after.Count -gt 0) { $script:AutoEraseFailed++ }
    # CredEnumerate cannot verify other applications' Windows Vault entries.
    $script:AutoEraseFailed++
} catch { $script:AutoEraseFailed++ }
'@
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
    } catch { $script:AutoEraseFailed++ }
}
'@

    # Standard categories ----------------------------------------------
    'clipboard' = @'
Add-Type -AssemblyName System.Runtime.WindowsRuntime -ErrorAction Stop
$null = [Windows.ApplicationModel.DataTransfer.Clipboard, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
if (-not [Windows.ApplicationModel.DataTransfer.Clipboard]::ClearHistory()) { throw 'Clipboard history could not be cleared in this task session.' }
[Windows.ApplicationModel.DataTransfer.Clipboard]::Clear()
$asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.IsGenericType } | Select-Object -First 1
if (-not $asTask) { throw 'Clipboard history read-back is unavailable.' }
$resultType = [Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
$task = $asTask.MakeGenericMethod($resultType).Invoke($null, @([Windows.ApplicationModel.DataTransfer.Clipboard]::GetHistoryItemsAsync()))
if (-not $task.Wait(2000) -or $task.Result.Status.ToString() -ne 'Success' -or @($task.Result.Items).Count) { throw 'Clipboard history cleanup could not be verified.' }
'@
    'rdpHistory'        = @"
`$default = 'HKCU:\Software\Microsoft\Terminal Server Client\Default'
if (Test-Path `$default) {
    (Get-Item `$default).Property | Where-Object { `$_ -like 'MR*' } | ForEach-Object {
        Remove-ItemProperty -Path `$default -Name `$_ -ErrorAction SilentlyContinue
    }
}
'HKCU:\Software\Microsoft\Terminal Server Client\Servers' | ForEach-Object {
    if (Test-Path `$_) { Remove-Item `$_ -Recurse -Force -ErrorAction SilentlyContinue; New-Item `$_ -Force | Out-Null }
}
Erase-OneFile (Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'Default.rdp')
`$credentials = @(& cmdkey.exe /list 2>&1)
if (`$LASTEXITCODE -ne 0) { throw 'RDP credential read-back is unavailable.' }
if (@(`$credentials | Where-Object { `$_ -match '(?i)target=TERMSRV/' }).Count) { throw 'RDP history was cleared, but saved credentials were preserved. Cleanup is partial.' }
"@
    'eventLogs'         = @"
try { `$logs = @(Get-WinEvent -ListLog * -Force -ErrorAction Stop) }
catch { `$script:AutoEraseFailed++; `$logs = @() }
`$logs | Where-Object { `$_.RecordCount -gt 0 } | ForEach-Object {
    try { [System.Diagnostics.Eventing.Reader.EventLogSession]::GlobalSession.ClearLog(`$_.LogName) }
    catch { `$script:AutoEraseFailed++ }
}
"@
    'recentFiles'       = @"
Get-AutoEraseFiles -Path "`$env:APPDATA\Microsoft\Windows\Recent" -Recurse | ForEach-Object { Erase-OneFile `$_.FullName }
Get-AutoEraseDirectories -Path "`$env:APPDATA\Microsoft\Windows\Recent" | Where-Object { `$_.Name -notin 'AutomaticDestinations','CustomDestinations' } | ForEach-Object { Erase-Dir `$_.FullName }
Get-AutoEraseFiles -Path "`$env:APPDATA\Microsoft\Office\Recent" -Recurse | ForEach-Object { Erase-OneFile `$_.FullName }
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
    'dnsCache'          = "ipconfig /flushdns | Out-Null; if (`$LASTEXITCODE -ne 0) { `$script:AutoEraseFailed++ }"
    'browserFootprints' = @'
function Assert-BrowserProfilePath {
    param([string]$Path)
    $full = [IO.Path]::GetFullPath($Path).TrimEnd('\')
    $base = [IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\')
    if (-not $full.StartsWith($base + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'A browser profile is outside the selected user profile.' }
    $cursor = $full
    while ($cursor.Length -ge $base.Length) {
        $item = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'A browser profile path contains a reparse point.' }
        if ($cursor -ieq $base) { break }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
        if (-not $cursor) { throw 'A browser profile path could not be verified.' }
    }
}
$pf = $env:ProgramFiles
$pf86 = ${env:ProgramFiles(x86)}
$local = $env:LOCALAPPDATA
$roaming = $env:APPDATA
$browsers = @(
    @{ root="$local\Google\Chrome\User Data"; engine='Chromium'; exes=@("$pf\Google\Chrome\Application\chrome.exe", "$pf86\Google\Chrome\Application\chrome.exe", "$local\Google\Chrome\Application\chrome.exe") },
    @{ root="$local\Microsoft\Edge\User Data"; engine='Chromium'; exes=@("$pf\Microsoft\Edge\Application\msedge.exe", "$pf86\Microsoft\Edge\Application\msedge.exe", "$local\Microsoft\Edge\Application\msedge.exe") },
    @{ root="$local\BraveSoftware\Brave-Browser\User Data"; engine='Chromium'; exes=@("$pf\BraveSoftware\Brave-Browser\Application\brave.exe", "$pf86\BraveSoftware\Brave-Browser\Application\brave.exe", "$local\BraveSoftware\Brave-Browser\Application\brave.exe") },
    @{ root="$roaming\Opera Software\Opera Stable"; engine='Opera'; exes=@("$pf\Opera\launcher.exe", "$pf\Opera\opera.exe", "$pf86\Opera\launcher.exe", "$pf86\Opera\opera.exe", "$local\Programs\Opera\launcher.exe", "$local\Programs\Opera\opera.exe") },
    @{ root="$roaming\Opera Software\Opera GX Stable"; engine='Opera'; exes=@("$pf\Opera GX\launcher.exe", "$pf\Opera GX\opera.exe", "$pf86\Opera GX\launcher.exe", "$pf86\Opera GX\opera.exe", "$local\Programs\Opera GX\launcher.exe", "$local\Programs\Opera GX\opera.exe") },
    @{ root="$local\Vivaldi\User Data"; engine='Chromium'; exes=@("$pf\Vivaldi\Application\vivaldi.exe", "$local\Vivaldi\Application\vivaldi.exe") },
    @{ root="$local\Chromium\User Data"; engine='Chromium'; exes=@("$pf\Chromium\Application\chrome.exe", "$pf86\Chromium\Application\chrome.exe") },
    @{ root="$local\Arc\User Data"; engine='Chromium'; exes=@("$local\Arc\Application\arc.exe") },
    @{ root="$roaming\Mozilla\Firefox"; engine='Gecko'; exes=@("$pf\Mozilla Firefox\firefox.exe", "$pf86\Mozilla Firefox\firefox.exe") },
    @{ root="$roaming\LibreWolf"; engine='Gecko'; exes=@("$pf\LibreWolf\librewolf.exe", "$pf86\LibreWolf\librewolf.exe", "$local\LibreWolf\librewolf.exe") },
    @{ root="$roaming\Floorp"; engine='Gecko'; exes=@("$pf\Floorp\floorp.exe", "$pf86\Floorp\floorp.exe") },
    @{ root="$roaming\Waterfox"; engine='Gecko'; exes=@("$pf\Waterfox\waterfox.exe", "$pf86\Waterfox\waterfox.exe") },
    @{ root="$roaming\Zen Browser"; engine='Gecko'; exes=@("$pf\Zen Browser\zen.exe", "$local\Zen Browser\zen.exe", "$roaming\Zen Browser\zen.exe") },
    @{ root="$roaming\Mullvad Browser"; engine='Gecko'; exes=@("$pf\Mullvad Browser\mullvadbrowser.exe") },
    @{ root="$roaming\Tor Browser"; engine='Gecko'; exes=@("$pf\Tor Browser\Browser\firefox.exe", "$env:USERPROFILE\Desktop\Tor Browser\Browser\firefox.exe") }
)
$chromium = @('History','Cookies','Login Data','Top Sites','Favicons','Web Data','Visited Links','Shortcuts','Cache','Code Cache','IndexedDB','Local Storage','Session Storage','Network','Download Metadata')
$gecko = @('places.sqlite','cookies.sqlite','logins.json','favicons.sqlite','formhistory.sqlite','cache2','storage','sessionstore.jsonlz4')
foreach ($browser in $browsers) {
    try {
        if (-not @($browser.exes | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) }).Count) { continue }
        if (-not (Test-Path -LiteralPath $browser.root -PathType Container)) { continue }
        Assert-BrowserProfilePath $browser.root
        $profiles = @()
        if ($browser.engine -eq 'Chromium') {
            $profiles = @(Get-AutoEraseDirectories -Path $browser.root | Where-Object { $_.Name -eq 'Default' -or $_.Name -match '^Profile [0-9]+$' } | ForEach-Object { $_.FullName })
        }
        elseif ($browser.engine -eq 'Opera') { $profiles = @($browser.root) }
        else {
            $ini = Join-Path $browser.root 'profiles.ini'
            if (Test-Path -LiteralPath $ini -PathType Leaf) {
                $section = $false; $profilePath = $null; $relative = $true
                foreach ($line in @((Get-Content -LiteralPath $ini -ErrorAction Stop) + '[End]')) {
                    if ($line -match '^\[') {
                        if ($section -and $profilePath) { $profiles += if ($relative) { Join-Path $browser.root $profilePath } else { $profilePath } }
                        $section = $line -match '^\[Profile'; $profilePath = $null; $relative = $true
                    } elseif ($section -and $line -match '^Path=(.+)$') { $profilePath = $Matches[1].Trim() -replace '/', '\' }
                    elseif ($section -and $line -match '^IsRelative=(\d)') { $relative = $Matches[1] -eq '1' }
                }
            }
            if (-not $profiles.Count) { $profiles = @(Get-AutoEraseDirectories -Path (Join-Path $browser.root 'Profiles') | ForEach-Object { $_.FullName }) }
        }
        foreach ($profile in @($profiles | Sort-Object -Unique)) {
            if (-not (Test-Path -LiteralPath $profile -PathType Container)) { continue }
            Assert-BrowserProfilePath $profile
            $artifacts = if ($browser.engine -eq 'Gecko') { $gecko } else { $chromium }
            foreach ($artifact in $artifacts) {
                $path = Join-Path $profile $artifact
                if (-not (Test-Path -LiteralPath $path)) { continue }
                $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
                if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'A browser artifact is a reparse point.' }
                if ($item.PSIsContainer) { Erase-Dir $path } else { Erase-OneFile $path | Out-Null }
                if (Test-Path -LiteralPath $path) { $script:AutoEraseFailed++ }
            }
        }
    } catch { $script:AutoEraseFailed++ }
}
'@
    'prefetchFiles'     = @"
Get-AutoEraseFiles -Path "`$env:SystemRoot\Prefetch" -Filter '*.pf' | ForEach-Object { Erase-OneFile `$_.FullName }
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
Erase-RegistryValues -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\AppCompatCache' -NamePattern 'AppCompatCache'

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
    Erase-RegistryValues -Path `$muiCache -ExcludedNames @('LangID')
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
    'wlanProfiles'      = $script:WlanProfileFunctions + "`n" + @'
$result = Invoke-WCWlanClear
$script:AutoEraseRemoved += [int]$result.removed
$script:AutoEraseFailed += [int]$result.failed
if (-not $result.ok -and [int]$result.failed -eq 0) { $script:AutoEraseFailed++ }
'@
    'netDrives' = @'
$drives = @(Get-PSDrive -PSProvider FileSystem -ErrorAction Stop | Where-Object { $_.DisplayRoot -like '\\*' })
foreach ($drive in $drives) {
    if ($drive.Name -notmatch '^[A-Za-z]$') { throw 'A mapped-drive identity could not be validated.' }
    $null = & net.exe use ($drive.Name + ':') /delete /yes 2>&1
    if ($LASTEXITCODE -ne 0) { throw 'Mapped-drive removal failed.' }
    if (@(Get-PSDrive -PSProvider FileSystem -ErrorAction Stop | Where-Object { $_.Name -eq $drive.Name -and $_.DisplayRoot -like '\\*' }).Count) { throw 'Mapped-drive removal could not be verified.' }
    $script:AutoEraseRemoved++
}
'@
    'ntfsJournals'      = "`$fsTool = 'fs' + 'util'; Get-Volume -ErrorAction Stop | Where-Object { `$_.DriveLetter -and `$_.FileSystem -eq 'NTFS' } | ForEach-Object { & `$fsTool usn deletejournal /d (`$_.DriveLetter + ':') 2>`$null | Out-Null; if (`$LASTEXITCODE -ne 0) { `$script:AutoEraseFailed++ } }"
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
    'notepadState'      = @'
$packages = @(Get-AutoEraseDirectories -Path (Join-Path $env:LOCALAPPDATA 'Packages') -Filter 'Microsoft.WindowsNotepad_*')
foreach ($package in $packages) {
    try {
        $tabState = Join-Path $package.FullName 'LocalState\TabState'
        Erase-FileSet @(Get-AutoEraseFiles -Path $tabState -Recurse)
        if (@(Get-AutoEraseFiles -Path $tabState -Recurse).Count) { throw 'Notepad tab-state cleanup is incomplete.' }
    } catch { $script:AutoEraseFailed++ }
}
'@
    'pcaDatabase'       = @"
Get-AutoEraseFiles -Path "`$env:SystemRoot\appcompat\pca" | ForEach-Object { Erase-OneFile `$_.FullName }
"@
    'crashDumps'        = @"
`$targets = @(
    "`$env:LOCALAPPDATA\CrashDumps",
    "`$env:SystemRoot\Minidump",
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
foreach (`$root in @(`$env:LOCALAPPDATA, `$env:APPDATA)) {
    `$files = @(foreach (`$pattern in @('*.wal','*.shm')) { Get-AutoEraseFiles -Path `$root -Filter `$pattern -Recurse -Depth 5 })
    Erase-FileSet @(`$files | Select-Object -First 100)
    if (@(foreach (`$pattern in @('*.wal','*.shm')) { Get-AutoEraseFiles -Path `$root -Filter `$pattern -Recurse -Depth 5 }).Count) { throw 'WAL cleanup is incomplete within the scanned coverage.' }
}
"@
    'printSpooler'      = @"
Invoke-AutoEraseWithServices -Names Spooler -Body {
    Erase-FileSet @(Get-AutoEraseFiles -Path "`$env:SystemRoot\System32\spool\PRINTERS")
    Erase-FileSet @(Get-AutoEraseFiles -Path "`$env:LOCALAPPDATA\Microsoft\Windows\Spool" -Filter '*.spl')
}
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
if (`$LASTEXITCODE -ne 0) { throw 'BranchCache flush failed.' }
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
`$diagService = Get-Service DiagTrack -ErrorAction Stop
`$diagWasRunning = `$diagService.Status -eq 'Running'
try {
if (`$diagWasRunning) { Stop-Service DiagTrack -Force -ErrorAction Stop }
`$etDir = "`$env:ProgramData\Microsoft\Diagnosis\EventTranscript"
if (Test-Path `$etDir) {
    foreach (`$pattern in @('EventTranscript.db', '*.jrs', '*.rbs')) {
        Get-AutoEraseFiles -Path `$etDir -Filter `$pattern | ForEach-Object { Erase-OneFile `$_.FullName }
    }
}
`$etlDir = "`$env:ProgramData\Microsoft\Diagnosis\ETLLogs"
if (Test-Path `$etlDir) {
    Get-AutoEraseFiles -Path `$etlDir -Recurse -Filter '*.etl' | ForEach-Object { Erase-OneFile `$_.FullName }
}
} finally {
    if (`$diagWasRunning) { Start-Service DiagTrack -ErrorAction Stop }
}
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
        Get-AutoEraseFiles -Path `$t | ForEach-Object { Erase-OneFile `$_.FullName }
    }
}
"@
    'deviceInstallLogs'  = @"
`$infDir = "`$env:SystemRoot\INF"
if (Test-Path `$infDir) {
    Get-AutoEraseFiles -Path `$infDir -Filter 'setupapi.*.log' | ForEach-Object { Erase-OneFile `$_.FullName }
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
        Get-AutoEraseFiles -Path `$t -Filter '*.etl' | ForEach-Object { Erase-OneFile `$_.FullName }
    }
}
"@
    'defenderHistory'    = @"
`$historyDir = "`$env:ProgramData\Microsoft\Windows Defender\Scans\History\Service"
if (Test-Path `$historyDir) {
    Get-AutoEraseFiles -Path `$historyDir -Recurse | ForEach-Object { Erase-OneFile `$_.FullName }
}
`$mpLog = "`$env:ProgramData\Microsoft\Windows Defender\Support\MpCmdRun.log"
if (Test-Path `$mpLog) { Erase-OneFile `$mpLog }
"@
    # Firewall packet logs are operational records, not firewall policy.  The
    # firewall service opens a fresh log after removal, so this is safe to run
    # periodically under SYSTEM without changing rules or connectivity.
    'firewallLog'        = @"
foreach (`$profile in @(Get-NetFirewallProfile -ErrorAction Stop)) {
    `$path = [Environment]::ExpandEnvironmentVariables([string]`$profile.LogFileName)
    if (`$path -match '%[^%]+%') { throw 'A firewall log path could not be resolved.' }
    if (`$path) { Erase-OneFile `$path | Out-Null }
}
"@

    # Extended app-usage / office / web-cache / P2P update categories --------
    'appLaunchHistory' = @'
$bamRoot = 'HKLM:\SYSTEM\CurrentControlSet\Services\bam\State\UserSettings'
if ($null -ne (Get-AutoEraseRegistryItem $bamRoot)) {
    Get-ChildItem -LiteralPath $bamRoot -ErrorAction Stop | ForEach-Object {
        Erase-RegistryValues -Path $_.PSPath -ExcludedNames @('Version','SequenceNumber')
    }
}
'@
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
Get-AutoEraseDirectories -Path "`$env:LOCALAPPDATA" -Recurse -Filter 'EBWebView' -Depth 4 | ForEach-Object { Erase-Dir `$_.FullName }
"@
    'p2pUpdateCache'     = @"
Invoke-AutoEraseWithServices -Names DoSvc -Body {
`$doDir = "`$env:SystemRoot\SoftwareDistribution\DeliveryOptimization"
if (Test-Path `$doDir) {
    Get-ChildItem -Path `$doDir -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { try { Erase-OneFile `$_.FullName } catch {} }
}
}
"@
    'reliabilityHistory' = @"
`$racDir = "`$env:ProgramData\Microsoft\RAC\StateData"
if (Test-Path `$racDir) {
    Get-AutoEraseFiles -Path `$racDir -Recurse | ForEach-Object { Erase-OneFile `$_.FullName }
}
"@
    'remoteAccessLogs' = @'
$roots = @(
    (Join-Path $env:APPDATA 'TeamViewer'),
    (Join-Path $env:APPDATA 'AnyDesk'),
    (Join-Path $env:ProgramData 'AnyDesk'),
    (Join-Path $env:APPDATA 'RealVNC'),
    (Join-Path $env:LOCALAPPDATA 'Google\Chrome Remote Desktop')
)
foreach ($root in $roots) {
    foreach ($pattern in @('Connections*.txt', '*.trace', '*.log')) {
        Get-AutoEraseFiles -Path $root -Filter $pattern -Recurse | ForEach-Object { Erase-OneFile $_.FullName | Out-Null }
    }
}
'@
    'gameLauncherLogs' = @'
foreach ($root in @(
    (Join-Path ${env:ProgramFiles(x86)} 'Steam\logs'),
    (Join-Path $env:LOCALAPPDATA 'EpicGamesLauncher\Saved\Logs'),
    (Join-Path $env:APPDATA 'Battle.net\Logs'),
    (Join-Path $env:ProgramData 'Battle.net\Logs')
)) {
    Get-AutoEraseFiles -Path $root -Recurse | ForEach-Object { Erase-OneFile $_.FullName | Out-Null }
}
'@
    'spotlightCache' = @'
$packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
Get-AutoEraseDirectories -Path $packagesRoot -Filter 'Microsoft.Windows.ContentDeliveryManager_*' | ForEach-Object {
    Get-AutoEraseFiles -Path (Join-Path $_.FullName 'LocalState\Assets') -Recurse | ForEach-Object { Erase-OneFile $_.FullName | Out-Null }
}
'@
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
`$process = Start-Process cleanmgr.exe -ArgumentList "/sagerun:`$sageRunId" -Wait -NoNewWindow -PassThru -ErrorAction Stop
if (`$process.ExitCode -ne 0) { throw 'Disk cleanup failed.' }
"@
    'connectivityHistory' = @'
foreach ($root in @(
    'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Profiles',
    'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Signatures\Unmanaged',
    'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Signatures\Managed'
)) {
    if ($null -ne (Get-AutoEraseRegistryItem $root)) {
        Get-ChildItem -LiteralPath $root -ErrorAction Stop | ForEach-Object { Erase-RegistryTree $_.PSPath }
    }
}
'@
    'shadowCopies' = @'
$shadows = @(Get-CimInstance Win32_ShadowCopy -ErrorAction Stop)
foreach ($shadow in $shadows) {
    Remove-CimInstance -InputObject $shadow -ErrorAction Stop
    if (@(Get-CimInstance Win32_ShadowCopy -Filter ("ID='" + $shadow.ID.Replace("'", "''") + "'") -ErrorAction Stop).Count) { throw 'Shadow copy deletion could not be verified.' }
    $script:AutoEraseRemoved++
}
'@
    'amcache' = @'
Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:SystemRoot 'AppCompat\Programs') -Filter 'Amcache.hve*')
'@
    'searchIndex' = @'
Invoke-AutoEraseWithServices -Names WSearch -Body {
    $root = Join-Path $env:ProgramData 'Microsoft\Search\Data\Applications\Windows'
    foreach ($pattern in @('Windows.edb', 'Windows*.db*')) { Erase-FileSet @(Get-AutoEraseFiles -Path $root -Filter $pattern) }
    foreach ($sub in @('Projects', 'GatherLogs')) { Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $root $sub) -Recurse) }
}
'@
    'srumData' = @'
Invoke-AutoEraseWithServices -Names DPS -Body {
    Erase-OneFile (Join-Path $env:SystemRoot 'System32\sru\SRUDB.dat') | Out-Null
}
'@
    'recallDb' = @'
foreach ($relative in @('CoreAIPlatform.00','ConnectedDevicesPlatform')) {
    foreach ($pattern in @('*.db','*.db-wal','*.db-shm','*.db-journal','*.edb')) {
        Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:LOCALAPPDATA $relative) -Filter $pattern -Recurse)
    }
}
'@
    'developerCaches' = @'
foreach ($root in @(
    (Join-Path $env:APPDATA 'npm-cache'), (Join-Path $env:LOCALAPPDATA 'npm-cache'),
    (Join-Path $env:LOCALAPPDATA 'pip\Cache'), (Join-Path $env:USERPROFILE '.cargo\registry\cache'),
    (Join-Path $env:USERPROFILE '.cargo\registry\src')
)) {
    try {
        Erase-Dir $root
        try { $null = Get-Item -LiteralPath $root -Force -ErrorAction Stop; throw 'Developer cache cleanup is incomplete.' }
        catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0) }
    } catch { $script:AutoEraseFailed++ }
}
foreach ($relative in @('.npmrc','.cargo\config.toml','.cargo\credentials.toml')) {
    try {
        $target = Join-Path $env:USERPROFILE $relative
        Erase-OneFile $target | Out-Null
        if (Test-Path -LiteralPath $target -ErrorAction Stop) { throw 'Developer configuration cleanup is incomplete.' }
    } catch { $script:AutoEraseFailed++ }
}
'@
    'networkWizardHistory' = "Erase-RegistryTree 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Network\Network Connection Wizard'"
    'werHistory' = @'
foreach ($root in @('HKCU:\Software\Microsoft\Windows\Windows Error Reporting','HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting')) {
    foreach ($suffix in @('Consent','ExclusionList')) { Erase-RegistryTree (Join-Path $root $suffix) }
}
'@
    'stickyNotes' = @'
Get-AutoEraseDirectories -Path (Join-Path $env:LOCALAPPDATA 'Packages') -Filter 'Microsoft.MicrosoftStickyNotes_*' | ForEach-Object {
    foreach ($name in @('plum.sqlite','plum.sqlite-wal','plum.sqlite-shm')) { Erase-OneFile (Join-Path $_.FullName "LocalState\$name") | Out-Null }
}
'@
    'oneDriveMetadata' = @'
Get-AutoEraseDirectories -Path (Join-Path $env:LOCALAPPDATA 'Microsoft\OneDrive\settings') | ForEach-Object {
    foreach ($pattern in @('*.dat','*.db')) { Erase-FileSet @(Get-AutoEraseFiles -Path $_.FullName -Filter $pattern -Recurse) }
}
'@
    'fontCache' = @'
Invoke-AutoEraseWithServices -Names FontCache -Body {
    Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:WINDIR 'ServiceProfiles\LocalService\AppData\Local\FontCache') -Filter '*.dat' -Recurse)
}
'@
    'legacyIconCache' = "Erase-OneFile (Join-Path `$env:LOCALAPPDATA 'IconCache.db') | Out-Null"
    'gameCaptures' = @'
Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:USERPROFILE 'Videos\Captures') -Recurse)
Get-AutoEraseDirectories -Path (Join-Path $env:LOCALAPPDATA 'Packages') | Where-Object { $_.Name -like 'Microsoft.XboxGamingOverlay_*' -or $_.Name -like 'Microsoft.GamingApp_*' } | ForEach-Object {
    Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $_.FullName 'LocalState') -Recurse)
}
'@
    'photosCache' = @'
Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:LOCALAPPDATA 'Packages\Microsoft.Windows.Photos_8wekyb3d8bbwe\LocalState') -Recurse)
'@
    'xboxCache' = @'
Get-AutoEraseDirectories -Path (Join-Path $env:LOCALAPPDATA 'Packages') | Where-Object { $_.Name -like 'Microsoft.GamingApp_*' -or $_.Name -like 'Microsoft.XboxApp_*' } | ForEach-Object {
    Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $_.FullName 'LocalState') -Recurse)
}
'@
    'communicationCaches' = @'
foreach ($relative in @('Microsoft\Teams\Cache','Microsoft\Teams\IndexedDB','Slack\Cache','Slack\IndexedDB','Slack\Local Storage\leveldb','discord\Cache','discord\IndexedDB','discord\Local Storage\leveldb')) {
    Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:APPDATA $relative) -Recurse)
}
'@
    'editorHistory' = @'
foreach ($pattern in @('storage.json','state.vscdb')) { Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:APPDATA 'Code\User\globalStorage') -Filter $pattern) }
Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:APPDATA 'Code\User\History') -Recurse)
Get-AutoEraseDirectories -Path (Join-Path $env:APPDATA 'JetBrains') | ForEach-Object {
    Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $_.FullName 'options') -Filter 'recentProjects.xml' -Recurse)
    Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:LOCALAPPDATA ("JetBrains\{0}\LocalHistory" -f $_.Name)) -Recurse)
}
'@
    'sshState' = @'
try {
    if ((Invoke-WCSshAdd -Option '-D') -ne 0) { throw 'SSH agent identities could not be removed.' }
    if ((Invoke-WCSshAdd -Option '-l') -ne 1) { throw 'SSH agent cleanup could not be verified.' }
} catch { $script:AutoEraseFailed++ }
try {
    $knownHosts = Join-Path $env:USERPROFILE '.ssh\known_hosts'
    Erase-OneFile $knownHosts | Out-Null
    if (Test-Path -LiteralPath $knownHosts -ErrorAction Stop) { throw 'SSH known-host cleanup is incomplete.' }
} catch { $script:AutoEraseFailed++ }
'@
    'passwordManagerCaches' = @'
foreach ($relative in @('1Password\Cache','1Password\Local Storage\leveldb')) {
    try {
        $root = Join-Path $env:LOCALAPPDATA $relative
        Erase-Dir $root
        try { $null = Get-Item -LiteralPath $root -Force -ErrorAction Stop; throw 'Password-manager cache cleanup is incomplete.' }
        catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0) }
    } catch { $script:AutoEraseFailed++ }
}
foreach ($relative in @('Bitwarden\Cache','Bitwarden\Local Storage\leveldb','Bitwarden\Session Storage','LastPass\Cache','LastPass\Local Storage\leveldb','LastPass\Session Storage')) {
    try {
        $root = Join-Path $env:APPDATA $relative
        Erase-Dir $root
        try { $null = Get-Item -LiteralPath $root -Force -ErrorAction Stop; throw 'Password-manager cache cleanup is incomplete.' }
        catch [System.Management.Automation.ItemNotFoundException] { $Error.RemoveAt(0) }
    } catch { $script:AutoEraseFailed++ }
}
'@
    'adobeRecent' = @'
$root = 'HKCU:\Software\Adobe\Acrobat Reader'
if ($null -ne (Get-AutoEraseRegistryItem $root)) {
    Get-ChildItem -LiteralPath $root -ErrorAction Stop | ForEach-Object {
        foreach ($suffix in @('AVGeneral\cRecentFiles','AVGeneral\cDigitalSignatures')) { Erase-RegistryValues (Join-Path $_.PSPath $suffix) }
    }
}
'@
    'officeTempFiles' = @'
foreach ($relative in @('Documents','Desktop','Downloads')) {
    foreach ($pattern in @('~$*.doc*','~$*.xls*','~$*.ppt*')) { Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:USERPROFILE $relative) -Filter $pattern -Recurse -Depth 2) }
}
Erase-FileSet @(Get-AutoEraseFiles -Path $env:TEMP -Filter '*.tmp' -Recurse -Depth 2)
foreach ($relative in @('Documents','Desktop','Downloads')) {
    foreach ($pattern in @('~$*.doc*','~$*.xls*','~$*.ppt*')) {
        if (@(Get-AutoEraseFiles -Path (Join-Path $env:USERPROFILE $relative) -Filter $pattern -Recurse -Depth 2).Count) { throw 'Office temporary-file cleanup is incomplete.' }
    }
}
if (@(Get-AutoEraseFiles -Path $env:TEMP -Filter '*.tmp' -Recurse -Depth 2).Count) { throw 'Temporary-file cleanup is incomplete.' }
'@
    'neighborCache' = @'
$neighbors = @(Get-NetNeighbor -ErrorAction Stop | Where-Object { $_.State -ne 'Permanent' })
foreach ($neighbor in $neighbors) {
    Remove-NetNeighbor -InterfaceIndex $neighbor.InterfaceIndex -IPAddress $neighbor.IPAddress -Confirm:$false -ErrorAction Stop
    if (@(Get-NetNeighbor -ErrorAction Stop | Where-Object { $_.InterfaceIndex -eq $neighbor.InterfaceIndex -and $_.IPAddress -eq $neighbor.IPAddress }).Count) { throw 'Neighbor-cache cleanup could not be verified.' }
    $script:AutoEraseRemoved++
}
'@
    'netbiosCache' = @'
$null = & nbtstat.exe -R 2>&1
if ($LASTEXITCODE -ne 0) { throw 'NetBIOS-cache cleanup failed.' }
$cache = @(& nbtstat.exe -c 2>&1)
if ($LASTEXITCODE -ne 0 -or @($cache | Where-Object { $_ -match '<[0-9A-Fa-f]{2}>' }).Count) { throw 'NetBIOS-cache cleanup could not be verified.' }
'@
    'geolocationCache' = @'
Invoke-AutoEraseWithServices -Names lfsvc -Body {
    Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:ProgramData 'Microsoft\Windows\LfSvc') -Recurse)
}
'@
    'vpnPhonebooks' = @'
foreach ($root in @((Join-Path $env:APPDATA 'Microsoft\Network\Connections\Pbk'), (Join-Path $env:LOCALAPPDATA 'Microsoft\Network\Connections'), (Join-Path $env:ProgramData 'Microsoft\Network\Connections\Pbk'))) {
    foreach ($pattern in @('rasphone.pbk','*.log','*.etl')) { Erase-FileSet @(Get-AutoEraseFiles -Path $root -Filter $pattern -Recurse) }
}
'@
    'proxyCache' = "Erase-RegistryValues 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings\Connections'"
    'bitsQueue' = @'
$jobs = @(Get-BitsTransfer -AllUsers -ErrorAction Stop)
foreach ($job in $jobs) {
    Remove-BitsTransfer -BitsJob $job -Confirm:$false -ErrorAction Stop
    if (@(Get-BitsTransfer -AllUsers -ErrorAction Stop | Where-Object JobId -eq $job.JobId).Count) { throw 'BITS job removal could not be verified.' }
    $script:AutoEraseRemoved++
}
Invoke-AutoEraseWithServices -Names BITS -Body {
    Erase-FileSet @(Get-AutoEraseFiles -Path (Join-Path $env:ProgramData 'Microsoft\Network\Downloader') -Filter 'qmgr*.dat')
}
'@
}

# Public: the set of categoryIds this scheduler accepts. Frontend's
# Privacy Clean panel hardcodes the same set in privacyCleanCategories.ts
# (SUPPORTED_AUTOERASE_IDS) — keep them in sync.
$script:AutoEraseProtectedCategories = @(
    'wlanProfiles', 'browserFootprints', 'notepadState', 'wslData',
    'dockerDesktopData', 'virtualMachineArtifacts', 'developerCaches',
    'credentialManager', 'sshState', 'passwordManagerCaches'
)
# Newly supported cards require a per-card schedule; the bulk shortcut must
# never create these tasks, even when a client bypasses the UI. Keep this list
# in sync with MANUAL_ONLY_SCHEDULE_IDS in cleanupManualPolicy.ts.
$script:AutoEraseManualOnlyCategories = @(
    'connectivityHistory', 'shadowCopies', 'amcache', 'searchIndex', 'srumData', 'recallDb',
    'dockerDesktopData', 'virtualMachineArtifacts', 'developerCaches', 'networkWizardHistory', 'werHistory', 'stickyNotes', 'oneDriveMetadata',
    'spotlightCache', 'fontCache', 'legacyIconCache', 'gameCaptures', 'photosCache', 'xboxCache',
    'communicationCaches', 'editorHistory', 'sshState', 'remoteAccessLogs', 'passwordManagerCaches',
    'gameLauncherLogs', 'adobeRecent', 'officeTempFiles', 'neighborCache', 'netbiosCache',
    'geolocationCache', 'vpnPhonebooks', 'proxyCache', 'bitsQueue'
)
$script:AutoEraseUnsupportedCategories = [ordered]@{
    cloudPlaceholders = 'Requires provider-aware placeholder removal and read-back.'
    inactiveUserProtectionMetadata = 'Requires identity-bound recovery-key authorization.'
    cellularHistory = 'Requires locale-independent mobile-profile APIs.'
    gitActivity = 'Requires typed credential deletion and Git configuration verification.'
    ntfsJournals = 'Requires the native fixed-volume journal reset, sizing preservation, and identity read-back.'
}
function Get-AutoEraseSupportedCategories {
    $all = @($script:AutoEraseScripts.Keys | Where-Object { $_ -notin @('maintenanceTrim','diskCleanup') -and -not $script:AutoEraseUnsupportedCategories.Contains($_) } | ForEach-Object { if ($_ -eq 'clipboard') { 'clipboardHistory' } else { $_ } } | Sort-Object)
    @{
        categories = @($all | Where-Object { $_ -notin $script:AutoEraseProtectedCategories -and $_ -notin $script:AutoEraseManualOnlyCategories })
        schedulableCategories = $all
        unsupportedCategories = @($script:AutoEraseUnsupportedCategories.Keys | ForEach-Object { @{ categoryId=$_; reason=$script:AutoEraseUnsupportedCategories[$_] } })
    }
}

$script:AutoEraseSystemCategories = @(
    'wlanProfiles', 'eventLogs', 'eventTranscript', 'servicingLogs', 'deviceInstallLogs',
    'usageTraceLogs', 'appLaunchHistory', 'reliabilityHistory', 'defenderHistory', 'maintenanceTrim',
    'connectivityHistory', 'shadowCopies', 'amcache', 'searchIndex', 'srumData', 'branchCache',
    'neighborCache', 'netbiosCache', 'bitsQueue', 'p2pUpdateCache', 'fontCache', 'geolocationCache'
)
$script:AutoEraseHourlyCategories = @(
    'ntUserTraces', 'pcaDatabase', 'eventTranscript', 'activitiesTimeline',
    'servicingLogs', 'deviceInstallLogs', 'usageTraceLogs', 'defenderHistory',
    'firewallLog', 'appLaunchHistory', 'officeMru', 'reliabilityHistory',
    'explorerSearchHistory', 'remoteAccessLogs', 'gameLauncherLogs', 'spotlightCache',
    'amcache', 'searchIndex', 'srumData', 'recallDb', 'notepadState', 'developerCaches', 'networkWizardHistory', 'wslData', 'virtualMachineArtifacts', 'credentialManager', 'dockerDesktopData',
    'werHistory', 'stickyNotes', 'oneDriveMetadata', 'fontCache', 'legacyIconCache',
    'gameCaptures', 'photosCache', 'xboxCache', 'communicationCaches', 'editorHistory',
    'sshState', 'passwordManagerCaches', 'adobeRecent', 'officeTempFiles', 'neighborCache',
    'netbiosCache', 'geolocationCache', 'vpnPhonebooks', 'proxyCache', 'bitsQueue',
    'crashDumps', 'walFiles', 'printSpooler', 'webCache', 'thumbnailDb', 'notificationDb',
    'branchCache', 'rdpBitmapCache', 'embeddedWebCache', 'p2pUpdateCache', 'searchPersonalization'
)

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

function Get-AutoEraseNextDue {
    param($State, [string]$LegacyLastSuccess, [DateTime]$FirstDueUtc, [int]$IntervalMinutes)
    $style = [Globalization.DateTimeStyles]::RoundtripKind
    $culture = [Globalization.CultureInfo]::InvariantCulture
    $lastSuccess = $null
    if ($State -and $State.lastSuccessfulUtc) {
        $lastSuccess = [DateTime]::Parse([string]$State.lastSuccessfulUtc, $culture, $style).ToUniversalTime()
    } elseif ($LegacyLastSuccess) {
        $lastSuccess = [DateTime]::Parse($LegacyLastSuccess, $culture, $style).ToUniversalTime()
    }
    if ($State -and $State.nextDueUtc -and [int]$State.intervalMinutes -eq $IntervalMinutes) {
        $nextDue = [DateTime]::Parse([string]$State.nextDueUtc, $culture, $style).ToUniversalTime()
    } elseif ($lastSuccess) {
        $nextDue = $lastSuccess.AddMinutes($IntervalMinutes)
    } elseif ($State -and $State.attemptedUtc) {
        $nextDue = [DateTime]::Parse([string]$State.attemptedUtc, $culture, $style).ToUniversalTime().AddMinutes($IntervalMinutes)
    } else {
        $nextDue = $FirstDueUtc.ToUniversalTime()
    }
    @{ nextDue = $nextDue; lastSuccess = $lastSuccess }
}

function New-AutoEraseResult {
    param([DateTime]$CompletedUtc, $LastSuccess, [int]$IntervalMinutes, [int]$Removed, [int]$Deferred, [int]$Failed)
    $completed = $CompletedUtc.ToUniversalTime()
    $success = ($Failed -eq 0 -and $Deferred -eq 0)
    if ($success) { $LastSuccess = $completed }
    @{
        attemptedUtc = $completed.ToString('o')
        lastSuccessfulUtc = if ($LastSuccess) { $LastSuccess.ToUniversalTime().ToString('o') } else { $null }
        nextDueUtc = $completed.AddMinutes($IntervalMinutes).ToString('o')
        intervalMinutes = $IntervalMinutes; success = $success
        removed = $Removed; deferred = $Deferred; failed = $Failed
    }
}

function ConvertTo-AutoEraseTaskArgument {
    param(
        [string]$CategoryId,
        [int]$IntervalMinutes,
        [string]$Script,
        [string]$ExecutionScope = 'default',
        [string]$FirstDueUtc,
        [int]$PayloadRevision = 0,
        [bool]$AllowDefaultExcluded = $false
    )
    # Every scheduled task invocation goes through a small queue/catch-up wrapper:
    #   - one global mutex makes categories run sequentially instead of all at once
    #   - a last-run marker makes boot/logon triggers catch up after sleep/offline
    #     time while skipping duplicate runs shortly after a normal interval run.
    $categoryLiteral = $CategoryId.Replace("'", "''")
    $scopeKey = ConvertTo-AutoEraseSafeScope $ExecutionScope
    $scopeLiteral = $scopeKey.Replace("'", "''")
    $generation = [Guid]::NewGuid().ToString('N')
    $wrapper = @"
`$ErrorActionPreference = 'Stop'
`$categoryId = '$categoryLiteral'
`$allowDefaultExcluded = `$$($AllowDefaultExcluded.ToString().ToLowerInvariant())
if (`$categoryId -in @('wlanProfiles','browserFootprints','notepadState','wslData','dockerDesktopData','virtualMachineArtifacts','developerCaches','credentialManager','sshState','passwordManagerCaches') -and -not `$allowDefaultExcluded) { throw 'This protected schedule has no explicit manual authorization.' }
`$scopeKey = '$scopeLiteral'
`$payloadGeneration = '$generation'
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
`$firstDue = [DateTime]::Parse(`$firstDueUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind)
`$state = `$null
`$legacy = ''
if (Test-Path -LiteralPath `$marker) {
    try { `$state = Get-Content -LiteralPath `$marker -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop } catch { throw 'Scheduled cleanup state could not be read.' }
} elseif (Test-Path -LiteralPath `$legacyMarker) {
    try { `$legacy = [System.IO.File]::ReadAllText(`$legacyMarker).Trim() } catch { throw 'Scheduled cleanup history could not be read.' }
}
# Timing belongs to the installed payload generation. A replaced task may
# still be running and must not change this task's next due time.
if (-not `$state -or [string]`$state.payloadGeneration -ne `$payloadGeneration) { `$state = `$null }
`$legacy = ''
try { `$schedule = Get-AutoEraseNextDue `$state `$legacy `$firstDue `$intervalMinutes } catch { throw 'Scheduled cleanup timing is invalid.' }
`$due = (`$now.ToUniversalTime() -ge `$schedule.nextDue)
if (`$due) {
    `$mutex = New-Object System.Threading.Mutex(`$false, 'Global\WinCommanderAutoEraseQueue')
    `$hasLock = `$false
    try {
        `$hasLock = `$mutex.WaitOne()
        if (`$hasLock) {
            `$now = Get-Date
            if (Test-Path -LiteralPath `$marker) {
                try { `$state = Get-Content -LiteralPath `$marker -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop } catch { throw 'Scheduled cleanup state could not be read.' }
            } else { `$state = `$null }
            if (-not `$state -or [string]`$state.payloadGeneration -ne `$payloadGeneration) { `$state = `$null }
            try { `$schedule = Get-AutoEraseNextDue `$state `$legacy `$firstDue `$intervalMinutes } catch { throw 'Scheduled cleanup timing is invalid.' }
            `$lastSuccess = `$schedule.lastSuccess
            `$due = (`$now.ToUniversalTime() -ge `$schedule.nextDue)
            if (`$due) {
                `$Error.Clear()
                try {
__AUTO_ERASE_BODY__
                } catch { `$script:AutoEraseFailed++ }
                # Legacy category scripts can suppress a non-terminating error. It must still invalidate this run.
                if (`$Error.Count -gt 0 -and `$script:AutoEraseFailed -eq 0) { `$script:AutoEraseFailed++ }
                # Record a truthful outcome (not just "it ran"): how many files
                # were actually removed, how many are queued for reboot, and how
                # many could not be cleared. Consumed by the UI/telemetry so a
                # clear can never silently report success while leaving traces.
                try {
                    `$completedUtc = (Get-Date).ToUniversalTime()
                    `$res = New-AutoEraseResult `$completedUtc `$lastSuccess `$intervalMinutes `$script:AutoEraseRemoved `$script:AutoEraseDeferred `$script:AutoEraseFailed
                    `$res.payloadGeneration = `$payloadGeneration
                    [System.IO.File]::WriteAllText((Join-Path `$stateRoot ("`$categoryId.`$scopeKey.result.json")), (`$res | ConvertTo-Json -Compress))
                    [System.IO.File]::WriteAllText(`$marker, (`$res | ConvertTo-Json -Compress))
                } catch { throw 'Scheduled cleanup outcome could not be saved.' }
                if (-not `$res.success) { throw 'Scheduled cleanup was incomplete. See its result record.' }
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
    $dueFunction = "function Get-AutoEraseNextDue {`n" + ${function:Get-AutoEraseNextDue}.ToString() + "`n}"
    $resultFunction = "function New-AutoEraseResult {`n" + ${function:New-AutoEraseResult}.ToString() + "`n}"
    $categoryFunctions = switch ($CategoryId) {
        'credentialManager' { $script:ScheduledCredentialFunctions }
        'sshState' { $script:ScheduledSshFunctions }
        'wslData' { $script:ScheduledWslFunctions }
        'virtualMachineArtifacts' { $script:ScheduledVmFunctions }
        'dockerDesktopData' { $script:ScheduledDockerFunctions }
        default { '' }
    }
    $fullScript = $script:EraseFunctions + "`n" + $categoryFunctions + "`n" + $dueFunction + "`n" + $resultFunction + "`n" + $wrapper.Replace('__AUTO_ERASE_BODY__', $Script)

    $storage = Initialize-AutoEraseStorage
    $revisionSuffix = if ($PayloadRevision -gt 0) { ".r$PayloadRevision" } else { '' }
    # Stage a new payload for every registration. Reusing a path would mutate
    # an existing task before Scheduler has accepted its replacement.
    $scriptPath = Join-Path $storage.scripts "$CategoryId.$scopeKey$revisionSuffix.g$generation.ps1"
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
    remoteAccessLogs = '44'; gameLauncherLogs = '45'; spotlightCache = '46'
    connectivityHistory = '47'; shadowCopies = '48'; amcache = '49'; searchIndex = '50'
    srumData = '51'; recallDb = '52'; developerCaches = '53'; networkWizardHistory = '54'
    werHistory = '55'; stickyNotes = '56'; oneDriveMetadata = '57'; fontCache = '58'
    legacyIconCache = '59'; gameCaptures = '60'; photosCache = '61'; xboxCache = '62'
    communicationCaches = '63'; editorHistory = '64'; sshState = '65'; passwordManagerCaches = '66'
    adobeRecent = '67'; officeTempFiles = '68'; neighborCache = '69'; netbiosCache = '70'
    geolocationCache = '71'; vpnPhonebooks = '72'; proxyCache = '73'; bitsQueue = '74'
    wslData = '75'; dockerDesktopData = '76'; virtualMachineArtifacts = '77'; credentialManager = '78'
    cloudPlaceholders = '79'; inactiveUserProtectionMetadata = '80'; cellularHistory = '81'; gitActivity = '82'
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

function Invoke-AutoEraseScheduleLock {
    param([string]$CategoryId, [scriptblock]$Operation)
    if ($CategoryId -eq 'clipboardHistory') { $CategoryId = 'clipboard' }
    try { $code = Get-AutoEraseCategoryCode $CategoryId }
    catch { return @{ error = $true; message = 'Unknown cleanup schedule category.' } }
    $mutex = $null; $acquired = $false
    try {
        $mutex = New-Object System.Threading.Mutex($false, "Global\WinCommander.CleanupSchedule.$code")
        try { $acquired = $mutex.WaitOne([TimeSpan]::FromSeconds(15)) }
        catch [System.Threading.AbandonedMutexException] { $acquired = $true }
        if (-not $acquired) { return @{ error = $true; message = 'Another schedule update is still in progress.' } }
        & $Operation
    } catch { @{ error = $true; message = "Could not safely update the cleanup schedule: $($_.Exception.Message)" } }
    finally {
        if ($acquired -and $mutex) { try { $mutex.ReleaseMutex() } catch {} }
        if ($mutex) { $mutex.Dispose() }
    }
}

function Set-AutoEraseScheduleCore {
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
        [switch]$ManagedByAutoSet,
        [switch]$AllowDefaultExcluded
    )
    if ($CategoryId -eq 'clipboardHistory') { $CategoryId = 'clipboard' }
    if ($ManagedByAutoSet -and $CategoryId -in $script:AutoEraseManualOnlyCategories) {
        return @{ error = $true; message = "Category '$CategoryId' requires an individual schedule and cannot be auto-set" }
    }
    if ($CategoryId -in $script:AutoEraseProtectedCategories -and ($ManagedByAutoSet -or -not $AllowDefaultExcluded)) {
        return @{ error = $true; message = "Category '$CategoryId' requires explicit per-card authorization and cannot be auto-set" }
    }
    if ($script:AutoEraseUnsupportedCategories.Contains($CategoryId)) { return @{ error=$true; message=$script:AutoEraseUnsupportedCategories[$CategoryId] } }
    if (-not $script:AutoEraseScripts.ContainsKey($CategoryId)) {
        return @{ error = $true; message = "Category '$CategoryId' is not supported for auto-erase" }
    }
    if ($IntervalMinutes -lt 1) {
        return @{ error = $true; message = "IntervalMinutes must be >= 1" }
    }
    if ($CategoryId -in $script:AutoEraseHourlyCategories -and $IntervalMinutes -lt 60) {
        return @{ error = $true; message = "This category requires an interval of at least 60 minutes" }
    }
    if ($CategoryId -in $script:AutoEraseSystemCategories) {
        $RunAsSystem = $true
    } elseif ($RunAsSystem) {
        return @{ error = $true; message = "This category requires the selected user's context" }
    }
    Assert-AutoEraseAdmin

    try {
        $ownerSid = if ($RunAsSystem) { 'S-1-5-18' } else { Resolve-AutoEraseSid $TargetUser }
        $migration = Invoke-AutoEraseMigrationCore -CategoryId $CategoryId
        if ($migration.error) { throw $migration.message }
        Initialize-SystemMaintenanceTaskFolder
        $taskName = Get-AutoEraseTaskName $CategoryId $ownerSid
        # Enumerate with terminating errors: suppressing a Scheduler access error
        # here could mistake an occupied name for an absent task and force-replace it.
        $existingTasksAtName = @(Get-ScheduledTask -ErrorAction Stop | Where-Object {
            $_.TaskPath -eq $script:SystemMaintenanceTaskPath -and $_.TaskName -eq $taskName
        })
        if ($existingTasksAtName.Count -gt 1) { throw 'The existing task name could not be verified uniquely.' }
        $existing = if ($existingTasksAtName.Count -eq 1) { $existingTasksAtName[0] } else { $null }
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
                taskState       = [string]$existing.State
            }
        }

        $eraseScript = $script:AutoEraseScripts[$CategoryId]
        $scope = if ($RunAsSystem) { 'system' } else { "user-$TargetUser" }
        $firstDueUtc = [DateTime]::UtcNow.AddMinutes($IntervalMinutes).ToString('o')
        $argument = ConvertTo-AutoEraseTaskArgument -CategoryId $CategoryId -IntervalMinutes $IntervalMinutes -Script $eraseScript -ExecutionScope $scope -FirstDueUtc $firstDueUtc -AllowDefaultExcluded ([bool]$AllowDefaultExcluded)

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
                    -StartWhenAvailable

        if ($RunAsSystem) {
            $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
        } elseif ($CategoryId -in @('credentialManager', 'wslData', 'virtualMachineArtifacts', 'dockerDesktopData')) {
            # These providers are bound to the user's interactive session.
            $principal = New-ScheduledTaskPrincipal -UserId $TargetUser -LogonType Interactive -RunLevel Highest
        } else {
            $principal = New-ScheduledTaskPrincipal -UserId $TargetUser -LogonType S4U -RunLevel Highest
        }

        $description = if ($CategoryId -in $script:AutoEraseProtectedCategories) {
            'System Maintenance scheduled cleanup v17 (manual opt-in)'
        } elseif ($ManagedByAutoSet) {
            'System Maintenance scheduled cleanup v17 (managed)'
        } else {
            'System Maintenance scheduled cleanup v17'
        }
        Register-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $taskName -Action $action -Trigger $triggers `
                               -Principal $principal -Settings $settings -Description $description -Force -ErrorAction Stop | Out-Null

        # A successful registration call alone is not proof that Scheduler retained
        # the requested task. Fail closed when its independent readback is missing
        # or differs; never delete a task in response to a failed verification.
        $saved = Get-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $taskName -ErrorAction Stop
        $savedIdentity = if ($saved) { Get-AutoEraseTaskIdentity $saved } else { $null }
        $savedActions = @($saved.Actions)
        $savedIntervals = @($saved.Triggers | ForEach-Object { $_.Repetition.Interval } | Where-Object { $_ })
        if (-not $savedIdentity -or $savedIdentity.categoryId -ne $CategoryId -or $savedIdentity.ownerSid -ne $ownerSid -or
            $saved.TaskPath -ne $script:SystemMaintenanceTaskPath -or $saved.TaskName -ne $taskName -or
            $savedActions.Count -ne 1 -or [string]$savedActions[0].Execute -ne 'powershell.exe' -or
            [string]$savedActions[0].Arguments -ne $argument -or [string]$saved.Description -ne $description -or
            -not (Test-AutoEraseTaskEnabled ([string]$saved.State)) -or $savedIntervals.Count -ne 1 -or
            [Xml.XmlConvert]::ToTimeSpan([string]$savedIntervals[0]) -ne (New-TimeSpan -Minutes $IntervalMinutes)) {
            throw 'The saved schedule could not be verified in Task Scheduler. Refresh schedules before trying again.'
        }

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

function Set-AutoEraseSchedule {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string]$CategoryId,
        [Parameter(Mandatory)] [int]$IntervalMinutes,
        [bool]$RunAsSystem = $false,
        [string]$TargetUser = $env:USERNAME,
        [switch]$PreserveExisting,
        [switch]$ManagedByAutoSet,
        [switch]$AllowDefaultExcluded
    )
    $forward = @{ CategoryId=$CategoryId; IntervalMinutes=$IntervalMinutes; RunAsSystem=$RunAsSystem; TargetUser=$TargetUser; PreserveExisting=[bool]$PreserveExisting; ManagedByAutoSet=[bool]$ManagedByAutoSet; AllowDefaultExcluded=[bool]$AllowDefaultExcluded }
    Invoke-AutoEraseScheduleLock -CategoryId $CategoryId -Operation { Set-AutoEraseScheduleCore @forward }
}

# This separate command rejects an older sidecar that cannot enforce explicit
# per-card consent for a default-excluded target.
function Set-IndividualAutoEraseSchedule {
    [CmdletBinding()]
    param([Parameter(Mandatory)] [string]$CategoryId, [Parameter(Mandatory)] [int]$IntervalMinutes, [bool]$RunAsSystem = $false, [switch]$AllowDefaultExcluded)
    Set-AutoEraseSchedule -CategoryId $CategoryId -IntervalMinutes $IntervalMinutes -RunAsSystem $RunAsSystem -AllowDefaultExcluded:$AllowDefaultExcluded
}

# This separate command prevents an older sidecar from silently ignoring the
# flags that protect existing individual tasks during Auto-set.
function Set-ManagedAutoEraseSchedule {
    [CmdletBinding()]
    param([Parameter(Mandatory)] [string]$CategoryId, [Parameter(Mandatory)] [int]$IntervalMinutes, [bool]$RunAsSystem = $false)
    Invoke-AutoEraseScheduleLock -CategoryId $CategoryId -Operation {
    if ($CategoryId -eq 'clipboardHistory') { $CategoryId = 'clipboard' }
    if ($CategoryId -in $script:AutoEraseProtectedCategories -or $CategoryId -in $script:AutoEraseManualOnlyCategories) {
        return @{ error = $true; message = "Category '$CategoryId' requires an individual schedule and cannot be auto-set" }
    }
    if ($script:AutoEraseUnsupportedCategories.Contains($CategoryId) -or -not $script:AutoEraseScripts.ContainsKey($CategoryId)) {
        return @{ error = $true; message = "Category '$CategoryId' is not supported for auto-erase" }
    }
    if ($IntervalMinutes -lt 1 -or ($CategoryId -in $script:AutoEraseHourlyCategories -and $IntervalMinutes -lt 60)) {
        return @{ error = $true; message = 'The requested schedule interval is invalid.' }
    }
    Assert-AutoEraseAdmin
    try {
        # Preserve a current-user OR SYSTEM task for this category, including
        # disabled tasks, before choosing an execution account for the new task.
        $currentSid = Resolve-AutoEraseSid $env:USERNAME
        $existingTasks = @()
        foreach ($task in @(Get-ScheduledTask -ErrorAction Stop)) {
            $identity = Get-AutoEraseTaskIdentity $task
            if ($identity -and $identity.categoryId -eq $CategoryId -and $identity.ownerSid -in @($currentSid, 'S-1-5-18')) { $existingTasks += $task }
        }
        if ($existingTasks.Count -gt 1) { return @{ error = $true; message = 'Multiple existing schedules need review before Auto-set.' } }
        if ($existingTasks.Count -eq 1) {
            $state = [string]$existingTasks[0].State
            if ($state -notin @('Ready', 'Running', 'Queued', 'Disabled')) { return @{ error = $true; message = 'Existing schedule state could not be verified.' } }
            return @{ status = 'alreadyConfigured'; categoryId = $CategoryId; taskState = $state; taskName = $existingTasks[0].TaskName }
        }
    } catch { return @{ error = $true; message = "Could not verify existing schedules for '$CategoryId': $($_.Exception.Message)" } }
    Set-AutoEraseScheduleCore -CategoryId $CategoryId -IntervalMinutes $IntervalMinutes -RunAsSystem $RunAsSystem -PreserveExisting -ManagedByAutoSet
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
        [string]$TargetUsers = '',
        [switch]$ManagedByAutoSet,
        [switch]$AllowDefaultExcluded
    )
    if ($CategoryId -eq 'clipboardHistory') { $CategoryId = 'clipboard' }
    # Bulk setup has no cross-user UI and cannot safely preserve every user's
    # existing choice through this legacy path. Require per-user setup instead.
    if ($ManagedByAutoSet) { return @{ error = $true; status = 'failed'; message = 'Managed multi-user schedules are not supported.' } }
    if ($ManagedByAutoSet -and $CategoryId -in $script:AutoEraseManualOnlyCategories) {
        return @{ error = $true; message = "Category '$CategoryId' requires an individual schedule and cannot be auto-set" }
    }
    if ($CategoryId -in $script:AutoEraseProtectedCategories -and ($ManagedByAutoSet -or -not $AllowDefaultExcluded)) {
        return @{ error = $true; message = "Category '$CategoryId' requires explicit per-card authorization and cannot be auto-set" }
    }
    if ($script:AutoEraseUnsupportedCategories.Contains($CategoryId)) { return @{ error=$true; message=$script:AutoEraseUnsupportedCategories[$CategoryId] } }
    if (-not $script:AutoEraseScripts.ContainsKey($CategoryId)) { return @{ error=$true; message="Category '$CategoryId' is not supported for auto-erase" } }
    if ($CategoryId -in $script:AutoEraseSystemCategories) { $RunAsSystem = $true }
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
            -TargetUser      'SYSTEM' `
            -ManagedByAutoSet:$ManagedByAutoSet `
            -AllowDefaultExcluded:$AllowDefaultExcluded
        if ($r.error) { return @{ error = $true; status = 'failed'; results = @($r); total = 1 } }
        return @{ status = 'ok'; results = @($r); total = 1 }
    }

    $results = @()
    foreach ($user in $targetArr) {
        $r = Set-AutoEraseSchedule `
            -CategoryId      $CategoryId `
            -IntervalMinutes $IntervalMinutes `
            -RunAsSystem     $RunAsSystem `
            -TargetUser      $user `
            -ManagedByAutoSet:$ManagedByAutoSet `
            -AllowDefaultExcluded:$AllowDefaultExcluded
        $results += $r
    }
    if (@($results | Where-Object { $_.error }).Count) { return @{ error = $true; status = 'failed'; results = $results; total = $results.Count } }
    @{ status = 'ok'; results = $results; total = $results.Count }
}

# Remove-MultiUserAutoEraseSchedule: remove per-user tasks created by
# Set-MultiUserAutoEraseSchedule. Pass the same TargetUsers list used when
# creating; empty = remove for all users (finds all matching task names).
function Remove-MultiUserAutoEraseScheduleCore {
    [CmdletBinding()]
    param([Parameter(Mandatory)] [string]$CategoryId, [string]$TargetUsers = '', [switch]$ManagedOnly)
    Assert-AutoEraseAdmin
    try {
        $owners = @($TargetUsers -split ',' | Where-Object { $_.Trim() } |
            ForEach-Object { Resolve-AutoEraseSid $_.Trim() })
        $removed = @()
        $removedOwnerSids = @()
        foreach ($task in @(Get-ScheduledTask -ErrorAction Stop)) {
            $identity = Get-AutoEraseTaskIdentity $task
            if (-not $identity -or $identity.categoryId -ne $CategoryId) { continue }
            if ($owners.Count -gt 0 -and $identity.ownerSid -notin $owners) { continue }
            if ($ManagedOnly -and -not (Test-AutoEraseManagedTask $task)) { continue }
            Unregister-ScheduledTask -TaskPath $task.TaskPath -TaskName $task.TaskName -Confirm:$false -ErrorAction Stop
            $removed += $task.TaskName
            $removedOwnerSids += $identity.ownerSid
        }
        @{ status = 'disabled'; categoryId = $CategoryId; removed = $removed; removedOwnerSids = $removedOwnerSids }
    } catch { @{ error = $true; message = "Failed to remove schedule: $($_.Exception.Message)" } }
}

function Remove-MultiUserAutoEraseSchedule {
    [CmdletBinding()]
    param([Parameter(Mandatory)] [string]$CategoryId, [string]$TargetUsers = '', [switch]$ManagedOnly)
    $forward = @{ CategoryId=$CategoryId; TargetUsers=$TargetUsers; ManagedOnly=[bool]$ManagedOnly }
    Invoke-AutoEraseScheduleLock -CategoryId $CategoryId -Operation { Remove-MultiUserAutoEraseScheduleCore @forward }
}

function Get-AutoEraseRemovedScopeKeys {
    param([string]$CurrentSid, [string]$CurrentUser, [string[]]$RemovedOwnerSids, [switch]$ManagedOnly)
    if (-not $ManagedOnly -or $CurrentSid -in $RemovedOwnerSids) { "user-$CurrentUser" }
    if (-not $ManagedOnly -or 'S-1-5-18' -in $RemovedOwnerSids) { 'system' }
}

function Remove-AutoEraseScheduleCore {
    [CmdletBinding()]
    param([Parameter(Mandatory)] [string]$CategoryId, [switch]$ManagedOnly)
    Assert-AutoEraseAdmin
    try {
        $currentSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $owners = @($currentSid, 'S-1-5-18')
        $result = Remove-MultiUserAutoEraseScheduleCore -CategoryId $CategoryId -TargetUsers ($owners -join ',') -ManagedOnly:$ManagedOnly
        if ($result.error) { throw $result.message }
        # A disabled schedule must not leave a catch-up marker behind. Limit
        # cleanup to this caller's base task plus the system scope; explicitly
        # selected other-user schedules have their own remove command/state.
        $scopeKeys = @(Get-AutoEraseRemovedScopeKeys -CurrentSid $currentSid -CurrentUser $env:USERNAME -RemovedOwnerSids @($result.removedOwnerSids) -ManagedOnly:$ManagedOnly)
        foreach ($root in @(
            (Join-Path $env:LOCALAPPDATA 'WinCommander\auto-erase'),
            (Join-Path $env:ProgramData 'WinCommander\auto-erase\state')
        )) {
            foreach ($scopeKey in $scopeKeys) {
                Remove-Item -LiteralPath (Join-Path $root "$CategoryId.$scopeKey.state.json") -Force -ErrorAction SilentlyContinue
                Remove-Item -LiteralPath (Join-Path $root "$CategoryId.$scopeKey.last") -Force -ErrorAction SilentlyContinue
            }
        }
        @{ status = 'disabled'; categoryId = $CategoryId; removed = @($result.removed) }
    }
    catch {
        @{ error = $true; message = "Failed to remove auto-erase schedule for '$CategoryId': $($_.Exception.Message)" }
    }
}

function Remove-AutoEraseSchedule {
    [CmdletBinding()]
    param([Parameter(Mandatory)] [string]$CategoryId, [switch]$ManagedOnly)
    $forward = @{ CategoryId=$CategoryId; ManagedOnly=[bool]$ManagedOnly }
    Invoke-AutoEraseScheduleLock -CategoryId $CategoryId -Operation { Remove-AutoEraseScheduleCore @forward }
}

# Separate command identity makes an older sidecar reject this request instead
# of ignoring an unknown flag and removing a manually configured task.
function Remove-ManagedAutoEraseSchedule {
    [CmdletBinding()]
    param([Parameter(Mandatory)] [string]$CategoryId)
    Remove-AutoEraseSchedule -CategoryId $CategoryId -ManagedOnly
}

function Test-AutoEraseManagedTask {
    param($Task)
    return ($Task.Description -match '^System Maintenance scheduled cleanup v(?:[456789]|10|11|12|13|14|15|16|17) \(managed\)$' -or $Task.Description -like 'WinCommander Auto-set scheduled wipe*')
}

function Test-AutoEraseTaskEnabled {
    param([string]$State)
    return $State -in @('Ready', 'Running', 'Queued')
}

function ConvertTo-AutoEraseTimestamp {
    param($Value)
    if ($null -eq $Value -or ($Value -isnot [datetime] -and $Value -isnot [datetimeoffset])) { return $null }
    if ($Value.Year -le 1900 -or ($Value.Year -eq 1999 -and $Value.Month -eq 11 -and $Value.Day -eq 30)) { return $null }
    return $Value.ToString('o', [Globalization.CultureInfo]::InvariantCulture)
}

function Get-AutoEraseRecordedOutcome {
    param($Task, $Identity, [string]$CurrentSid, [int]$IntervalMinutes)
    try {
        if ($Identity.ownerSid -notin @($CurrentSid, 'S-1-5-18') -or $IntervalMinutes -lt 1) { return $null }
        if ($Task.TaskPath -ne $script:SystemMaintenanceTaskPath -or $Task.TaskName -ne $Identity.taskName) { return $null }
        $arguments = [string]@($Task.Actions)[0].Arguments
        if ($arguments -notmatch '(?i)(?:^|\s)-File\s+"([^"]+)"\s*$') { return $null }
        $scriptPath = [IO.Path]::GetFullPath($Matches[1])
        $scriptsRoot = [IO.Path]::GetFullPath((Join-Path $env:ProgramData 'WinCommander\auto-erase\scripts'))
        if ([IO.Path]::GetDirectoryName($scriptPath) -ine $scriptsRoot) { return $null }
        $scriptItem = Get-Item -LiteralPath $scriptPath -Force -ErrorAction Stop
        if ($scriptItem.PSIsContainer -or ($scriptItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $scriptItem.Length -gt 1048576) { return $null }
        $payload = [IO.File]::ReadAllText($scriptPath)
        $categoryPattern = '(?m)^\$categoryId = ''' + [regex]::Escape([string]$Identity.categoryId) + '''\r?$'
        if ($payload -notmatch $categoryPattern -or $payload -notmatch '(?m)^\$scopeKey = ''([A-Za-z0-9._-]{1,80})''\r?$') { return $null }
        $scopeKey = $Matches[1]
        if ($Identity.ownerSid -eq 'S-1-5-18') {
            if ($scopeKey -ne 'system') { return $null }
            $root = Join-Path $env:ProgramData 'WinCommander\auto-erase\state'
        } else {
            if (-not $scopeKey.StartsWith('user-', [StringComparison]::Ordinal)) { return $null }
            $root = Join-Path $env:LOCALAPPDATA 'WinCommander\auto-erase'
        }
        $expected = '^' + [regex]::Escape([string]$Identity.categoryId + '.' + $scopeKey) + '(?:\.r\d+)?(?:\.g[0-9a-f]{32})?\.ps1$'
        if ([IO.Path]::GetFileName($scriptPath) -notmatch $expected) { return $null }
        $path = Join-Path $root ("$($Identity.categoryId).$scopeKey.result.json")
        $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -gt 16384) { return $null }
        $result = [IO.File]::ReadAllText($path) | ConvertFrom-Json -ErrorAction Stop
        # A previously running generation may finish after a task was replaced.
        # Only attribute a receipt to the generated task that actually wrote it.
        if ([IO.Path]::GetFileName($scriptPath) -match '\.g([0-9a-f]{32})\.ps1$') {
            $fileGeneration = $Matches[1]
            if ($payload -match "(?m)^\`$payloadGeneration = '([0-9a-f]{32})'\r?$") {
                if ($Matches[1] -ne $fileGeneration -or [string]$result.payloadGeneration -ne $fileGeneration) { return $null }
            } else { return $null }
        }
        if ($result.success -isnot [bool] -or $result.intervalMinutes -isnot [int] -or $result.intervalMinutes -ne $IntervalMinutes) { return $null }
        foreach ($key in @('removed', 'deferred', 'failed')) {
            if ($result.$key -isnot [int] -or $result.$key -lt 0) { return $null }
        }
        if ($result.success -ne ($result.failed -eq 0 -and $result.deferred -eq 0)) { return $null }
        $attempt = [DateTimeOffset]::MinValue
        if (-not [DateTimeOffset]::TryParseExact([string]$result.attemptedUtc, 'o', [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::None, [ref]$attempt)) { return $null }
        if ($attempt.UtcDateTime -lt $scriptItem.LastWriteTimeUtc -or $attempt.UtcDateTime -gt [DateTime]::UtcNow.AddMinutes(5)) { return $null }
        return @{ attemptedUtc = $attempt.ToUniversalTime().ToString('o'); success = $result.success; removed = $result.removed; deferred = $result.deferred; failed = $result.failed }
    } catch { return $null }
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
                legacy           = ($t.TaskPath -ne $script:SystemMaintenanceTaskPath -or $t.TaskName -ne $identity.taskName)
                state            = [string]$t.State
                enabled          = (Test-AutoEraseTaskEnabled ([string]$t.State))
                intervalMinutes  = $minutes
                targetUser       = if ($identity.ownerSid -notin @($currentSid, 'S-1-5-18')) { [string]$t.Principal.UserId } else { $null }
                scope            = if ($identity.ownerSid -eq 'S-1-5-18') { 'system' } elseif ($identity.ownerSid -eq $currentSid) { 'current-user' } else { 'other-user' }
                ownerAccount     = $t.Principal.UserId
                managedByAutoSet = (Test-AutoEraseManagedTask $t)
                lastRun          = if ($info) { ConvertTo-AutoEraseTimestamp $info.LastRunTime } else { $null }
                nextRun          = if ($info) { ConvertTo-AutoEraseTimestamp $info.NextRunTime } else { $null }
                lastResult       = if ($info) { $info.LastTaskResult } else { $null }
                cleanupOutcome   = Get-AutoEraseRecordedOutcome $t $identity $currentSid $minutes
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

function Test-AutoEraseCurrentPayload {
    param($Task, $Identity)
    $argument = [string](@($Task.Actions)[0].Arguments)
    if ($argument -notmatch '(?i)(?:^|\s)-File\s+"([^"]+)"\s*$') { return $false }
    $path = [IO.Path]::GetFullPath($Matches[1])
    $root = [IO.Path]::GetFullPath((Join-Path $env:ProgramData 'WinCommander\auto-erase\scripts'))
    if ([IO.Path]::GetDirectoryName($path) -ine $root) { throw 'The cleanup task payload path is not owned.' }
    $parent = [IO.Directory]::GetParent($path)
    while ($parent) {
        $directory = Get-Item -LiteralPath $parent.FullName -Force -ErrorAction Stop
        if (-not $directory.PSIsContainer -or ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The cleanup task payload directory cannot be verified.' }
        $parent = $parent.Parent
    }
    try { $file = Get-Item -LiteralPath $path -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { return $false }
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $file.Length -gt 1MB) { throw 'The cleanup task payload cannot be verified.' }
    $body = Get-Content -LiteralPath $path -Raw -ErrorAction Stop
    $category = '(?m)^\$categoryId = ''' + [regex]::Escape([string]$Identity.categoryId) + '''\r?$'
    if (-not $body -or $body -notmatch $category) { throw 'The cleanup task payload cannot be verified.' }
    return $true
}

# Copy task XML instead of recreating schedules: dates, disabled state, selected
# account, scope and settings belong to the user and survive a naming migration.
function Invoke-AutoEraseMigrationCore {
    [CmdletBinding()]
    param([string]$CategoryId = '')
    Assert-AutoEraseAdmin
    Initialize-SystemMaintenanceTaskFolder
    $migrated = @()
    $disabledProtected = @()
    $errors = @()
    foreach ($task in @(Get-ScheduledTask -ErrorAction Stop)) {
        try {
            $identity = Get-AutoEraseTaskIdentity $task
            if (-not $identity -or ($CategoryId -and $identity.categoryId -ne $CategoryId)) { continue }
            $manualOptIn = $task.Description -match '^System Maintenance scheduled cleanup v(?:[6789]|10|11|12|13|14|15|16|17) \(manual opt-in\)$'
            if ($script:AutoEraseUnsupportedCategories.Contains($identity.categoryId) -or ($identity.categoryId -in $script:AutoEraseProtectedCategories -and -not $manualOptIn)) {
                Disable-ScheduledTask -TaskPath $task.TaskPath -TaskName $task.TaskName -ErrorAction Stop | Out-Null
                $protectedReadback = Get-ScheduledTask -TaskPath $task.TaskPath -TaskName $task.TaskName -ErrorAction Stop
                if ([string]$protectedReadback.State -ne 'Disabled') { throw 'A protected cleanup schedule could not be disabled.' }
                $disabledProtected += $identity.categoryId
                continue
            }
            $currentPayload = $task.Description -match '^System Maintenance scheduled cleanup v17(?: \((?:manual opt-in|managed)\))?$'
            $argument = [string](@($task.Actions)[0].Arguments)
            $upgradePayload = -not ($argument -like '*WinCommander\auto-erase\scripts*' -and $currentPayload)
            if (-not $upgradePayload -and -not (Test-AutoEraseCurrentPayload -Task $task -Identity $identity)) { $upgradePayload = $true }
            $makeVisible = $task.Settings -and $task.Settings.Hidden -eq $true
            if ($task.TaskPath -eq $script:SystemMaintenanceTaskPath -and $task.TaskName -eq $identity.taskName -and -not $upgradePayload -and -not $makeVisible) { continue }
            if ([string]$task.State -eq 'Running') { throw 'An active scheduled cleanup must finish before its task can be migrated.' }
            $renaming = $task.TaskName -ne $identity.taskName -or $task.TaskPath -ne $script:SystemMaintenanceTaskPath
            if ($renaming -and (Get-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $identity.taskName -ErrorAction SilentlyContinue)) {
                throw 'The replacement task name is occupied. Both existing tasks were left unchanged.'
            }
            $originalXml = Export-ScheduledTask -TaskPath $task.TaskPath -TaskName $task.TaskName -ErrorAction Stop
            [xml]$xml = $originalXml
            if ($makeVisible) {
                $hidden = $xml.SelectSingleNode('/*[local-name()="Task"]/*[local-name()="Settings"]/*[local-name()="Hidden"]')
                if (-not $hidden) { throw 'The hidden task setting could not be verified.' }
                $hidden.InnerText = 'false'
            }
            if ($upgradePayload) {
                $repetition = @($task.Triggers | ForEach-Object { $_.Repetition.Interval } | Where-Object { $_ }) | Select-Object -First 1
                if (-not $repetition) { throw 'The existing schedule interval could not be checked.' }
                $minutes = [int][Xml.XmlConvert]::ToTimeSpan($repetition).TotalMinutes
                if ($minutes -lt 1) { throw 'The existing schedule interval is invalid.' }
                $owner = [string]$task.Principal.UserId
                $scope = if ($identity.ownerSid -eq 'S-1-5-18') { 'system' } else { "user-$owner" }
                $info = Get-ScheduledTaskInfo -TaskPath $task.TaskPath -TaskName $task.TaskName -ErrorAction Stop
                $firstDue = if ($info.NextRunTime -gt [DateTime]::MinValue) { $info.NextRunTime.ToUniversalTime() } else { [DateTime]::UtcNow.AddMinutes($minutes) }
                $revision = 17
                $xml.Task.Actions.Exec.Arguments = ConvertTo-AutoEraseTaskArgument -CategoryId $identity.categoryId -IntervalMinutes $minutes -Script $script:AutoEraseScripts[$identity.categoryId] -ExecutionScope $scope -FirstDueUtc $firstDue.ToString('o') -PayloadRevision $revision -AllowDefaultExcluded $manualOptIn
                $xml.Task.RegistrationInfo.Description = if ($manualOptIn) { 'System Maintenance scheduled cleanup v17 (manual opt-in)' } elseif ($task.Description -like '*managed*') { 'System Maintenance scheduled cleanup v17 (managed)' } else { 'System Maintenance scheduled cleanup v17' }
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
    if ($errors.Count) { return @{ error = $true; status = 'partial'; migrated = $migrated; disabledProtected = $disabledProtected; message = ($errors -join ' '); errors = $errors } }
    @{ status = 'ok'; migrated = $migrated; disabledProtected = $disabledProtected }
}

# A migration can rewrite an existing task. Serialize each category with
# create and remove so it cannot overwrite a newly chosen individual cadence.
function Invoke-AutoEraseMigration {
    [CmdletBinding()]
    param([string]$CategoryId = '')
    if ($CategoryId) {
        return Invoke-AutoEraseScheduleLock -CategoryId $CategoryId -Operation { Invoke-AutoEraseMigrationCore -CategoryId $CategoryId }
    }
    Assert-AutoEraseAdmin
    try {
        $ids = @(@(Get-ScheduledTask -ErrorAction Stop) | ForEach-Object {
            $identity = Get-AutoEraseTaskIdentity $_
            if ($identity) { $identity.categoryId }
        } | Sort-Object -Unique)
        $migrated = @(); $disabledProtected = @(); $errors = @()
        foreach ($id in $ids) {
            $result = Invoke-AutoEraseScheduleLock -CategoryId $id -Operation { Invoke-AutoEraseMigrationCore -CategoryId $id }
            if ($result.migrated) { $migrated += @($result.migrated) }
            if ($result.disabledProtected) { $disabledProtected += @($result.disabledProtected) }
            if ($result.error) { $errors += [string]$result.message }
        }
        if ($errors.Count) { return @{ error=$true; status='partial'; migrated=$migrated; disabledProtected=$disabledProtected; errors=$errors; message=($errors -join ' ') } }
        @{ status='ok'; migrated=$migrated; disabledProtected=$disabledProtected }
    } catch { @{ error=$true; status='failed'; message="Could not inspect cleanup schedules for migration: $($_.Exception.Message)" } }
}
