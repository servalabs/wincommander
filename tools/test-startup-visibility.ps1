# SPDX-License-Identifier: AGPL-3.0-or-later
# Build first: cargo build --locked -p commander-free --example startup-visibility-probe (from src-tauri).
[CmdletBinding()]
param(
    [string]$ProbePath,
    [string]$OutputDirectory = (Join-Path $env:TEMP ('wincommander-visibility-' + [Guid]::NewGuid().ToString('N')))
)
$ErrorActionPreference = 'Stop'
if (-not $ProbePath) { $ProbePath = Join-Path $PSScriptRoot '..\src-tauri\target\debug\examples\startup-visibility-probe.exe' }
$ProbePath = (Resolve-Path -LiteralPath $ProbePath).Path
$repo = Split-Path -Parent $PSScriptRoot
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$manifestTool = Get-ChildItem -LiteralPath $sdkRoot -Filter mt.exe -Recurse |
    Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $manifestTool) { throw 'Windows SDK x64 manifest tool is required for this disposable probe.' }
# Cargo examples need the same Common Controls v6 activation as the application.
& $manifestTool.FullName -nologo -manifest (Join-Path $repo 'src-tauri\commander-free\app.manifest') "-outputresource:$ProbePath;#1"
if ($LASTEXITCODE -ne 0) { throw 'Probe manifest embedding failed.' }
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class StartupVisibilityLaunch {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
  struct STARTUPINFO {
    public int cb; public string reserved, desktop, title;
    public int x,y,cx,cy,xchars,ychars,fill,flags;
    public short show, reserved2; public IntPtr reservedPtr,input,output,error;
  }
  [StructLayout(LayoutKind.Sequential)]
  struct PROCESS_INFORMATION { public IntPtr process,thread; public int pid,tid; }
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  static extern bool CreateProcessW(string app, StringBuilder command, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string dir, ref STARTUPINFO startup, out PROCESS_INFORMATION process);
  [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr h, uint ms);
  [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr h, out uint code);
  [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr h, uint code);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  public static uint Run(string executable, string output, short show) {
    var startup = new STARTUPINFO(); startup.cb=Marshal.SizeOf(startup); startup.flags=1; startup.show=show;
    PROCESS_INFORMATION process;
    var command=new StringBuilder("\""+executable+"\" \""+output+"\"");
    if(!CreateProcessW(executable,command,IntPtr.Zero,IntPtr.Zero,false,0,IntPtr.Zero,null,ref startup,out process)) throw new Win32Exception();
    try {
      if(WaitForSingleObject(process.process,45000)!=0) { TerminateProcess(process.process,124); throw new Exception("Disposable probe timed out"); }
      uint code; GetExitCodeProcess(process.process,out code); return code;
    } finally { CloseHandle(process.thread); CloseHandle(process.process); }
  }
}
'@
$failed = $false
foreach ($mode in @(@{Name='hidden';Code=0}, @{Name='normal';Code=1}, @{Name='minimized';Code=2}, @{Name='maximized';Code=3})) {
    $output = Join-Path $OutputDirectory ($mode.Name + '.json')
    $exitCode = [StartupVisibilityLaunch]::Run($ProbePath, $output, $mode.Code)
    if ($exitCode -ne 0) { throw "Probe failed in $($mode.Name): $exitCode" }
    $result = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json
    foreach ($snapshot in $result.snapshots) {
        $expected = $snapshot.phase -eq 'tray_reveal'
        $matches = $snapshot.tauriVisible -eq $expected -and $snapshot.nativeVisible -eq $expected
        if (-not $matches) { $failed = $true }
        Write-Output "$($mode.Name) $($snapshot.phase): native=$($snapshot.nativeVisible) tauri=$($snapshot.tauriVisible) expected=$expected"
        foreach ($window in $snapshot.windows) {
            Write-Output "  HWND=$($window.hwnd) class=$($window.class) title=$($window.title) visible=$($window.visible) style=$($window.style)"
        }
    }
}
Write-Output "Evidence: $OutputDirectory"
if ($failed) { throw 'Native and Tauri startup visibility did not match the expected lifecycle.' }
