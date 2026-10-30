$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
function Assert-AutoEraseAdmin {}
function Initialize-SystemMaintenanceTaskFolder {}
function Get-AutoEraseTaskIdentity { param($Task) @{ categoryId=$Task.Category; ownerSid='S-1-5-21-1-2-3-1001'; taskName=$Task.TaskName } }
function Test-AutoEraseCurrentPayload { $true }
function Get-ScheduledTask { [CmdletBinding()] param($TaskPath,$TaskName) $script:task }
function Export-ScheduledTask { [CmdletBinding()] param($TaskPath,$TaskName) $script:xml }
function Register-ScheduledTask { throw 'An incompatible or current task must not be registered.' }
function Disable-ScheduledTask { throw 'An incompatible task must not be disabled.' }
function Unregister-ScheduledTask { throw 'An incompatible task must not be removed.' }
$script:task = [pscustomobject]@{
    Category=''; TaskPath='\System Maintenance\'; TaskName='SM-U-test'; State='Ready'
    Description='System Maintenance scheduled cleanup v20 (manual opt-in)'
    Settings=[pscustomobject]@{ Hidden=$false }
    Actions=@([pscustomobject]@{ Arguments='-File "C:\ProgramData\WinCommander\auto-erase\scripts\task.ps1"' })
}
$template = @'
<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><Principals><Principal><UserId>Example</UserId><LogonType>LOGON</LogonType></Principal></Principals></Task>
'@
foreach ($category in @('credentialManager','wslData','virtualMachineArtifacts','dockerDesktopData')) {
    $script:task.Category = $category
    foreach ($version in @(6,20)) {
        $script:task.Description = "System Maintenance scheduled cleanup v$version (manual opt-in)"
        $script:xml = $template.Replace('LOGON','S4U')
        $result = Invoke-AutoEraseMigrationCore -CategoryId $category
        if (-not $result.error -or $result.message -notmatch 'incompatible Windows sign-in mode' -or $result.message -notmatch 'left unchanged') { throw "Incompatible $category v$version was not reported safely." }
    }
    $script:task.Description = 'System Maintenance scheduled cleanup v20 (manual opt-in)'
    $script:xml = $template.Replace('LOGON','InteractiveToken')
    $result = Invoke-AutoEraseMigrationCore -CategoryId $category
    if ($result.error -or @($result.migrated).Count -ne 0) { throw "Current interactive $category should remain unchanged: $($result.message) migrated=$($result.migrated.Count)." }
}
Write-Output 'Passed: legacy and current incompatible principals fail without mutating tasks; interactive tasks remain valid.'
