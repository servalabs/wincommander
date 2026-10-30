$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module could not be parsed.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-PSHistory' }, $true)
if (-not $definition) { throw 'PowerShell history reader is missing.' }
. ([scriptblock]::Create($definition.Extent.Text))
function Get-Item { param($LiteralPath, $ErrorAction) return @{ exists = $true } }
function Get-Content { param($LiteralPath, $ErrorAction) 1..650 | ForEach-Object { "synthetic-command-$_" } }
$result = Get-PSHistory
if ($result.error -or $result.total -ne 650 -or $result.entries.Count -ne 650 -or $result.truncated) { throw 'The full history was not returned.' }
if ($result.entries[0].command -ne 'synthetic-command-650' -or $result.entries[649].command -ne 'synthetic-command-1') { throw 'History order or record contents were not preserved.' }
Write-Output 'Passed: the local PowerShell history reader returns more than 300 records in order.'
