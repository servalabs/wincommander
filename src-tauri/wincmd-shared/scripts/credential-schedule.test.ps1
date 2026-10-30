$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

function Assert-True($Condition, $Message) {
    if (-not $Condition) { throw $Message }
}

# Exercise the exact embedded task script with fake targets. No real Windows
# credential is read or deleted by this fixture.
$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:targets = @(
    [pscustomobject]@{ Name='example'; Type=1 },
    [pscustomobject]@{ Name='example'; Type=2 }
)
$script:deleted = [System.Collections.Generic.List[string]]::new()
function Get-WCScheduledCredentials { return @($script:targets) }
function Remove-WCScheduledCredential($Name, $Type) {
    $script:deleted.Add("$Name/$Type")
    $script:targets = @($script:targets | Where-Object { $_.Name -ne $Name -or $_.Type -ne $Type })
}
Invoke-Expression $script:AutoEraseScripts['credentialManager']
Assert-True ($script:AutoEraseRemoved -eq 2) 'Each credential type must be removed separately.'
Assert-True ($script:AutoEraseFailed -gt 0) 'Unverified Windows Vault coverage must be reported as incomplete.'
Assert-True ($script:deleted.Contains('example/1') -and $script:deleted.Contains('example/2')) 'Typed identities must be preserved.'

$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:targets = @([pscustomobject]@{ Name='denied'; Type=1 })
function Remove-WCScheduledCredential($Name, $Type) { throw 'access denied' }
Invoke-Expression $script:AutoEraseScripts['credentialManager']
Assert-True ($script:AutoEraseFailed -gt 0) 'A deletion failure must be reported.'
Assert-True ($script:AutoEraseRemoved -eq 0) 'A failed deletion must not be reported as removed.'

$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:targets = @([pscustomobject]@{ Name='remaining'; Type=1 })
function Remove-WCScheduledCredential($Name, $Type) { }
Invoke-Expression $script:AutoEraseScripts['credentialManager']
Assert-True ($script:AutoEraseFailed -gt 0) 'Readback must detect a credential still present.'

$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
function Get-WCScheduledCredentials { throw 'enumeration denied' }
Invoke-Expression $script:AutoEraseScripts['credentialManager']
Assert-True ($script:AutoEraseFailed -gt 0) 'An unavailable credential store must not report success.'

Write-Output 'Credential scheduled cleanup fixtures passed.'
