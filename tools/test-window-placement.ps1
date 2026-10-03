param()
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$manifestTool = Get-ChildItem -LiteralPath $sdkRoot -Filter mt.exe -Recurse |
    Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $manifestTool) { throw 'Windows SDK x64 manifest tool is required for the disposable probe.' }
Push-Location (Join-Path $repo 'src-tauri')
try {
    cargo build --locked -p commander-free --example window-placement-probe
    if ($LASTEXITCODE -ne 0) { throw 'Native probe build failed.' }
    # Cargo examples do not inherit the application's Common Controls v6 manifest.
    & $manifestTool.FullName -nologo -manifest commander-free/app.manifest '-outputresource:target\debug\examples\window-placement-probe.exe;#1'
    if ($LASTEXITCODE -ne 0) { throw 'Probe manifest embedding failed.' }
    $logs = Join-Path $env:TEMP ('wincommander-placement-verification-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $logs | Out-Null
    $output = Join-Path $logs 'stdout.log'
    $errors = Join-Path $logs 'stderr.log'
    $probe = Start-Process -FilePath .\target\debug\examples\window-placement-probe.exe -WindowStyle Hidden -PassThru -RedirectStandardOutput $output -RedirectStandardError $errors
    $null = $probe.Handle # Retain the handle so Windows PowerShell keeps ExitCode after exit.
    if (-not $probe.WaitForExit(45000)) {
        $probe.Kill()
        throw "Disposable placement probe timed out. Logs: $logs"
    }
    $probe.WaitForExit()
    Get-Content -LiteralPath $output
    if ($probe.ExitCode -ne 0) {
        Get-Content -LiteralPath $errors
        throw "Native placement verification failed. Logs: $logs"
    }
} finally { Pop-Location }
