$ErrorActionPreference = 'Stop'
$fixture = Join-Path $env:TEMP ('wc-visibility-' + [guid]::NewGuid().ToString('N'))
$oldAppData, $oldProgramData = $env:APPDATA, $env:ProgramData
$env:APPDATA = Join-Path $fixture 'profile'
$env:ProgramData = Join-Path $fixture 'machine'
$script:programs = Join-Path $fixture 'Programs'
$script:desktop = Join-Path $fixture 'Desktop'
$script:shared = Join-Path $fixture 'Shared\WinCommander.lnk'
$script:target = Join-Path $env:SystemRoot 'System32\notepad.exe'

try {
    New-Item -ItemType Directory -Path $env:APPDATA,$env:ProgramData,$script:programs,$script:desktop -Force | Out-Null
    $module = Resolve-Path (Join-Path $PSScriptRoot '../../src-tauri/commander-free/scripts/modules/dependencies/dependencies.ps1')
    Invoke-Expression ([IO.File]::ReadAllText($module, [Text.Encoding]::UTF8))
    function Assert-IsAdmin {}
    function Get-WinCommanderCalculatorProgramsPath { $script:programs }
    function Get-WinCommanderCalculatorDesktopPath { $script:desktop }
    function Get-WinCommanderCalculatorUserRoots { @($script:programs, $script:desktop) }
    function Get-WinCommanderStartMenuTargets { @((Join-Path $script:programs 'WinCommander.lnk'), (Join-Path $script:programs 'WinCommander Pro.lnk'), $script:shared) }
    function Test-WinCommanderManagedSharedShortcut { param([string]$Path) return $Path -ieq $script:shared }
    function Get-WinCommanderExePath { $script:target }
    function Get-WinCommanderUninstallKeys { @() }
    function Get-WinCommanderAppPaths { @() }
    function Invoke-ShellChangeNotify {}
    function Test-Path {
        param([string]$Path, [string]$LiteralPath, $PathType, $ErrorAction)
        $candidate = if ($LiteralPath) { $LiteralPath } else { $Path }
        if ($candidate -match '^HK(CU|LM):') { return $false }
        $params = @{ LiteralPath = $candidate }
        if ($PathType) { $params.PathType = $PathType }
        Microsoft.PowerShell.Management\Test-Path @params
    }
    $original = Join-Path $script:programs 'WinCommander.lnk'
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($original)
    $link.TargetPath = $script:target
    $link.Hotkey = 'CTRL+ALT+F11'
    $link.Arguments = '--original'
    $link.Save()
    $originalBytes = [Convert]::ToBase64String([IO.File]::ReadAllBytes($original))
    New-Item -ItemType Directory -Path (Split-Path $script:shared -Parent) -Force | Out-Null
    $sharedLink = (New-Object -ComObject WScript.Shell).CreateShortcut($script:shared)
    $sharedLink.TargetPath = $script:target
    $sharedLink.Save()
    $sharedBytes = [Convert]::ToBase64String([IO.File]::ReadAllBytes($script:shared))
    $unrelated = Join-Path $script:programs 'WinCommander Pro.lnk'
    $otherLink = (New-Object -ComObject WScript.Shell).CreateShortcut($unrelated)
    $otherLink.TargetPath = Join-Path $env:SystemRoot 'explorer.exe'
    $otherLink.Save()
    $unrelatedBytes = [Convert]::ToBase64String([IO.File]::ReadAllBytes($unrelated))
    foreach ($order in @('lock-first', 'hide-first')) {
        if ($order -eq 'lock-first') {
            $null = Set-WinCommanderCalculatorShortcuts $true
            $null = Set-WinCommanderCalculatorShortcuts $false
        }
        $hideResult = Set-WinCommanderVisibility $true
        if (-not ($hideResult.warnings -contains 'Shared WinCommander shortcuts need service reconciliation.')) { throw 'Direct hide omitted the shared-link reconciliation warning' }
        $null = Set-WinCommanderVisibility $true
        $null = Set-WinCommanderCalculatorShortcuts $true
        $null = Set-WinCommanderCalculatorShortcuts $false
        $result = Set-WinCommanderVisibility $false
        if ($result.error -or -not (Test-Path -LiteralPath $original)) { throw "Restore failed for $order" }
        if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($original)) -cne $originalBytes) { throw "Shortcut bytes changed for $order" }
        if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($script:shared)) -cne $sharedBytes) { throw 'A shared shortcut changed' }
        if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($unrelated)) -cne $unrelatedBytes) { throw 'An unrelated shortcut changed' }
    }
    # Removing Hide mode while the lock remains armed restores the snapshot,
    # then Calculator Lock takes ownership again until it is switched off.
    $null = Set-WinCommanderVisibility $true
    $null = Set-WinCommanderCalculatorShortcuts $true
    $null = Set-WinCommanderVisibility $false
    $null = Set-WinCommanderCalculatorShortcuts $true
    $null = Set-WinCommanderCalculatorShortcuts $false
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($original)) -cne $originalBytes) { throw 'Hide-first disable lost the original bytes' }
    # Hide can begin after another mode has already removed every account link.
    # An explicitly empty snapshot is valid, not a missing recovery record.
    Remove-Item -LiteralPath $original
    $null = Set-WinCommanderVisibility $true
    $null = Set-WinCommanderCalculatorShortcuts $true
    $null = Set-WinCommanderCalculatorShortcuts $false
    $emptyRestore = Set-WinCommanderVisibility $false
    if ($emptyRestore.warnings -match 'No shortcut state found') { throw 'An empty saved snapshot reported a false missing-state warning' }
    $missingRestore = Set-WinCommanderVisibility $false
    if (-not ($missingRestore.warnings -match 'No shortcut state found')) { throw 'Missing recovery state was not reported' }
    [IO.File]::WriteAllBytes($original, [Convert]::FromBase64String($originalBytes))
    $null = Set-WinCommanderVisibility $true
    [IO.File]::WriteAllBytes($original, [byte[]]@(1,2,3))
    $null = Set-WinCommanderVisibility $false
    if (-not (Test-Path -LiteralPath (Get-WinCommanderVisibilityStatePath))) { throw 'Collision discarded recovery state' }
    if ([IO.File]::ReadAllBytes($original).Length -ne 3) { throw 'Collision overwrote the existing file' }
    $retryHide = Set-WinCommanderVisibility $true
    if (-not $retryHide.error) { throw 'A new hide overwrote unresolved recovery state' }
    Remove-Item -LiteralPath $original
    $null = Set-WinCommanderVisibility $false
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($original)) -cne $originalBytes) { throw 'Retry failed to restore original bytes' }
    $null = Set-WinCommanderVisibility $true
    $legacyPath = Get-WinCommanderVisibilityStatePath
    $legacy = Get-Content -LiteralPath $legacyPath -Raw | ConvertFrom-Json
    $legacy.hiddenShortcuts[0].PSObject.Properties.Remove('originalBytes')
    $legacy | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $legacyPath -Encoding UTF8
    [IO.File]::WriteAllBytes($original, [Convert]::FromBase64String($originalBytes))
    $legacyResult = Set-WinCommanderVisibility $false
    if (Test-Path -LiteralPath $legacyPath) { throw 'Matching legacy shortcut left recovery state indefinitely' }
    if (-not ($legacyResult.warnings -match 'original file bytes were not recorded')) { throw 'Legacy byte limitation was not reported' }
    $null = Set-WinCommanderVisibility $true
    $legacy = Get-Content -LiteralPath $legacyPath -Raw | ConvertFrom-Json
    $legacy.hiddenShortcuts[0].PSObject.Properties.Remove('originalBytes')
    $legacy | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $legacyPath -Encoding UTF8
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($original)
    $link.TargetPath = Join-Path $env:SystemRoot 'explorer.exe'
    $link.Save()
    $null = Set-WinCommanderVisibility $false
    if (-not (Test-Path -LiteralPath $legacyPath)) { throw 'Different legacy shortcut discarded recovery state' }
    Remove-Item -LiteralPath $original
    $null = Set-WinCommanderVisibility $false
    [IO.File]::WriteAllBytes($original, [Convert]::FromBase64String($originalBytes))
    @{ hiddenShortcuts = @(@{ path = $script:shared; originalBytes = $sharedBytes }) } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $legacyPath -Encoding UTF8
    $sharedRestore = Set-WinCommanderVisibility $false
    if (-not (Test-Path -LiteralPath $legacyPath)) { throw 'Shared legacy recovery record was discarded' }
    if (-not ($sharedRestore.warnings -match 'legacy shared shortcut')) { throw 'Shared recovery limitation was not reported' }
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($script:shared)) -cne $sharedBytes) { throw 'Shared legacy shortcut changed' }
    Remove-Item -LiteralPath $legacyPath
    $script:blockedState = Join-Path $fixture 'not-a-directory'
    [IO.File]::WriteAllText($script:blockedState, 'fixture')
    function Get-WinCommanderVisibilityStatePath { Join-Path $script:blockedState 'state.json' }
    $failed = Set-WinCommanderVisibility $true
    if (-not $failed.error) { throw 'A failed snapshot write reported success' }
    if (Test-Path -LiteralPath (Get-WinCommanderHideFlagPath)) { throw 'A failed snapshot write left the hide flag set' }
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($original)) -cne $originalBytes) { throw 'A failed snapshot write changed the shortcut' }
    Write-Output 'PASS: exact shortcut recovery in both toggle orders, repeated hide, and collision retry'
} finally {
    $env:APPDATA, $env:ProgramData = $oldAppData, $oldProgramData
    $resolved = [IO.Path]::GetFullPath($fixture)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolved -Leaf) -like 'wc-visibility-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
    }
}
