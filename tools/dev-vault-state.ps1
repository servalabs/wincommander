[CmdletBinding()]
param([switch]$InspectOnly)

function Get-WcNativeVaultCount {
    # Query the driver directly: its mount list includes slots not owned by the
    # WinCommander service. Never print or retain the returned container paths.
    if (-not ('WcDevVaultInventory' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
public static class WcDevVaultInventory {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool DeviceIoControl(SafeFileHandle h, uint code, byte[] input, int inputSize, byte[] output, int outputSize, out int returned, IntPtr overlapped);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern uint QueryDosDevice(string name, StringBuilder target, int capacity);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr OpenSCManager(string machine, string database, uint access);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr OpenService(IntPtr manager, string name, uint access);
    [DllImport("advapi32.dll")]
    static extern bool CloseServiceHandle(IntPtr handle);
    static string DosTarget(string name) {
        var target = new StringBuilder(32768);
        if (QueryDosDevice(name, target, target.Capacity) != 0) return target.ToString();
        if (Marshal.GetLastWin32Error() == 2) return null;
        throw new InvalidOperationException("DOS device inventory unavailable");
    }
    static bool DriverServicesAbsent() {
        var manager = OpenSCManager(null, null, 1);
        if (manager == IntPtr.Zero) throw new InvalidOperationException("Service inventory unavailable");
        try {
            foreach (var name in new [] { "VeraCrypt", "WinCommanderEncVol" }) {
                var service = OpenService(manager, name, 4);
                if (service != IntPtr.Zero) { CloseServiceHandle(service); return false; }
                if (Marshal.GetLastWin32Error() != 1060) throw new InvalidOperationException("Driver service state unavailable");
            }
            return true;
        } finally { CloseServiceHandle(manager); }
    }
    public static bool DefinitelyAbsent() {
        if (DosTarget("VeraCrypt") != null || !DriverServicesAbsent()) return false;
        for (char letter = 'A'; letter <= 'Z'; letter++) {
            string target = DosTarget(letter + ":");
            if (target != null && target.StartsWith(@"\Device\VeraCryptVolume", StringComparison.OrdinalIgnoreCase)) return false;
        }
        return true;
    }
    static uint Query(SafeFileHandle h, uint code, int size) {
        byte[] buffer = new byte[size];
        try {
            int returned;
            if (!DeviceIoControl(h, code, buffer, size, buffer, size, out returned, IntPtr.Zero) || returned != size)
                throw new InvalidOperationException("VeraCrypt inventory unavailable");
            return BitConverter.ToUInt32(buffer, 0);
        } finally { Array.Clear(buffer, 0, buffer.Length); }
    }
    public static int Count() {
        using (var h = CreateFile(@"\\.\VeraCrypt", 0, 3, IntPtr.Zero, 3, 0, IntPtr.Zero)) {
            if (h.IsInvalid) {
                if (Marshal.GetLastWin32Error() == 2) return -1;
                throw new InvalidOperationException("VeraCrypt driver inaccessible");
            }
            if (Query(h, 0x222004, 4) != 0x126)
                throw new InvalidOperationException("VeraCrypt driver unavailable or unsupported");
            uint first = Query(h, 0x222018, 17424);
            uint second = Query(h, 0x222018, 17424);
            if (first != second || (first >> 26) != 0)
                throw new InvalidOperationException("VeraCrypt inventory changed or is invalid");
            int count = 0;
            while (first != 0) { count += (int)(first & 1); first >>= 1; }
            return count;
        }
    }
}
'@
    }
    return Resolve-WcNativeVaultCount ([WcDevVaultInventory]::Count()) { [WcDevVaultInventory]::DefinitelyAbsent() }
}

function Resolve-WcNativeVaultCount([int]$Count, [scriptblock]$DefinitelyAbsent) {
    if ($Count -eq -1) {
        if (& $DefinitelyAbsent) { return 0 }
        throw 'VeraCrypt driver state is ambiguous'
    }
    if ($Count -lt 0 -or $Count -gt 26) { throw 'Invalid native mount count' }
    return $Count
}

function Get-WcDevelopmentVaultState {
    # This is a preparation decision, never proof of native driver readiness.
    try {
        $nativeCount = Get-WcNativeVaultCount
        if ($nativeCount -lt 0 -or $nativeCount -gt 26) { throw 'Invalid native mount count' }
        if ($nativeCount -gt 0) { return [pscustomobject]@{ state = 'mounted'; mountedCount = $nativeCount } }
        $registryPath = Join-Path $env:ProgramData 'WinCommander\policy\vault-active-mounts-v1.json'
        if (-not (Test-Path -LiteralPath $registryPath -PathType Leaf -ErrorAction Stop)) {
            $service = Get-Service -Name WinCommanderSvc -ErrorAction SilentlyContinue
            $desktop = Get-Process wincommander-free, wincommander-pro, WinCommander -ErrorAction SilentlyContinue
            # A running service may own mounts even without a readable journal.
            # A stopped service with no desktop process cannot own a live
            # presentation; managed mounts persist their journal before mount.
            if (($service -and $service.Status -ne 'Stopped') -or $desktop) {
                return [pscustomobject]@{ state = 'unknown'; mountedCount = 0 }
            }
            return [pscustomobject]@{ state = 'free'; mountedCount = 0 }
        }
        $file = [IO.FileInfo]::new($registryPath)
        if ($file.Length -gt 4MB) { throw 'Unbounded mount journal' }
        $registry = Get-Content -LiteralPath $registryPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        if ($null -eq $registry -or $null -eq $registry.PSObject.Properties['mounts'] -or
            $registry.mounts -isnot [pscustomobject]) { throw 'Invalid mount journal' }
        $count = @($registry.mounts.PSObject.Properties).Count
        if ($count -gt 26) { throw 'Invalid mount count' }
        return [pscustomobject]@{ state = $(if ($count -gt 0) { 'mounted' } else { 'free' }); mountedCount = $count }
    }
    catch {
        return [pscustomobject]@{ state = 'unknown'; mountedCount = 0 }
    }
}

function Assert-WcNativeVaultsUnmounted {
    try { $nativeCount = Get-WcNativeVaultCount }
    catch { throw 'Cannot inspect VeraCrypt mounts. Existing service and Vaults were preserved.' }
    if ($nativeCount -lt 0 -or $nativeCount -gt 26) { throw 'Cannot inspect VeraCrypt mounts. Existing service and Vaults were preserved.' }
    if ($nativeCount -gt 0) { throw 'VeraCrypt volumes are mounted. Existing service and Vaults were preserved.' }
}

if ($InspectOnly) { Get-WcDevelopmentVaultState | ConvertTo-Json -Compress }
