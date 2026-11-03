# Runs the real registry accessors against one fresh HKCU test subtree only.
# No Windows privacy, update, Explorer or firewall setting is touched.
$ErrorActionPreference = 'Stop'
$moduleRoot = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/modules'
function Import-TestFunctions {
    param([string]$File, [string[]]$Names, [switch]$Rename, [switch]$RedirectMachine)
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile($File, [ref]$null, [ref]$errors)
    if ($errors.Count) { throw ($errors | Out-String) }
    foreach ($name in $Names) {
        $node = $ast.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }.GetNewClosure(), $true)
        if ($null -eq $node) { throw "Missing function $name" }
        $text = $node.Extent.Text
        if ($Rename) {
            $text = $text.Replace("function $name {", "function ${name}Impl {")
            if (-not $text.StartsWith("function ${name}Impl {")) { throw "Could not isolate $name." }
        }
        if ($RedirectMachine) {
            $text = $text.Replace("OpenBaseKey('LocalMachine', 'Registry64')", "OpenBaseKey('CurrentUser', 'Registry64')")
            if ($text.Contains("'LocalMachine'")) { throw "Could not redirect machine registry access in $name." }
        }
        # Import into script scope, not this helper's local scope.
        $text = $text -replace '^function ', 'function script:'
        . ([scriptblock]::Create($text))
    }
}
Import-TestFunctions (Join-Path $moduleRoot 'privacy/telemetry.ps1') @('Get-AdditionalPermissionRegistryValue','Set-AdditionalPermissionRegistryValue','Remove-AdditionalPermissionRegistryValue') -Rename
Import-TestFunctions (Join-Path $moduleRoot 'privacy/telemetry.ps1') @('Get-AdditionalPermissionState','Set-AdditionalAppPermission')
Import-TestFunctions (Join-Path $moduleRoot 'tweaks/system.ps1') @('Get-UpdateRestartRegistryValue','Set-UpdateRestartRegistryValue','Remove-UpdateRestartRegistryValue') -Rename -RedirectMachine
Import-TestFunctions (Join-Path $moduleRoot 'tweaks/system.ps1') @('Get-UpdateRestartPolicyState','Set-UpdateRestartPolicy')
Import-TestFunctions (Join-Path $moduleRoot 'tweaks/ui.ps1') @('Get-ShellVisibilityValue','Set-ShellVisibilityValue','Remove-ShellVisibilityValue') -Rename -RedirectMachine
Import-TestFunctions (Join-Path $moduleRoot 'tweaks/ui.ps1') @('Get-ShellVisibilitySpec','Test-ShellVisibilityValue','Get-ShellVisibilityState','Set-ShellVisibility')

$script:testId = [Guid]::NewGuid().ToString('N')
if ($script:testId -cnotmatch '^[0-9a-f]{32}$') { throw 'Invalid test identifier.' }
$script:testRoot = 'SOFTWARE\WinCommander\Tests\SelectedControls-' + $script:testId
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
$created = $false
function Get-TestKey([string]$Scope, [string]$Key) {
    if ($Scope -cnotin @('Machine','User','LocalMachine','CurrentUser') -or $Key -notmatch '^SOFTWARE\\' -or $Key.Contains('..')) { throw 'Test registry target escaped its disposable scope.' }
    return "$script:testRoot\$Scope\$Key"
}
function Test-IsAdmin { return $true }
function Assert-IsAdmin {}
function Get-ShellVisibilityPlatform { return $true }
function Invoke-WithAdditionalPermissionMutex { param([scriptblock]$Action) & $Action }
function Invoke-WithUpdateRestartMutex { param([scriptblock]$Action) & $Action }
function Invoke-WithShellVisibilityMutex { param([scriptblock]$Action) & $Action }
function Get-AdditionalPermissionRegistryValue { param([string]$Scope,[string]$Key,[string]$Name) Get-AdditionalPermissionRegistryValueImpl 'User' (Get-TestKey $Scope $Key) $Name }
function Set-AdditionalPermissionRegistryValue { param([string]$Scope,[string]$Key,[string]$Name,$Value,[string]$Kind) Set-AdditionalPermissionRegistryValueImpl 'User' (Get-TestKey $Scope $Key) $Name $Value $Kind }
function Remove-AdditionalPermissionRegistryValue { param([string]$Scope,[string]$Key,[string]$Name) Remove-AdditionalPermissionRegistryValueImpl 'User' (Get-TestKey $Scope $Key) $Name }
function Get-UpdateRestartRegistryValue { param([string]$Key,[string]$Name) Get-UpdateRestartRegistryValueImpl (Get-TestKey 'Machine' $Key) $Name }
function Set-UpdateRestartRegistryValue { param([string]$Key,[string]$Name,[int]$Value) Set-UpdateRestartRegistryValueImpl (Get-TestKey 'Machine' $Key) $Name $Value }
function Remove-UpdateRestartRegistryValue { param([string]$Key,[string]$Name) Remove-UpdateRestartRegistryValueImpl (Get-TestKey 'Machine' $Key) $Name }
function Get-ShellVisibilityValue { param([string]$Hive,[string]$Key,[string]$Name) Get-ShellVisibilityValueImpl 'CurrentUser' (Get-TestKey $Hive $Key) $Name }
function Set-ShellVisibilityValue { param([string]$Key,[string]$Name,$Value,[string]$Kind) Set-ShellVisibilityValueImpl (Get-TestKey 'LocalMachine' $Key) $Name $Value $Kind }
function Remove-ShellVisibilityValue { param([string]$Key,[string]$Name) Remove-ShellVisibilityValueImpl (Get-TestKey 'LocalMachine' $Key) $Name }
function Expect-Failure([scriptblock]$Action) { $failed=$false; try { & $Action | Out-Null } catch { $failed=$true }; if (-not $failed) { throw 'Expected the operation to fail.' } }

try {
    $existing = $base.OpenSubKey($script:testRoot, $false)
    if ($null -ne $existing) { $existing.Dispose(); throw 'Unexpected test key collision.' }
    $root = $base.CreateSubKey($script:testRoot, $true)
    if ($null -eq $root) { throw 'Could not create disposable test key.' }
    $root.Dispose(); $created = $true

    $permissionKey = 'SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\bluetooth'
    Set-AdditionalPermissionRegistryValue Machine $permissionKey 'Value' 'Allow' 'String'
    Set-AdditionalPermissionRegistryValue User $permissionKey 'Value' 'Prompt' 'String'
    Set-AdditionalPermissionRegistryValue User "$permissionKey\Example.App" 'Value' 'Allow' 'String'
    if (-not (Set-AdditionalAppPermission bluetooth User deny).verified) { throw 'User permission change failed.' }
    if ((Get-AdditionalPermissionRegistryValue User "$permissionKey\Example.App" 'Value').value -cne 'Allow') { throw 'An individual app permission changed.' }
    if ((Set-AdditionalAppPermission bluetooth User restore).permission.value -cne 'Prompt') { throw 'User permission was not restored.' }
    if (-not (Set-AdditionalAppPermission bluetooth Machine deny).verified) { throw 'Machine permission change failed.' }
    if ((Set-AdditionalAppPermission bluetooth Machine restore).permission.value -cne 'Allow') { throw 'Machine permission was not restored.' }

    $settingsKey = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer'
    Set-ShellVisibilityValue $settingsKey 'SettingsPageVisibility' 'hide:privacy;bluetooth;' 'String'
    if (-not (Set-ShellVisibility SettingsHome hide).verified) { throw 'Settings visibility change failed.' }
    if ((Get-ShellVisibilityValue LocalMachine $settingsKey 'SettingsPageVisibility').value -cne 'hide:privacy;bluetooth;home') { throw 'Existing Settings pages were lost.' }
    if (-not (Set-ShellVisibility SettingsHome restore).verified) { throw 'Settings visibility restore failed.' }
    if ((Get-ShellVisibilityValue LocalMachine $settingsKey 'SettingsPageVisibility').value -cne 'hide:privacy;bluetooth;') { throw 'Settings policy was not restored exactly.' }
    Set-ShellVisibilityValue $settingsKey 'SettingsPageVisibility' 'showonly:privacy' 'String'
    Expect-Failure { Set-ShellVisibility SettingsHome hide }
    if ((Get-ShellVisibilityValue LocalMachine $settingsKey 'SettingsPageVisibility').value -cne 'showonly:privacy') { throw 'External Settings policy was changed.' }
    if (-not (Set-ShellVisibility StartRecommended hide).verified -or -not (Set-ShellVisibility StartRecommended restore).verified) { throw 'Start visibility round trip failed.' }

    $updateKey = 'SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU'
    if (-not (Set-UpdateRestartPolicy applyWithSchedule).configured) { throw 'Update policy round trip could not apply.' }
    if (-not (Set-UpdateRestartPolicy revert).canPrepareSchedule) { throw 'Update policy round trip could not restore.' }
    if ((Get-UpdateRestartRegistryValue $updateKey 'AUOptions').exists -or (Get-UpdateRestartRegistryValue $updateKey 'NoAutoRebootWithLoggedOnUsers').exists) { throw 'Update policy values remained after restore.' }
    Set-UpdateRestartRegistryValue $updateKey 'NoAutoRebootWithLoggedOnUsers' 0
    Expect-Failure { Set-UpdateRestartPolicy applyWithSchedule }
    if ((Get-UpdateRestartRegistryValue $updateKey 'NoAutoRebootWithLoggedOnUsers').value -ne 0 -or (Get-UpdateRestartRegistryValue $updateKey 'AUOptions').exists) { throw 'External update policy was changed.' }
    Expect-Failure { Set-AdditionalAppPermission '../other' User deny }
} finally {
    if ($created -and $script:testRoot -ceq ('SOFTWARE\WinCommander\Tests\SelectedControls-' + $script:testId)) { $base.DeleteSubKeyTree($script:testRoot, $false) }
    $base.Dispose()
}
$checkBase = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
try {
    $remaining = $checkBase.OpenSubKey($script:testRoot, $false)
    if ($null -ne $remaining) { $remaining.Dispose(); throw 'Disposable test key was not removed.' }
} finally { $checkBase.Dispose() }
Write-Output 'Disposable selected-controls registry round trips passed.'
