$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-GitActivityInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:credentialMode = 'success'
$script:gitMode = 'success'
$script:gitCalls = 0
$script:ScheduledCredentialFunctions = @'
function Get-WCScheduledCredentials {
    if ($script:credentialMode -eq 'denied') { throw 'Synthetic credential failure' }
    [pscustomobject]@{ Name='git:https://fixture.invalid'; Type=1; UserName='synthetic-user' }
    [pscustomobject]@{ Name='GIT:https://case.invalid'; Type=2; UserName='synthetic-user' }
    [pscustomobject]@{ Name='unrelated'; Type=1; UserName='synthetic-user' }
    [pscustomobject]@{ Name='service:git:unrelated'; Type=1; UserName='synthetic-user' }
}
'@
function cmdkey.exe { throw 'Localized cmdkey output must not be parsed.' }
function git.exe {
    $script:gitCalls++
    if ($script:gitMode -in @('failed', 'denied')) { $global:LASTEXITCODE = 128; return }
    if ($script:gitMode -in @('empty', 'missing')) { $global:LASTEXITCODE = 1; return }
    if ($script:gitMode -eq 'inconsistent') { $global:LASTEXITCODE = 1; 'C:\Fixture\Unverified'; return }
    if ($script:gitMode -eq 'xdg') { $global:LASTEXITCODE = 0; 'D:\Fixture\XDG'; return }
    $global:LASTEXITCODE = 0; 'C:\Fixture\One'; 'D:\Fixture\Two'
}
function Get-Item { throw 'A single .gitconfig path must not gate Git global configuration.' }
$result = Get-GitActivityInfo
if ($result.error -or -not $result.partial -or $result.total -ne 4) { throw 'Git activity count did not include each returned entry with partial Vault coverage.' }
$target = @($result.files | Where-Object { $_.name -like 'Git credential target:*' })
if ($target.Count -ne 2 -or $target[0].target -ne 'git:https://fixture.invalid' -or $target[0].credentialType -ne 1 -or $target[0].username -ne 'synthetic-user' -or $target[1].target -ne 'GIT:https://case.invalid') { throw 'Native Git credential identity details or case-insensitive prefix matching were not preserved.' }
$entries = @($result.files | Where-Object { $_.name -eq 'Global Git safe.directory' })
if ($entries.Count -ne 2 -or $entries[0].value -ne 'C:\Fixture\One' -or $entries[1].value -ne 'D:\Fixture\Two') { throw 'Captured safe.directory values were not preserved.' }
$script:credentialMode = 'denied'
$result = Get-GitActivityInfo
if ($result.error -or -not $result.partial -or $result.total -ne 2 -or $result.message -notmatch 'credential identities') { throw 'A credential failure must preserve known Git configuration and report incomplete coverage.' }
$script:credentialMode = 'success'
foreach ($mode in @('failed','denied','inconsistent')) {
    $script:gitMode = $mode; $result = Get-GitActivityInfo
    if ($result.error -or -not $result.partial -or $result.total -ne 2 -or $result.message -notmatch 'configuration') { throw "A $mode configuration must preserve the Git credentials and report incomplete coverage." }
}
foreach ($mode in @('empty','missing')) {
    $script:gitMode = $mode; $result = Get-GitActivityInfo
    if ($result.error -or -not $result.partial -or $result.total -ne 2 -or $result.message -match 'could not be checked') { throw "A $mode configuration must not be reported as inaccessible." }
}
$script:gitMode = 'xdg'; $beforeCalls = $script:gitCalls
$result = Get-GitActivityInfo
$xdg = @($result.files | Where-Object { $_.name -eq 'Global Git safe.directory' })
if ($script:gitCalls -ne ($beforeCalls + 1) -or $result.error -or $result.total -ne 3 -or $xdg.Count -ne 1 -or $xdg[0].value -ne 'D:\Fixture\XDG') { throw 'Git global XDG configuration was not queried and preserved.' }
Write-Output 'Passed: Git safe directory values are preserved.'
