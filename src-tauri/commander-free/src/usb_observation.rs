// SPDX-License-Identifier: AGPL-3.0-or-later
use serde_json::Value;
use std::time::Duration;

pub(super) const VOLUME_QUERY: &str = r#"
$ErrorActionPreference='Stop'
$usbDisks=@(Get-Disk | Where-Object { $_.BusType -eq 'USB' } | Select-Object -ExpandProperty Number)
$rows=@(Get-CimInstance Win32_DiskDrive | Where-Object { $_.Index -in $usbDisks } | ForEach-Object {
  $model=$_.Model; $serial=$_.SerialNumber; $node=$_.PNPDeviceID
  $parent=$node
  for($i=0; $i -lt 8 -and $parent -notlike 'USB\VID_*'; $i++) {
    $parent=(Get-PnpDeviceProperty -InstanceId $parent -KeyName 'DEVPKEY_Device_Parent' -ErrorAction Stop).Data
    if([string]::IsNullOrWhiteSpace($parent)) { break }
  }
  if($parent -notlike 'USB\VID_*') { return }
  Get-CimAssociatedInstance -InputObject $_ -Association Win32_DiskDriveToDiskPartition | ForEach-Object {
    Get-CimAssociatedInstance -InputObject $_ -Association Win32_LogicalDiskToPartition | ForEach-Object {
      [pscustomobject]@{driveLetter=$_.DeviceID;label=$_.VolumeName;model=$model;serial=$serial;instanceId=$parent}
    }
  }
})
ConvertTo-Json -InputObject $rows -Compress
"#;

pub(super) async fn volumes() -> Result<Value, String> {
    let mut command = tokio::process::Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", VOLUME_QUERY]);
    command.kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| "USB volume query timed out".to_string())?
        .map_err(|error| format!("USB volume query failed: {error}"))?;
    if !output.status.success() {
        return Err("Windows could not resolve USB volume identities".to_string());
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("USB volume JSON: {error}"))?;
    if !value.is_array() {
        return Err("Windows returned an invalid USB volume list".to_string());
    }
    Ok(value)
}
