$ErrorActionPreference = 'Stop'
$fixture = Join-Path $env:TEMP ('wc-install-path-' + [guid]::NewGuid().ToString('N'))
try {
    New-Item -ItemType Directory -Path $fixture -Force | Out-Null
    $script:exe = Join-Path $fixture 'wincommander-free.exe'
    [IO.File]::WriteAllBytes($script:exe, [byte[]]@(1))
    $module = Resolve-Path (Join-Path $PSScriptRoot '../../src-tauri/commander-free/scripts/modules/dependencies/dependencies.ps1')
    Invoke-Expression ([IO.File]::ReadAllText($module, [Text.Encoding]::UTF8))
    function Get-WinCommanderUninstallKeys { 'fixture' }
    function Get-ItemProperty { param($Path, $ErrorAction) $script:properties }
    $script:properties = @{ DisplayIcon = ('"' + $script:exe + '",0'); InstallLocation = 'invalid' }
    if ((Get-WinCommanderExePath) -ine $script:exe) { throw 'Quoted DisplayIcon was not resolved' }
    $script:properties = @{ DisplayIcon = ''; InstallLocation = ('"' + $fixture + '"') }
    if ((Get-WinCommanderExePath) -ine $script:exe) { throw 'Quoted InstallLocation was not resolved' }
    Write-Output 'PASS: quoted uninstall registry paths resolve without falling back'
} finally {
    $resolved = [IO.Path]::GetFullPath($fixture)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolved -Leaf) -like 'wc-install-path-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
    }
}
