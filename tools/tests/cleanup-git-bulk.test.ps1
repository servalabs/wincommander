$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module did not parse.' }
foreach ($name in @('Assert-CleanupCategoriesAllowed', 'Invoke-CleanupCurrentUserGitActivity', 'Invoke-CleanupClearForUser')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing fixture function $name." }
    . ([scriptblock]::Create($definition.Extent.Text))
}

# The real helper is replaced before invocation; no Windows credential API or
# actual Git executable is called. The non-Git identity must never be removed.
$script:ScheduledCredentialFunctions = @'
function Get-WCScheduledCredentials {
    if ($script:mode -eq 'enumeration-failure' -or ($script:mode -eq 'verification-failure' -and $script:reads -gt 0)) { throw 'synthetic read failure' }
    $script:reads++
    $rows = @([pscustomobject]@{ Name='service:git:not-a-git-target'; Type=1; UserName='fixture' })
    if (-not $script:removed -or $script:mode -eq 'leftover') { $rows += [pscustomobject]@{ Name='GiT:synthetic'; Type=1; UserName='fixture' } }
    return $rows
}
function Remove-WCScheduledCredential {
    param($Name, $Type, $UserName)
    if ($Name -ne 'GiT:synthetic' -or $Type -ne 1) { throw 'Unexpected credential deletion.' }
    $script:deleteCalls++
    if ($script:mode -eq 'delete-failure') { throw 'synthetic delete failure' }
    $script:removed = $true
}
'@
function cmdkey.exe { throw 'Localized cmdkey must not be used.' }
function Get-Command { param($Name, $ErrorAction) if ($Name -eq 'git.exe' -and $script:mode -ne 'git-missing') { [pscustomobject]@{ Source='Invoke-GitFixture' } } }
function Invoke-GitFixture {
    if ($args -contains '--unset-all') { $script:unsetCalls++ }
    if ($script:mode -eq 'query-failure') { $global:LASTEXITCODE=128; return }
    if ($script:mode -eq 'inconsistent') { $global:LASTEXITCODE=1; return 'unverified' }
    if ($script:mode -eq 'config') {
        if ($args -contains '--unset-all') { $script:safeRemoved=$true; $global:LASTEXITCODE=0; return }
        if (-not $script:safeRemoved) { $global:LASTEXITCODE=0; return 'synthetic-directory' }
    }
    $global:LASTEXITCODE=1
}
$profile = [Environment]::GetFolderPath('UserProfile')
if (-not $profile) { throw 'Fixture requires a Windows profile identity.' }
foreach ($mode in @('success', 'config', 'enumeration-failure', 'verification-failure', 'delete-failure', 'leftover', 'query-failure', 'inconsistent', 'git-missing')) {
    $script:mode=$mode; $script:reads=0; $script:removed=$false; $script:safeRemoved=$false; $script:deleteCalls=0; $script:unsetCalls=0
    $result = Invoke-CleanupClearForUser -UserProfilePath $profile -CategoryIds @('gitActivity')
    $expected = if ($mode -in @('success','config')) { 'partial' } else { 'failed' }
    if ($result.gitActivity -ne $expected) { throw "Unexpected verification result for $mode." }
    if ($mode -in @('query-failure','inconsistent','git-missing') -and ($script:deleteCalls -ne 0 -or $script:unsetCalls -ne 0)) { throw "Invalid preflight modified data in $mode." }
}
$script:deleteCalls=0; $script:unsetCalls=0
$other = Join-Path $env:SystemDrive 'Users\SyntheticOther'
$result = Invoke-CleanupClearForUser -UserProfilePath $other -CategoryIds @('gitActivity')
if ($result.gitActivity -ne 'failed' -or $script:deleteCalls -ne 0 -or $script:unsetCalls -ne 0) { throw 'Another profile must not use current-user Git providers.' }
'Passed: per-user Git cleanup uses synthetic identities, verifies both sources and never accesses another user.'
