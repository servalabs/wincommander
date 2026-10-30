$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-CellularHistoryInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:hasProfile = $true; $script:failCommand = $false
function netsh.exe {
    $global:LASTEXITCODE = if ($script:failCommand) { 1 } else { 0 }
    if (($args -join ' ') -like '*show interfaces*') { 'Name : Mock cellular interface' }
    elseif ($script:hasProfile) { 'Profile Name : Mock profile' }
}
$populated = Get-CellularHistoryInfo
if ($populated.error -or $populated.total -ne 1 -or $populated.interfaceCount -ne 1) { throw 'Cellular profile count is incorrect.' }
$script:hasProfile = $false
$empty = Get-CellularHistoryInfo
if ($empty.error -or $empty.total -ne 0 -or $empty.interfaceCount -ne 1) { throw 'A live cellular interface was counted as history.' }
$script:failCommand = $true
if (-not (Get-CellularHistoryInfo).error) { throw 'A failed cellular command was reported as empty.' }
$script:failCommand = $false
function netsh.exe {
    $global:LASTEXITCODE = 0
    if (($args -join ' ') -like '*show interfaces*') { 'Name : first'; 'Name : second' }
    elseif (($args -join ' ') -like '*second*') { $global:LASTEXITCODE = 1 }
    else { 'Profile Name : observed' }
}
$partial = Get-CellularHistoryInfo
if (-not $partial.partial -or $partial.status -ne 'partial' -or $partial.total -ne 1 -or $partial.files[0].profileName -ne 'observed' -or $partial.files[0].interface -ne 'first') { throw 'A later adapter failure discarded observed cellular details or appeared complete.' }
Write-Output 'Passed: cellular profiles counted; interfaces remain metadata; command failure is explicit.'
