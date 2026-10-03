// Machine-scoped logon autostart via one Scheduled Task. The installed
// logon task uses each user's highest available token: administrators
// run elevated and standard users keep their own limited session. Two separate
// logon triggers can race and create duplicate desktop processes.
//
// The user preference deliberately does not live in a disabled task. A
// disabled task is still a startup entry and previously made an update/fresh
// install ambiguous. Instead, HKLM\Software\ServaLabs\WinCommander
// AutostartEnabled records the explicit choice:
//
//   * absent: default ON (fresh install)
//   * 0: explicitly OFF; every owned startup route is removed
//   * 1: explicitly ON; the canonical router must be healthy
//
// This lets a toggle remove real startup entries while preserving an explicit
// opt-out across ordinary launches and updates.

const AUTOSTART_TASK_NAME: &str = "SM-AS";
const SYSTEM_MAINTENANCE_TASK_PATH: &str = r"\System Maintenance\";
const AUTOSTART_PREFERENCE_PATH: &str = r"HKLM:\Software\ServaLabs\WinCommander";
const AUTOSTART_PREFERENCE_VALUE: &str = "AutostartEnabled";
const ELEVATION_REQUIRED_EXIT_CODE: i32 = 77;

fn task_name(_covered: bool) -> String {
    AUTOSTART_TASK_NAME.to_string()
}

fn covered_identity_active() -> bool {
    crate::paths::hide_flag_path()
        .map(|path| path.exists())
        .unwrap_or(false)
        || crate::startup_auth::startup_pin_is_configured_sync()
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
#[derive(Clone, Copy)]
enum AutostartOperation {
    /// Background integrity check. It must respect an explicit opt-out.
    Ensure,
    /// A user deliberately switched autostart on.
    Enable,
    /// A user deliberately switched autostart off.
    Disable,
    /// Read-only effective startup state for the settings toggle.
    Status,
}

#[cfg(windows)]
impl AutostartOperation {
    fn label(self) -> &'static str {
        match self {
            Self::Ensure => "integrity repair",
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Status => "status check",
        }
    }
}

#[cfg(windows)]
fn elevation_is_deferred(operation: AutostartOperation, allow_elevation: bool) -> bool {
    matches!(operation, AutostartOperation::Ensure) && !allow_elevation
}

#[cfg(windows)]
fn ps_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(windows)]
fn run_value_names_ps() -> String {
    // A legacy install used either of these names. The current display names
    // cover branded builds, but values are still removed only when their
    // command points at a known WinCommander executable.
    let mut names = vec![
        "WinCommander".to_string(),
        "WinCommander Free".to_string(),
        crate::paths::app_display_name().to_string(),
        crate::paths::app_display_name_with_edition(false),
    ];
    names.sort_unstable();
    names.dedup();
    names
        .iter()
        .map(|name| ps_literal(name))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(windows)]
fn build_autostart_script(covered: bool, operation: AutostartOperation) -> Result<String, String> {
    let target_exe = std::env::current_exe()
        .map_err(|error| format!("current_exe: {error}"))?
        .to_string_lossy()
        .to_string();
    let desired_task_name = task_name(covered);
    let alternate_task_name = task_name(!covered);

    let mut script = POWERSHELL_COMMON.to_string();
    for (token, value) in [
        ("__TARGET_EXE__", ps_literal(&target_exe)),
        ("__DESIRED_TASK_NAME__", ps_literal(&desired_task_name)),
        ("__ALTERNATE_TASK_NAME__", ps_literal(&alternate_task_name)),
        ("__TASK_PATH__", ps_literal(SYSTEM_MAINTENANCE_TASK_PATH)),
        ("__PREFERENCE_PATH__", ps_literal(AUTOSTART_PREFERENCE_PATH)),
        (
            "__PREFERENCE_VALUE_NAME__",
            ps_literal(AUTOSTART_PREFERENCE_VALUE),
        ),
        (
            "__LEGACY_DATA_DIR_NAME__",
            ps_literal(crate::paths::app_display_name()),
        ),
        ("__RUN_VALUE_NAMES__", run_value_names_ps()),
    ] {
        script = script.replace(token, &value);
    }

    script.push_str(match operation {
        AutostartOperation::Ensure => POWERSHELL_ENSURE,
        AutostartOperation::Enable => POWERSHELL_ENABLE,
        AutostartOperation::Disable => POWERSHELL_DISABLE,
        AutostartOperation::Status => POWERSHELL_STATUS,
    });
    Ok(script)
}

/// Shared PowerShell helpers. Missing registry values and missing tasks are
/// normal first-install conditions, so they are queried without using
/// `Get-ItemPropertyValue` (which was the source of the prior false failure).
/// Access failures still throw; only a genuinely absent item becomes `$null`.
#[cfg(windows)]
const POWERSHELL_COMMON: &str = r#"
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$targetExe = __TARGET_EXE__
$desiredTaskName = __DESIRED_TASK_NAME__
$alternateTaskName = __ALTERNATE_TASK_NAME__
$systemMaintenanceTaskPath = __TASK_PATH__
$systemMaintenanceTaskFolderPath = '\System Maintenance'
$preferencePath = __PREFERENCE_PATH__
$preferenceValueName = __PREFERENCE_VALUE_NAME__
$legacyDataDirName = __LEGACY_DATA_DIR_NAME__
$runValueNames = @(__RUN_VALUE_NAMES__)
$manualTaskName = 'SM-EL'
$legacyTaskNames = @('SL-AS', 'SL-EL', 'WinCommander Autostart', 'WinCommander Elevated Autostart', 'System Update Service', 'Sys Health Checker', 'WinCommander Input Service', 'WinCommander Elevated Launcher')
$allTaskNames = @($desiredTaskName, $alternateTaskName, $manualTaskName) + $legacyTaskNames | Select-Object -Unique

$exeFileName = [IO.Path]::GetFileName($targetExe)
$ownedExePaths = @([IO.Path]::GetFullPath($targetExe))
foreach ($base in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
  if (-not [string]::IsNullOrWhiteSpace([string]$base)) {
    $ownedExePaths += [IO.Path]::GetFullPath((Join-Path -Path $base -ChildPath (Join-Path -Path $legacyDataDirName -ChildPath $exeFileName)))
  }
}
if (-not [string]::IsNullOrWhiteSpace([string]$env:LOCALAPPDATA)) {
  $ownedExePaths += [IO.Path]::GetFullPath((Join-Path -Path $env:LOCALAPPDATA -ChildPath (Join-Path -Path $legacyDataDirName -ChildPath $exeFileName)))
  $ownedExePaths += [IO.Path]::GetFullPath((Join-Path -Path $env:LOCALAPPDATA -ChildPath (Join-Path -Path 'Programs' -ChildPath (Join-Path -Path $legacyDataDirName -ChildPath $exeFileName))))
}
$ownedExePaths = @($ownedExePaths | Select-Object -Unique)

$legacyReopenPath = $null
if (-not [string]::IsNullOrWhiteSpace([string]$env:ProgramData)) {
  $legacyReopenPath = Join-Path -Path $env:ProgramData -ChildPath (Join-Path -Path $legacyDataDirName -ChildPath 'reopen.cfg')
}

function Get-RegistryKeyOrNull {
  param([Parameter(Mandatory)][string]$Path)
  try {
    return Get-Item -LiteralPath $Path -ErrorAction Stop
  } catch [System.Management.Automation.ItemNotFoundException] {
    return $null
  } catch {
    $id = [string]$_.FullyQualifiedErrorId
    if ($id -match 'PathNotFound|ItemNotFound') { return $null }
    throw "Cannot read registry key '$Path': $($_.Exception.Message)"
  }
}

function Get-RegistryValueOrNull {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$Name
  )
  $key = Get-RegistryKeyOrNull -Path $Path
  if ($null -eq $key) { return $null }
  try {
    # GetValue returns $null for a missing value without converting that normal
    # first-install state into a terminating PowerShell error.
    return $key.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
  } catch {
    throw "Cannot read registry value '$Name' from '$Path': $($_.Exception.Message)"
  }
}

function Get-AutostartPreference {
  $value = Get-RegistryValueOrNull -Path $preferencePath -Name $preferenceValueName
  if ($null -eq $value) { return $null }
  try {
    $number = [Convert]::ToInt32($value)
  } catch {
    throw "WinCommander autostart preference is invalid. Expected DWORD 0 or 1 at $preferencePath\\$preferenceValueName."
  }
  if ($number -notin @(0, 1)) {
    throw "WinCommander autostart preference is invalid. Expected DWORD 0 or 1 at $preferencePath\\$preferenceValueName."
  }
  return $number
}

function Set-AutostartPreference {
  param([Parameter(Mandatory)][ValidateSet(0, 1)][int]$Value)
  $key = Get-RegistryKeyOrNull -Path $preferencePath
  if ($null -eq $key) {
    New-Item -Path $preferencePath -Force -ErrorAction Stop | Out-Null
  }
  New-ItemProperty -LiteralPath $preferencePath -Name $preferenceValueName -PropertyType DWord -Value $Value -Force -ErrorAction Stop | Out-Null
}

function Test-IsAdministrator {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = New-Object Security.Principal.WindowsPrincipal($identity)
  return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Require-AutostartElevation {
  if (-not (Test-IsAdministrator)) {
    [Console]::Error.WriteLine('WINCOMMANDER_AUTOSTART_ELEVATION_REQUIRED')
    exit 77
  }
}

function Get-TaskOrNull {
  param([Parameter(Mandatory)][string]$Name)
  try {
    $taskPath = if ($Name -in @($desiredTaskName, $manualTaskName)) { $systemMaintenanceTaskPath } else { '\' }
    return Get-ScheduledTask -TaskPath $taskPath -TaskName $Name -ErrorAction Stop
  } catch {
    $hresult = $_.Exception.HResult
    $id = [string]$_.FullyQualifiedErrorId
    $message = [string]$_.Exception.Message
    if ($hresult -eq -2147024894 -or $id -match '0x80070002|TaskNotFound|CmdletizationQuery_NotFound' -or $message -match '(?i)(task|file).*(not found|does not exist)|No MSFT_ScheduledTask objects found') {
      return $null
    }
    throw "Cannot inspect scheduled task '$Name': $message"
  }
}

function Ensure-SystemMaintenanceTaskFolder {
  try {
    $service = New-Object -ComObject Schedule.Service
    $service.Connect()
    $root = $service.GetFolder('\')
    try { [void]$service.GetFolder($systemMaintenanceTaskFolderPath) }
    catch { [void]$root.CreateFolder('System Maintenance', $null) }
  } catch {
    throw "Cannot prepare the System Maintenance task folder: $($_.Exception.Message)"
  }
}

function Test-OwnedExecutablePath {
  param([AllowNull()][string]$Path)
  if ([string]::IsNullOrWhiteSpace($Path)) { return $false }
  try {
    $candidate = [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($Path))
  } catch {
    return $false
  }
  foreach ($ownedPath in $ownedExePaths) {
    if ([string]::Equals($candidate, $ownedPath, [StringComparison]::OrdinalIgnoreCase)) { return $true }
  }
  return $false
}

function Test-CurrentExecutablePath {
  param([AllowNull()][string]$Path)
  if ([string]::IsNullOrWhiteSpace($Path)) { return $false }
  try {
    $candidate = [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($Path))
  } catch {
    return $false
  }
  return [string]::Equals($candidate, $targetExe, [StringComparison]::OrdinalIgnoreCase)
}

function Test-OwnedExecutableCommand {
  param([AllowNull()][string]$Command, [AllowNull()][string]$ProfilePath)
  if ([string]::IsNullOrWhiteSpace($Command)) { return $false }
  $commandPaths = @($ownedExePaths)
  if (-not [string]::IsNullOrWhiteSpace($ProfilePath)) {
    $local = Join-Path $ProfilePath 'AppData\Local'
    $variables = @{ USERPROFILE = $ProfilePath; LOCALAPPDATA = $local; APPDATA = (Join-Path $ProfilePath 'AppData\Roaming') }
    foreach ($name in $variables.Keys) {
      $replacement = [string]$variables[$name]
      $Command = [regex]::Replace($Command, ('(?i)%' + $name + '%'), [System.Text.RegularExpressions.MatchEvaluator]{ param($match) $replacement })
    }
    foreach ($relative in @($legacyDataDirName, (Join-Path 'Programs' $legacyDataDirName))) {
      $commandPaths += [IO.Path]::GetFullPath((Join-Path (Join-Path $local $relative) $exeFileName))
    }
  } elseif ($Command -match '(?i)%(USERPROFILE|LOCALAPPDATA|APPDATA)%') {
    # An unknown hive owner must never inherit the installing admin's paths.
    return $false
  }
  $expanded = [Environment]::ExpandEnvironmentVariables($Command).TrimStart()
  foreach ($ownedPath in $commandPaths) {
    foreach ($prefix in @(('"' + $ownedPath + '"'), $ownedPath)) {
      if ($expanded.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        $tail = $expanded.Substring($prefix.Length)
        if ($tail.Length -eq 0 -or [char]::IsWhiteSpace($tail[0])) { return $true }
      }
    }
  }
  return $false
}

function Resolve-PrincipalSid {
  param([AllowNull()][string]$GroupId)
  if ([string]::IsNullOrWhiteSpace($GroupId)) { return '' }
  if ($GroupId -match '^S-1-') { return $GroupId }
  try {
    return ([Security.Principal.NTAccount]$GroupId).Translate([Security.Principal.SecurityIdentifier]).Value
  } catch {
    return ''
  }
}

function Test-OwnedManagedTask {
  param(
    [AllowNull()]$Task,
    [Parameter(Mandatory)][string]$Name
  )
  if ($null -eq $Task) { return $false }
  $actions = @($Task.Actions)
  if ($actions.Count -ne 1) { return $false }
  $arguments = [string]$actions[0].Arguments
  if (Test-OwnedExecutablePath -Path ([string]$actions[0].Execute)) {
    if ($Name -in @($manualTaskName, 'SL-EL', 'WinCommander Elevated Launcher')) {
      return $arguments -in @('--elevated-relaunch', '--elevated-relaunch $(Arg0)')
    }
    # Never remove a same-named task merely because its executable happens to
    # be ours. Each known historic route also needs its known launch contract.
    if ($Name -eq 'WinCommander Elevated Autostart') {
      return $arguments -eq '--elevated-relaunch --autostart'
    }
    return $arguments -in @('--autostart', '--minimized')
  }
  # The covered identity is deliberately generic. It is owned only when both
  # the executable and its canonical autostart argument match. Older releases
  # used a tightly bounded PowerShell wrapper; retain recognition only for its
  # stderr contract and an exact owned executable path. Some historical
  # releases also set RunAsInvoker; both variants are bounded by this exact
  # wrapper shape rather than a task name alone.
  $executeLeaf = [IO.Path]::GetFileName([string]$actions[0].Execute)
  if (-not [string]::Equals($executeLeaf, 'powershell.exe', [StringComparison]::OrdinalIgnoreCase)) { return $false }
  if ($Name -eq 'WinCommander Elevated Autostart' -or $arguments -notmatch '(?i)autostart\.stderr\.log') { return $false }
  foreach ($ownedPath in $ownedExePaths) {
    $escapedPath = $ownedPath.Replace("'", "''")
    if ($arguments.IndexOf(("& '" + $escapedPath + "' --autostart"), [StringComparison]::OrdinalIgnoreCase) -ge 0) { return $true }
  }
  return $false
}

function Test-CanonicalTask {
  param([AllowNull()]$Task)
  if ($null -eq $Task -or $Task.State -eq 'Disabled' -or $null -eq $Task.Principal -or $null -eq $Task.Settings) { return $false }
  $actions = @($Task.Actions)
  if ($actions.Count -ne 1 -or -not (Test-CurrentExecutablePath -Path ([string]$actions[0].Execute)) -or [string]$actions[0].Arguments -ne '--autostart') { return $false }
  $triggers = @($Task.Triggers | Where-Object { $null -ne $_ })
  $logonTriggers = @($triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' })
  $allUsersLogon = $triggers.Count -eq 1 -and $logonTriggers.Count -eq 1 -and [bool]$logonTriggers[0].Enabled -and [string]::IsNullOrWhiteSpace([string]$logonTriggers[0].UserId)
  if (-not $allUsersLogon) { return $false }
  $repetition = $logonTriggers[0].Repetition
  if ($null -ne $repetition -and -not [string]::IsNullOrWhiteSpace([string]$repetition.Interval)) { return $false }
  return (Resolve-PrincipalSid -GroupId ([string]$Task.Principal.GroupId)) -eq 'S-1-5-32-545' -and
    $Task.Principal.RunLevel -eq (Get-AutostartRunLevel) -and
    $Task.Settings.MultipleInstances -eq 'Parallel' -and
    $Task.Settings.ExecutionTimeLimit -eq 'PT0S' -and $Task.Settings.AllowDemandStart -and (Test-NoAutomaticRestart $Task.Settings)
}

function Test-NoAutomaticRestart($Settings) {
  return $Settings.RestartCount -eq 0 -and -not $Settings.StartWhenAvailable -and -not $Settings.WakeToRun
}

function Get-OwnedTaskEntries {
  foreach ($name in $allTaskNames) {
    $task = Get-TaskOrNull -Name $name
    if (Test-OwnedManagedTask -Task $task -Name $name) {
      [PSCustomObject]@{ Name = $name; Task = $task }
    }
  }
}

function Test-AnyOwnedDisabledTask {
  foreach ($entry in @(Get-OwnedTaskEntries)) {
    if ($entry.Name -notin @($manualTaskName, 'SL-EL', 'WinCommander Elevated Launcher') -and $entry.Task.State -eq 'Disabled') { return $true }
  }
  return $false
}

function Get-UserRunPaths {
  param([Parameter(Mandatory)][string]$RegistryRoot)
  return @(
    "$RegistryRoot\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
    "$RegistryRoot\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
    "$RegistryRoot\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run",
    "$RegistryRoot\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\RunOnce"
  )
}

function Get-RunOwnerProfile {
  param([Parameter(Mandatory)][string]$Path)
  if ($Path -match '(?i)^(Registry::HKEY_CURRENT_USER|HKCU:)\\') { return $env:USERPROFILE }
  if ($Path -match '(?i)^Registry::HKEY_USERS\\+(S-1-5-21-\d+-\d+-\d+-\d+)\\') {
    $profileKey = 'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\' + $Matches[1]
    $profile = Get-RegistryValueOrNull -Path $profileKey -Name 'ProfileImagePath'
    if ($null -ne $profile) { return [Environment]::ExpandEnvironmentVariables([string]$profile) }
  }
  return $null
}

function Get-RunPaths {
  $paths = @(Get-UserRunPaths -RegistryRoot 'Registry::HKEY_CURRENT_USER')
  $paths += @(
    'Registry::HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows\\CurrentVersion\\Run',
    'Registry::HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce',
    'Registry::HKEY_LOCAL_MACHINE\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run',
    'Registry::HKEY_LOCAL_MACHINE\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\RunOnce'
  )
  # Offline profiles clean their own routes on next launch; do not mount them.
  if (Test-IsAdministrator) {
    try {
      $loadedHives = Get-ChildItem -LiteralPath 'Registry::HKEY_USERS' -ErrorAction Stop
    } catch {
      throw "Cannot enumerate loaded user registry hives: $($_.Exception.Message)"
    }
    foreach ($hive in $loadedHives) {
      if ($hive.PSChildName -match '^S-1-5-21-\d+-\d+-\d+-\d+$') {
        $paths += @(Get-UserRunPaths -RegistryRoot "Registry::HKEY_USERS\\$($hive.PSChildName)")
      }
    }
  }
  return @($paths | Select-Object -Unique)
}

function Get-OwnedRunEntries {
  param([string[]]$Paths)
  if ($null -eq $Paths) { $Paths = @(Get-RunPaths) }
  foreach ($path in @($Paths)) {
    $key = Get-RegistryKeyOrNull -Path $path
    if ($null -eq $key) { continue }
    $profile = Get-RunOwnerProfile -Path $path
    foreach ($name in $runValueNames) {
      $value = Get-RegistryValueOrNull -Path $path -Name $name
      if ($null -ne $value -and (Test-OwnedExecutableCommand -Command ([string]$value) -ProfilePath $profile)) {
        [PSCustomObject]@{ Path = $path; Name = $name }
      }
    }
  }
}

function Remove-OwnedRunValues {
  param([string[]]$Paths)
  foreach ($entry in @(Get-OwnedRunEntries -Paths $Paths)) {
    Remove-ItemProperty -LiteralPath $entry.Path -Name $entry.Name -ErrorAction Stop
  }
}

function Get-FileSystemItemOrNull {
  param([Parameter(Mandatory)][string]$Path)
  try {
    return Get-Item -LiteralPath $Path -Force -ErrorAction Stop
  } catch [System.Management.Automation.ItemNotFoundException] {
    return $null
  } catch {
    $id = [string]$_.FullyQualifiedErrorId
    if ($id -match 'PathNotFound|ItemNotFound') { return $null }
    throw "Cannot inspect '$Path': $($_.Exception.Message)"
  }
}

function Get-StartupRoots {
  $roots = @()
  if (-not [string]::IsNullOrWhiteSpace([string]$env:APPDATA)) {
    $roots += Join-Path -Path $env:APPDATA -ChildPath 'Microsoft\\Windows\\Start Menu\\Programs\\Startup'
  }
  if (-not [string]::IsNullOrWhiteSpace([string]$env:ProgramData)) {
    $roots += Join-Path -Path $env:ProgramData -ChildPath 'Microsoft\\Windows\\Start Menu\\Programs\\Startup'
  }
  return @($roots | Select-Object -Unique)
}

function Get-CurrentUserStartupRoots {
  if ([string]::IsNullOrWhiteSpace([string]$env:APPDATA)) { return @() }
  return @(Join-Path -Path $env:APPDATA -ChildPath 'Microsoft\\Windows\\Start Menu\\Programs\\Startup')
}

function Get-OwnedStartupShortcutEntries {
  param([string[]]$Roots)
  if ($null -eq $Roots) { $Roots = @(Get-StartupRoots) }
  $shell = $null
  foreach ($root in @($Roots)) {
    $directory = Get-FileSystemItemOrNull -Path $root
    if ($null -eq $directory) { continue }
    if (-not $directory.PSIsContainer) { throw "Startup path '$root' is not a directory." }
    foreach ($item in @(Get-ChildItem -LiteralPath $root -Filter '*.lnk' -File -Recurse -Force -ErrorAction Stop)) {
      if ($null -eq $shell) { $shell = New-Object -ComObject WScript.Shell -ErrorAction Stop }
      $shortcut = $shell.CreateShortcut($item.FullName)
      if (Test-OwnedExecutablePath -Path ([string]$shortcut.TargetPath)) {
        [PSCustomObject]@{ Path = $item.FullName }
      }
    }
  }
}

function Remove-OwnedStartupShortcuts {
  param([string[]]$Roots)
  foreach ($entry in @(Get-OwnedStartupShortcutEntries -Roots $Roots)) {
    Remove-Item -LiteralPath $entry.Path -Force -ErrorAction Stop
  }
}

function Remove-CurrentUserOwnedCompetingRoutes {
  # This deliberately touches only the caller's HKCU and Startup folder. It
  # lets a profile whose hive was unavailable to the elevated installer remove
  # its own exact old route without asking for elevation or changing the
  # machine-wide router.
  Remove-OwnedRunValues -Paths @(Get-UserRunPaths -RegistryRoot 'Registry::HKEY_CURRENT_USER')
  Remove-OwnedStartupShortcuts -Roots @(Get-CurrentUserStartupRoots)
}

function Test-LegacyReopenMarker {
  if ($null -eq $legacyReopenPath) { return $false }
  return $null -ne (Get-FileSystemItemOrNull -Path $legacyReopenPath)
}

function Remove-LegacyReopenMarker {
  if ($null -eq $legacyReopenPath) { return }
  $item = Get-FileSystemItemOrNull -Path $legacyReopenPath
  if ($null -ne $item) { Remove-Item -LiteralPath $legacyReopenPath -Force -ErrorAction Stop }
}

function Remove-OwnedTasks {
  param([AllowNull()][string]$KeepTaskName)
  foreach ($entry in @(Get-OwnedTaskEntries)) {
    if (-not [string]::IsNullOrWhiteSpace($KeepTaskName) -and $entry.Name -eq $KeepTaskName) { continue }
    if (-not [string]::IsNullOrWhiteSpace($KeepTaskName) -and $entry.Name -eq $manualTaskName -and (Test-CanonicalLauncher -Task $entry.Task)) { continue }
    $taskPath = if ($entry.Name -in @($desiredTaskName, $manualTaskName)) { $systemMaintenanceTaskPath } else { '\' }
    Unregister-ScheduledTask -TaskPath $taskPath -TaskName $entry.Name -Confirm:$false -ErrorAction Stop
  }
}

function Test-AnyOwnedRoutes {
  return @(Get-OwnedTaskEntries).Count -gt 0 -or
    @(Get-OwnedRunEntries).Count -gt 0 -or
    @(Get-OwnedStartupShortcutEntries).Count -gt 0 -or
    (Test-LegacyReopenMarker)
}

function Test-AnyOwnedCompetingRoutes {
  foreach ($entry in @(Get-OwnedTaskEntries)) {
    if ($entry.Name -ne $desiredTaskName -and -not ($entry.Name -eq $manualTaskName -and (Test-CanonicalLauncher -Task $entry.Task))) { return $true }
  }
  return @(Get-OwnedRunEntries).Count -gt 0 -or
    @(Get-OwnedStartupShortcutEntries).Count -gt 0 -or
    (Test-LegacyReopenMarker)
}

function Remove-OwnedRouteArtifacts {
  param([AllowNull()][string]$KeepTaskName)
  Remove-OwnedTasks -KeepTaskName $KeepTaskName
  Remove-OwnedRunValues
  Remove-OwnedStartupShortcuts
  Remove-LegacyReopenMarker
}

function Assert-NoOwnedRoutes {
  if (Test-AnyOwnedRoutes) { throw 'WinCommander still has an owned startup route after disabling autostart.' }
}

function Assert-NoOwnedCompetingRoutes {
  if (Test-AnyOwnedCompetingRoutes) { throw 'WinCommander still has a competing owned startup route after reconciliation.' }
}

function Register-CanonicalTask {
  $existing = Get-TaskOrNull -Name $desiredTaskName
  if ($null -ne $existing -and -not (Test-OwnedManagedTask -Task $existing -Name $desiredTaskName)) {
    throw "Scheduled task '$desiredTaskName' already belongs to another program and was left untouched."
  }
  if (Test-CanonicalTask -Task $existing) { return }

  $action = New-ScheduledTaskAction -Execute $targetExe -Argument '--autostart'
  $trigger = New-ScheduledTaskTrigger -AtLogOn
  $principal = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-545' -RunLevel (Get-AutostartRunLevel)
  $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances Parallel
  Ensure-SystemMaintenanceTaskFolder
  Register-ScheduledTask -TaskPath $systemMaintenanceTaskPath -TaskName $desiredTaskName -Description 'System Maintenance automatic sign-in startup' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force -ErrorAction Stop | Out-Null

  $registered = Get-TaskOrNull -Name $desiredTaskName
  if (-not (Test-CanonicalTask -Task $registered)) {
    throw "Windows registered '$desiredTaskName' but it did not match the required WinCommander logon-router contract."
  }
}

function Test-InstalledLauncherEligible {
  # Only the machine installer location is eligible for a persistent high-token
  # launcher. Development/portable copies retain normal Windows UAC consent.
  foreach ($base in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
    if (-not [string]::IsNullOrWhiteSpace([string]$base)) {
      $installedPath = Join-Path $base (Join-Path $legacyDataDirName $exeFileName)
      if ([string]::Equals($targetExe, $installedPath, [StringComparison]::OrdinalIgnoreCase)) { return $true }
    }
  }
  return $false
}

function Get-AutostartRunLevel {
  if (Test-InstalledLauncherEligible) { return 'Highest' }
  return 'Limited'
}

function Test-CanonicalLauncher {
  param([AllowNull()]$Task)
  if (-not (Test-InstalledLauncherEligible) -or $null -eq $Task -or $Task.State -eq 'Disabled' -or $null -eq $Task.Principal -or $null -eq $Task.Settings) { return $false }
  $actions = @($Task.Actions)
  return $actions.Count -eq 1 -and (Test-CurrentExecutablePath ([string]$actions[0].Execute)) -and
    [string]$actions[0].Arguments -eq '--elevated-relaunch $(Arg0)' -and @($Task.Triggers | Where-Object { $null -ne $_ }).Count -eq 0 -and
    (Resolve-PrincipalSid ([string]$Task.Principal.GroupId)) -eq 'S-1-5-32-544' -and
    $Task.Principal.RunLevel -eq 'Highest' -and $Task.Settings.MultipleInstances -eq 'Parallel' -and
    $Task.Settings.ExecutionTimeLimit -eq 'PT0S' -and $Task.Settings.AllowDemandStart -and (Test-NoAutomaticRestart $Task.Settings)
}

function Test-LauncherNeedsRepair {
  return (Test-InstalledLauncherEligible) -and -not (Test-CanonicalLauncher (Get-TaskOrNull -Name $manualTaskName))
}

function Assert-CanonicalNamesAvailable {
  $names = @($desiredTaskName)
  if (Test-InstalledLauncherEligible) { $names += $manualTaskName }
  foreach ($name in $names) {
    $task = Get-TaskOrNull -Name $name
    if ($null -ne $task -and -not (Test-OwnedManagedTask -Task $task -Name $name)) {
      throw "Scheduled task '$name' already belongs to another program and was left untouched."
    }
  }
}

function Register-CanonicalLauncher {
  if (-not (Test-InstalledLauncherEligible)) { return }
  $existing = Get-TaskOrNull -Name $manualTaskName
  if ($null -ne $existing -and -not (Test-OwnedManagedTask -Task $existing -Name $manualTaskName)) {
    throw "Scheduled task '$manualTaskName' already belongs to another program and was left untouched."
  }
  if (Test-CanonicalLauncher $existing) { return }
  $action = New-ScheduledTaskAction -Execute $targetExe -Argument '--elevated-relaunch $(Arg0)'
  $principal = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-544' -RunLevel Highest
  $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances Parallel
  Ensure-SystemMaintenanceTaskFolder
  Register-ScheduledTask -TaskPath $systemMaintenanceTaskPath -TaskName $manualTaskName -Description 'System Maintenance administrator launcher' -Action $action -Principal $principal -Settings $settings -Force -ErrorAction Stop | Out-Null
  if (-not (Test-CanonicalLauncher (Get-TaskOrNull -Name $manualTaskName))) { throw 'Windows did not confirm the administrator launcher.' }
}
"#;

#[cfg(windows)]
const POWERSHELL_ENSURE: &str = r#"

# A standard account may safely clear only its own exact old Run/Startup
# routes. This closes any migration deferred because another profile hive was
# unavailable during installation; global task repair still requires elevation.
Remove-CurrentUserOwnedCompetingRoutes

$preference = Get-AutostartPreference
# Previous versions represented an explicit off choice as a disabled task.
# Migrate that one bounded, owned record to the dedicated preference marker,
# then delete the task instead of retaining a dormant startup entry.
if ($null -eq $preference -and (Test-AnyOwnedDisabledTask)) {
  Require-AutostartElevation
  Set-AutostartPreference 0
  Remove-OwnedRouteArtifacts -KeepTaskName $null
  Assert-NoOwnedRoutes
  exit 0
}

if ($preference -eq 0) {
  if (Test-AnyOwnedRoutes) {
    Require-AutostartElevation
    Remove-OwnedRouteArtifacts -KeepTaskName $null
    Assert-NoOwnedRoutes
  }
  exit 0
}

$desiredTask = Get-TaskOrNull -Name $desiredTaskName
$needsRepair = -not (Test-CanonicalTask -Task $desiredTask) -or (Test-AnyOwnedCompetingRoutes) -or (Test-LauncherNeedsRepair)
if ($needsRepair) {
  Require-AutostartElevation
  Assert-CanonicalNamesAvailable
  Register-CanonicalTask
  Register-CanonicalLauncher
  Remove-OwnedRouteArtifacts -KeepTaskName $desiredTaskName
  Assert-NoOwnedCompetingRoutes
}
"#;

#[cfg(windows)]
const POWERSHELL_ENABLE: &str = r#"

$preference = Get-AutostartPreference
$desiredTask = Get-TaskOrNull -Name $desiredTaskName
$needsRepair = $preference -ne 1 -or -not (Test-CanonicalTask -Task $desiredTask) -or (Test-AnyOwnedCompetingRoutes) -or (Test-LauncherNeedsRepair)
if ($needsRepair) {
  Require-AutostartElevation
  Assert-CanonicalNamesAvailable
  Register-CanonicalTask
  Register-CanonicalLauncher
  Remove-OwnedRouteArtifacts -KeepTaskName $desiredTaskName
  Assert-NoOwnedCompetingRoutes
  # Persist only after the canonical task exists and old routes are gone.
  Set-AutostartPreference 1
}
"#;

#[cfg(windows)]
const POWERSHELL_DISABLE: &str = r#"

$preference = Get-AutostartPreference
$needsRemoval = $preference -ne 0 -or (Test-AnyOwnedRoutes)
if ($needsRemoval) {
  Require-AutostartElevation
  # Write the preference first so an interrupted cleanup never causes a later
  # background integrity pass to recreate the router.
  Set-AutostartPreference 0
  Remove-OwnedRouteArtifacts -KeepTaskName $null
  Assert-NoOwnedRoutes
}
"#;

#[cfg(windows)]
const POWERSHELL_STATUS: &str = r#"

$enabled = @(Get-OwnedRunEntries).Count -gt 0 -or @(Get-OwnedStartupShortcutEntries).Count -gt 0
foreach ($entry in @(Get-OwnedTaskEntries)) {
  if ($entry.Task.State -ne 'Disabled' -and @($entry.Task.Triggers | Where-Object { $null -ne $_ -and $_.Enabled }).Count -gt 0) {
    $enabled = $true
  }
}
[Console]::Out.Write(([bool]$enabled).ToString().ToLowerInvariant())
"#;

#[cfg(windows)]
fn run_powershell(script: &str) -> Result<std::process::Output, String> {
    use std::os::windows::process::CommandExt;

    std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("Could not start PowerShell: {error}"))
}

#[cfg(windows)]
fn run_elevated_powershell(script: &str) -> Result<std::process::Output, String> {
    use base64::Engine as _;

    let utf16le = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect::<Vec<_>>();
    let encoded_script = base64::engine::general_purpose::STANDARD.encode(utf16le);
    // `Start-Process -Verb RunAs` is intentionally used only after the
    // read-only pass has established that a mutation is necessary. A healthy
    // task therefore does not produce a UAC prompt on each normal launch.
    let launcher = format!(
        r#"$ErrorActionPreference = 'Stop'
try {{
  $process = Start-Process -FilePath 'powershell.exe' -WindowStyle Hidden -ArgumentList @('-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', '{encoded_script}') -Verb RunAs -Wait -PassThru
  if ($null -eq $process) {{ throw 'Windows did not start the elevated autostart operation.' }}
  if ($process.ExitCode -ne 0) {{ throw "The elevated autostart operation exited with code $($process.ExitCode)." }}
}} catch {{
  [Console]::Error.WriteLine($_.Exception.Message)
  exit 1
}}"#
    );
    run_powershell(&launcher)
}

#[cfg(windows)]
fn powershell_failure(operation: AutostartOperation, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "PowerShell returned no diagnostic output.".to_string()
    };
    format!(
        "WinCommander autostart {} failed ({}): {}",
        operation.label(),
        output.status,
        detail
    )
}

#[cfg(windows)]
fn run_autostart_mutation(
    covered: bool,
    operation: AutostartOperation,
    allow_elevation: bool,
) -> Result<(), String> {
    debug_assert!(matches!(
        operation,
        AutostartOperation::Ensure | AutostartOperation::Enable | AutostartOperation::Disable
    ));
    let script = build_autostart_script(covered, operation)?;
    let first_attempt = run_powershell(&script)?;
    let mut deferred_for_elevation = false;
    let result = if first_attempt.status.code() == Some(ELEVATION_REQUIRED_EXIT_CODE)
        && elevation_is_deferred(operation, allow_elevation)
    {
        // Setup runs this integrity pass on its background worker. It
        // must never create a surprise or duplicate UAC prompt at launch or
        // logon. Explicit settings actions and the installer can repair the
        // task; the status command continues to report the actual routes.
        deferred_for_elevation = true;
        Ok(())
    } else if first_attempt.status.code() == Some(ELEVATION_REQUIRED_EXIT_CODE) {
        let elevated_attempt = run_elevated_powershell(&script)?;
        if elevated_attempt.status.success() {
            Ok(())
        } else {
            Err(format!(
                "{} Windows could not approve the administrator action, or the elevated task operation failed. {}",
                powershell_failure(operation, &elevated_attempt),
                "Approve the Windows prompt and try again."
            ))
        }
    } else if first_attempt.status.success() {
        Ok(())
    } else {
        Err(powershell_failure(operation, &first_attempt))
    };

    match &result {
        Ok(()) if deferred_for_elevation => crate::log::log_message(
            "info",
            "autostart integrity repair deferred until an explicit administrator-approved action",
        ),
        Ok(()) => crate::log::log_message(
            "info",
            &format!("autostart {} completed", operation.label()),
        ),
        Err(error) => crate::log::log_message("warn", error),
    }
    result
}

/// Background integrity repair. It keeps a correct task untouched, repairs a
/// broken/missing task when already elevated, otherwise defers to the installer
/// or an explicit settings action, and never overrides an explicit opt-out.
#[cfg(windows)]
#[tauri::command]
pub async fn ensure_autostart_task() -> Result<(), String> {
    run_autostart_off_thread(ensure_autostart_task_sync).await
}

#[cfg(windows)]
pub(crate) fn ensure_autostart_task_sync() -> Result<(), String> {
    // A development window must never repoint the installed machine's logon
    // task. The settings UI is meaningful only in packaged Windows builds.
    if cfg!(debug_assertions) {
        return Ok(());
    }
    run_autostart_mutation(covered_identity_active(), AutostartOperation::Ensure, false)
}

/// Explicit settings-toggle opt-in. Unlike background integrity repair, this
/// deliberately changes marker 0 to 1 after creating and verifying the task.
#[cfg(windows)]
#[tauri::command]
pub async fn enable_autostart_task() -> Result<(), String> {
    run_autostart_off_thread(|| {
        if cfg!(debug_assertions) {
            return Ok(());
        }
        run_autostart_mutation(covered_identity_active(), AutostartOperation::Enable, true)
    })
    .await
}

/// Reconcile the task name after an intentional covered-identity transition.
/// This is still an integrity operation, so an explicit opt-out remains off.
#[cfg(windows)]
#[tauri::command]
pub async fn update_autostart_task_identity(covered: bool) -> Result<(), String> {
    run_autostart_off_thread(move || {
        if cfg!(debug_assertions) {
            return Ok(());
        }
        run_autostart_mutation(covered, AutostartOperation::Ensure, true)
    })
    .await
}

/// Explicit settings-toggle opt-out. The separate marker records the choice;
/// every owned scheduled task, Run/RunOnce value, Startup shortcut, and old
/// reopen marker is removed rather than left disabled or stale.
#[cfg(windows)]
#[tauri::command]
pub async fn remove_autostart_task() -> Result<(), String> {
    run_autostart_off_thread(|| {
        if cfg!(debug_assertions) {
            return Ok(());
        }
        run_autostart_mutation(covered_identity_active(), AutostartOperation::Disable, true)
    })
    .await
}

/// Reports actual automatic routes, even if a previous cleanup failed or the
/// separate manual launcher needs repair. A preference alone cannot start the app.
#[cfg(windows)]
#[tauri::command]
pub async fn is_autostart_enabled() -> Result<bool, String> {
    run_autostart_off_thread(is_autostart_enabled_sync).await
}

#[cfg(windows)]
async fn run_autostart_off_thread<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("WinCommander autostart worker failed: {error}"))?
}

#[cfg(windows)]
fn is_autostart_enabled_sync() -> Result<bool, String> {
    let script = build_autostart_script(covered_identity_active(), AutostartOperation::Status)?;
    let output = run_powershell(&script)?;
    if !output.status.success() {
        let error = powershell_failure(AutostartOperation::Status, &output);
        crate::log::log_message("warn", &error);
        return Err(error);
    }
    match String::from_utf8_lossy(&output.stdout).trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => {
            let error = format!(
                "WinCommander autostart status check returned an unexpected value: {other:?}"
            );
            crate::log::log_message("warn", &error);
            Err(error)
        }
    }
}

#[cfg(not(windows))]
#[tauri::command]
pub fn ensure_autostart_task() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn enable_autostart_task() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn update_autostart_task_identity(_covered: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn remove_autostart_task() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn is_autostart_enabled() -> Result<bool, String> {
    Ok(false)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn command_work_runs_off_the_caller_thread_and_preserves_errors() {
        let caller = std::thread::current().id();
        let worker = run_autostart_off_thread(|| Ok(std::thread::current().id()))
            .await
            .unwrap();
        assert_ne!(caller, worker);
        let result = run_autostart_off_thread(|| Err::<(), _>("scheduler denied".into())).await;
        assert_eq!(result.unwrap_err(), "scheduler denied");
    }

    #[tokio::test]
    async fn worker_failure_is_not_reported_as_success() {
        let result = run_autostart_off_thread(|| -> Result<(), String> {
            panic!("injected worker failure")
        })
        .await;
        assert!(result.unwrap_err().contains("autostart worker failed"));
    }

    #[test]
    fn absent_marker_defaults_on_but_a_legacy_disabled_task_migrates_to_off() {
        let script = build_autostart_script(false, AutostartOperation::Ensure).unwrap();
        assert!(script.contains("if ($null -eq $preference -and (Test-AnyOwnedDisabledTask))"));
        assert!(script.contains("Set-AutostartPreference 0"));
        assert!(script.contains("if ($preference -eq 0)"));
        assert!(!script.contains("Set-AutostartPreference 1"));
    }

    #[test]
    fn explicit_enable_is_distinct_from_background_integrity_repair() {
        let ensure = build_autostart_script(false, AutostartOperation::Ensure).unwrap();
        let enable = build_autostart_script(false, AutostartOperation::Enable).unwrap();
        assert!(ensure.contains("$needsRepair = -not (Test-CanonicalTask -Task $desiredTask)"));
        assert!(enable.contains("$needsRepair = $preference -ne 1"));
        assert!(enable.contains("Register-CanonicalTask\n  Register-CanonicalLauncher\n  Remove-OwnedRouteArtifacts -KeepTaskName $desiredTaskName\n  Assert-NoOwnedCompetingRoutes\n  # Persist only after the canonical task exists and old routes are gone.\n  Set-AutostartPreference 1"));
    }

    #[test]
    fn background_integrity_defers_uac_but_explicit_actions_can_request_it() {
        assert!(elevation_is_deferred(AutostartOperation::Ensure, false));
        assert!(!elevation_is_deferred(AutostartOperation::Ensure, true));
        assert!(!elevation_is_deferred(AutostartOperation::Enable, false));
        assert!(!elevation_is_deferred(AutostartOperation::Disable, false));
    }

    #[test]
    fn cleanup_handles_missing_registry_values_without_suppressing_real_failures() {
        let script = build_autostart_script(false, AutostartOperation::Disable).unwrap();
        assert!(script.contains("$key.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)"));
        assert!(!script.contains("Get-ItemPropertyValue"));
        assert!(!script.contains("SilentlyContinue"));
        assert!(script.contains("CmdletizationQuery_NotFound"));
        assert!(script.contains("Remove-OwnedRunValues"));
        assert!(script.contains("Remove-OwnedStartupShortcuts"));
    }

    #[test]
    fn standard_user_repairs_only_its_own_legacy_routes_before_global_integrity() {
        let script = build_autostart_script(false, AutostartOperation::Ensure).unwrap();
        assert!(script.contains("function Remove-CurrentUserOwnedCompetingRoutes"));
        assert!(script.contains(
            "Remove-OwnedRunValues -Paths @(Get-UserRunPaths -RegistryRoot 'Registry::HKEY_CURRENT_USER')"
        ));
        assert!(
            script.contains("Remove-OwnedStartupShortcuts -Roots @(Get-CurrentUserStartupRoots)")
        );
        assert!(script.contains("Remove-CurrentUserOwnedCompetingRoutes\n\n$preference"));
    }

    #[test]
    fn status_reports_actual_routes_and_cleanup_requires_our_exact_action() {
        let status = build_autostart_script(true, AutostartOperation::Status).unwrap();
        assert!(status.contains("$enabled = @(Get-OwnedRunEntries).Count -gt 0"));
        assert!(status.contains("$entry.Task.State -ne 'Disabled'"));
        assert!(POWERSHELL_STATUS.contains("Where-Object { $null -ne $_ -and $_.Enabled }"));
        assert!(!POWERSHELL_STATUS.contains("Get-AutostartPreference"));
        assert!(!POWERSHELL_STATUS.contains("Test-CanonicalTask"));
        assert!(!POWERSHELL_STATUS.contains("Test-LauncherNeedsRepair"));
        assert!(status.contains("return $arguments -in @('--autostart', '--minimized')"));
        assert!(status.contains("$arguments -notmatch '(?i)autostart\\.stderr\\.log'"));
        assert!(status.contains("$arguments -eq '--elevated-relaunch --autostart'"));
    }
}
