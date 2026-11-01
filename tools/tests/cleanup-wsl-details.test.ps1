$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-WSLDataInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))

# Synthetic provider inventory; do not read or mutate real WSL data.
function Test-Path { param($Path, $PathType, $ErrorAction) return $true }
$script:denyPackages = $false
function Get-ChildItem {
    param($Path, [switch]$Directory, [switch]$Force, $ErrorAction)
    if ($script:denyPackages -and $Path -like '*Packages') { throw 'Synthetic access denied' }
    if ($Path -like '*Packages') { [pscustomobject]@{ Name='StoreFixture'; FullName='C:\Fixture\StoreFixture' } }
    else { [pscustomobject]@{ PSPath='fixtureRegistry'; PSChildName='fixtureId' } }
}
function Get-ItemProperty { param($Path, $ErrorAction) [pscustomobject]@{ DistributionName='ImportedFixture'; BasePath='C:\Fixture\Imported' } }
function Get-Item { param($LiteralPath, [switch]$Force, $ErrorAction) [pscustomobject]@{ FullName=$LiteralPath; Length=1024; LastWriteTime=[datetime]'2026-01-02' } }
function Get-Command { param($Name, $ErrorAction) return $null }
$result = Get-WSLDataInfo
if ($result.error -or $result.total -ne 2) { throw 'WSL synthetic disks were not returned.' }
if (@($result.files | Where-Object { $_.path -eq 'C:\Fixture\StoreFixture\LocalState\ext4.vhdx' }).Count -ne 1 -or @($result.files | Where-Object { $_.path -eq 'C:\Fixture\Imported\ext4.vhdx' }).Count -ne 1) { throw 'WSL disk paths were lost.' }
$script:denyPackages = $true
$partial = Get-WSLDataInfo
if (-not $partial.partial -or $partial.status -ne 'partial' -or $partial.total -ne 1 -or @($partial.files | Where-Object { $_.path -eq 'C:\Fixture\Imported\ext4.vhdx' }).Count -ne 1) { throw 'WSL access failure must keep available rows and report incomplete.' }
Write-Output 'Passed: WSL Store and imported disk paths are retained.'
