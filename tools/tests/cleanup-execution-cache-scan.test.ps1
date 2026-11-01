$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-ExecutionCache' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))

# Synthetic keys only; no real execution history is read.
$script:denied = $false
function Test-Path { param($LiteralPath, $ErrorAction) return $LiteralPath -like '*Compatibility Assistant\Store' -or $LiteralPath -like '*Shell\MuiCache' }
function Get-ItemProperty {
    param($LiteralPath, $ErrorAction)
    if ($LiteralPath -like '*Shell\MuiCache') {
        if ($script:denied) { throw 'Access denied' }
        return [pscustomobject]@{ 'C:\Example\second.exe' = 'Example' }
    }
    return [pscustomobject]@{ 'C:\Example\first.exe' = 1 }
}
$complete = Get-ExecutionCache
if ($complete.error -or $complete.partial -or @($complete.entries).Count -ne 2) { throw 'Accessible sources must return both entries.' }
$script:denied = $true
$partial = Get-ExecutionCache
if ($partial.error -or -not $partial.partial -or @($partial.entries).Count -ne 1 -or $partial.entries[0].path -ne 'C:\Example\first.exe') { throw 'An inaccessible source must retain other entries and mark the result incomplete.' }
Write-Output 'Passed: execution history preserves accessible entries and reports incomplete sources.'
