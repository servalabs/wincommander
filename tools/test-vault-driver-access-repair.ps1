[CmdletBinding()]
param([Parameter(Mandatory)][string]$PinnedDriver, [Parameter(Mandatory)][string]$FixtureParent)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'repair-vault-driver-access.ps1')
if (-not (Test-Path -LiteralPath $FixtureParent -PathType Container) -or @(Get-ChildItem -LiteralPath $FixtureParent -Force).Count -ne 0) {
    throw 'Use a new empty fixture directory.'
}
function Set-EmptyAcl([string]$Path) {
    $acl = New-Object Security.AccessControl.FileSecurity
    $acl.SetOwner([Security.Principal.SecurityIdentifier]'S-1-5-32-544')
    $acl.SetAccessRuleProtection($true, $false)
    Set-Acl -LiteralPath $Path -AclObject $acl
}
$driver = Join-Path $FixtureParent 'EncVolKm.sys'
Copy-Item -LiteralPath $PinnedDriver -Destination $driver
Set-EmptyAcl $driver
$result = Repair-WcPinnedDriverAccess $driver
if (-not $result.repaired) { throw 'Empty DACL was not repaired.' }
if ((Repair-WcPinnedDriverAccess $driver).repaired) { throw 'Repeated repair was not idempotent.' }
$bad = Join-Path $FixtureParent 'wrong-driver.sys'
Copy-Item -LiteralPath $driver -Destination $bad
$bytes = [IO.File]::ReadAllBytes($bad)
$bytes[1024] = $bytes[1024] -bxor 1
[IO.File]::WriteAllBytes($bad, $bytes)
Set-EmptyAcl $bad
$before = (Get-Acl -LiteralPath $bad).Sddl
$rejected = $false
try { Repair-WcPinnedDriverAccess $bad | Out-Null } catch { $rejected = $_.Exception.Message -like '*hash*' }
if (-not $rejected -or (Get-Acl -LiteralPath $bad).Sddl -ne $before) { throw 'Invalid payload was not rejected with ACL rollback.' }
$denied = Join-Path $FixtureParent 'custom-denied.sys'
Copy-Item -LiteralPath $driver -Destination $denied
$acl = Get-Acl -LiteralPath $denied
$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule ([Security.Principal.SecurityIdentifier]'S-1-5-32-545'), 'ReadData', 'Deny'))
Set-Acl -LiteralPath $denied -AclObject $acl
$before = (Get-Acl -LiteralPath $denied).Sddl
try { Repair-WcPinnedDriverAccess $denied | Out-Null } catch {}
if ((Get-Acl -LiteralPath $denied).Sddl -ne $before) { throw 'Custom policy changed.' }
$writable = Join-Path $FixtureParent 'writable-driver.sys'
Copy-Item -LiteralPath $driver -Destination $writable
$acl = Get-Acl -LiteralPath $writable
$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule ([Security.Principal.SecurityIdentifier]'S-1-5-32-545'), 'WriteData', 'Allow'))
Set-Acl -LiteralPath $writable -AclObject $acl
$before = (Get-Acl -LiteralPath $writable).Sddl
$rejected = $false
try { Repair-WcPinnedDriverAccess $writable | Out-Null } catch { $rejected = $_.Exception.Message -like '*untrusted writer*' }
if (-not $rejected -or (Get-Acl -LiteralPath $writable).Sddl -ne $before) { throw 'Writable payload was accepted or modified.' }
$linked = Join-Path $FixtureParent 'hardlink.sys'
New-Item -ItemType HardLink -Path $linked -Target $driver | Out-Null
$rejected = $false
try { Repair-WcPinnedDriverAccess $linked | Out-Null } catch { $rejected = $_.Exception.Message -like '*linked*' }
if (-not $rejected) { throw 'Hard link was not rejected.' }
'PASS: pinned signed payload repair, readback, idempotence, wrong-hash rollback, custom-policy preservation, untrusted-writer rejection, hard-link rejection.'
