$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
foreach ($name in @('Test-WCVisibleUserProfile', 'Get-UserProfiles')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing function: $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
function Test-IsAdmin { $false }
$script:denyProfileInventory = $false
function Get-CimInstance {
    [CmdletBinding()] param([string]$ClassName)
    if ($script:denyProfileInventory) { throw [UnauthorizedAccessException]::new('Synthetic inventory denial') }
    foreach ($profile in @(
        @{ Name='codex-owner'; Sid='S-1-5-21-111-222-333-1001'; Special=$false },
        @{ Name='Sandbox'; Sid='S-1-5-21-111-222-333-1002'; Special=$false },
        @{ Name='defaultuser0'; Sid='S-1-5-21-111-222-333-1003'; Special=$false },
        @{ Name='service-account'; Sid='S-1-5-80-123'; Special=$false },
        @{ Name='special-profile'; Sid='S-1-5-21-111-222-333-1004'; Special=$true }
    )) {
        [pscustomobject]@{ LocalPath=(Join-Path "$env:SystemDrive\Users" $profile.Name); SID=$profile.Sid; Special=$profile.Special }
    }
}
$result = Get-UserProfiles
$names = @($result.profiles | ForEach-Object { $_.name })
if ($result.total -ne 2 -or 'codex-owner' -notin $names -or 'Sandbox' -notin $names) { throw 'A real profile name was omitted.' }
if ('defaultuser0' -in $names -or 'service-account' -in $names -or 'special-profile' -in $names) { throw 'A Windows default, service or special profile was included.' }
if (-not (Test-WCVisibleUserProfile -Name 'Alice' -DisplayName 'Alice' -Path 'C:\Users\sandbox-team\Alice')) { throw 'A real account was excluded by its path.' }
$script:denyProfileInventory = $true
$denied = Get-UserProfiles
if (-not $denied.error -or @($denied.profiles).Count -ne 0) { throw 'A failed profile inventory was presented as a successful empty inventory.' }
Write-Output 'Passed: real profile names and paths are preserved while Windows defaults, service and special profiles are excluded.'
