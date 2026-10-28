$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-PSHistory' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:mode = 'empty'
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath)
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('Denied') }
    if ($script:mode -eq 'missing') { throw [System.Management.Automation.ItemNotFoundException]::new('Missing') }
    [pscustomobject]@{ Name='Mock_history.txt' }
}
function Get-Content {
    [CmdletBinding()] param([string]$LiteralPath)
    if ($script:mode -eq 'empty') { return }
    1..305 | ForEach-Object { "Mock command $_" }
    '  '
}
$empty=Get-PSHistory
if ($empty.error -or $empty.total -ne 0 -or $empty.entries.Count -ne 0) { throw 'An empty history file did not report zero.' }
$script:mode='populated'
$full=Get-PSHistory
if ($full.error -or $full.total -ne 305 -or $full.fileTotal -ne 305 -or $full.entries.Count -ne 300 -or $full.sampleCount -ne 300) { throw 'History total was confused with the 300-row preview.' }
if ($full.entries[0].command -ne 'Mock command 305' -or $full.entries[-1].command -ne 'Mock command 6') { throw 'History preview lost newest-first ordering.' }
$script:mode='missing'
$missing=Get-PSHistory
if ($missing.error -or $missing.total -ne 0) { throw 'Absent history was not a successful empty scan.' }
$script:mode='denied'
if (-not (Get-PSHistory).error) { throw 'Denied history was reported as zero.' }
Write-Output 'Passed: empty and absent history; complete 305-line total with newest 300 preview; denied read.'
