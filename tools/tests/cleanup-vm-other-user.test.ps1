$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
foreach ($name in @('Get-CleanupOtherUserVmInventory','Get-CleanupSummaryAllUsers','New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
$fixtureSid = 'S-1-5-21-999-999-999-998'
$fixtureRoot = "$env:SystemDrive\Users\wc-vm-fixture"
$fixtureHive = "Registry::HKEY_USERS\$fixtureSid"
$mruPath = "$fixtureHive\Software\VMware, Inc.\VMware Workstation"
$vmRoot = "$fixtureRoot\Documents\Virtual Machines"
$boxRoot = "$fixtureRoot\VirtualBox VMs"
$snapshotRoot = "$boxRoot\Snapshots"
$script:mode = 'found'; $script:enumerated = @()
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID=$fixtureSid; LocalPath=$fixtureRoot } }
function Test-WCVisibleUserProfile { return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@('wc-vm-fixture') } }
function Get-Item {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    if ($LiteralPath -eq $fixtureHive) { return [pscustomobject]@{ PSIsContainer=$true } }
    if ($LiteralPath -eq $mruPath) { return [pscustomobject]@{ PSIsContainer=$true; Property=@('Recent1','Other') } }
    if ($LiteralPath -in @((Split-Path -Parent $fixtureRoot), $fixtureRoot, "$fixtureRoot\Documents", $vmRoot, $boxRoot, $snapshotRoot, "$fixtureRoot\.VirtualBox")) {
        if ($LiteralPath -eq $vmRoot -and $script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new() }
        return [pscustomobject]@{ FullName=$LiteralPath; Name=(Split-Path -Leaf $LiteralPath); PSIsContainer=$true; Attributes=if ($LiteralPath -eq $vmRoot -and $script:mode -eq 'link') { 1024 } else { 0 } }
    }
    if ($LiteralPath -eq "$fixtureRoot\.VirtualBox\VirtualBox.xml") { return [pscustomobject]@{ FullName=$LiteralPath; Name='VirtualBox.xml'; PSIsContainer=$false; Attributes=0; Length=2048; LastWriteTime=[datetime]'2026-01-01' } }
    throw [System.Management.Automation.ItemNotFoundException]::new()
}
function Get-ChildItem {
    param([Alias('Path')][string]$LiteralPath, [switch]$Force, [Alias('EA')]$ErrorAction)
    $script:enumerated += $LiteralPath
    if ($LiteralPath -eq $boxRoot) { return [pscustomobject]@{ Name='Snapshots'; FullName=$snapshotRoot; PSIsContainer=$true; Attributes=0 } }
    if ($LiteralPath -eq $snapshotRoot) { return [pscustomobject]@{ Name='snapshot.vdi'; FullName="$snapshotRoot\snapshot.vdi"; PSIsContainer=$false; Attributes=0; Length=1024; LastWriteTime=[datetime]'2026-01-01' } }
    if ($LiteralPath -ne $vmRoot) { throw 'Unexpected enumeration.' }
    @([pscustomobject]@{ Name='machine-000001.vmdk'; FullName="$vmRoot\machine-000001.vmdk"; PSIsContainer=$false; Attributes=0; Length=1024; LastWriteTime=[datetime]'2026-01-01' },
      [pscustomobject]@{ Name='ordinary.vmdk'; FullName="$vmRoot\ordinary.vmdk"; PSIsContainer=$false; Attributes=0; Length=1024; LastWriteTime=[datetime]'2026-01-01' },
      [pscustomobject]@{ Name='redirect'; FullName="$vmRoot\redirect"; PSIsContainer=$true; Attributes=1024 })
}
function Get-ItemPropertyValue { param([Alias('Path')][string]$LiteralPath, [string]$Name, [Alias('EA')]$ErrorAction) if ($LiteralPath -eq $mruPath -and $Name -eq 'Recent1') { return 'D:\external\machine.vmx' }; throw 'Unexpected read.' }
function Get-Command { throw 'No VM provider command may run.' }
$category = (Get-CleanupSummaryAllUsers -CategoryIds 'virtualMachineArtifacts' -TargetUsers $fixtureSid).users[0].categories[0]
if ($category.error -or -not $category.partial -or $category.count -ne 4 -or @($category.records).Count -ne 4) { throw 'Selected-account VM data was not retained as partial.' }
if (@($category.records | Where-Object { $_.path -eq "$vmRoot\machine-000001.vmdk" }).Count -ne 1 -or @($category.records | Where-Object { $_.value -eq 'D:\external\machine.vmx' }).Count -ne 1) { throw 'VM snapshot or recent registry value was lost.' }
if (@($category.records | Where-Object { $_.path -eq "$snapshotRoot\snapshot.vdi" }).Count -ne 1 -or @($category.records | Where-Object { $_.path -eq "$fixtureRoot\.VirtualBox\VirtualBox.xml" }).Count -ne 1) { throw 'VirtualBox snapshot or configuration metadata was lost.' }
if ($script:enumerated -contains "$vmRoot\redirect" -or $script:enumerated -contains 'D:\external\machine.vmx') { throw 'Redirected or external VM paths were traversed.' }
foreach ($mode in @('denied','link')) {
    $script:mode = $mode; $script:enumerated = @()
    $category = (Get-CleanupSummaryAllUsers -CategoryIds 'virtualMachineArtifacts' -TargetUsers $fixtureSid).users[0].categories[0]
    if ($category.error -or -not $category.partial -or $category.count -ne 3 -or $script:enumerated -contains $vmRoot -or $category.message -notmatch 'could not be checked') { throw "Unsafe or inaccurate $mode VM result." }
}
Write-Output 'Passed: selected-account VM metadata is partial and external or redirected paths are not traversed.'
