$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
function Assert-True($Condition, $Message) { if (-not $Condition) { throw $Message } }
Assert-True ($script:ScheduledWslFunctions.Contains('WaitForExit(300000)') -and $script:ScheduledWslFunctions.Contains('$process.Kill()')) 'WSL provider timeout must be bounded and fail.'

# Denied registry, package or disk access must never be mistaken for absence.
. ([scriptblock]::Create($script:ScheduledWslFunctions))
# WSL normally emits UTF-16LE, but an inventory consisting entirely of
# non-ASCII characters need not contain a zero byte or a final newline.
$unicodeName = [string]::Concat([char]0x4E2D, [char]0x6587)
$unicodeInventory = @([pscustomobject]@{ Name=$unicodeName })
$wide = [Text.Encoding]::Unicode.GetBytes($unicodeName)
Assert-True (-not ($wide -contains 0)) 'Fixture must exercise UTF-16 without null bytes.'
Assert-True (@(Convert-WCScheduledWslProviderBytes -Bytes $wide -Inventory $unicodeInventory)[0] -ceq $unicodeName) 'UTF-16 inventory without null bytes was not decoded.'
$utf8 = [Text.Encoding]::UTF8.GetBytes($unicodeName)
Assert-True (@(Convert-WCScheduledWslProviderBytes -Bytes $utf8 -Inventory $unicodeInventory)[0] -ceq $unicodeName) 'UTF-8 inventory was not decoded.'
$threw = $false
try { Convert-WCScheduledWslProviderBytes -Bytes $wide -Inventory @([pscustomobject]@{ Name='different' }) | Out-Null } catch { $threw = $true }
Assert-True $threw 'Provider output that differs from the registry must fail closed.'
$script:probeMode = 'denied'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:probeMode -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic access denial.') }
    throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic missing item.')
}
foreach ($probe in @({ Get-WCScheduledWslInventory }, { Test-WCScheduledWslOrphans }, { Test-WCScheduledWslDisk -Path 'C:\synthetic\ext4.vhdx' })) {
    $threw = $false
    try { & $probe | Out-Null } catch [UnauthorizedAccessException] { $threw = $true }
    Assert-True $threw 'Denied WSL inventory or disk access was treated as empty.'
}
$script:probeMode = 'missing'
Assert-True (@(Get-WCScheduledWslInventory).Count -eq 0) 'A missing WSL registry root should be empty.'
Assert-True (-not (Test-WCScheduledWslOrphans)) 'A missing package root should be empty.'
Assert-True (-not (Test-WCScheduledWslDisk -Path 'C:\synthetic\ext4.vhdx')) 'A missing WSL disk should be absent.'

# The task payload is exercised with a disposable in-memory inventory only.
$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:inventory = @([pscustomobject]@{ Name='ExampleLinux'; Disk='' }, [pscustomobject]@{ Name='ExampleImport'; Disk='' })
function Get-WCScheduledWslInventory { return @($script:inventory) }
function Get-WCScheduledWslProviderNames { return @($script:inventory | ForEach-Object { $_.Name }) }
function Test-WCScheduledWslOrphans { return $false }
function Remove-WCScheduledWslDistro($Name) { $script:inventory = @($script:inventory | Where-Object { $_.Name -ne $Name }) }
Invoke-Expression $script:AutoEraseScripts['wslData']
Assert-True ($script:AutoEraseRemoved -eq 2 -and $script:AutoEraseFailed -eq 0) 'Registered distributions must be unregistered and verified.'

$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:probeMode = 'denied'
$script:inventory = @([pscustomobject]@{ Name='ExampleLinux'; Disk='C:\synthetic\ext4.vhdx' })
Invoke-Expression $script:AutoEraseScripts['wslData']
Assert-True ($script:AutoEraseRemoved -eq 0 -and $script:AutoEraseFailed -gt 0) 'A distribution with inaccessible disk readback was falsely marked removed.'

$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:inventory = @([pscustomobject]@{ Name='ExampleLinux'; Disk='' })
function Remove-WCScheduledWslDistro($Name) { }
Invoke-Expression $script:AutoEraseScripts['wslData']
Assert-True ($script:AutoEraseRemoved -eq 0 -and $script:AutoEraseFailed -gt 0) 'A distribution still registered after cleanup is a failure.'

$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:inventory = @([pscustomobject]@{ Name='docker-desktop'; Disk='' })
$script:attempted = $false
function Remove-WCScheduledWslDistro($Name) { $script:attempted = $true }
Invoke-Expression $script:AutoEraseScripts['wslData']
Assert-True (-not $script:attempted -and $script:AutoEraseFailed -gt 0) 'Docker-owned distributions must not be unregistered by WSL cleanup.'

$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:inventory = @([pscustomobject]@{ Name='docker-desktop'; Disk='' }, [pscustomobject]@{ Name='ExampleLinux'; Disk='' })
$script:deleted = @()
function Remove-WCScheduledWslDistro($Name) {
    $script:deleted += $Name
    $script:inventory = @($script:inventory | Where-Object { $_.Name -ne $Name })
}
Invoke-Expression $script:AutoEraseScripts['wslData']
Assert-True ($script:deleted.Count -eq 1 -and $script:deleted[0] -eq 'ExampleLinux') 'Mixed inventories should process only non-Docker distributions.'
Assert-True ($script:AutoEraseRemoved -eq 1 -and $script:AutoEraseFailed -gt 0) 'Docker-owned distributions make a mixed result incomplete.'

$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:inventory = @()
function Test-WCScheduledWslOrphans { return $true }
Invoke-Expression $script:AutoEraseScripts['wslData']
Assert-True ($script:AutoEraseFailed -gt 0) 'Unmanaged distribution disks must not count as a complete wipe.'

$script:AutoEraseFailed = 0
$script:inventory = @([pscustomobject]@{ Name='ExampleLinux'; Disk='' })
$script:deleted = @()
function Get-WCScheduledWslProviderNames { return @('ExampleLinux', 'UnregisteredElsewhere') }
Invoke-Expression $script:AutoEraseScripts['wslData']
Assert-True ($script:AutoEraseFailed -gt 0 -and $script:deleted.Count -eq 0) 'A mismatched provider inventory must fail before unregistering anything.'

Write-Output 'WSL scheduled cleanup fixtures passed.'
