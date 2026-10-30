$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
function Assert-True($Condition, $Message) { if (-not $Condition) { throw $Message } }
Assert-True ($script:ScheduledWslFunctions.Contains('WaitForExit(300000)') -and $script:ScheduledWslFunctions.Contains('$process.Kill()')) 'WSL provider timeout must be bounded and fail.'

# The task payload is exercised with a disposable in-memory inventory only.
$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:inventory = @([pscustomobject]@{ Name='ExampleLinux'; Disk='' }, [pscustomobject]@{ Name='ExampleImport'; Disk='' })
function Get-WCScheduledWslInventory { return @($script:inventory) }
function Test-WCScheduledWslOrphans { return $false }
function Remove-WCScheduledWslDistro($Name) { $script:inventory = @($script:inventory | Where-Object { $_.Name -ne $Name }) }
Invoke-Expression $script:AutoEraseScripts['wslData']
Assert-True ($script:AutoEraseRemoved -eq 2 -and $script:AutoEraseFailed -eq 0) 'Registered distributions must be unregistered and verified.'

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

Write-Output 'WSL scheduled cleanup fixtures passed.'
