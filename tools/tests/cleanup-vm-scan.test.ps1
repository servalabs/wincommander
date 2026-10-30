$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-VirtualMachineArtifactsInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:scenario = 'empty'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($LiteralPath -like '*VMware Workstation' -and $script:scenario -in @('mru', 'mru-denied')) {
        return [pscustomobject]@{ PSPath=$LiteralPath; Property=@('Recent0') }
    }
    if ($LiteralPath -like '*Virtual Machines' -or $LiteralPath -like '*VirtualBox VMs') {
        return [pscustomobject]@{ PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory }
    }
    throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path.')
}
function Get-ItemPropertyValue {
    param([string]$LiteralPath, [string]$Name, $ErrorAction)
    if ($script:scenario -eq 'mru-denied') { throw [UnauthorizedAccessException]::new('Synthetic registry denial.') }
    return 'C:\Synthetic\VM\machine.vmx'
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Directory, [string]$Filter, [switch]$Force, $ErrorAction)
    if ($LiteralPath -like '*Virtual Machines' -and $Filter -eq '*-0000*.vmdk' -and $script:scenario -ne 'empty') {
        [pscustomobject]@{ Name='vm-000001.vmdk'; FullName="$LiteralPath\vm-000001.vmdk"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    }
    if ($LiteralPath -like '*Virtual Machines' -and $Filter -eq '*.vmsn' -and $script:scenario -eq 'partial') { throw [UnauthorizedAccessException]::new('Synthetic denial.') }
}
$result = Get-VirtualMachineArtifactsInfo
if ($result.partial -or $result.total -ne 0 -or $result.coverage -ne 'limited') { throw 'An accessible empty inventory must retain its limited scope.' }
$script:scenario = 'partial'; $result = Get-VirtualMachineArtifactsInfo
if (-not $result.partial -or $result.status -ne 'partial' -or $result.total -ne 1 -or $result.files[0].name -notlike '*vm-000001.vmdk') { throw 'A denied source must preserve captured rows and report a partial inventory.' }
$script:scenario = 'mru'; $result = Get-VirtualMachineArtifactsInfo
$mru = @($result.files | Where-Object { $_.key -eq 'Recent0' })
if ($mru.Count -ne 1 -or $mru[0].value -ne 'C:\Synthetic\VM\machine.vmx' -or -not $mru[0].registryPath) { throw 'A readable VMware recent value must remain visible.' }
$script:scenario = 'mru-denied'; $result = Get-VirtualMachineArtifactsInfo
$mru = @($result.files | Where-Object { $_.key -eq 'Recent0' })
if (-not $result.partial -or $mru.Count -ne 1 -or $mru[0].readStatus -ne 'unavailable') { throw 'An unreadable VMware recent value must retain its key and report partial coverage.' }
Write-Output 'Passed: VM scan preserves captured rows and marks inaccessible sources incomplete.'
