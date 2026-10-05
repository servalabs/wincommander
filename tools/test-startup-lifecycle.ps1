# SPDX-License-Identifier: AGPL-3.0-or-later
[CmdletBinding()]
param([switch]$SkipBuild, [switch]$Release)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$manifestTool = Get-ChildItem -LiteralPath $sdkRoot -Filter mt.exe -Recurse |
    Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $manifestTool) { throw 'Windows SDK x64 manifest tool is required for the disposable probe.' }
$logs = Join-Path $env:TEMP ('wincommander-lifecycle-verification-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $logs | Out-Null
$logs = (Resolve-Path -LiteralPath $logs).Path
Push-Location (Join-Path $repo 'src-tauri')
try {
    if (-not $SkipBuild) {
        $buildArgs = @('build', '--locked', '-p', 'commander-free', '--example', 'startup-lifecycle-probe')
        if ($Release) { $buildArgs += '--release' }
        & cargo @buildArgs
        if ($LASTEXITCODE -ne 0) { throw 'Native lifecycle probe build failed.' }
    }
    $profile = if ($Release) { 'release' } else { 'debug' }
    $executable = Join-Path (Get-Location) "target\$profile\examples\startup-lifecycle-probe.exe"
    & $manifestTool.FullName -nologo -manifest commander-free/app.manifest "-outputresource:${executable};#1"
    if ($LASTEXITCODE -ne 0) { throw 'Probe manifest embedding failed.' }
    foreach ($mode in @('Hidden', 'Normal', 'Minimized', 'Maximized')) {
        $profileRoot = Join-Path $logs "profile-$mode"
        New-Item -ItemType Directory -Path $profileRoot | Out-Null
        $profileRoot = (Resolve-Path -LiteralPath $profileRoot).Path
        $output = Join-Path $logs "$mode.stdout.log"
        $errors = Join-Path $logs "$mode.stderr.log"
        $probe = $null
        try {
            # The explicit launch modes are part of the native visibility test.
            # Preferences and WebView2 state live only below this run's private directory.
            $probe = Start-Process -FilePath $executable -ArgumentList ('"' + $profileRoot + '"') -WindowStyle $mode -PassThru -RedirectStandardOutput $output -RedirectStandardError $errors
            $null = $probe.Handle
            if (-not $probe.WaitForExit(45000)) { throw "Disposable lifecycle probe timed out in $mode. Logs: $logs" }
            $probe.WaitForExit()
            Write-Output "Launch mode: $mode; exit: $($probe.ExitCode)"
            Get-Content -LiteralPath $output | Select-String '^PASS|^FAIL'
            if ($probe.ExitCode -ne 0) {
                Get-Content -LiteralPath $errors -Tail 30
                throw "Native lifecycle verification failed in $mode. Logs: $logs"
            }
        } finally {
            # Terminate only the process handle launched by this test, never a name/PID search.
            if ($probe -and -not $probe.HasExited) {
                $probe.Kill()
                $null = $probe.WaitForExit(5000)
            }
            if ($probe) { $probe.Dispose() }
            $resolvedProfile = [System.IO.Path]::GetFullPath($profileRoot)
            if (-not $resolvedProfile.StartsWith($logs + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
                throw 'Refusing cleanup outside the isolated lifecycle evidence directory.'
            }
            for ($attempt = 0; $attempt -lt 5 -and (Test-Path -LiteralPath $resolvedProfile); $attempt++) {
                try { Remove-Item -LiteralPath $resolvedProfile -Recurse -Force -ErrorAction Stop }
                catch {
                    if ($attempt -eq 4) { Write-Warning "Disposable WebView2 profile is still in use; retained at $resolvedProfile" }
                    else { Start-Sleep -Milliseconds 300 }
                }
            }
        }
    }
    Write-Output "Passed 32 native lifecycle cycles. Evidence: $logs"
    Write-Output 'This probe does not exercise installed personal-settings persistence, remote PCs, or real sign-in.'
} finally { Pop-Location }
