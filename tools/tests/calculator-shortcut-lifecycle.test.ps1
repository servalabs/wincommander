$ErrorActionPreference = 'Stop'
$fixture = Join-Path $env:TEMP ('wincommander-calculator-shortcuts-' + [guid]::NewGuid().ToString('N'))
$savedAppData = $env:APPDATA
$savedProgramData = $env:ProgramData
$env:APPDATA = Join-Path $fixture 'profile'
$env:ProgramData = Join-Path $fixture 'machine'
$script:programs = Join-Path $fixture 'Programs'
$script:desktop = Join-Path $fixture 'Desktop'
$script:quickLaunch = Join-Path $fixture 'Quick Launch'
$script:shared = Join-Path $fixture 'Shared\WinCommander.lnk'
$script:target = Join-Path $env:SystemRoot 'System32\notepad.exe'
$script:otherTarget = Join-Path $env:SystemRoot 'explorer.exe'

function Assert-True($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function New-TestLink([string]$Path, [string]$Target) {
    $parent = Split-Path $Path -Parent
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($Path)
    $link.TargetPath = $Target
    $link.Arguments = '--test-argument'
    $link.Description = 'Original user shortcut'
    $link.Hotkey = 'CTRL+ALT+F11'
    $link.WindowStyle = 3
    $link.Save()
}
function Read-Bytes([string]$Path) { [Convert]::ToBase64String([IO.File]::ReadAllBytes($Path)) }

try {
    New-Item -ItemType Directory -Path $env:APPDATA,$env:ProgramData,$script:programs,$script:desktop,$script:quickLaunch -Force | Out-Null
    $module = Join-Path $PSScriptRoot '../../src-tauri/commander-free/scripts/modules/dependencies/dependencies.ps1'
    Invoke-Expression ([IO.File]::ReadAllText((Resolve-Path $module), [Text.Encoding]::UTF8))
    function Get-WinCommanderCalculatorProgramsPath { $script:programs }
    function Get-WinCommanderCalculatorUserRoots { @($script:programs,$script:desktop,$script:quickLaunch) }
    function Get-WinCommanderExePath { $script:target }
    function Get-WinCommanderStartMenuTargets { @($script:shared) }
    function Invoke-ShellChangeNotify {}

    $originals = @((Join-Path $script:programs 'Vendor\Custom name.lnk'), (Join-Path $script:desktop 'WinCommander.lnk'), (Join-Path $script:quickLaunch 'My launcher.lnk'), (Join-Path $script:programs 'ServaLabs\WinCommander\Launcher.lnk'))
    $snapshots = @{}
    foreach ($path in $originals) { New-TestLink $path $script:target; $snapshots[$path] = Read-Bytes $path }
    New-TestLink $script:shared $script:target
    $sharedBytes = Read-Bytes $script:shared
    $pinned = Join-Path $script:quickLaunch 'User Pinned\TaskBar\WinCommander.lnk'
    New-TestLink $pinned $script:target
    $pinnedBytes = Read-Bytes $pinned
    $unrelated = Join-Path $script:desktop 'Other.lnk'
    New-TestLink $unrelated $script:otherTarget
    $unrelatedBytes = Read-Bytes $unrelated
    $hidden = Set-WinCommanderCalculatorShortcuts $true
    Assert-True ($hidden.status -eq 'hidden') ("Enable failed: " + ($hidden | ConvertTo-Json -Depth 5 -Compress))
    foreach ($path in $originals) { Assert-True (-not (Test-Path -LiteralPath $path)) 'Original user shortcut was not replaced' }
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $script:programs 'ServaLabs'))) 'Empty owned product folders remained visible'
    Assert-True (Test-Path -LiteralPath (Join-Path $script:programs 'Vendor')) 'An unrelated shortcut folder was removed'
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) { Assert-True (Test-Path -LiteralPath (Join-Path $root 'Calculator.lnk')) 'Calculator shortcut is missing' }
    Assert-True ((Read-Bytes $script:shared) -ceq $sharedBytes) 'Shared shortcut changed'
    Assert-True ((Read-Bytes $pinned) -ceq $pinnedBytes) 'Pinned shortcut changed'
    Assert-True ((Read-Bytes $unrelated) -ceq $unrelatedBytes) 'Unrelated shortcut changed'
    $again = Set-WinCommanderCalculatorShortcuts $true
    Assert-True ($again.status -eq 'hidden') 'Repeated enable failed'
    $restored = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ($restored.status -eq 'restored') 'Disable failed'
    foreach ($path in $originals) { Assert-True ((Read-Bytes $path) -ceq $snapshots[$path]) 'Original shortcut bytes were not restored exactly' }
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) { Assert-True (-not (Test-Path -LiteralPath (Join-Path $root 'Calculator.lnk'))) 'Owned Calculator shortcut was not removed' }

    $collision = Join-Path $script:desktop 'Calculator.lnk'
    New-TestLink $collision $script:otherTarget
    $collisionBytes = Read-Bytes $collision
    $null = Set-WinCommanderCalculatorShortcuts $true
    Assert-True (Test-Path -LiteralPath $originals[1]) 'An original was hidden despite a replacement collision'
    Assert-True ((Read-Bytes $collision) -ceq $collisionBytes) 'Existing Calculator shortcut was overwritten'
    $null = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ((Read-Bytes $collision) -ceq $collisionBytes) 'Existing Calculator shortcut was removed on disable'
    Remove-Item -LiteralPath $collision

    $null = Set-WinCommanderCalculatorShortcuts $true
    New-TestLink $collision $script:target
    $changed = (New-Object -ComObject WScript.Shell).CreateShortcut($collision)
    $changed.Arguments = '--external-arguments'
    $changed.IconLocation = "$env:SystemRoot\System32\shell32.dll,1"
    $changed.Save()
    $externalBytes = Read-Bytes $collision
    $null = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ((Read-Bytes $collision) -ceq $externalBytes) 'Externally changed replacement was removed'
    Assert-True ((Read-Bytes $originals[1]) -ceq $snapshots[$originals[1]]) 'Original shortcut was not restored after an external edit'
    $statePath = Join-Path $env:APPDATA 'WinCommander\calc_shortcut_state.json'
    Assert-True (Test-Path -LiteralPath $statePath) 'External replacement recovery state was lost'
    Remove-Item -LiteralPath $collision
    $null = Set-WinCommanderCalculatorShortcuts $false

    $null = Set-WinCommanderCalculatorShortcuts $true
    New-TestLink $originals[1] $script:otherTarget
    $occupiedBytes = Read-Bytes $originals[1]
    $null = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ((Read-Bytes $originals[1]) -ceq $occupiedBytes) 'An externally occupied original path was overwritten'
    $retained = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    $savedOriginal = @($retained.hiddenShortcuts | Where-Object { $_.path -eq $originals[1] })[0]
    Assert-True ($savedOriginal.originalBytes -ceq $snapshots[$originals[1]]) 'Original backup was discarded after a restore collision'
    Remove-Item -LiteralPath $originals[1]
    $null = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ((Read-Bytes $originals[1]) -ceq $snapshots[$originals[1]]) 'Retry did not restore the original after removing the collision'

    New-TestLink $originals[1] $script:otherTarget
    $legacyExternalBytes = Read-Bytes $originals[1]
    @{ hiddenShortcuts = @(@{ path = $originals[1]; targetPath = $script:target }); calculatorShortcutCreated = $false } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $statePath
    $null = Set-WinCommanderCalculatorShortcuts $true
    Assert-True ((Read-Bytes $originals[1]) -ceq $legacyExternalBytes) 'Legacy recovery entry deleted an externally retargeted shortcut'

    $boundedRoot = Join-Path $fixture 'bounded-scan'
    foreach ($index in 1..257) { [void][IO.Directory]::CreateDirectory((Join-Path $boundedRoot "directory-$index")) }
    $capped = $false
    $null = Get-WinCommanderCalculatorShortcutFiles @($boundedRoot) (New-Object -ComObject WScript.Shell) $script:target ([ref]$capped)
    Assert-True $capped 'Shortcut traversal did not enforce its directory bound'
    Write-Output 'PASS: account scope, exact restoration, empty owned folders, collisions, external edits, legacy recovery, and bounded scan'
} finally {
    $env:APPDATA = $savedAppData
    $env:ProgramData = $savedProgramData
    $resolved = [IO.Path]::GetFullPath($fixture)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolved -Leaf) -like 'wincommander-calculator-shortcuts-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
    }
}
