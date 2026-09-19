// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::os::windows::process::ExitStatusExt;
use std::process::{Command, ExitStatus};

fn output(code: u32, bytes: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(code),
        stdout: bytes.to_vec(),
        stderr: Vec::new(),
    }
}

#[test]
fn query_errors_and_malformed_output_are_not_unlocked_observations() {
    assert!(parse(&output(1, b"0")).is_err());
    for bytes in [b"".as_slice(), b"warning\n0", b"True", b"0\n1", &[0xff]] {
        assert!(parse(&output(0, bytes)).is_err());
    }
    assert_eq!(parse(&output(0, b" 0\r\n")), Ok(false));
    assert_eq!(parse(&output(0, b"1\r\n")), Ok(true));
}

// Both firewall cmdlets are local functions. No test calls Windows firewall APIs.
fn mocked_query(rules: &str, ports: &str) -> Output {
    let script = format!(
        r#"
$PSModuleAutoloadingPreference = 'None'
function Get-NetFirewallRule {{
    [CmdletBinding()] param($PolicyStore, $Name)
    $fixtures = @({rules})
    foreach ($fixture in $fixtures) {{ if (!$Name -or $fixture.Name -eq $Name) {{ $fixture }} }}
}}
function Get-NetFirewallPortFilter {{ [CmdletBinding()] param($AssociatedNetFirewallRule) {ports} }}
{SCRIPT}
"#
    );
    Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .unwrap()
}

const VALID_RULE: &str = "[pscustomobject]@{ Name = '{generated-id}'; DisplayName = 'WC-LockRDP'; Direction = 'Inbound'; Action = 'Block'; Enabled = 'True'; Profile = 'Any' }";
const VALID_PORT: &str = "[pscustomobject]@{ Protocol = 'TCP'; LocalPort = '3389' }";

#[test]
fn netsh_display_name_with_a_generated_internal_name_is_recognized() {
    let result = mocked_query(VALID_RULE, VALID_PORT);
    assert_eq!(
        parse(&result),
        Ok(true),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn properties_from_different_rules_cannot_combine_into_a_false_lock() {
    let rules = "[pscustomobject]@{ Name='WC-LockRDP'; DisplayName='WC-LockRDP'; Direction='Inbound'; Action='Allow'; Enabled='True'; Profile='Any' }; [pscustomobject]@{ Name='WC-LockRDP'; DisplayName='WC-LockRDP'; Direction='Outbound'; Action='Block'; Enabled='True'; Profile='Any' }";
    assert_eq!(parse(&mocked_query(rules, VALID_PORT)), Ok(false));
}

#[test]
fn disabled_profile_limited_wrong_port_and_unrelated_rules_are_not_the_lock() {
    for rules in [
        VALID_RULE.replace("'True'", "'False'"),
        VALID_RULE.replace("'Any'", "'Private'"),
        VALID_RULE.replace("'WC-LockRDP'", "'Other'"),
    ] {
        assert_eq!(parse(&mocked_query(&rules, VALID_PORT)), Ok(false));
    }
    assert_eq!(
        parse(&mocked_query(
            VALID_RULE,
            "[pscustomobject]@{ Protocol='UDP'; LocalPort='3389' }"
        )),
        Ok(false)
    );
    assert_eq!(
        parse(&mocked_query(
            VALID_RULE,
            "[pscustomobject]@{ Protocol='TCP'; LocalPort='3390' }"
        )),
        Ok(false)
    );
}

#[test]
fn provider_and_port_filter_failures_remain_errors() {
    assert!(parse(&mocked_query("throw 'provider unavailable'", VALID_PORT)).is_err());
    assert!(parse(&mocked_query(VALID_RULE, "throw 'port query denied'")).is_err());
}

#[test]
fn a_successful_empty_query_is_an_unlocked_observation() {
    assert_eq!(parse(&mocked_query("", VALID_PORT)), Ok(false));
}
