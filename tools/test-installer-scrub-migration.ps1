# SPDX-License-Identifier: AGPL-3.0-or-later
# Manual local registry fixtures. Never runs setup or changes live menu keys.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$tokens = $null; $errors = $null
$source = Join-Path $PSScriptRoot '../src-tauri/commander-free/nsis/migrate-scrub-context-menus.ps1'
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Migration parse failed' }
$definition = @($ast.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.FunctionDefinitionAst] })
if ($definition.Count -ne 1) { throw 'Unexpected migration function surface' }
. ([scriptblock]::Create($definition[0].Extent.Text))
$fixture = 'Software\ServaLabs\WinCommander\ScrubMigrationTests\' + [guid]::NewGuid().ToString('N')
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryView]::Registry64)
$classes = $base.CreateSubKey($fixture)
$target = 'C:\Program Files\WinCommander\wincommander-free.exe'
$expected = '"' + $target + '" "--scrub" "%1"'
$old = '"D:\Disposable Developer Build\wincommander-free.exe" "--scrub" "%1"'
function Seed-Verb([string]$Class, [string]$Value, [string]$Kind = 'String') {
    $verb = $classes.CreateSubKey("$Class\shell\WinCommanderScrub")
    try {
        $verb.SetValue('', 'Scrub metadata')
        $verb.SetValue('MultiSelectModel', 'Player')
        $verb.SetValue('Icon', 'untouched-icon')
        $command = $verb.CreateSubKey('command')
        try { $command.SetValue('', $Value, [Microsoft.Win32.RegistryValueKind]$Kind) }
        finally { $command.Dispose() }
    } finally { $verb.Dispose() }
}
function Read-Command([string]$Class) {
    $key = $classes.OpenSubKey("$Class\shell\WinCommanderScrub\command")
    try { return $key.GetValue('', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
    finally { $key.Dispose() }
}
try {
    foreach ($class in @('*', 'Directory', 'AllFilesystemObjects')) { Seed-Verb $class $old 'ExpandString' }
    $result = Repair-ScrubClasses $classes $target
    if ($result.updated -ne 3 -or $result.skipped -ne 0) { throw 'Three recognized routes were not migrated' }
    foreach ($class in @('*', 'Directory', 'AllFilesystemObjects')) {
        if ((Read-Command $class) -cne $expected) { throw 'Command readback differs' }
        $verb = $classes.OpenSubKey("$class\shell\WinCommanderScrub")
        $command = $verb.OpenSubKey('command')
        try {
            if ($verb.GetValue('Icon') -ne 'untouched-icon' -or $command.GetValueKind('') -ne [Microsoft.Win32.RegistryValueKind]::ExpandString) { throw 'Noncommand value or type changed' }
        } finally { $command.Dispose(); $verb.Dispose() }
    }
    if ((Repair-ScrubClasses $classes $target).updated -ne 0) { throw 'Repeated upgrade is not idempotent' }
    foreach ($foreign in @(
        '"C:\Foreign\other.exe" "--scrub" "%1"',
        ($old + ' --extra'),
        '"D:\Build\wincommander-free.exe" "--scrub" "%1" & other.exe',
        '"\\remote\share\wincommander-free.exe" "--scrub" "%1"',
        '"D:\Build\wincommander-free.exe" "--scrub" "%V"'
    )) {
        Seed-Verb '*' $foreign
        $result = Repair-ScrubClasses $classes $target
        if ($result.updated -ne 0 -or $result.skipped -ne 1 -or (Read-Command '*') -cne $foreign) { throw 'Unknown command was changed' }
    }
    foreach ($flag in @('LegacyDisable', 'ProgrammaticAccessOnly', 'ExplorerCommandHandler', 'SubCommands', 'ExtendedSubCommandsKey', 'MUIVerb')) {
        Seed-Verb '*' $old
        $verb = $classes.OpenSubKey('*\shell\WinCommanderScrub', $true)
        try { $verb.SetValue($flag, 'preserve') } finally { $verb.Dispose() }
        if ((Repair-ScrubClasses $classes $target).updated -ne 0 -or (Read-Command '*') -cne $old) { throw 'Disabled or extended menu was changed' }
        $verb = $classes.OpenSubKey('*\shell\WinCommanderScrub', $true)
        try { $verb.DeleteValue($flag) } finally { $verb.Dispose() }
    }
    Seed-Verb '*' $old
    $key = $classes.OpenSubKey('*\shell\WinCommanderScrub\command', $true)
    try { $key.SetValue('DelegateExecute', '{disposable}') } finally { $key.Dispose() }
    if ((Repair-ScrubClasses $classes $target).updated -ne 0 -or (Read-Command '*') -cne $old) { throw 'Delegate command was changed' }
    $classes.DeleteSubKeyTree('*\shell\WinCommanderScrub')
    Seed-Verb '*' $old
    $key = $classes.OpenSubKey('*\shell\WinCommanderScrub\command', $true)
    try { $key.DeleteValue('') } finally { $key.Dispose() }
    Seed-Verb 'Directory' $old
    $result = Repair-ScrubClasses $classes $target
    if ($result.skipped -ne 1 -or $result.updated -ne 1 -or (Read-Command 'Directory') -cne $expected) { throw 'Missing default prevented later migration' }
    $classes.DeleteSubKeyTree('*\shell\WinCommanderScrub')
    $result = Repair-ScrubClasses $classes $target
    if ($null -ne $classes.OpenSubKey('*\shell\WinCommanderScrub') -or $result.updated -ne 0) { throw 'Missing menu was enabled' }
    $text = [IO.File]::ReadAllText($source)
    if ($text -match 'reg(?:\.exe)?\s+load|Set-Acl|Register-ScheduledTask|Remove-Item') { throw 'Migration acquired unrelated mutation authority' }
    $hooks = [IO.File]::ReadAllText((Join-Path $PSScriptRoot '../src-tauri/commander-free/nsis/hooks.nsh'))
    if ($hooks.IndexOf('wincommander-migrate-scrub-context-menus.ps1" -SharedExecutable') -lt 0 -or
        $hooks.Substring($hooks.IndexOf('!macro NSIS_HOOK_PREUNINSTALL')).Contains('WC_SCRUB_MENU_MIGRATION')) { throw 'Migration hook is absent or runs during uninstall' }
    'PASS: scrub migration, preserved types/values, idempotence, foreign/extended/disabled/missing menu rejection; no setup executed'
} finally {
    $classes.Dispose()
    if ($fixture -notmatch '^Software\\ServaLabs\\WinCommander\\ScrubMigrationTests\\[0-9a-f]{32}$') { throw 'Invalid fixture cleanup target' }
    $base.DeleteSubKeyTree($fixture, $false)
    $base.Dispose()
}
