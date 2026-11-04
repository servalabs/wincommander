$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-CellularHistoryInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:hasProfile = $true; $script:failCommand = $false
function Get-NetAdapter { [CmdletBinding()] param([switch]$IncludeHidden) [pscustomobject]@{ InterfaceType = 243; NdisPhysicalMedium = 8 } }
function netsh.exe {
    $global:LASTEXITCODE = if ($script:failCommand) { 1 } else { 0 }
    if (($args -join ' ') -like '*show interfaces*') { 'Name : Mock cellular interface' }
    elseif ($script:hasProfile) { 'Profile Name : Mock profile' }
}
$populated = Get-CellularHistoryInfo
if ($populated.error -or $populated.total -ne 1 -or $populated.interfaceCount -ne 1) { throw 'Cellular profile count is incorrect.' }
$script:hasProfile = $false
$empty = Get-CellularHistoryInfo
if ($empty.error -or -not $empty.partial -or $empty.total -ne 0 -or $empty.interfaceCount -ne 1) { throw 'Unrecognized profile output must not be treated as a verified empty inventory.' }
function netsh.exe {
    $global:LASTEXITCODE = 0
    if (($args -join ' ') -like '*show interfaces*') { 'Name : Mock cellular interface' }
    else { 'There are no profiles on interface Mock cellular interface.' }
}
$verifiedEmpty = Get-CellularHistoryInfo
if ($verifiedEmpty.error -or $verifiedEmpty.partial -or $verifiedEmpty.total -ne 0 -or $verifiedEmpty.status -ne 'complete') { throw 'An explicit no-profiles response should be a verified empty inventory.' }
function netsh.exe {
    $global:LASTEXITCODE = if ($script:failCommand) { 1 } else { 0 }
    if (($args -join ' ') -like '*show interfaces*') { 'Name : Mock cellular interface' }
    elseif ($script:hasProfile) { 'Profile Name : Mock profile' }
}
$script:failCommand = $true
$script:serviceStatus = 'Stopped'
function Get-Service { [CmdletBinding()] param([string]$Name) if ($script:serviceStatus) { [pscustomobject]@{ Status = $script:serviceStatus } } }
$stopped = Get-CellularHistoryInfo
if (-not $stopped.error -or $stopped.message -notmatch 'service is stopped') { throw 'A stopped broadband service must be reported as unavailable, not empty.' }
$script:serviceStatus = $null
if ((Get-CellularHistoryInfo).message -notmatch 'service is unavailable') { throw 'An unavailable broadband service must be identified.' }
$script:serviceStatus = 'Running'
if ((Get-CellularHistoryInfo).message -notmatch 'interfaces could not be read') { throw 'A running service with a failed query must remain an error.' }
function Get-NetAdapter { [CmdletBinding()] param([switch]$IncludeHidden) [pscustomobject]@{ InterfaceType = 6; NdisPhysicalMedium = 0 } }
$noHardware = Get-CellularHistoryInfo
if ($noHardware.error -or $noHardware.partial -or -not $noHardware.noHardware -or $noHardware.status -ne 'not_applicable' -or $noHardware.coverageNote -notmatch 'disconnected') { throw 'No current broadband adapter must be distinguished from a verified empty profile inventory.' }
function Get-NetAdapter { [CmdletBinding()] param([switch]$IncludeHidden) throw 'Synthetic adapter enumeration failure' }
if (-not (Get-CellularHistoryInfo).error) { throw 'Failed adapter inventory must not become an empty or no-hardware result.' }
$script:failCommand = $false
function netsh.exe {
    $global:LASTEXITCODE = 0
    if (($args -join ' ') -like '*show interfaces*') { 'Name : first'; 'Name : second' }
    elseif (($args -join ' ') -like '*second*') { $global:LASTEXITCODE = 1 }
    else { 'Profile Name : observed' }
}
$partial = Get-CellularHistoryInfo
if (-not $partial.partial -or $partial.status -ne 'partial' -or $partial.total -ne 1 -or $partial.files[0].profileName -ne 'observed' -or $partial.files[0].interface -ne 'first') { throw 'A later adapter failure discarded observed cellular details or appeared complete.' }
function netsh.exe {
    $global:LASTEXITCODE = 0
    if (($args -join ' ') -like '*show interfaces*') { 'Nom : interface mobile' }
}
$unrecognized = Get-CellularHistoryInfo
if ($unrecognized.error -or -not $unrecognized.partial -or $unrecognized.status -ne 'partial') { throw 'Unrecognized interface output must not be reported as a verified empty inventory.' }
function netsh.exe {
    $global:LASTEXITCODE = 0
    if (($args -join ' ') -like '*show interfaces*') { 'Name : cellular' }
    else { 'Nom du profil : exemple' }
}
$unrecognizedProfile = Get-CellularHistoryInfo
if ($unrecognizedProfile.error -or -not $unrecognizedProfile.partial -or $unrecognizedProfile.status -ne 'partial') { throw 'Unrecognized profile output must not be reported as a verified empty inventory.' }
Write-Output 'Passed: cellular profiles counted; interfaces remain metadata; command failure is explicit.'
