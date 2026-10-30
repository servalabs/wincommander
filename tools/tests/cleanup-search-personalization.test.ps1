$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-SearchPersonalizationInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:mode = 'normal'
function Get-Item {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:mode -eq 'missing') { throw [System.Management.Automation.ItemNotFoundException]::new() }
    if ($LiteralPath -like '*TrainedDataStore') { return [pscustomobject]@{ PSIsContainer = $true } }
    if ($LiteralPath -like '*\Launch') { throw [System.UnauthorizedAccessException]::new() }
    $key = [pscustomobject]@{ Property = @('sampleString', 'sampleBytes') }
    $key | Add-Member ScriptMethod GetValue { param($name, $default, $options) if ($name -eq 'sampleString') { 'sample text' } else { return ,([byte[]](1, 2, 3)) } }
    $key | Add-Member ScriptMethod GetValueKind { param($name) if ($name -eq 'sampleString') { 'String' } else { 'Binary' } }
    $key
}
function Get-ChildItem {
    param($LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    if ($LiteralPath -like '*JumplistData') { throw [System.UnauthorizedAccessException]::new() }
    if ($LiteralPath -like '*TrainedDataStore') { return [pscustomobject]@{ Name='sample.dat'; FullName='C:\synthetic\sample.dat'; Length=1024; LastWriteTime=[datetime]'2020-01-01' } }
}
$result = Get-SearchPersonalizationInfo
if (-not $result.partial -or $result.total -ne 3 -or @($result.files | Where-Object { $_.value -eq 'sample text' -and $_.valueType -eq 'String' }).Count -ne 1 -or @($result.files | Where-Object { $_.valueType -eq 'Binary' -and @($_.value).Count -eq 3 }).Count -ne 1 -or @($result.files | Where-Object { $_.path -eq 'C:\synthetic\sample.dat' }).Count -ne 1) { throw 'Accessible values and paths must survive another source failure.' }
$script:mode = 'missing'
$result = Get-SearchPersonalizationInfo
if ($result.partial -or $result.total -ne 0) { throw 'Missing optional locations should report an empty scan.' }
Write-Output 'Passed: search personalization retains values and paths and reports partial sources.'
