$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module could not be parsed.' }
foreach ($name in @('Get-CleanupArtifactFiles','Get-CleanupArtifactInventory','Get-RemoteAccessLogsInfo','Get-GameLauncherLogsInfo','Get-CleanupTargetFiles')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('wc-cleanup-sources-' + [guid]::NewGuid().ToString('N'))
$old = @{ AppData = $env:APPDATA; LocalAppData = $env:LOCALAPPDATA; ProgramData = $env:ProgramData; ProgramFiles = $env:ProgramFiles; ProgramFilesX86 = ${env:ProgramFiles(x86)} }
try {
    $env:APPDATA = Join-Path $fixture 'Roaming'; $env:LOCALAPPDATA = Join-Path $fixture 'Local'
    $env:ProgramData = Join-Path $fixture 'Shared'; $env:ProgramFiles = Join-Path $fixture 'Programs64'
    ${env:ProgramFiles(x86)} = Join-Path $fixture 'Programs32'
    $teamViewer = Join-Path $env:ProgramFiles 'TeamViewer'; $anyDesk = Join-Path $env:APPDATA 'AnyDesk'
    New-Item -ItemType Directory -Path $teamViewer, $anyDesk, (Join-Path $teamViewer 'nested') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $teamViewer 'TeamViewer15_Logfile.log'), 'fixture')
    [IO.File]::WriteAllText((Join-Path $teamViewer 'nested\unrelated.log'), 'fixture')
    [IO.File]::WriteAllText((Join-Path $anyDesk 'connection_trace.txt'), 'fixture')
    $remote = Get-RemoteAccessLogsInfo
    if ($remote.error -or $remote.partial -or $remote.total -ne 2) { throw 'Selected TeamViewer and AnyDesk logs were not enumerated exactly.' }
    $targetPreview = @(Get-CleanupTargetFiles @(@{ path = $teamViewer; patterns = @('*.log'); recurse = $false }))
    if ($targetPreview.Count -ne 1 -or $targetPreview[0] -notmatch 'TeamViewer15_Logfile\.log$') { throw 'TeamViewer target selection must remain nonrecursive.' }
    $emptyGames = Get-GameLauncherLogsInfo
    if ($emptyGames.error -or $emptyGames.partial -or $emptyGames.total -ne 0 -or $emptyGames.status -ne 'complete' -or $emptyGames.coverage -ne 'limited') { throw 'Absent default launcher locations should produce a verified empty selected scope.' }
    $steam = Join-Path ${env:ProgramFiles(x86)} 'Steam\logs'
    New-Item -ItemType Directory -Path $steam -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $steam 'connection_log.txt'), 'fixture')
    $games = Get-GameLauncherLogsInfo
    if ($games.error -or $games.partial -or $games.total -ne 1) { throw 'Steam default log source was not enumerated.' }
} finally {
    $env:APPDATA = $old.AppData; $env:LOCALAPPDATA = $old.LocalAppData; $env:ProgramData = $old.ProgramData
    $env:ProgramFiles = $old.ProgramFiles; ${env:ProgramFiles(x86)} = $old.ProgramFilesX86
    if (Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
'Passed: selected remote-access and game-launcher sources, empty scope and nonrecursive TeamViewer folder.'
$fleet = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-FleetForensicProjection' }, $true)
. ([scriptblock]::Create($fleet.Extent.Text))
function Get-RemoteAccessLogsInfo { @{ partial = $true; files = @(); total = 0 } }
function Get-GameLauncherLogsInfo { @{ partial = $true; files = @(); total = 0 } }
function Get-CellularHistoryInfo { @{ noHardware = $true; files = @(); total = 0 } }
foreach ($category in @('remote_access_logs', 'game_launcher_logs', 'cellular_history')) {
    $projection = Get-FleetForensicProjection -Category $category
    if (-not $projection.error -or $projection.total -ne 0) { throw "$category incomplete Fleet data was presented as a complete empty inventory." }
}
'Passed: Fleet rejects incomplete selected-source and no-adapter projections.'
