$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-GeolocationCacheInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:mode = 'normal'
function New-TestLocationKey($name, $path) {
    $key = [pscustomobject]@{ PSChildName=$name; PSPath=$path; Property=@('Value', 'LastUsedTimeStop') }
    $key | Add-Member ScriptMethod GetValue { param($name, $default, $options) if ($name -eq 'Value') { 'Allow' } else { [long]42 } }
    $key | Add-Member ScriptMethod GetValueKind { param($name) if ($name -eq 'Value') { 'String' } else { 'QWord' } }
    $key
}
function Get-Item {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($LiteralPath -like 'C:\ProgramData*') {
        if ($script:mode -eq 'denied') { throw [System.UnauthorizedAccessException]::new() }
        throw [System.Management.Automation.ItemNotFoundException]::new()
    }
    if ($script:mode -eq 'missing') { throw [System.Management.Automation.ItemNotFoundException]::new() }
    New-TestLocationKey 'location' $LiteralPath
}
function Get-ChildItem {
    param($LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    New-TestLocationKey 'SyntheticApp' ($LiteralPath + '\SyntheticApp')
}
$result = Get-GeolocationCacheInfo
if ($result.partial -or $result.total -ne 3 -or @($result.files | Where-Object { $_.value -eq 'Allow' -and $_.valueType -eq 'String' }).Count -ne 1) { throw 'Root location consent values were not preserved.' }
$usage = @($result.files | Where-Object { $_['values'] })
if ($usage.Count -ne 1 -or $usage[0]['values']['Value'].value -ne 'Allow' -or $usage[0]['values']['LastUsedTimeStop'].value -ne 42) { throw 'Location usage values were not preserved.' }
$script:mode = 'denied'; $result = Get-GeolocationCacheInfo
if (-not $result.partial -or $result.total -ne 3) { throw 'An inaccessible cache must retain consent details and report incomplete.' }
$script:mode = 'missing'; $result = Get-GeolocationCacheInfo
if ($result.partial -or $result.total -ne 0) { throw 'Missing locations should report an empty scan.' }
Write-Output 'Passed: geolocation details and partial source handling.'
