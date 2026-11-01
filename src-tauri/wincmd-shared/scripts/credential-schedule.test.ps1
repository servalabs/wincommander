$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

function Assert-True($Condition, $Message) {
    if (-not $Condition) { throw $Message }
}

# Exercise the exact embedded task script with fake targets. No real Windows
# credential is read or deleted by this fixture.
. ([scriptblock]::Create($script:ScheduledCredentialFunctions))
Initialize-WCScheduledCredentials # Compiles declarations only; makes no native calls.
$expectedUserOffset = if ([IntPtr]::Size -eq 8) { 72 } else { 48 }
Assert-True ([int][Runtime.InteropServices.Marshal]::OffsetOf([type]'WcCleanup.Credential', 'UserName') -eq $expectedUserOffset) 'Credential structure must match the native user-name offset.'
Assert-True ((Get-WCCredentialDeleteTarget 'example' 6 'alice') -eq 'example|alice') 'Extended credentials need a user-specific target.'
foreach ($identity in @(@('example', ''), @('example|other', 'alice'), @('example', 'alice|other'))) {
    $rejected = $false
    try { Get-WCCredentialDeleteTarget $identity[0] 6 $identity[1] | Out-Null } catch { $rejected = $true }
    Assert-True $rejected 'Ambiguous extended credential identities must be rejected.'
}
$script:AutoEraseRemoved = 0
$script:AutoEraseFailed = 0
$script:targets = @(
    [pscustomobject]@{ Name='example'; Type=1 },
    [pscustomobject]@{ Name='example'; Type=2 },
    [pscustomobject]@{ Name='example'; Type=6; UserName='alice' },
    [pscustomobject]@{ Name='example'; Type=6; UserName='bob' }
)
$script:deleted = [System.Collections.Generic.List[string]]::new()
function Get-WCScheduledCredentials { return @($script:targets) }
function Remove-WCScheduledCredential($Name, $Type, $UserName) {
    $target = Get-WCCredentialDeleteTarget $Name $Type $UserName
    $script:deleted.Add("$target/$Type")
    if ($Type -eq 6 -and $UserName -eq 'bob') { return }
    $script:targets = @($script:targets | Where-Object { $_.Name -ne $Name -or $_.Type -ne $Type -or ($Type -eq 6 -and $_.UserName -ne $UserName) })
}
Invoke-Expression $script:AutoEraseScripts['credentialManager']
Assert-True ($script:AutoEraseRemoved -eq 3) 'Readback must distinguish users sharing an extended credential target.'
Assert-True ($script:AutoEraseFailed -gt 0 -and $script:AutoEraseCoverageIncomplete) 'A retained identity and unverified Windows Vault coverage must be reported independently.'
Assert-True ($script:deleted.Contains('example/1') -and $script:deleted.Contains('example/2')) 'Typed identities must be preserved.'
Assert-True ($script:deleted.Contains('example|alice/6') -and $script:deleted.Contains('example|bob/6')) 'Extended credentials must be targeted by user.'

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

$script:AutoEraseRemoved = 0; $script:AutoEraseFailed = 0; $script:AutoEraseCoverageIncomplete = $false
function Get-WCScheduledCredentials { return @() }
Invoke-Expression $script:AutoEraseScripts['credentialManager']
Assert-True ($script:AutoEraseFailed -eq 0 -and $script:AutoEraseCoverageIncomplete) 'Unverified Vault coverage must not be counted as a deletion failure.'

Write-Output 'Credential scheduled cleanup fixtures passed.'
