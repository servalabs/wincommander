# Read-only standard-user probe. Run inside the account being diagnosed.
# This does not initialize the engine, start a service, mount, or dismount.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
if (-not ('WcVaultNativeAccessProbe' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class WcVaultNativeAccessProbe {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr CreateFileW(string path, uint access, uint share,
        IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool CloseHandle(IntPtr handle);
}
'@
}

$results = foreach ($access in @([uint32]0, [uint32]3221225472)) {
    $handle = [WcVaultNativeAccessProbe]::CreateFileW('\\.\VeraCrypt', $access, 3, [IntPtr]::Zero, 3, 0, [IntPtr]::Zero)
    $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    $opened = $handle -ne [IntPtr](-1)
    if ($opened) { [void][WcVaultNativeAccessProbe]::CloseHandle($handle) }
    [ordered]@{ probe = $(if ($access -eq 0) { 'driver_device_presence' } else { 'driver_device_read_write' }); opened = $opened; windows_error = $(if ($opened) { 0 } else { $errorCode }) }
}
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
[ordered]@{
    elevated = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    session_id = [Diagnostics.Process]::GetCurrentProcess().SessionId
    probes = @($results)
} | ConvertTo-Json -Depth 4 -Compress
