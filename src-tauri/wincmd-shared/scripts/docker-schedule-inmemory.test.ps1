$ErrorActionPreference = 'Stop'
$fixturePath = Join-Path $PSScriptRoot 'docker-schedule.test.ps1'
$fixture = [IO.File]::ReadAllText($fixturePath)
$marker = '$fixtureRoot = Join-Path ([IO.Path]::GetTempPath())'
$cut = $fixture.IndexOf($marker, [StringComparison]::Ordinal)
if ($cut -lt 0 -or $fixture.IndexOf($marker, $cut + $marker.Length, [StringComparison]::Ordinal) -ge 0) { throw 'Expected one boundary before the disk-writing fixture.' }
$prefix = $fixture.Substring(0, $cut)
# Do not execute a changed upstream prefix until its operations are reviewed.
# Normalizing line endings keeps the guard stable across Git checkouts.
$sha = [Security.Cryptography.SHA256]::Create()
try { $digest = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($prefix.Replace("`r`n", "`n")))).Replace('-', '').ToLowerInvariant() }
finally { $sha.Dispose() }
if ($digest -ne '1d3de712379ba3403e2a0ab7b858f800c67ffd21fdb4578f1de812c2dce8c77b') { throw 'Docker fixture changed; review its prefix before updating this safety guard.' }
$moduleLoader = ". (Join-Path `$PSScriptRoot 'auto-erase.ps1')"
if (-not $prefix.Contains($moduleLoader)) { throw 'Docker fixture module loader was not found.' }
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
$prefix = $prefix.Replace($moduleLoader, '')
$loader = ". ([scriptblock]::Create(`$script:ScheduledDockerFunctions))"
if (-not $prefix.Contains($loader)) { throw 'Docker fixture loader was not found.' }
# The original provider scenarios use fake Docker commands and a fake erase
# function. These additional stubs prevent the helper from creating or deleting
# its isolated CLI directory. Reject any unexpected file operation outright.
$stub = @'
function New-Item { [CmdletBinding()] param($ItemType, $Path) if ($ItemType -ne 'Directory' -or [IO.Path]::GetFileName([string]$Path) -notlike 'wincommander-docker-cli-*') { throw 'Unexpected fixture write.' }; return $null }
function Test-Path { [CmdletBinding()] param($LiteralPath, $PathType) return $false }
function Remove-Item { throw 'Fixture file deletion is forbidden.' }
'@
$prefix = $prefix.Replace($loader, $loader + "`n" + $stub)
. ([scriptblock]::Create($prefix))
Write-Output 'Passed: Docker provider scenarios ran in memory without creating or deleting files.'
