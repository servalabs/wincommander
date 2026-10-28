$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
$script:targets = @(); $script:registryTargets = @(); $script:erased = @(); $script:populate = $false
function Get-AutoEraseFiles {
    param($Path,$Filter='*',[switch]$Recurse,$Depth=-1)
    $script:targets += @{ path=$Path; filter=$Filter; recurse=[bool]$Recurse; depth=$Depth }
    if ($script:populate) { [pscustomobject]@{ FullName=(Join-Path $Path 'synthetic.tmp'); LastWriteTime=(Get-Date) } }
}
function Erase-FileSet { param($Files) $script:erased += @($Files) }
function Erase-OneFile { param($Path) $script:erased += $Path }
function Erase-RegistryTree { param($Path) $script:registryTargets += $Path }
function Invoke-AutoEraseWithServices { param($Names,$Body) & $Body }
function Run-Payload($id) {
    $script:targets = @(); $script:registryTargets = @(); $script:erased = @()
    & ([scriptblock]::Create($script:AutoEraseScripts[$id]))
}
Run-Payload 'fontCache'
if ($script:targets.Count -ne 1 -or $script:targets[0].path -notlike '*\ServiceProfiles\LocalService\AppData\Local\FontCache' -or $script:targets[0].filter -ne '*.dat') { throw 'Font scheduling touched installed fonts or non-cache files.' }
Run-Payload 'communicationCaches'
if ($script:targets.Count -ne 8 -or @($script:targets | Where-Object { $_.path -match 'Zoom|MSTeams|Session Storage|Code Cache|GPUCache' }).Count) { throw 'Communication scheduling widened beyond the paired cache contract.' }
Run-Payload 'photosCache'
if ($script:targets.Count -ne 1 -or $script:targets[0].path -notlike '*\Microsoft.Windows.Photos_8wekyb3d8bbwe\LocalState') { throw 'Photos publisher scope widened.' }
Run-Payload 'geolocationCache'
if ($script:targets.Count -ne 1 -or $script:targets[0].path -ne (Join-Path $env:ProgramData 'Microsoft\Windows\LfSvc') -or $script:registryTargets.Count) { throw 'Geolocation scheduling changed permissions or another data root.' }
Run-Payload 'recallDb'
if ($script:targets.Count -ne 10 -or @($script:targets | Where-Object { $_.path -like '*\ActionCenter' -or $_.path -like '*\Windows\Recall' }).Count) { throw 'Recall scheduling widened beyond paired database roots.' }
function Get-NetFirewallProfile { [CmdletBinding()] param() [pscustomobject]@{ LogFileName='%WC_TEST_FIREWALL_ROOT%\synthetic.log' } }
$env:WC_TEST_FIREWALL_ROOT = 'C:\SyntheticFirewall'
Run-Payload 'firewallLog'
if ($script:erased.Count -ne 1 -or $script:erased[0] -ne 'C:\SyntheticFirewall\synthetic.log') { throw 'Firewall environment paths were not resolved.' }
$script:valueExclusions = @()
function Get-AutoEraseRegistryItem { param($Path) [pscustomobject]@{ PSPath=$Path } }
function Get-ChildItem { [CmdletBinding()] param($LiteralPath) [pscustomobject]@{ PSPath='HKLM:\SyntheticBam\SyntheticSid' } }
function Erase-RegistryValues { param($Path,$ExcludedNames) $script:valueExclusions += $ExcludedNames }
Run-Payload 'appLaunchHistory'
if (@($script:valueExclusions).Count -ne 2 -or 'Version' -notin $script:valueExclusions -or 'SequenceNumber' -notin $script:valueExclusions) { throw 'BAM control values were included in scheduled cleanup.' }

$script:removedNeighbors = @(); $script:neighbors = @(
    [pscustomobject]@{ InterfaceIndex=1; IPAddress='192.0.2.1'; State='Permanent' },
    [pscustomobject]@{ InterfaceIndex=1; IPAddress='192.0.2.2'; State='Reachable' }
)
function Get-NetNeighbor { [CmdletBinding()] param() $script:neighbors }
function Remove-NetNeighbor {
    [CmdletBinding(SupportsShouldProcess)] param($InterfaceIndex,$IPAddress)
    $script:removedNeighbors += $IPAddress
    $script:neighbors = @($script:neighbors | Where-Object IPAddress -ne $IPAddress)
}
Run-Payload 'neighborCache'
if ($script:removedNeighbors.Count -ne 1 -or $script:removedNeighbors[0] -ne '192.0.2.2') { throw 'A static neighbor was scheduled for deletion.' }

function netsh { $global:LASTEXITCODE = 5 }
function Test-Path { throw 'A failed native flush must stop before filesystem work.' }
$caught = $false
try { Run-Payload 'branchCache' } catch { $caught = $_.Exception.Message -eq 'BranchCache flush failed.' }
if (-not $caught) { throw 'A failed native flush was accepted.' }
$script:populate = $true; $caught = $false
try { Run-Payload 'officeTempFiles' } catch { $caught = $_.Exception.Message -like '*cleanup is incomplete*' }
if (-not $caught) { throw 'Office temporary-file cleanup did not detect fresh leftovers.' }
function Test-Path { param($Path) $false }
function cmdkey.exe {
    if ($args.Count -ne 1 -or $args[0] -ne '/list') { throw 'RDP scheduling tried to delete a saved credential.' }
    $global:LASTEXITCODE = 0
    'Target: Domain:target=TERMSRV/synthetic-host'
}
$caught = $false
try { Run-Payload 'rdpHistory' } catch { $caught = $_.Exception.Message -like '*saved credentials were preserved*' }
if (-not $caught) { throw 'RDP scheduling falsely reported completion while preserved credentials remained.' }
Write-Output 'Passed: cache target parity, static-neighbor preservation, native flush failure, and fresh temporary-file read-back.'
