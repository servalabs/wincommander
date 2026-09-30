param()
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$manifestTool = Get-ChildItem -LiteralPath $sdkRoot -Filter mt.exe -Recurse |
    Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $manifestTool) { throw 'Windows SDK x64 manifest tool is required for the disposable probe.' }
Push-Location (Join-Path $repo 'src-tauri')
try {
    cargo build -p commander-free --example window-placement-probe
    if ($LASTEXITCODE -ne 0) { throw 'Native probe build failed.' }
    # Cargo examples do not inherit the application's Common Controls v6 manifest.
    & $manifestTool.FullName -nologo -manifest commander-free/app.manifest '-outputresource:target\debug\examples\window-placement-probe.exe;#1'
    if ($LASTEXITCODE -ne 0) { throw 'Probe manifest embedding failed.' }
    & .\target\debug\examples\window-placement-probe.exe
    if ($LASTEXITCODE -ne 0) { throw 'Native placement verification failed.' }
} finally { Pop-Location }
