$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\..\src-tauri\wincmd-shared\scripts\auto-erase.ps1')
function Assert-AutoEraseAdmin {}
function Get-ScheduledTask { [CmdletBinding()] param() @('dnsCache','usageTraceLogs') }
function Get-AutoEraseTaskIdentity { param($Task, [switch]$AllowUnknownOwner) @{ categoryId=$Task } }
function Invoke-AutoEraseScheduleLock { param($CategoryId, $Operation) @{ error=$true; message='Access is denied.' } }
$result = Invoke-AutoEraseMigration
if (-not $result.error -or $result.status -ne 'partial' -or $result.errors.Count -ne 2) { throw 'Migration lost individual failures.' }
if ($result.message -ne 'Access is denied.') { throw 'Migration repeated the same failure message.' }
'PASS: migration keeps individual failures while presenting each distinct reason once.'
