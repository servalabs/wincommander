$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$modulePath = Join-Path $repoRoot 'src-tauri\commander-free\scripts\modules\dependencies\dependencies.ps1'
$ast = [System.Management.Automation.Language.Parser]::ParseInput([IO.File]::ReadAllText($modulePath), [ref]$null, [ref]$null)
$functions = $ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $false)
foreach ($function in $functions) { . ([scriptblock]::Create($function.Extent.Text)) }

$script:hasCli = $true
$script:hasDaemon = $false
$script:cliInstalls = 0
$script:wingetInstalls = 0
function Assert-IsAdmin {}
function Get-EverythingExePath { if ($script:hasDaemon) { 'C:\Fixture\Everything.exe' } }
function Test-Path {
    param([string]$Path, [string]$LiteralPath, [string]$PathType, $ErrorAction)
    $candidate = if ($LiteralPath) { $LiteralPath } else { $Path }
    return ($script:hasCli -and $candidate -eq "$env:ProgramData\WinCommander\bin\es.exe")
}
function Get-Item { param($Path) [pscustomobject]@{ VersionInfo = @{ ProductVersion = '1.1.0.37' } } }
function Get-Command { param($Name, $ErrorAction) return $null }
function Install-EverythingSearchCli { $script:cliInstalls++; $script:hasCli = $true; 'C:\Fixture\es.exe' }
function Resolve-WingetPath { 'Invoke-FixtureWinget' }
function Invoke-WingetSourceUpdate { param($WingetCmd) }
function Invoke-FixtureWinget { $script:wingetInstalls++; $global:LASTEXITCODE = 0 }

if ((Test-InstantSearchInstalled).installed) { throw 'The search CLI alone must not count as a complete engine.' }
$script:hasCli = $false
$script:hasDaemon = $true
if ((Test-InstantSearchInstalled).installed) { throw 'Everything alone must not count as a complete engine.' }
$result = Install-InstantSearch
if (-not $result.success -or $script:cliInstalls -ne 1 -or $script:wingetInstalls -ne 0) {
    throw 'Repairing the missing CLI must reuse the existing Everything installation.'
}
if (-not (Test-InstantSearchInstalled).installed) { throw 'The completed installation was not detected.' }
$script:hasDaemon = $false
$failed = $false
try { Install-InstantSearch | Out-Null } catch { $failed = $true }
if (-not $failed) { throw 'A successful installer exit without an engine must fail readback.' }

function Read-DepStatusCache {
    @{ status = @([pscustomobject]@{ id = 'instantSearch'; installed = $false; version = $null; missing = $null }); cacheAgeSecs = 100 }
}
$script:hasDaemon = $true
$cached = Get-DependencyStatus
if (-not $cached.dependencies[0].installed) { throw 'A stale negative disk cache hid the existing search engine.' }
Remove-Variable -Scope Script -Name _depStatusCache,_depStatusCacheTime -ErrorAction SilentlyContinue
$script:hasDaemon = $false
$cached = Get-DependencyStatus
if ($cached.dependencies[0].installed) { throw 'A disk cache concealed a removed search engine.' }

$script:hasDaemon = $true
function Set-BackendAppsVisibility { param($Apps, $Hidden) $script:hasDaemon = $false; @{ itemsChanged = 0 } }
function Update-DepStatusCacheEntry { param($depId, $mergeProps) $script:lastCachedStatus = $mergeProps.installed }
$result = Install-Dependency -Id instantSearch
if (-not $result.error -or $script:lastCachedStatus -ne $false) { throw 'Post-install failure must not be cached or reported as installed.' }

Write-Output 'Instant Search tests passed: partial installs, CLI-only repair, readback, stale caches, and post-install failure.'
