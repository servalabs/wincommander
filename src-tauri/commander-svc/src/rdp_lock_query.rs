// SPDX-License-Identifier: AGPL-3.0-or-later
//! A failed query is not an unlocked observation; each rule is checked alone.

use std::process::Output;

pub(crate) const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
try {
    $rules = @(Get-NetFirewallRule -PolicyStore ActiveStore -ErrorAction Stop)
    $locked = $false
    foreach ($rule in $rules) {
        if ($rule.DisplayName -ne 'WC-LockRDP' -or $rule.Direction -ne 'Inbound' -or
            $rule.Action -ne 'Block' -or $rule.Enabled -ne 'True' -or $rule.Profile -ne 'Any') { continue }
        $ports = @(Get-NetFirewallPortFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop)
        if ($ports.Count -ne 1) { continue }
        $port = $ports[0]
        $localPorts = @($port.LocalPort)
        if (($port.Protocol -eq 'TCP' -or $port.Protocol -eq '6') -and
            $localPorts.Count -eq 1 -and $localPorts[0] -eq '3389') { $locked = $true; break }
    }
    if ($locked) { '1' } else { '0' }
} catch {
    [Console]::Error.WriteLine('Firewall state could not be verified')
    exit 1
}
"#;

pub(crate) fn parse(output: &Output) -> Result<bool, ()> {
    if !output.status.success() {
        return Err(());
    }
    match std::str::from_utf8(&output.stdout).map_err(|_| ())?.trim() {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(()),
    }
}

#[cfg(test)]
#[path = "rdp_lock_query_tests.rs"]
mod tests;
