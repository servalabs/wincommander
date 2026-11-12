# Compiles the actual NSIS path macro into a disposable, silent probe. The
# probe writes only a temporary result file; it does not run an installer.
param([string]$NsisCompiler = 'makensis.exe')
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$hooks = Get-Content -Raw -LiteralPath (Join-Path $repo 'src-tauri/commander-free/nsis/hooks.nsh')
$match = [regex]::Match($hooks, '(?ms)^!macro WC_LOAD_PROGRAMDATA_OR_ABORT stage\r?\n.*?^!macroend')
if (-not $match.Success) { throw 'Installer known-folder macro was not found.' }
$compiler = Get-Command $NsisCompiler -ErrorAction Stop | Select-Object -ExpandProperty Source
$root = Join-Path ([IO.Path]::GetTempPath()) ('wc-known-folder-' + [guid]::NewGuid().ToString('N'))
$original = [Environment]::GetEnvironmentVariable('ProgramData', 'Process')
New-Item -ItemType Directory -Path $root | Out-Null
try {
    $prefix = @'
Unicode True
!include "LogicLib.nsh"
!macro WC_WRITE_LIFECYCLE_DIAGNOSTIC stage exit detail
!macroend
'@
    $suffix = @'
OutFile "probe.exe"
RequestExecutionLevel user
SilentInstall silent
Section
  !insertmacro WC_LOAD_PROGRAMDATA_OR_ABORT "fixture"
  FileOpen $1 "$EXEDIR\observed.bin" w
  IfErrors failed
  FileWriteUTF16LE $1 "$R5"
  FileClose $1
  Quit
  failed:
  SetErrorLevel 1
SectionEnd
'@
    [IO.File]::WriteAllText((Join-Path $root 'probe.nsi'), $prefix + "`r`n" + $match.Value + "`r`n" + $suffix, [Text.Encoding]::UTF8)
    Push-Location $root
    try {
        & $compiler /V1 probe.nsi
        if ($LASTEXITCODE -ne 0) { throw 'Could not compile the disposable NSIS fixture.' }
        # A changed inherited value must not redirect the machine-data root.
        $env:ProgramData = Join-Path $root 'fake-program-data'
        $result = Start-Process -FilePath (Join-Path $root 'probe.exe') -Wait -PassThru
        if ($result.ExitCode -ne 0) { throw 'The known-folder probe failed.' }
        $actual = [IO.File]::ReadAllText((Join-Path $root 'observed.bin'), [Text.Encoding]::Unicode).TrimStart([char]0xFEFF)
        $expected = [Environment]::GetFolderPath([Environment+SpecialFolder]::CommonApplicationData)
        if (-not $expected -or -not [string]::Equals($actual, $expected, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'The installer did not use the Windows machine-data known folder.'
        }
    } finally { Pop-Location }
    Write-Output 'Passed: inherited ProgramData cannot redirect the installer known-folder resolver.'
} finally {
    [Environment]::SetEnvironmentVariable('ProgramData', $original, 'Process')
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
