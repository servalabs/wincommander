$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-ProcessIntelligence' }, $true)
if (-not $definition) { throw 'Process Review reader not found.' }
. ([scriptblock]::Create($definition.Extent.Text))

$script:denied = $false
function Get-Process {
    [CmdletBinding()] param()
    if ($script:denied) { throw [UnauthorizedAccessException]::new('Process inventory denied') }
    [pscustomobject]@{ Id = 42; ProcessName = 'synthetic.exe'; MainModule = $null }
}
function Test-Path { $false }
function Get-CimInstance { $null }
function Get-CleanupProcessElevation { param([int]$ProcessId) 'Unknown' }
$populated = Get-ProcessIntelligence
if ($populated.error -or @($populated.processes).Count -ne 1 -or $populated.processes[0].name -ne 'synthetic.exe') {
    throw 'Process Review did not return the one synthetic process.'
}
$script:denied = $true
$failure = Get-ProcessIntelligence
if (-not $failure.error -or $failure.message -notmatch 'denied') { throw 'Denied process enumeration appeared as a clean result.' }
'PASS: Process Review finds one synthetic process and rejects denied enumeration; no live process inventory ran.'
