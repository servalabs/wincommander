$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-ProxyCacheInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:mode = 'normal'
function Get-Item {
    param($LiteralPath, $ErrorAction)
    if ($script:mode -eq 'missing') { throw [System.Management.Automation.ItemNotFoundException]::new() }
    if ($script:mode -eq 'denied') { throw [System.UnauthorizedAccessException]::new() }
    $item = [pscustomobject]@{ Property=@('sampleString','sampleBytes','failedValue') }
    $item | Add-Member ScriptMethod GetValue { param($name, $default, $options) if ($name -eq 'failedValue') { throw 'synthetic unavailable value' }; if ($name -eq 'sampleString') { 'sample text' } else { [byte[]](1,2,3) } }
    $item | Add-Member ScriptMethod GetValueKind { param($name) if ($name -eq 'sampleString') { 'String' } else { 'Binary' } }
    $item
}
$result = Get-ProxyCacheInfo
if (-not $result.partial -or $result.total -ne 2 -or @($result.files | Where-Object { $_.valueType -eq 'String' -and $_.value -eq 'sample text' }).Count -ne 1 -or @($result.files | Where-Object { $_.valueType -eq 'Binary' -and @($_.value).Count -eq 3 }).Count -ne 1) { throw 'Proxy values, types, or partial scan status were lost.' }
$script:mode = 'missing'; $result = Get-ProxyCacheInfo
if ($result.partial -or $result.total -ne 0) { throw 'A missing proxy registry key should report an empty scan.' }
$script:mode = 'denied'; $result = Get-ProxyCacheInfo
if (-not $result.partial -or $result.total -ne 0) { throw 'An inaccessible proxy registry key must not report a complete empty scan.' }
Write-Output 'Passed: proxy scan retains available registry values and reports incomplete sources.'
