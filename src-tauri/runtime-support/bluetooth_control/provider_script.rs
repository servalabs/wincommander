// SPDX-License-Identifier: AGPL-3.0-or-later
//! Only fixed WMI policy class/key/leaf names; no renderer-supplied code or target.
pub(super) const SCRIPT: &str = r#"
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=New-Object Text.UTF8Encoding($false)
function Fail([string]$code){throw [InvalidOperationException]::new($code)}
try {
 if(-not [Security.Principal.WindowsIdentity]::GetCurrent().IsSystem){Fail 'BT_ACCESS_DENIED'}
 Import-Module (Join-Path $PSHOME 'Modules\CimCmdlets\CimCmdlets.psd1') -ErrorAction Stop
 $ns='root\cimv2\mdm\dmmap'
 $configClass='MDM_Policy_Config01_Connectivity02'
 $resultClass='MDM_Policy_Result01_Connectivity02'
 function PolicyClass([string]$name) {
  try {Get-CimClass -Namespace $ns -ClassName $name -OperationTimeoutSec 2}
  catch {
   if($_.Exception -is [Microsoft.Management.Infrastructure.CimException] -and [string]$_.Exception.StatusCode -in @('InvalidNamespace','InvalidClass','NotSupported')){Fail 'BT_POLICY_UNSUPPORTED'}
   throw
  }
 }
 function Observe {
  $os=Get-CimInstance -ClassName Win32_OperatingSystem -OperationTimeoutSec 2
  $client=$os.ProductType -eq 1 -and [int]$os.BuildNumber -ge 10240 -and [int]$os.OperatingSystemSKU -in @(4,27,48,49,69,70,72,84,103,121,122,125,126,129,130,161,162,188,191)
  $server=$os.ProductType -in @(2,3) -and [int]$os.BuildNumber -ge 26100 -and [int]$os.OperatingSystemSKU -gt 0
  if(-not ($client -or $server)){return @{supported=$false;managed=$false;context='';configured=$null;effective=$null}}
  $computer=Get-CimInstance -ClassName Win32_ComputerSystem -OperationTimeoutSec 2
  if($null -eq $computer.PartOfDomain){Fail 'BT_POLICY_UNVERIFIED'}
  if($mdmRegistered -isnot [bool]){Fail 'BT_POLICY_UNVERIFIED'}
  $managed=[bool]$computer.PartOfDomain -or $mdmRegistered
  $azure='Registry::HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\CloudDomainJoin\JoinInfo'
  if(Test-Path -LiteralPath $azure){if(@(Get-ChildItem -LiteralPath $azure).Count -gt 0){$managed=$true}}
  $accounts='Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Provisioning\OMADM\Accounts'
  if(Test-Path -LiteralPath $accounts){if(@(Get-ChildItem -LiteralPath $accounts).Count -gt 0){$managed=$true}}
  $guid=[string](Get-ItemProperty -LiteralPath 'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Cryptography' -Name MachineGuid).MachineGuid
  if([string]::IsNullOrWhiteSpace($guid)){Fail 'BT_POLICY_UNVERIFIED'}
  $material=$guid.ToLowerInvariant()+'|'+$os.ProductType+'|'+$os.BuildNumber+'|'+$os.OperatingSystemSKU+'|'+$managed
  $sha=[Security.Cryptography.SHA256]::Create()
  try{$context=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($material)))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
  foreach($name in @($configClass,$resultClass)){
   $class=PolicyClass $name
   $property=@($class.CimClassProperties|Where-Object {$_.Name -eq 'AllowBluetooth' -and $_.CimType -eq 'SInt32'})
   if($property.Count -ne 1 -or -not $property[0].Qualifiers['read'].Value -or ($name -eq $configClass -and -not $property[0].Qualifiers['write'].Value)){Fail 'BT_POLICY_UNSUPPORTED'}
  }
  $configs=@(Get-CimInstance -Namespace $ns -ClassName $configClass -Filter "ParentID='./Vendor/MSFT/Policy/Config' and InstanceID='Connectivity'" -OperationTimeoutSec 2)
  $results=@(Get-CimInstance -Namespace $ns -ClassName $resultClass -Filter "ParentID='./Vendor/MSFT/Policy/Result' and InstanceID='Connectivity'" -OperationTimeoutSec 2)
  if($configs.Count -gt 1 -or $results.Count -ne 1){Fail 'BT_POLICY_UNVERIFIED'}
  $configured=$null
  if($configs.Count -eq 1){$configured=$configs[0].AllowBluetooth}
  $effective=$results[0].AllowBluetooth
  if(($null -ne $configured -and $configured -notin @(0,2)) -or $null -eq $effective -or $effective -notin @(0,2)){Fail 'BT_POLICY_UNVERIFIED'}
  return @{supported=$true;managed=$managed;context=$context;configured=$configured;effective=$effective}
 }
 $before=Observe
 if($action -ne 'read'){
  if(-not $before.supported){Fail 'BT_POLICY_UNSUPPORTED'}
  if($before.managed -or $before.context -cne $receipt){Fail 'BT_POLICY_MANAGED'}
  $actual=if($null -eq $before.configured){'none'}else{[string]$before.configured}
  if($actual -cne $expected){Fail 'BT_POLICY_MANAGED'}
  $value=if($action -eq 'block' -and $expected -in @('none','2') -and $before.effective -eq 2){0}
   elseif($action -eq 'allow' -and $expected -eq '0' -and $before.effective -eq 0){2}else{Fail 'BT_POLICY_MANAGED'}
  $row=@(Get-CimInstance -Namespace $ns -ClassName $configClass -Filter "ParentID='./Vendor/MSFT/Policy/Config' and InstanceID='Connectivity'" -OperationTimeoutSec 2)
  if($row.Count -eq 0 -and $action -eq 'block' -and $expected -eq 'none'){
   $null=New-CimInstance -Namespace $ns -ClassName $configClass -Property @{ParentID='./Vendor/MSFT/Policy/Config';InstanceID='Connectivity';AllowBluetooth=[int]$value} -OperationTimeoutSec 2
  }elseif($row.Count -eq 1){
   $current=if($null -eq $row[0].AllowBluetooth){'none'}else{[string]$row[0].AllowBluetooth}
   if($current -cne $expected){Fail 'BT_POLICY_MANAGED'}
   $null=Set-CimInstance -InputObject $row[0] -Property @{AllowBluetooth=[int]$value} -OperationTimeoutSec 2
  }else{Fail 'BT_POLICY_MANAGED'}
  $before=Observe
 }
 $before|ConvertTo-Json -Compress
 exit 0
}catch{
 $code=[string]$_.Exception.Message
 if($code -notin @('BT_POLICY_UNSUPPORTED','BT_POLICY_MANAGED','BT_POLICY_UNVERIFIED','BT_ACCESS_DENIED')){$code=if($action -eq 'read'){'BT_POLICY_UNVERIFIED'}else{'BT_POLICY_REJECTED'}}
 [Console]::Out.WriteLine($code)
 exit 1
}
"#;
