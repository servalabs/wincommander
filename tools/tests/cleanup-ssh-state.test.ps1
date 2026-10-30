$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Unable to parse cleanup module.' }
foreach ($name in @('Get-SSHStateInfo', 'Invoke-CleanupClearForUser')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing $name." }
    . ([scriptblock]::Create($definition.Extent.Text))
}

# Calls are fixtures; no real SSH agent or known-hosts file is read or changed.
$script:agentDenied = $false
$script:toolAvailable = $true
$script:knownHostsRemain = $false
$script:attemptedClear = $false
function Invoke-WCSshAdd { param([string]$Option) if (-not $script:toolAvailable) { throw 'Synthetic missing Windows OpenSSH.' }; if ($script:agentDenied) { return 2 }; if ($Option -eq '-D') { return 0 }; return 1 }
function Get-Command { throw 'PATH-based command search is forbidden.' }
function Test-Path { param([Alias('Path')][string]$LiteralPath, [string]$PathType, $ErrorAction) return $script:knownHostsRemain }
function Get-Item { param([string]$LiteralPath, [switch]$Force, $ErrorAction) return [pscustomobject]@{ Length = 1024; LastWriteTime = [DateTime]'2026-01-01' } }
function Remove-ItemSecure { param([string]$Path) $script:attemptedClear = $true }
function Assert-CleanupCategoriesAllowed { param([string[]]$CategoryIds, [switch]$AllowDefaultExcluded) }

$profile = [Environment]::GetFolderPath('UserProfile')
if (-not $profile) { throw 'The fixture requires a Windows user profile identity.' }
$scan = Get-SSHStateInfo
if ($scan.error -or $scan.total -ne 0) { throw 'An empty agent fixture should scan successfully.' }
$result = Invoke-CleanupClearForUser -UserProfilePath $profile -CategoryIds @('sshState') -AllowDefaultExcluded $true
if ($result.sshState -ne 'cleaned' -or -not $script:attemptedClear) { throw 'Verified current-user SSH cleanup was not reported.' }

$script:agentDenied = $true; $script:attemptedClear = $false
$scan = Get-SSHStateInfo
if (-not $scan.partial -or $scan.status -ne 'partial') { throw 'Agent errors must not be reported as an empty scan.' }
$result = Invoke-CleanupClearForUser -UserProfilePath $profile -CategoryIds @('sshState') -AllowDefaultExcluded $true
if ($result.sshState -ne 'failed' -or -not $script:attemptedClear) { throw 'Agent failure must leave a truthful partial result.' }

$script:agentDenied = $false; $script:toolAvailable = $false
$scan = Get-SSHStateInfo
if (-not $scan.partial -or $scan.status -ne 'partial') { throw 'Missing Windows OpenSSH must not be reported as a complete scan.' }
$result = Invoke-CleanupClearForUser -UserProfilePath $profile -CategoryIds @('sshState') -AllowDefaultExcluded $true
if ($result.sshState -ne 'failed') { throw 'Missing Windows OpenSSH must not report successful cleanup.' }

$script:knownHostsRemain = $true
$scan = Get-SSHStateInfo
if (-not $scan.partial -or $scan.total -ne 1 -or $scan.files[0].name -ne 'SSH known_hosts') { throw 'Known hosts should remain visible even when the SSH agent cannot be checked.' }
$script:knownHostsRemain = $false

$script:toolAvailable = $true; $script:attemptedClear = $false
$result = Invoke-CleanupClearForUser -UserProfilePath 'C:\Fixture\OtherUser' -CategoryIds @('sshState') -AllowDefaultExcluded $true
if ($result.sshState -ne 'failed' -or -not $script:attemptedClear) { throw 'Another account must not inherit the current user agent result.' }
Write-Output 'Passed: trusted SSH scan, account scope and manual cleanup readback fixtures.'
