Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\src-tauri\commander-free\scripts\modules\privacy\privacy_shield.ps1')

$script:session = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
$script:candidate = [pscustomobject]@{ SessionId = $script:session }
$script:details = [pscustomobject]@{ SessionId = $script:session; Name = 'pythonw.exe'; CommandLine = 'python --wc-privacy-shield' }
function Test-Path { [CmdletBinding()] param($LiteralPath) return $true }
function Get-Content { [CmdletBinding()] param($LiteralPath, [switch]$Raw) return '9002' }
function Get-Process { [CmdletBinding()] param($Id) return $script:candidate }
function Get-CimInstance { [CmdletBinding()] param($ClassName, $Filter) return $script:details }
function Remove-Item { [CmdletBinding()] param($LiteralPath, [switch]$Force) }

if ((Get-PrivacyShieldPidFromMarker) -ne 9002) { throw 'Valid detector was not identified' }
$script:details.CommandLine = 'unrelated.py'
if ($null -ne (Get-PrivacyShieldPidFromMarker)) { throw 'Unrelated reused PID was accepted' }
$script:details.CommandLine = 'python --wc-privacy-shield-unrelated'
if ($null -ne (Get-PrivacyShieldPidFromMarker)) { throw 'A partial detector marker was accepted' }
$script:details.CommandLine = ''
if ((Get-PrivacyShieldPidFromMarker) -ne -1) { throw 'Unreadable detector identity was not unknown' }
$script:candidate.SessionId = $script:session + 1
if ($null -ne (Get-PrivacyShieldPidFromMarker)) { throw 'Another session was accepted' }

$script:statuses = @()
$script:stopped = @()
$script:denyStop = $false
function Get-PrivacyShieldStatus { $next = $script:statuses[0]; $script:statuses = @($script:statuses | Select-Object -Skip 1); return $next }
function Stop-Process { [CmdletBinding()] param($Id, [switch]$Force) if ($script:denyStop) { throw 'access denied' }; $script:stopped += $Id }
function Wait-Process { [CmdletBinding()] param($Id, $Timeout) }
function Clear-PrivacyShieldPidMarker { throw 'Stop must not clear a marker owned by another session' }

$script:statuses = @(@{ error = $true })
$result = Stop-PrivacyShield
if (-not $result.error -or $script:stopped.Count -ne 0) { throw 'Unknown status was treated as a successful stop' }
$script:statuses = @(@{ running = $false })
$result = Stop-PrivacyShield
if (-not $result.success) { throw 'An already stopped detector was not confirmed' }
$script:statuses = @(@{ running = $true; processId = 9002 }, @{ running = $false })
$result = Stop-PrivacyShield
if (-not $result.success -or $script:stopped.Count -ne 1 -or $script:stopped[0] -ne 9002) { throw 'Confirmed stop was not reported correctly' }
$script:statuses = @(@{ running = $true; processId = 9002 }, @{ running = $true })
$result = Stop-PrivacyShield
if (-not $result.error) { throw 'Running detector was reported stopped' }
$script:statuses = @(@{ running = $true; processId = 9002 })
$script:denyStop = $true
$result = Stop-PrivacyShield
if (-not $result.error) { throw 'Denied stop was reported successful' }
Write-Output 'Privacy Shield stop safety fixtures passed.'
