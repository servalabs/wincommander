$ErrorActionPreference = 'Stop'
$savedProgramData = $env:ProgramData
$env:ProgramData = Join-Path $env:TEMP 'wincommander-shortcut-scope-fixture'
try {
    $modulePath = (Resolve-Path (Join-Path $PSScriptRoot '../../src-tauri/commander-free/scripts/modules/dependencies/dependencies.ps1')).Path
    Invoke-Expression ([IO.File]::ReadAllText($modulePath, [Text.Encoding]::UTF8))
    $profileRoot = Join-Path $env:TEMP 'wincommander-shortcut-scope-profile'
    $roots = @((Join-Path $profileRoot 'Programs'), (Join-Path $profileRoot 'Desktop'))
    if (-not (Test-WinCommanderCalculatorUserPath (Join-Path $roots[1] 'WinCommander.lnk') $roots)) { throw 'User desktop shortcut must be in scope' }
    if (-not (Test-WinCommanderCalculatorUserPath (Join-Path $roots[0].ToUpperInvariant() 'WinCommander\WinCommander.lnk') $roots)) { throw 'User Start shortcut must be in scope' }
    if (Test-WinCommanderCalculatorUserPath 'C:\ProgramData\Microsoft\Windows\Start Menu\Programs\WinCommander.lnk' $roots) { throw 'Shared Start shortcut must not be in scope' }
    if (Test-WinCommanderCalculatorUserPath 'C:\Users\Public\Desktop\WinCommander.lnk' $roots) { throw 'Public desktop shortcut must not be in scope' }
    if (Test-WinCommanderCalculatorUserPath (Join-Path $profileRoot 'DesktopOther\WinCommander.lnk') $roots) { throw 'Path prefix must respect directory boundary' }
    Write-Output 'PASS: Calculator Lock only hides shortcuts in the current user profile'
} finally {
    $env:ProgramData = $savedProgramData
}
