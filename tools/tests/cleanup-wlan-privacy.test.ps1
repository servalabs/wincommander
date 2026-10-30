$ErrorActionPreference = 'Stop'
$cleanupPath = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$routerPath = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\core\router.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $cleanupPath), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-WlanProfiles' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
. $routerPath
function ConvertTo-JsonOutput { param($Value) $Value }
$script:secretRequests = 0
function Get-WCWlanSnapshot { @{ InterfaceCount=1; Profiles=@(@{ Name='Synthetic Network'; InterfaceId='synthetic'; Flags=0 }) } }
function Assert-WCWlanSnapshot { param($Snapshot) }
function Get-WCWlanKey { param($InterfaceId, $Name) $script:secretRequests++; 'synthetic-only' }
$savedCommand = $env:WINCMD_COMMAND; $savedParams = $env:WINCMD_PARAMS_JSON
try {
    $env:WINCMD_COMMAND = 'Get-WlanProfiles'
    $env:WINCMD_PARAMS_JSON = '{"IncludeSecrets":false}'
    $result = Invoke-BackendCommand
    if ($result.error -or $result.profiles.Count -ne 1 -or $script:secretRequests -ne 0 -or $null -ne $result.profiles[0].password) { throw 'Cleanup scan requested a Wi-Fi secret.' }
    $env:WINCMD_PARAMS_JSON = '{"IncludeSecrets":true}'
    $result = Invoke-BackendCommand
    if ($result.error -or $script:secretRequests -ne 1 -or $result.profiles[0].password -ne 'synthetic-only') { throw 'Explicit backend opt-in was not preserved.' }
} finally {
    $env:WINCMD_COMMAND = $savedCommand; $env:WINCMD_PARAMS_JSON = $savedParams
}
Write-Output 'Passed: the Free router keeps cleanup Wi-Fi reads secret-free; explicit opt-in remains available.'
