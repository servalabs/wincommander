$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/modules/tweaks/ui.ps1'
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$null, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
foreach ($name in @('Get-ContextItemSpec','Get-ContextItemState','Get-ContextItemVisibility','Set-ContextItemVisibility')) {
    $node = $ast.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }.GetNewClosure(), $true)
    . ([scriptblock]::Create($node.Extent.Text))
}
function Reset-Fixture {
    $script:keys = @{}; $script:values = @{}; $script:snapshots = @{}; $script:sid = 'S-1-5-21-123'; $script:admin = $true
    $script:failRemoveAt = 0; $script:removeCount = 0; $script:failRestore = $false
}
function Test-IsAdmin { $script:admin }
function Assert-IsAdmin { if (-not $script:admin) { throw 'Administrator required.' } }
function Get-ContextItemAccountSid { $script:sid }
function Invoke-WithShellVisibilityMutex { param([scriptblock]$Action) & $Action }
function Get-ShellVisibilityValue {
    param([string]$Hive,[string]$Key,[string]$Name)
    $value = $script:snapshots["$Hive|$Key|$Name"]
    if ($null -eq $value) { return @{exists=$false;value=$null;kind=$null} }
    return @{exists=$true;value=$value.value;kind=$value.kind}
}
function Set-ShellVisibilityValue { param([string]$Key,[string]$Name,$Value,[string]$Kind) $script:snapshots["LocalMachine|$Key|$Name"] = @{value=$Value;kind=$Kind} }
function Remove-ShellVisibilityValue { param([string]$Key,[string]$Name) $script:snapshots.Remove("LocalMachine|$Key|$Name") }
function Get-ContextHandlerValue {
    param([string]$Hive,[string]$Key)
    $id = "$Hive|$Key"; $entry = $script:values[$id]
    if ($null -eq $entry) { return @{keyExists=[bool]$script:keys[$id];exists=$false;value=$null;kind=$null} }
    return @{keyExists=[bool]$script:keys[$id];exists=$true;value=$entry.value;kind=$entry.kind}
}
function Set-ContextHandlerValue {
    param([string]$Hive,[string]$Key,[string]$Value)
    if ($script:failRestore) { throw 'Mock restoration failure.' }
    $id = "$Hive|$Key"
    if (-not $script:keys[$id] -or $script:values.ContainsKey($id)) { throw 'Unexpected restore target.' }
    $script:values[$id] = @{value=$Value;kind='String'}
}
function Remove-ContextHandlerValue {
    param([string]$Hive,[string]$Key,[string]$Expected)
    $script:removeCount++
    if ($script:failRemoveAt -eq $script:removeCount) { throw 'Mock removal failure.' }
    $id = "$Hive|$Key"
    if (-not $script:keys[$id] -or $script:values[$id].value -ine $Expected) { throw 'Unexpected removal target.' }
    $script:values.Remove($id)
}
function Add-Fixture([string]$Item) {
    $spec = Get-ContextItemSpec $Item
    foreach ($path in $spec.Paths) {
        foreach ($hive in @('LocalMachine','CurrentUser')) {
            $id = "$hive|$path"; $script:keys[$id] = $true; $script:values[$id] = @{value=$spec.Id;kind='String'}
        }
    }
}
function Expect-Failure([scriptblock]$Action) { $failed=$false; try { & $Action | Out-Null } catch { $failed=$true }; if (-not $failed) { throw 'Expected failure.' } }

foreach ($item in @('IncludeLibrary','GiveAccess','Share')) {
    Reset-Fixture
    Add-Fixture $item
    $before = $script:values.Count
    $receipt = Set-ContextItemVisibility $item hide
    if (-not $receipt.verified -or -not $receipt.item.owned -or $script:values.Count -ne 0) { throw "Hide failed: $item" }
    if ($script:keys.Count -ne $before) { throw 'A registry key was deleted.' }
    $receipt = Set-ContextItemVisibility $item restore
    if (-not $receipt.verified -or $receipt.item.conflict -or $script:values.Count -ne $before) { throw "Restore failed: $item" }
}
Reset-Fixture
Add-Fixture GiveAccess
$script:failRemoveAt = 2
Expect-Failure { Set-ContextItemVisibility GiveAccess hide }
$state = Get-ContextItemState GiveAccess
if (-not $state.pending -or $state.conflict) { throw 'Interrupted change was not recoverable.' }
$script:failRemoveAt = 0
if (-not (Set-ContextItemVisibility GiveAccess restore).verified -or $script:values.Count -ne 12) { throw 'Partial change was not restored.' }
Reset-Fixture
Add-Fixture IncludeLibrary
Set-ContextItemVisibility IncludeLibrary hide | Out-Null
$script:sid = 'S-1-5-21-456'
if (-not (Get-ContextItemState IncludeLibrary).otherAccount) { throw 'Different account was not detected.' }
Expect-Failure { Set-ContextItemVisibility IncludeLibrary restore }
Reset-Fixture
Add-Fixture Share
$path = (Get-ContextItemSpec Share).Paths[0]
$script:values["LocalMachine|$path"].value = '{00000000-0000-0000-0000-000000000000}'
Expect-Failure { Set-ContextItemVisibility Share hide }
Reset-Fixture
Add-Fixture IncludeLibrary
Set-ContextItemVisibility IncludeLibrary hide | Out-Null
$path = (Get-ContextItemSpec IncludeLibrary).Paths[0]
$script:values["LocalMachine|$path"] = @{value='{00000000-0000-0000-0000-000000000000}';kind='String'}
Expect-Failure { Set-ContextItemVisibility IncludeLibrary restore }
if ($script:values["LocalMachine|$path"].value -ne '{00000000-0000-0000-0000-000000000000}') { throw 'External registration was overwritten.' }
Reset-Fixture
Add-Fixture Share
Set-ContextItemVisibility Share hide | Out-Null
$script:failRestore = $true
Expect-Failure { Set-ContextItemVisibility Share restore }
if (-not (Get-ContextItemState Share).pending) { throw 'Failed restore lost its backup.' }
Reset-Fixture
$script:admin = $false
Expect-Failure { Set-ContextItemVisibility Share hide }
Expect-Failure { Set-ContextItemVisibility '../other' hide }
Write-Output 'Right-click visibility fixture tests passed.'
