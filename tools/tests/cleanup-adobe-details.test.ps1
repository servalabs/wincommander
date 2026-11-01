$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-AdobeRecentInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:mode = 'normal'
function Get-Item {
    param($LiteralPath, $ErrorAction)
    if ($LiteralPath -eq 'HKCU:\Software\Adobe\Acrobat Reader') {
        if ($script:mode -eq 'missing') { throw [System.Management.Automation.ItemNotFoundException]::new() }
        if ($script:mode -eq 'denied') { throw [System.UnauthorizedAccessException]::new() }
        return [pscustomobject]@{ Property = @() }
    }
    if ($LiteralPath -like '*cDigitalSignatures') { throw [System.UnauthorizedAccessException]::new() }
    $item = [pscustomobject]@{ Property = @('sample') }
    $item | Add-Member ScriptMethod GetValue { param($name, $default, $options) 'synthetic document path' }
    $item | Add-Member ScriptMethod GetValueKind { param($name) 'String' }
    return $item
}
function Get-ChildItem {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    [pscustomobject]@{ PSPath = 'HKCU:\Software\Adobe\Acrobat Reader\Synthetic' }
}
$result = Get-AdobeRecentInfo
if (-not $result.partial -or $result.total -ne 1 -or $result.files[0].value -ne 'synthetic document path' -or $result.files[0].valueType -ne 'String' -or $result.files[0].path -notlike '*cRecentFiles') { throw 'Adobe details or partial status were lost.' }
$script:mode = 'missing'; $result = Get-AdobeRecentInfo
if ($result.partial -or $result.total -ne 0) { throw 'A missing Adobe root should be empty.' }
$script:mode = 'denied'; $result = Get-AdobeRecentInfo
if (-not $result.partial -or $result.total -ne 0) { throw 'An inaccessible Adobe root must be incomplete.' }
Write-Output 'Passed: Adobe details and partial coverage.'
