$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
foreach ($helperName in @('Test-CleanupSystemOwnedPath', 'Get-CleanupScanFailureKind', 'Add-CleanupPrivateCoverage')) { . ([scriptblock]::Create($ast.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $helperName }, $true).Extent.Text)) }
if ($errors.Count) { throw 'Unable to parse cleanup module.' }
foreach ($name in @('Get-SSHStateInfo', 'Invoke-CleanupClearForUser')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing $name." }
    . ([scriptblock]::Create($definition.Extent.Text))
}

# Calls are fixtures; no real SSH agent or known-hosts file is read or changed.
$script:agentDenied = $false
$script:agentStatus = 1
$script:toolAvailable = $true
$script:knownHostsRemain = $false
$script:knownHostsDenied = $false
$script:attemptedClear = $false
$script:serviceStatus = 'Running'
$script:serviceMissing = $false; $script:toolFailure = $false
$env:SSH_AUTH_SOCK = ''
function Get-Service { param([string]$Name, $ErrorAction) if ($script:serviceMissing) { return $null }; [pscustomobject]@{ Status = $script:serviceStatus } }
function Invoke-WCSshAdd { param([string]$Option) if ($script:toolFailure) { throw 'Synthetic ssh-add failure.' }; if (-not $script:toolAvailable) { throw 'Windows OpenSSH agent tooling is unavailable.' }; if ($script:agentDenied) { return 2 }; if ($Option -eq '-D') { return 0 }; return $script:agentStatus }
function Get-Command { throw 'PATH-based command search is forbidden.' }
function Test-Path { param([Alias('Path')][string]$LiteralPath, [string]$PathType, $ErrorAction) return $script:knownHostsRemain }
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:knownHostsDenied) { throw [UnauthorizedAccessException]::new('Synthetic access denied') }
    if (-not $script:knownHostsRemain) { throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic missing item') }
    return [pscustomobject]@{ FullName=$LiteralPath; Length=1024; LastWriteTime=[DateTime]'2026-01-01'; PSIsContainer=$false; Attributes=0 }
}
function Remove-ItemSecure { param([string]$Path) $script:attemptedClear = $true }
function Assert-CleanupCategoriesAllowed { param([string[]]$CategoryIds, [switch]$AllowDefaultExcluded) }

$profile = [Environment]::GetFolderPath('UserProfile')
if (-not $profile) { throw 'The fixture requires a Windows user profile identity.' }
$scan = Get-SSHStateInfo
if ($scan.error -or $scan.total -ne 0) { throw 'An empty agent fixture should scan successfully.' }
$script:knownHostsDenied = $true
$script:agentStatus = 0
$scan = Get-SSHStateInfo
if (-not $scan.partial -or $scan.status -ne 'partial' -or $scan.message -notmatch 'known-hosts' -or $scan.total -ne 1 -or $scan.files[0].name -ne 'SSH agent identities loaded') { throw 'Denied known-hosts metadata must not hide observed SSH agent state or be reported complete.' }
$script:knownHostsDenied = $false
$script:agentStatus = 1
$result = Invoke-CleanupClearForUser -UserProfilePath $profile -CategoryIds @('sshState') -AllowDefaultExcluded $true
if ($result.sshState -ne 'cleaned' -or -not $script:attemptedClear) { throw 'Verified current-user SSH cleanup was not reported.' }

$script:agentDenied = $true; $script:attemptedClear = $false
$scan = Get-SSHStateInfo
if (-not $scan.partial -or $scan.status -ne 'partial') { throw 'Agent errors must not be reported as an empty scan.' }
$script:serviceMissing = $true
$scan = Get-SSHStateInfo
if ($scan.partial -or $scan.coverage -ne 'limited' -or $scan.coverageNote -notmatch 'not installed') { throw 'An absent Windows SSH agent service is a normal state.' }
$script:serviceMissing = $false
$script:serviceStatus = 'Stopped'
$savedAgentSocket = $env:SSH_AUTH_SOCK
try {
    $env:SSH_AUTH_SOCK = ''
    $scan = Get-SSHStateInfo
    if ($scan.partial -or $scan.coverage -ne 'limited' -or $scan.coverageNote -notmatch 'stopped') { throw 'A stopped Windows agent must report its limited scope.' }
    $env:SSH_AUTH_SOCK = 'fixture-socket'
    $scan = Get-SSHStateInfo
    if ($scan.partial -or $scan.coverage -ne 'limited' -or $scan.coverageNote -notmatch 'different SSH agent') { throw 'A custom agent outside the Windows agent scope must report limited coverage.' }
} finally { $env:SSH_AUTH_SOCK = $savedAgentSocket; $script:serviceStatus = 'Running' }
$result = Invoke-CleanupClearForUser -UserProfilePath $profile -CategoryIds @('sshState') -AllowDefaultExcluded $true
if ($result.sshState -ne 'failed' -or -not $script:attemptedClear) { throw 'Agent failure must leave a truthful partial result.' }

$script:agentDenied = $false; $script:toolAvailable = $false
$scan = Get-SSHStateInfo
if ($scan.partial -or $scan.status -ne 'complete' -or $scan.coverage -ne 'limited' -or $scan.coverageNote -notmatch 'not installed') { throw 'Missing Windows OpenSSH is a normal state and must report limited coverage.' }
$script:toolFailure = $true
$scan = Get-SSHStateInfo
if (-not $scan.partial -or $scan.status -ne 'partial') { throw 'An unexpected ssh-add failure must stay incomplete.' }
$script:toolFailure = $false
$result = Invoke-CleanupClearForUser -UserProfilePath $profile -CategoryIds @('sshState') -AllowDefaultExcluded $true
if ($result.sshState -ne 'failed') { throw 'Missing Windows OpenSSH must not report successful cleanup.' }

$script:knownHostsRemain = $true
$scan = Get-SSHStateInfo
if ($scan.partial -or $scan.total -ne 1 -or $scan.files[0].name -ne 'SSH known_hosts' -or -not $scan.files[0].path) { throw 'Known hosts and path should remain visible even when the SSH agent cannot be checked.' }
$script:knownHostsRemain = $false

$script:toolAvailable = $true; $script:attemptedClear = $false
$result = Invoke-CleanupClearForUser -UserProfilePath 'C:\Fixture\OtherUser' -CategoryIds @('sshState') -AllowDefaultExcluded $true
if ($result.sshState -ne 'failed' -or -not $script:attemptedClear) { throw 'Another account must not inherit the current user agent result.' }
Write-Output 'Passed: trusted SSH scan, account scope and manual cleanup readback fixtures.'
