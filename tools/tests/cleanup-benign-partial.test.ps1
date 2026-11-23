$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module did not parse.' }
foreach ($name in @('Test-CleanupSystemOwnedPath', 'Get-CleanupScanFailureKind', 'Add-CleanupPrivateCoverage', 'Get-CleanupCacheFileInventory', 'Get-CleanupArtifactFiles', 'Get-CleanupArtifactInventory', 'Get-EmbeddedWebCacheInfo', 'Get-GitActivityInfo', 'Get-BITSQueueInfo', 'Get-ClipboardHistoryStatus')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing $name." }
    . ([scriptblock]::Create($definition.Extent.Text))
}

$script:mode = 'ok'
$script:gitPresent = $true
$script:bitsPresent = $true
$script:historyStatus = 'disabled'
function Get-Item {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:mode -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic missing') }
    [pscustomobject]@{ FullName = $LiteralPath; PSIsContainer = $true; Attributes = [IO.FileAttributes]::Directory }
}
function Get-ChildItem {
    param($LiteralPath, $Filter, [switch]$Recurse, [switch]$File, [switch]$Directory, [switch]$Force, $ErrorAction)
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denied') }
    if ($script:mode -eq 'locked') { throw [IO.IOException]::new('Synthetic sharing violation') }
    if ($script:mode -eq 'vanished') { throw [IO.DirectoryNotFoundException]::new('Synthetic vanished') }
    if ($Directory) { return }
    [pscustomobject]@{ Name = 'a.dat'; FullName = "$LiteralPath\a.dat"; Length = 1024; LastWriteTime = [datetime]'2026-01-01'; PSIsContainer = $false; Attributes = [IO.FileAttributes]::Normal }
}
function Get-Command { param($Name, $CommandType, $ErrorAction) if ($Name -eq 'git.exe' -and $script:gitPresent) { return 'git.exe' }; if ($Name -eq 'Get-BitsTransfer' -and $script:bitsPresent) { return 'Get-BitsTransfer' }; return $null }
function Get-Service { param($Name, $ErrorAction) if ($script:bitsPresent) { [pscustomobject]@{ Name = $Name; Status = 'Running' } } }
function Get-BitsTransfer { [CmdletBinding()] param([switch]$AllUsers) if (-not $script:bitsPresent) { throw [Management.Automation.CommandNotFoundException]::new('Synthetic missing BITS module') }; if ($script:mode -eq 'bits-denied') { throw [UnauthorizedAccessException]::new('Synthetic BITS denial') } }
function git.exe { $global:LASTEXITCODE = 1 }
$script:ScheduledCredentialFunctions = 'function Get-WCScheduledCredentials { @() }'
function Get-ItemProperty { param($Path, $Name, $EA, $ErrorAction) [pscustomobject]@{ EnableClipboardHistory = 0; EnableCloudClipboard = 1; AllowClipboardHistory = 1; AllowCrossDeviceClipboard = 1 } }
function Get-CleanupClipboardHistory { param([bool]$IncludePreviews = $false) @{ status = $script:historyStatus; items = @() } }

# --- classification
$ownRoot = Join-Path $env:TEMP 'wc-benign-partial-fixture'
$systemRoot = Join-Path $env:SystemRoot 'System32\WDI\LogFiles'
if (-not (Test-CleanupSystemOwnedPath -Path $systemRoot) -or -not (Test-CleanupSystemOwnedPath -Path (Join-Path $env:ProgramData 'Microsoft\Diagnosis\ETLLogs')) -or (Test-CleanupSystemOwnedPath -Path $ownRoot) -or (Test-CleanupSystemOwnedPath -Path (Join-Path $env:ProgramData 'TeamViewer'))) { throw 'System-owned location classification was wrong.' }
function Get-FailureKind($exception, $path) { try { throw $exception } catch { Get-CleanupScanFailureKind $_ $path } }
if ((Get-FailureKind ([Management.Automation.ItemNotFoundException]::new('x')) $ownRoot) -ne 'skip') { throw 'A missing item must be skipped.' }
if ((Get-FailureKind ([IO.IOException]::new('x')) $ownRoot) -ne 'skip') { throw 'A locked or vanished item must be skipped.' }
if ((Get-FailureKind ([UnauthorizedAccessException]::new('x')) $systemRoot) -ne 'private') { throw 'Denied system location must be a scope limit.' }
if ((Get-FailureKind ([UnauthorizedAccessException]::new('x')) $ownRoot) -ne 'failed') { throw 'Denied user location must stay a failure.' }
if ((Get-FailureKind ([InvalidOperationException]::new('x')) $systemRoot) -ne 'failed') { throw 'An unknown error must stay a failure.' }

# --- cache inventory
$script:mode = 'denied'
$result = Get-CleanupCacheFileInventory -Root $systemRoot -Patterns @('*.etl')
if ($result.partial -or $result.status -ne 'complete' -or $result.coverage -ne 'limited' -or $result.coverageNote -notmatch 'private to the system') { throw 'A denied system-owned log folder must be limited, not partial.' }
$result = Get-CleanupCacheFileInventory -Root $ownRoot -Patterns @('*.dat')
if (-not $result.partial -or $result.status -ne 'partial' -or $result.coverage) { throw 'A denied folder of the user''s own must stay partial.' }
foreach ($mode in @('locked', 'vanished')) {
    $script:mode = $mode
    $result = Get-CleanupCacheFileInventory -Root $ownRoot -Patterns @('*.dat')
    if ($result.partial -or $result.coverage) { throw "A $mode item must be skipped, not reported partial." }
}
$script:mode = 'ok'
$result = Get-CleanupCacheFileInventory -Root $ownRoot -Patterns @('*.dat')
if ($result.partial -or $result.total -ne 1 -or $result.coverage) { throw 'A readable folder must stay complete.' }

# --- artifact inventory keeps its source note and adds the private-location note
$script:mode = 'denied'
$result = Get-CleanupArtifactInventory -CoverageNote 'Selected locations only.' -Sources @(@{ label = 'System'; path = $systemRoot; patterns = @('*') })
if ($result.partial -or $result.coverage -ne 'limited' -or $result.coverageNote -notmatch '^Selected locations only\..*private to the system') { throw 'A denied system source must add a private-location note.' }
$result = Get-CleanupArtifactInventory -CoverageNote 'Selected locations only.' -Sources @(@{ label = 'User'; path = $ownRoot; patterns = @('*') })
if (-not $result.partial -or $result.coverageNote -ne 'Selected locations only.') { throw 'A denied user source must stay partial.' }
$script:mode = 'locked'
$result = Get-CleanupArtifactInventory -CoverageNote 'Selected locations only.' -Sources @(@{ label = 'User'; path = $ownRoot; patterns = @('*') })
if ($result.partial) { throw 'A locked file must be skipped.' }

# --- a hit scan limit stays incomplete
$script:mode = 'ok'
$result = Get-EmbeddedWebCacheInfo -MaxMilliseconds 0
if (-not $result.partial -or $result.status -ne 'partial') { throw 'A scan that hit its limit must stay partial.' }

# --- optional components
$script:gitPresent = $false
$result = Get-GitActivityInfo
if ($result.error -or $result.partial -or $result.status -ne 'complete' -or $result.coverageNote -notmatch 'Git is not installed') { throw 'A PC without git.exe is a normal state.' }
$script:gitPresent = $true
$result = Get-GitActivityInfo
if ($result.partial -or $result.coverageNote -match 'not installed') { throw 'Installed Git must scan normally.' }
$script:ScheduledCredentialFunctions = $null
$result = Get-GitActivityInfo
if (-not $result.partial) { throw 'An unreadable credential list must stay partial.' }

$script:bitsPresent = $false; $script:mode = 'missing'
$result = Get-BITSQueueInfo
if ($result.partial -or $result.status -ne 'not_applicable' -or $result.coverage -or $result.total -ne 0) { throw 'A PC without BITS is not applicable, not partial.' }
$script:bitsPresent = $true; $script:mode = 'bits-denied'
$result = Get-BITSQueueInfo
if (-not $result.partial -or $result.status -ne 'partial') { throw 'A BITS failure on a PC with BITS must stay partial.' }

# --- clipboard history turned off
$script:mode = 'ok'
$result = Get-ClipboardHistoryStatus
if ($result.partial -or $result.clipboardHistoryDisabled -ne $true -or $result.historyReadStatus -ne 'disabled') { throw 'Turned-off clipboard history is not an incomplete scan.' }
$script:historyStatus = 'failed'
$result = Get-ClipboardHistoryStatus
if (-not $result.partial) { throw 'An unreadable clipboard history must stay partial.' }

Write-Output 'Passed: benign access and missing-component conditions are not partial; real failures and hit limits stay partial.'
