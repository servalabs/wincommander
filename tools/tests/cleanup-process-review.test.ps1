$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-ProcessIntelligence' }, $true)
if (-not $definition) { throw 'Process Review reader not found.' }
. ([scriptblock]::Create($definition.Extent.Text))

$script:denied = $false
$script:case = 'missing-path'
function Get-Process {
    [CmdletBinding()] param()
    if ($script:denied) { throw [UnauthorizedAccessException]::new('Process inventory denied') }
    $module = if ($script:case -eq 'missing-path') { $null } else { [pscustomobject]@{ FileName = 'C:\synthetic\process.exe' } }
    [pscustomobject]@{ Id = 42; ProcessName = 'synthetic.exe'; MainModule = $module }
}
function Test-Path { return $script:case -ne 'path-inaccessible' }
function Get-AuthenticodeSignature {
    [CmdletBinding()] param($LiteralPath)
    switch ($script:case) {
        'signature-error' { throw [UnauthorizedAccessException]::new('Synthetic signature denial') }
        'signature-unknown' { return [pscustomobject]@{ Status='UnknownError'; SignerCertificate=$null } }
        'unsigned' { return [pscustomobject]@{ Status='NotSigned'; SignerCertificate=$null } }
        default { return [pscustomobject]@{ Status='Valid'; SignerCertificate=[pscustomobject]@{ Subject='Synthetic signer' } } }
    }
}
function Get-CimInstance { $null }
function Get-CleanupProcessElevation { param([int]$ProcessId) if ($script:case -eq 'elevation-error') { throw 'Synthetic elevation denial' }; if ($script:case -eq 'unknown-elevation' -or $script:case -eq 'missing-path') { return 'Unknown' }; 'No' }
function Get-CleanupProcessImagePath { param([int]$ProcessId, [long]$ExpectedCreationTime) if ($script:case -eq 'fallback-path' -and $ExpectedCreationTime -gt 0) { 'C:\synthetic\process.exe' } }
$populated = Get-ProcessIntelligence
if ($populated.error -or -not $populated.partial -or @($populated.processes).Count -ne 1 -or $populated.processes[0].name -ne 'synthetic.exe') {
    throw 'Process Review did not retain an incompletely inspected process.'
}
if ($null -ne $populated.processes[0].path -or $populated.processes[0].pathStatus -ne 'unavailable' -or $populated.processes[0].elevated -ne 'Unknown') { throw 'Unavailable process details were not labeled accurately.' }
if ($populated.observedProcesses -ne 1 -or $populated.unavailableDetails -ne 1 -or $populated.coverage -ne 'limited' -or $populated.coverageNote -notmatch 'Windows protects') { throw 'Protected process coverage was not explained separately from inventory failure.' }
foreach ($case in @('path-inaccessible', 'unknown-elevation', 'elevation-error', 'signature-error', 'signature-unknown')) {
    $script:case = $case; $result = Get-ProcessIntelligence
    if ($result.error -or -not $result.partial -or $result.status -ne 'partial' -or @($result.processes).Count -ne 1) { throw "An unavailable process detail was reported complete: $case" }
}
foreach ($case in @('valid', 'unsigned')) {
    $script:case = $case; $result = Get-ProcessIntelligence
    if ($result.error -or $result.partial -or $result.status -ne 'complete' -or @($result.processes).Count -ne 1) { throw "Readable process details were reported incomplete: $case" }
    if ($result.observedProcesses -ne 1 -or $result.unavailableDetails -ne 0 -or $result.coverage -ne 'complete') { throw 'Complete process coverage was mislabeled.' }
    if ($case -eq 'unsigned' -and $result.processes[0].signed -ne 'NotSigned') { throw 'Unsigned status was lost.' }
}
$script:case = 'fallback-path'
function Get-Process { [CmdletBinding()] param() [pscustomobject]@{ Id = 42; ProcessName = 'synthetic.exe'; MainModule = $null; StartTime = [datetime]::UtcNow } }
$fallback = Get-ProcessIntelligence
if ($fallback.partial -or $fallback.processes[0].path -ne 'C:\synthetic\process.exe') { throw 'Limited-access path fallback did not recover process details.' }
function Get-Process { [CmdletBinding()] param() [pscustomobject]@{ Id = 42; ProcessName = 'synthetic.exe'; MainModule = $null } }
$unverified = Get-ProcessIntelligence
if (-not $unverified.partial -or $unverified.processes[0].path) { throw 'A process without a creation time must not receive fallback path details.' }
$script:case = 'missing-path'
function Get-Process { [CmdletBinding()] param() throw [UnauthorizedAccessException]::new('Process inventory denied') }
# Fleet bounds: one verification per distinct image, a process cap and a time budget.
$script:case = 'valid'
$script:signatureCalls = 0; $script:signatureDelayMs = 0
function Test-Path { return $true }
function Get-AuthenticodeSignature {
    [CmdletBinding()] param($LiteralPath)
    $script:signatureCalls++
    if ($script:signatureDelayMs) { Start-Sleep -Milliseconds $script:signatureDelayMs }
    [pscustomobject]@{ Status = 'Valid'; SignerCertificate = [pscustomobject]@{ Subject = 'Synthetic signer' } }
}
function Get-Process {
    [CmdletBinding()] param()
    1..40 | ForEach-Object { [pscustomobject]@{ Id = 100 + $_; ProcessName = 'shared'; MainModule = [pscustomobject]@{ FileName = $(if ($_ % 2) { 'C:\synthetic\shared.exe' } else { 'C:\SYNTHETIC\SHARED.EXE' }) } } }
    [pscustomobject]@{ Id = 200; ProcessName = 'other'; MainModule = [pscustomobject]@{ FileName = 'C:\synthetic\other.exe' } }
}
$shared = Get-ProcessIntelligence
if ($shared.error -or @($shared.processes).Count -ne 41 -or $shared.totalProcesses -ne 41 -or $shared.truncated) { throw 'The unbounded Process Review must still list every process.' }
if ($script:signatureCalls -ne 2) { throw "Signatures must be verified once per distinct image, not once per process (calls=$($script:signatureCalls))." }
if (@($shared.processes | Where-Object { $_.signed -ne 'Valid' }).Count) { throw 'A cached signature result was not applied to every process sharing the image.' }
$capped = Get-ProcessIntelligence -MaxProcesses 10
if (@($capped.processes).Count -ne 10 -or $capped.totalProcesses -ne 41 -or -not $capped.truncated -or $capped.observedProcesses -ne 10) { throw 'The process cap must bound the rows and still report the full process count.' }
function Get-Process {
    [CmdletBinding()] param()
    1..3 | ForEach-Object { [pscustomobject]@{ Id = 300 + $_; ProcessName = "image$_"; MainModule = [pscustomobject]@{ FileName = "C:\synthetic\image$_.exe" } } }
}
$script:signatureCalls = 0; $script:signatureDelayMs = 1200
$budgeted = Get-ProcessIntelligence -TimeBudgetSeconds 1
if ($script:signatureCalls -ne 1 -or $budgeted.processes[0].signed -ne 'Valid' -or $budgeted.processes[1].signed -ne 'NotChecked' -or $budgeted.processes[2].signed -ne 'NotChecked') { throw 'Signature checks must stop once the time budget is spent.' }
if (-not $budgeted.partial -or $budgeted.signatureChecksSkipped -ne 2 -or $budgeted.coverageNote -notmatch 'time budget' -or @($budgeted.processes).Count -ne 3) { throw 'A budget-limited review must stay listed, partial and explained.' }
$script:signatureDelayMs = 0
$script:case = 'missing-path'
function Get-Process { [CmdletBinding()] param() throw [UnauthorizedAccessException]::new('Process inventory denied') }
$script:denied = $true
$failure = Get-ProcessIntelligence
if (-not $failure.error -or $failure.message -notmatch 'denied') { throw 'Denied process enumeration appeared as a clean result.' }
# Protected or exited processes are normal: the scanned rows come back as a partial (not failed) result with a warning-level note.
$script:denied = $false
$script:case = 'valid'
function Get-Process {
    [CmdletBinding()] param()
    [pscustomobject]@{ Id = 11; ProcessName = 'readable'; MainModule = [pscustomobject]@{ FileName = 'C:\synthetic\readable.exe' } }
    [pscustomobject]@{ Id = 12; ProcessName = 'protected'; MainModule = $null }
    [pscustomobject]@{ Id = 0; ProcessName = 'Idle'; MainModule = $null }
}
function Get-CleanupProcessElevation { param([int]$ProcessId) if ($ProcessId -eq 12) { return 'Unknown' }; 'No' }
$mixed = Get-ProcessIntelligence
if ($mixed.error -or $mixed.status -ne 'partial' -or -not $mixed.partial -or @($mixed.processes).Count -ne 2) { throw 'Protected processes must leave the scanned rows in a partial, non-error result.' }
if ($mixed.unavailableDetails -ne 1 -or $mixed.skippedProcesses -ne 1) { throw 'Unavailable and unlisted process counts were not reported.' }
if ($mixed.coverageNote -notmatch '^1 of 2 processes have limited details \(Windows protects some system processes; others may exit during the scan\)\. 1 could not be listed\.$') { throw "Unexpected limited-details note: $($mixed.coverageNote)" }
# A scan that read no process at all remains a genuine failure.
function Get-Process { [CmdletBinding()] param() [pscustomobject]@{ Id = 0; ProcessName = 'Idle'; MainModule = $null } }
$none = Get-ProcessIntelligence
if (-not $none.error -or @($none.processes).Count -ne 0) { throw 'A scan that read no process must be reported as a failure.' }
'PASS: Process Review finds one synthetic process and rejects denied enumeration; no live process inventory ran.'
