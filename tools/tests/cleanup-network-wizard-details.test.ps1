$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-NetworkWizardHistoryInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
function Get-Item {
    param($LiteralPath, $ErrorAction)
    $item = [pscustomobject]@{ Property=@('sampleString','sampleBytes') }
    $item | Add-Member ScriptMethod GetValue { param($name, $default, $options) if ($name -eq 'sampleString') { 'sample text' } else { [byte[]](1,2,3) } }
    $item | Add-Member ScriptMethod GetValueKind { param($name) if ($name -eq 'sampleString') { 'String' } else { 'Binary' } }
    $item
}
function Get-ChildItem { param($LiteralPath, [switch]$Force, $ErrorAction) @() }
$result = Get-NetworkWizardHistoryInfo
if ($result.error -or $result.total -ne 2 -or @($result.files | Where-Object { $_.valueType -eq 'String' -and $_.value -eq 'sample text' }).Count -ne 1 -or @($result.files | Where-Object { $_.valueType -eq 'Binary' -and @($_.value).Count -eq 3 }).Count -ne 1) { throw 'Network Wizard registry values or types were lost.' }
Write-Output 'Passed: Network Wizard retains locally available registry values and types.'
