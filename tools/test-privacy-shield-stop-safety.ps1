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

Write-Output 'Privacy Shield PID marker safety fixtures passed.'
