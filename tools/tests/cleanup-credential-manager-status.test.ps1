$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-CredentialManagerInfo' }, $true)
if (-not $definition) { throw 'Credential manager reader not found.' }
. ([scriptblock]::Create($definition.Extent.Text))

$script:credentialFixtureMode = 'ok'
$script:ScheduledCredentialFunctions = @'
function Get-WCScheduledCredentials {
    if ($script:credentialFixtureMode -eq 'failed') { throw 'Synthetic credential API failure' }
    if ($script:credentialFixtureMode -eq 'ok') { return [pscustomobject]@{ Name='synthetic-target'; Type=1; UserName='synthetic-user' } }
    if ($script:credentialFixtureMode -eq 'invalid') { return [pscustomobject]@{ Name=''; Type=1 } }
}
'@
function cmdkey.exe { throw 'Localized console output must not be parsed.' }

$result = Get-CredentialManagerInfo
if ($result.error -or -not $result.partial -or $result.total -ne 1 -or @($result.files).Count -ne 1 -or $result.files[0].name -ne 'Saved credential target: synthetic-target' -or $result.files[0].credentialType -ne 1 -or $result.files[0].username -ne 'synthetic-user') { throw 'Successful enumeration did not return the credential identity or partial coverage.' }
$script:credentialFixtureMode = 'empty'
$result = Get-CredentialManagerInfo
if ($result.error -or -not $result.partial -or $result.total -ne 0 -or @($result.files).Count -ne 0) { throw 'Successful empty enumeration did not remain empty with partial coverage.' }
$script:credentialFixtureMode = 'failed'
$result = Get-CredentialManagerInfo
if (-not $result.error) { throw 'Failed credential enumeration was reported as an empty result.' }
$script:credentialFixtureMode = 'invalid'
if (-not (Get-CredentialManagerInfo).error) { throw 'Invalid credential identity was accepted.' }
$script:ScheduledCredentialFunctions = ''
if (-not (Get-CredentialManagerInfo).error) { throw 'Missing credential reader appeared empty.' }
'Passed: native credential enumeration distinguishes success, empty and failure without accessing credentials.'
