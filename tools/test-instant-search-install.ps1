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
$script:cliPath = "$env:ProgramData\WinCommander\bin\es.exe"
function Test-IsAdmin { $false }
function Assert-IsAdmin { throw 'fixture is not elevated' }
function Get-EverythingExePath { if ($script:hasDaemon) { 'C:\Fixture\Everything.exe' } }
function Test-Path {
    param([string]$Path, [string]$LiteralPath, [string]$PathType, $ErrorAction)
    $candidate = if ($LiteralPath) { $LiteralPath } else { $Path }
    return ($script:hasCli -and $candidate -eq $script:cliPath)
}
function Get-Item { param($Path) [pscustomobject]@{ VersionInfo = @{ ProductVersion = '1.1.0.37' } } }
function Get-Command { param($Name, $ErrorAction) return $null }
function Install-EverythingSearchCli { $script:cliInstalls++; $script:hasCli = $true; 'C:\Fixture\es.exe' }
function Resolve-WingetPath { 'Invoke-FixtureWinget' }
function Invoke-WingetSourceUpdate { param($WingetCmd) }
function Invoke-FixtureWinget { $script:wingetInstalls++; $global:LASTEXITCODE = 0 }
function Set-BackendAppsVisibility { param($Apps, $Hidden) @{ itemsChanged = 0 } }
function Update-DepStatusCacheEntry { param($depId, $mergeProps) $script:lastCachedStatus = $mergeProps.installed }

if ((Test-InstantSearchInstalled).installed) { throw 'The search CLI alone must not count as a complete engine.' }
$script:hasCli = $false
$script:hasDaemon = $true
if ((Test-InstantSearchInstalled).installed) { throw 'Everything alone must not count as a complete engine.' }
$script:hasCli = $true
$script:cliPath = "$env:LOCALAPPDATA\WinCommander\bin\es.exe"
if (-not (Test-InstantSearchInstalled).installed) { throw 'The durable per-user CLI location was not detected.' }
$script:hasCli = $false
$script:cliPath = "$env:ProgramData\WinCommander\bin\es.exe"
$result = Install-InstantSearch
if (-not $result.success -or $script:cliInstalls -ne 1 -or $script:wingetInstalls -ne 0) {
    throw 'Repairing the missing CLI must reuse the existing Everything installation.'
}
if (-not (Test-InstantSearchInstalled).installed) { throw 'The completed installation was not detected.' }
$script:hasCli = $false
$result = Install-Dependency -Id instantSearch
if (-not $result.success -or $script:cliInstalls -ne 2) {
    throw 'The Fix action must reach the supported Everything installer flow from a standard session.'
}
$blocked = $false
try { Install-Dependency -Id diskHealthEngine | Out-Null } catch { $blocked = $_.Exception.Message -eq 'fixture is not elevated' }
if (-not $blocked) { throw 'The Instant Search exception widened another machine installer.' }
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
$result = Install-Dependency -Id instantSearch
if (-not $result.error -or $script:lastCachedStatus -ne $false) { throw 'Post-install failure must not be cached or reported as installed.' }

& {
    $utilsPath = Join-Path $repoRoot 'src-tauri\commander-free\scripts\core\utils.ps1'
    $utilsAst = [System.Management.Automation.Language.Parser]::ParseInput([IO.File]::ReadAllText($utilsPath), [ref]$null, [ref]$null)
    $installer = $utilsAst.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Install-EverythingSearchCli' }, $true)
    . ([scriptblock]::Create($installer.Extent.Text))
    $global:everythingFixtureCopiedTo = $null
    function Test-IsAdmin { $false }
    function Test-Path { param([string]$LiteralPath, [string]$PathType) return $LiteralPath -eq $global:everythingFixtureCopiedTo }
    function New-Item { param($ItemType, $Path, [switch]$Force, $ErrorAction) }
    function Invoke-WebRequest { param($Uri, $OutFile, [switch]$UseBasicParsing, $ErrorAction) }
    function Expand-Archive { param($LiteralPath, $DestinationPath, [switch]$Force, $ErrorAction) }
    function Get-ChildItem { param($LiteralPath, $Filter, [switch]$File, [switch]$Recurse, $ErrorAction) [pscustomobject]@{ FullName = (Join-Path $LiteralPath 'es.exe') } }
    function Get-AuthenticodeSignature { param($FilePath, $ErrorAction) [pscustomobject]@{ Status = 'Valid'; SignerCertificate = [pscustomobject]@{ Subject = 'CN=voidtools PTY LTD, O=voidtools PTY LTD' } } }
    function Copy-Item { param($LiteralPath, $Destination, [switch]$Force, $ErrorAction) $global:everythingFixtureCopiedTo = $Destination }
    function Remove-Item { param($LiteralPath, [switch]$Force, [switch]$Recurse, $ErrorAction) }
    $installedCli = Install-EverythingSearchCli
    $expected = Join-Path $env:LOCALAPPDATA 'WinCommander\bin\es.exe'
    if ($installedCli -ne $expected -or $global:everythingFixtureCopiedTo -ne $expected) { throw 'A standard session did not use the durable per-user CLI location.' }
    $global:everythingFixtureCopiedTo = $null
    function Get-AuthenticodeSignature { param($FilePath, $ErrorAction) [pscustomobject]@{ Status = 'HashMismatch'; SignerCertificate = $null } }
    $rejected = $false
    try { Install-EverythingSearchCli | Out-Null } catch { $rejected = $true }
    if (-not $rejected -or $global:everythingFixtureCopiedTo) { throw 'An untrusted Everything CLI payload was not rejected before copy.' }
    Remove-Variable -Scope Global -Name everythingFixtureCopiedTo -ErrorAction SilentlyContinue
}

Write-Output 'Instant Search tests passed: partial installs, CLI-only repair, readback, stale caches, and post-install failure.'
