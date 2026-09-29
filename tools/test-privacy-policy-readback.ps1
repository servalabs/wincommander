$ErrorActionPreference = 'Stop'
$source = Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri\commander-free\scripts\modules\privacy\telemetry.ps1'
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'Telemetry module contains invalid PowerShell.' }
$functions = @('Get-PrivacyPolicyReadback', 'Set-PrivacyPolicyValuesVerified', 'Get-RecallSnapshotsStatus',
    'Disable-RecallSnapshots', 'Enable-RecallSnapshots', 'Get-OfficeTelemetryTasks', 'Get-OfficeLoggingStatus',
    'Set-OfficeLoggingVerified', 'Disable-OfficeLogging', 'Enable-OfficeLogging',
    'Get-InternetCommunicationStatus', 'Disable-InternetCommunication', 'Enable-InternetCommunication')
foreach ($name in $functions) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }.GetNewClosure(), $true)
    if (-not $definition) { throw "Missing function: $name" }
    Invoke-Expression $definition.Extent.Text
}
foreach ($name in @('WC_RECALL_POLICY_KEYS', 'WC_OFFICE_LOGGING_KEYS', 'WC_INTERNET_COMM_KEYS')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq ('$Script:' + $name) }.GetNewClosure(), $true)
    if (-not $definition) { throw "Missing policy targets: $name" }
    Invoke-Expression $definition.Extent.Text
}

function Reset-Mocks {
    $script:registry = @{}
    $script:tasks = @()
    $script:ignoreWrite = $false
    $script:ignoreRemove = $false
    $script:ignoreTask = $false
    $script:denyRead = $false
    $script:denyWrite = $false
    $script:isAdmin = $true
    $script:writeCalls = 0
}
function Assert-IsAdmin { if (-not $script:isAdmin) { throw 'Administrator privileges required.' } }
function Test-Path {
    [CmdletBinding()] param([string]$LiteralPath)
    if ($script:denyRead) { throw 'Injected access denied.' }
    $script:registry.ContainsKey($LiteralPath)
}
function New-Item {
    [CmdletBinding()] param([string]$Path, [switch]$Force)
    $script:registry[$Path] = @{}
}
function Set-ItemProperty {
    [CmdletBinding()] param([string]$LiteralPath, [string]$Name, $Value, [string]$Type, [switch]$Force)
    $script:writeCalls++
    if ($script:denyWrite) { Write-Error 'Injected nonterminating write failure.'; return }
    if (-not $script:ignoreWrite) { $script:registry[$LiteralPath][$Name] = $Value }
}
function Get-ItemProperty {
    [CmdletBinding()] param([string]$LiteralPath)
    if ($script:denyRead) { throw 'Injected access denied.' }
    if (-not $script:registry.ContainsKey($LiteralPath)) { throw [System.Management.Automation.ItemNotFoundException]::new('Injected missing key.') }
    [pscustomobject]$script:registry[$LiteralPath]
}
function Remove-ItemProperty {
    [CmdletBinding()] param([string]$LiteralPath, [string]$Name)
    if (-not $script:ignoreRemove) { $script:registry[$LiteralPath].Remove($Name) }
}
function Get-ScheduledTask {
    [CmdletBinding()] param([string]$TaskPath, [string]$TaskName)
    if ($script:denyRead) { throw 'Injected task access denied.' }
    if ($TaskName) { @($script:tasks | Where-Object { $_.TaskPath -eq $TaskPath -and $_.TaskName -eq $TaskName }) }
    else { @($script:tasks) }
}
function Disable-ScheduledTask {
    [CmdletBinding()] param([string]$TaskPath, [string]$TaskName)
    if (-not $script:ignoreTask) {
        $script:tasks | Where-Object { $_.TaskPath -eq $TaskPath -and $_.TaskName -eq $TaskName } | ForEach-Object { $_.State = 'Disabled' }
    }
}
function Enable-ScheduledTask {
    [CmdletBinding()] param([string]$TaskPath, [string]$TaskName)
    if (-not $script:ignoreTask) {
        $script:tasks | Where-Object { $_.TaskPath -eq $TaskPath -and $_.TaskName -eq $TaskName } | ForEach-Object { $_.State = 'Ready' }
    }
}
function Assert-True([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Assert-Failed($Result, [string]$Message) {
    Assert-True ($Result.error -eq $true -and $Result.verified -eq $false -and -not $Result.status) $Message
}

foreach ($case in @(
    @{ Disable = 'Disable-RecallSnapshots'; Enable = 'Enable-RecallSnapshots'; Status = 'Get-RecallSnapshotsStatus'; Field = 'disabled' },
    @{ Disable = 'Disable-OfficeLogging'; Enable = 'Enable-OfficeLogging'; Status = 'Get-OfficeLoggingStatus'; Field = 'disabled' },
    @{ Disable = 'Disable-InternetCommunication'; Enable = 'Enable-InternetCommunication'; Status = 'Get-InternetCommunicationStatus'; Field = 'restricted' }
)) {
    Reset-Mocks
    $empty = & $case.Status
    Assert-True ($empty.verified -eq $true -and $empty[$case.Field] -eq $false) 'Missing policy must not look disabled.'
    $success = & $case.Disable
    Assert-True ($success.status -eq 'disabled' -and $success.verified -eq $true) 'Successful write did not verify.'
    $observed = & $case.Status
    Assert-True ($observed.verified -eq $true -and $observed[$case.Field] -eq $true) 'Reopened status did not match the applied policy.'
    $success = & $case.Enable
    Assert-True ($success.status -eq 'enabled' -and $success.verified -eq $true) 'Successful removal did not verify.'
    Assert-True ((& $case.Status)[$case.Field] -eq $false) 'Reopened status retained removed policies.'

    Reset-Mocks
    $script:ignoreWrite = $true
    Assert-Failed (& $case.Disable) 'Ignored registry writes falsely reported success.'
    Reset-Mocks
    $script:denyWrite = $true
    Assert-Failed (& $case.Disable) 'Nonterminating write error falsely reported success.'
    Reset-Mocks
    [void](& $case.Disable)
    $script:ignoreRemove = $true
    Assert-Failed (& $case.Enable) 'Ignored registry removal falsely reported success.'
    Reset-Mocks
    $script:denyRead = $true
    $unknown = & $case.Status
    Assert-True ($unknown.verified -eq $false -and $null -eq $unknown[$case.Field]) 'Unreadable policy must remain unknown.'
    Reset-Mocks
    $script:isAdmin = $false
    Assert-Failed (& $case.Disable) 'Denied administrator check falsely reported success.'
    Assert-True ($script:writeCalls -eq 0) 'Unprivileged mutation attempted registry writes.'
}

Reset-Mocks
$script:registry[$Script:WC_RECALL_POLICY_KEYS[0].Path] = @{ DisableAIDataAnalysis = 1 }
Assert-True ((Get-RecallSnapshotsStatus).disabled -eq $true) 'Machine Recall policy must remain effective without a user value.'

Reset-Mocks
$script:registry[$Script:WC_OFFICE_LOGGING_KEYS[4].Path] = @{ DisableLogManagement = 1 }
Assert-True ((Get-OfficeLoggingStatus).disabled -eq $false) 'One Office policy value must not prove all logging is disabled.'

Reset-Mocks
$script:tasks = @([pscustomobject]@{ TaskPath = '\Microsoft\Office\'; TaskName = 'OfficeTelemetryAgentLogOn'; State = 'Ready' })
$script:ignoreTask = $true
Assert-Failed (Disable-OfficeLogging) 'An ignored task disable falsely reported success.'
Assert-True ((Get-OfficeLoggingStatus).disabled -eq $false) 'Enabled telemetry task must remain visible to status.'
$script:ignoreTask = $false
Assert-True ((Disable-OfficeLogging).verified -eq $true) 'Office task disable failed to verify.'
$script:ignoreTask = $true
Assert-Failed (Enable-OfficeLogging) 'An ignored task restore falsely reported success.'
$script:ignoreTask = $false
Assert-True ((Enable-OfficeLogging).verified -eq $true) 'Office task restoration failed to verify.'

Write-Output 'PASS: three privacy policies, reopen readback, access failures, partial writes/removal, and Office task verification; all mutations mocked.'
