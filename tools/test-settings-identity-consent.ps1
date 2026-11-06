[CmdletBinding()]
param([Parameter(Mandatory)][string]$ScratchRoot)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$base = (Resolve-Path -LiteralPath $ScratchRoot).Path
$env:TEMP = $base; $env:TMP = $base
$work = Join-Path $base ('identity-consent-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$include = (Resolve-Path (Join-Path $PSScriptRoot '../src-tauri/commander-free/nsis/settings-identity-consent.nsh')).Path
$nsis = Join-Path ${env:ProgramFiles(x86)} 'NSIS/makensis.exe'
$output = Join-Path $work 'fixture.exe'
$source = Join-Path $work 'fixture.nsi'
@"
Unicode true
Name "WinCommander identity consent fixture"
OutFile "$output"
RequestExecutionLevel user
AutoCloseWindow true
Var WCSettingsIdentityConsent
!include "$include"
Section
!insertmacro WC_REQUEST_SETTINGS_IDENTITY_CONSENT
FileOpen `$0 "`$EXEDIR\consent-result.txt" w
FileWrite `$0 `$WCSettingsIdentityConsent
FileClose `$0
SectionEnd
"@ | Set-Content -LiteralPath $source -Encoding ascii
& $nsis /V2 $source
if ($LASTEXITCODE -ne 0) { throw 'Consent fixture did not compile.' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ConsentFixtureWin32 {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window, uint msg, IntPtr w, IntPtr l);
}
'@
foreach ($case in @(@{Name='yes'; Id=6; Expected='1'}, @{Name='no'; Id=7; Expected='0'}, @{Name='silent'; Id=0; Expected='0'})) {
    $dir = Join-Path $work $case.Name
    New-Item -ItemType Directory -Path $dir | Out-Null
    $exe = Join-Path $dir 'fixture.exe'; Copy-Item -LiteralPath $output -Destination $exe
    $p = if ($case.Name -eq 'silent') { Start-Process -FilePath $exe -ArgumentList '/S' -WindowStyle Hidden -PassThru }
         else { Start-Process -FilePath $exe -WindowStyle Hidden -PassThru }
    try {
        if ($case.Name -ne 'silent') {
            $clock = [Diagnostics.Stopwatch]::StartNew(); $clicked = $false
            while ($clock.Elapsed.TotalSeconds -lt 15 -and -not $clicked) {
                $window = [ConsentFixtureWin32]::FindWindow('#32770', 'WinCommander identity consent fixture Setup')
                if ($window -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 100; continue }
                [uint32]$ownerId = 0
                [void][ConsentFixtureWin32]::GetWindowThreadProcessId($window, [ref]$ownerId)
                if ($ownerId -ne $p.Id) { throw 'Dialog does not belong to this disposable fixture.' }
                $button = [ConsentFixtureWin32]::GetDlgItem($window, $case.Id)
                if ($button -eq [IntPtr]::Zero) { throw 'Fixture button unavailable.' }
                [void][ConsentFixtureWin32]::SendMessage($button, 0xf5, [IntPtr]::Zero, [IntPtr]::Zero)
                $clicked = $true
            }
            if (-not $clicked) { throw 'Disposable consent prompt did not appear.' }
        }
        if (-not $p.WaitForExit(15000)) { throw 'Disposable fixture did not exit.' }
        if ($p.ExitCode -ne 0) { throw "Disposable fixture failed: $($p.ExitCode)" }
        $choice = Get-Content -Raw -LiteralPath (Join-Path $dir 'consent-result.txt')
        if ($choice -ne $case.Expected) { throw "Unexpected $($case.Name) consent value: $choice" }
        Write-Output "$($case.Name): $choice"
    } finally {
        if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    }
}
