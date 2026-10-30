$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-GitActivityInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
function cmdkey.exe { 'Target=git:https://fixture.invalid' }
function git.exe { 'C:\Fixture\One'; 'D:\Fixture\Two' }
function Test-Path { param($Path, $PathType, $ErrorAction) $true }
$result = Get-GitActivityInfo
if ($result.error -or $result.total -ne 3) { throw 'Git activity count did not include each returned entry.' }
$entries = @($result.files | Where-Object { $_.name -eq 'Global Git safe.directory' })
if ($entries.Count -ne 2 -or $entries[0].value -ne 'C:\Fixture\One' -or $entries[1].value -ne 'D:\Fixture\Two') { throw 'Captured safe.directory values were not preserved.' }
Write-Output 'Passed: Git safe directory values are preserved.'
