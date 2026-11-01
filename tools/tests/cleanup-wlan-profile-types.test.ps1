$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-WlanProfiles' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
function Get-WCWlanSnapshot { @{ InterfaceCount=1; Profiles=@(@{ Name='all'; InterfaceId='adapter'; Flags=0 }, @{ Name='policy'; InterfaceId='adapter'; Flags=1 }, @{ Name='user'; InterfaceId='adapter'; Flags=2 }) } }
function Assert-WCWlanSnapshot { param($Snapshot) }
$script:keyReads = 0
function Get-WCWlanKey { param($InterfaceId, $Name) $script:keyReads++; "secret-$Name" }
$result = Get-WlanProfiles -IncludeSecrets:$false
if ($result.error -or $result.total -ne 1 -or @($result.profiles).Count -ne 1 -or @($result.additionalProfiles).Count -ne 2 -or $script:keyReads -ne 0) { throw 'Scan must distinguish removable profiles without requesting passwords.' }
if (@($result.additionalProfiles | Where-Object { $_.profileType -eq 'Group policy' -and $_.name -eq 'policy' }).Count -ne 1 -or @($result.additionalProfiles | Where-Object { $_.profileType -eq 'Current user' -and $_.name -eq 'user' }).Count -ne 1) { throw 'Additional Wi-Fi profile types were lost.' }
$result = Get-WlanProfiles -IncludeSecrets:$true
if ($script:keyReads -ne 3 -or @($result.additionalProfiles | Where-Object { $_.passwordStatus -eq 'available' }).Count -ne 2) { throw 'Explicit password lookup did not include available additional profiles.' }
Write-Output 'Passed: Wi-Fi profiles preserve visibility, cleanup scope and explicit password opt-in.'
