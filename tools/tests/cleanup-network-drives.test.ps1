$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-NetworkDrives' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
function Get-PSDrive {
    param($PSProvider, $ErrorAction)
    foreach ($drive in $script:fixtureDrives) { $drive }
}
foreach ($count in 0, 1, 2) {
    $script:fixtureDrives = @(if ($count -ge 1) { [pscustomobject]@{ Name='Z'; DisplayRoot='\\server\share' } }; if ($count -ge 2) { [pscustomobject]@{ Name='Y'; DisplayRoot='\\server\other' } })
    $result = Get-NetworkDrives
    if ($result.error -or $result.drives -isnot [array] -or $result.drives.Count -ne $count) { throw "Network drive array did not preserve $count results." }
    if ($count -gt 0 -and ($result.drives[0].localName -ne 'Z' -or $result.drives[0].remoteName -ne '\\server\share')) { throw 'Network drive details were not mapped to the card fields.' }
}
Write-Output 'Passed: zero, one and multiple mapped network drives.'
