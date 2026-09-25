$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$telemetryModule = Join-Path $repoRoot 'src-tauri\commander-free\scripts\modules\privacy\telemetry.ps1'
$systemModule = Join-Path $repoRoot 'src-tauri\commander-free\scripts\modules\tweaks\system.ps1'
$tokens = $null
$parseErrors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile($systemModule, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw ($parseErrors | Out-String) }
. $telemetryModule

$script:mockRegistry = @{}
$script:mockTasks = @()
$script:mockSessions = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
$script:stuckSessions = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
$script:ignoreWrites = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)

function Assert-IsAdmin {}

function Test-Path {
    [CmdletBinding()]
    param(
        [Parameter(Position = 0)][string]$Path,
        [string]$LiteralPath,
        [string]$PathType
    )
    $target = if ($LiteralPath) { $LiteralPath } else { $Path }
    $script:mockRegistry.ContainsKey($target)
}

function Set-ItemProperty {
    [CmdletBinding()]
    param(
        [string]$Path,
        [string]$LiteralPath,
        [string]$Name,
        [object]$Value,
        [string]$Type,
        [switch]$Force
    )
    $target = if ($LiteralPath) { $LiteralPath } else { $Path }
    if ($Name -eq 'Start' -and -not $script:ignoreWrites.Contains($target)) {
        $script:mockRegistry[$target].Start = [int]$Value
    }
}

function Get-ItemProperty {
    [CmdletBinding()]
    param(
        [string]$Path,
        [string]$LiteralPath,
        [string]$Name
    )
    $target = if ($LiteralPath) { $LiteralPath } else { $Path }
    if (-not $script:mockRegistry.ContainsKey($target)) { throw "Mock registry key not found: $target" }
    [pscustomobject]@{ Start = $script:mockRegistry[$target].Start }
}

function Get-ScheduledTask {
    [CmdletBinding()]
    param()
    @($script:mockTasks)
}

function Disable-ScheduledTask {
    [CmdletBinding()]
    param([string]$TaskPath, [string]$TaskName)
    $task = $script:mockTasks | Where-Object { $_.TaskPath -eq ($TaskPath + '\') -and $_.TaskName -eq $TaskName } | Select-Object -First 1
    if ($task) { $task.State = 'Disabled' }
}

function Enable-ScheduledTask {
    [CmdletBinding()]
    param([string]$TaskPath, [string]$TaskName)
    $task = $script:mockTasks | Where-Object { $_.TaskPath -eq ($TaskPath + '\') -and $_.TaskName -eq $TaskName } | Select-Object -First 1
    if ($task) { $task.State = 'Ready' }
}

function logman {
    $operation = [string]$args[0]
    $name = [string]$args[1]
    if ($operation -eq 'query' -and $name -eq '-ets') {
        foreach ($session in $script:mockSessions) { Write-Output "$session Trace Running" }
        $global:LASTEXITCODE = 0
        return
    }
    if ($operation -eq 'stop' -and $name) {
        if (-not $script:stuckSessions.Contains($name)) { [void]$script:mockSessions.Remove($name) }
        $global:LASTEXITCODE = 0
        return
    }
    $global:LASTEXITCODE = 1
}

function Reset-MockTracingState {
    $script:mockRegistry.Clear()
    $script:mockTasks = @()
    $script:mockSessions.Clear()
    $script:stuckSessions.Clear()
    $script:ignoreWrites.Clear()

    foreach ($logger in $Script:WC_DIAG_ETW_LOGGERS) {
        $path = "HKLM:\SYSTEM\CurrentControlSet\Control\WMI\Autologger\$logger"
        $script:mockRegistry[$path] = [pscustomobject]@{ Start = 1 }
    }
    foreach ($taskSpec in $Script:WC_DIAG_ETW_TASKS) {
        $script:mockTasks += [pscustomobject]@{
            TaskPath = $taskSpec.Path + '\'
            TaskName = $taskSpec.Name
            State = 'Ready'
        }
    }
}

Reset-MockTracingState
[void]$script:mockSessions.Add('SleepStudy')
[void]$script:mockSessions.Add('WdiContextLog')
$disabled = Disable-DiagnosticEventTracing
if ($disabled.error -or $disabled.status -ne 'disabled' -or -not $disabled.effectiveState.disabled) {
    throw 'The successful ETW disable path did not verify the requested off state.'
}
if ($script:mockSessions.Count -ne 0 -or @($script:mockRegistry.Values | Where-Object Start -ne 0).Count -ne 0) {
    throw 'ETW sessions or autologger registrations remained enabled after disable.'
}
if (@($script:mockTasks | Where-Object State -ne 'Disabled').Count -ne 0) {
    throw 'A diagnostic tracing scheduled task remained enabled after disable.'
}

$enabled = Enable-DiagnosticEventTracing
if ($enabled.error -or $enabled.status -ne 'enabled' -or $enabled.effectiveState.disabled) {
    throw 'The successful ETW re-enable path did not verify the requested on state.'
}
if (@($script:mockRegistry.Values | Where-Object Start -ne 1).Count -ne 0) {
    throw 'An autologger registration remained disabled after re-enable.'
}
if (@($script:mockTasks | Where-Object State -eq 'Disabled').Count -ne 0) {
    throw 'A diagnostic tracing scheduled task remained disabled after re-enable.'
}

Reset-MockTracingState
[void]$script:mockSessions.Add('SleepStudy')
[void]$script:stuckSessions.Add('SleepStudy')
$failedLoggerPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\WMI\Autologger\WdiContextLog'
[void]$script:ignoreWrites.Add($failedLoggerPath)
$partial = Disable-DiagnosticEventTracing
if (-not $partial.error -or $partial.status -ne 'partial' -or $partial.effectiveState.disabled) {
    throw 'A stuck session or unverifiable registry write was reported as a successful disable.'
}
if ($partial.errors.Count -lt 2) { throw 'The partial disable did not report both actionable failures.' }

Write-Output 'Diagnostic tracing transition contract passed: successful disable/re-enable and partial failure were verified with mocks.'
