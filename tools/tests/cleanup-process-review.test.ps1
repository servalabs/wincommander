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
$populated = Get-ProcessIntelligence
if ($populated.error -or -not $populated.partial -or @($populated.processes).Count -ne 1 -or $populated.processes[0].name -ne 'synthetic.exe') {
    throw 'Process Review did not retain an incompletely inspected process.'
}
if ($null -ne $populated.processes[0].path -or $populated.processes[0].pathStatus -ne 'unavailable' -or $populated.processes[0].elevated -ne 'Unknown') { throw 'Unavailable process details were not labeled accurately.' }
foreach ($case in @('path-inaccessible', 'unknown-elevation', 'elevation-error', 'signature-error', 'signature-unknown')) {
    $script:case = $case; $result = Get-ProcessIntelligence
    if ($result.error -or -not $result.partial -or $result.status -ne 'partial' -or @($result.processes).Count -ne 1) { throw "An unavailable process detail was reported complete: $case" }
}
foreach ($case in @('valid', 'unsigned')) {
    $script:case = $case; $result = Get-ProcessIntelligence
    if ($result.error -or $result.partial -or $result.status -ne 'complete' -or @($result.processes).Count -ne 1) { throw "Readable process details were reported incomplete: $case" }
    if ($case -eq 'unsigned' -and $result.processes[0].signed -ne 'NotSigned') { throw 'Unsigned status was lost.' }
}
$script:denied = $true
$failure = Get-ProcessIntelligence
if (-not $failure.error -or $failure.message -notmatch 'denied') { throw 'Denied process enumeration appeared as a clean result.' }
'PASS: Process Review finds one synthetic process and rejects denied enumeration; no live process inventory ran.'
