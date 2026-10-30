$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-WERHistoryInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))

# Synthetic registry values only; do not read the device registry.
function Get-Item {
    param($LiteralPath, $ErrorAction)
    $item = [pscustomobject]@{ Property=@('sampleString','sampleNumber','sampleBytes') }
    $item | Add-Member ScriptMethod GetValue { param($name, $default, $options) switch ($name) { sampleString { 'sample text' } sampleNumber { 7 } sampleBytes { [byte[]](1,2,3) } } }
    $item | Add-Member ScriptMethod GetValueKind { param($name) switch ($name) { sampleString { 'String' } sampleNumber { 'DWord' } sampleBytes { 'Binary' } } }
    return $item
}
function Get-ChildItem { param($LiteralPath, [switch]$Force, $ErrorAction) return @() }
$result = Get-WERHistoryInfo
if ($result.error -or $result.total -ne 12) { throw 'WER synthetic values were not returned.' }
$rows = @($result.files)
if (@($rows | Where-Object { $_.valueType -eq 'String' -and $_.value -eq 'sample text' }).Count -ne 4 -or @($rows | Where-Object { $_.valueType -eq 'DWord' -and $_.value -eq 7 }).Count -ne 4 -or @($rows | Where-Object { $_.valueType -eq 'Binary' -and @($_.value).Count -eq 3 }).Count -ne 4) { throw 'WER registry values or their types were lost.' }
Write-Output 'Passed: WER history retains locally returned registry values and types.'
