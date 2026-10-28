$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\tweaks\maintenance.ps1'
$tokens=$null; $errors=$null
$ast=[System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path),[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw 'Maintenance source did not parse.' }
foreach ($name in @('Invoke-SystemRepair','Invoke-WindowsUpdateRepair','Invoke-Defrag')) {
    $definition=$ast.Find({param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
function Assert-IsAdmin {}
$script:failNative=''; $script:restarted=@(); $script:removed=@(); $script:failOptimize=$false
function chkdsk.exe { $global:LASTEXITCODE=0 }
function sfc.exe { $global:LASTEXITCODE=0 }
function DISM { $global:LASTEXITCODE=if($script:failNative -eq 'DISM'){1}else{0} }
function netsh.exe { $global:LASTEXITCODE=if($script:failNative -eq 'netsh'){1}else{0} }
function Get-Service {
    [CmdletBinding()] param($Name)
    [pscustomobject]@{Status=if($Name -eq 'bits'){'Running'}else{'Stopped'}}
}
function Stop-Service { [CmdletBinding()] param($Name,[switch]$Force) }
function Start-Service { [CmdletBinding()] param($Name) $script:restarted += $Name }
function Test-Path { param($LiteralPath) return $LiteralPath -notin $script:removed }
function Remove-Item { [CmdletBinding()] param($LiteralPath,[switch]$Recurse,[switch]$Force) $script:removed += $LiteralPath }
function Get-Volume { [CmdletBinding()] param() [pscustomobject]@{DriveLetter='Z';DriveType='Fixed'} }
function Optimize-Volume { [CmdletBinding()] param($DriveLetter) if($script:failOptimize){throw 'Mock optimization failure'} }

if ((Invoke-SystemRepair).error) { throw 'Successful mock repair failed.' }
$script:failNative='DISM'
if (-not (Invoke-SystemRepair).error) { throw 'Failed DISM was reported as successful.' }
$script:failNative='netsh'
$update=Invoke-WindowsUpdateRepair
if (-not $update.error -or $script:restarted.Count -ne 1 -or $script:restarted[0] -ne 'bits') { throw 'Failed update repair did not restore the prior running services.' }
if ((Invoke-Defrag).error) { throw 'Successful mock optimization failed.' }
$script:failOptimize=$true
if (-not (Invoke-Defrag).error) { throw 'Failed optimization was reported as successful.' }
Write-Output 'Passed: repair exit failures, service restoration, and optimization outcomes.'
