param([switch]$SkipBuild, [switch]$Release)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$manifestTool = Get-ChildItem -LiteralPath $sdkRoot -Filter mt.exe -Recurse |
    Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $manifestTool) { throw 'Windows SDK x64 manifest tool is required for the disposable probe.' }
$logs = Join-Path $env:TEMP ('wincommander-renderer-verification-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $logs | Out-Null
Push-Location (Join-Path $repo 'src-tauri')
try {
    if (-not $SkipBuild) {
        $buildArgs = @('build', '--locked', '-p', 'commander-free', '--example', 'renderer-recovery-probe')
        if ($Release) { $buildArgs += '--release' }
        & cargo @buildArgs
        if ($LASTEXITCODE -ne 0) { throw 'Renderer probe build failed.' }
    }
    $profile = if ($Release) { 'release' } else { 'debug' }
    $executable = Join-Path (Get-Location) "target\$profile\examples\renderer-recovery-probe.exe"
    & $manifestTool.FullName -nologo -manifest commander-free/app.manifest "-outputresource:${executable};#1"
    if ($LASTEXITCODE -ne 0) { throw 'Probe manifest embedding failed.' }
    foreach ($mode in @('baseline', 'visible', 'hidden', 'minimized', 'locked')) {
        $launch = @{
            FilePath = $executable; WindowStyle = 'Hidden'; PassThru = $true
            RedirectStandardOutput = Join-Path $logs "$mode.stdout.log"
            RedirectStandardError = Join-Path $logs "$mode.stderr.log"
        }
        if ($mode -eq 'baseline') { $launch.ArgumentList = '--without-recovery' }
        elseif ($mode -ne 'visible') { $launch.ArgumentList = "--$mode" }
        # Only this isolated, freshly started probe is terminated on timeout.
        # No installed app, profile, scheduled task, service or registry is touched.
        $probe = Start-Process @launch
        $null = $probe.Handle # Retain the handle so Windows PowerShell keeps ExitCode after exit.
        if (-not $probe.WaitForExit(45000)) {
            $probe.Kill()
            throw "Renderer probe timed out in $mode mode. Logs: $logs"
        }
        $probe.WaitForExit()
        Get-Content -LiteralPath $launch.RedirectStandardOutput
        if ($probe.ExitCode -ne 0) {
            Get-Content -LiteralPath $launch.RedirectStandardError
            throw "Renderer probe failed in $mode mode ($($probe.ExitCode)). Logs: $logs"
        }
    }
    Write-Output "Renderer recovery checks passed. Logs: $logs"
} finally { Pop-Location }
