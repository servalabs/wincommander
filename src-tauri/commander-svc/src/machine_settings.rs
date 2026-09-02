// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fixed Windows implementations for `svc.apply.machine_setting`.
//!
//! The pipe payload is converted to typed shared enums before entering this
//! module.  Every executable, registry key, value name, firewall rule, and
//! port below is a service-owned constant; no caller text is executed.

use std::process::Command;

use wincmd_shared::svc::{ApplyMachineSettingRequest, MachineSettingObserved, MachineSettingValue};

const TERMINAL_SERVICES_POLICY: &str =
    r"HKLM\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services";
const RDP_LOCK_RULE_NAME: &str = "WC-LockRDP";
const MACHINE_SETTING_APPLY_FAILED: &str = "Windows did not accept the requested machine setting";

trait WindowsState {
    fn set_dword(&self, name: &'static str, value: u32) -> Result<(), ()>;
    fn read_dword(&self, name: &'static str) -> Result<Option<u32>, ()>;
    fn set_rdp_lock(&self, locked: bool) -> Result<(), ()>;
    fn rdp_lock_exists(&self) -> Result<bool, ()>;
}

struct LiveWindowsState;

impl LiveWindowsState {
    fn run(program: &str, args: &[&str]) -> Result<std::process::Output, ()> {
        Command::new(program).args(args).output().map_err(|_| ())
    }

    fn require_success(program: &str, args: &[&str]) -> Result<(), ()> {
        Self::run(program, args).and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(())
            }
        })
    }
}

impl WindowsState for LiveWindowsState {
    fn set_dword(&self, name: &'static str, value: u32) -> Result<(), ()> {
        let value = value.to_string();
        Self::require_success(
            "reg.exe",
            &[
                "add",
                TERMINAL_SERVICES_POLICY,
                "/v",
                name,
                "/t",
                "REG_DWORD",
                "/d",
                &value,
                "/f",
            ],
        )
    }

    fn read_dword(&self, name: &'static str) -> Result<Option<u32>, ()> {
        let output = Self::run("reg.exe", &["query", TERMINAL_SERVICES_POLICY, "/v", name])?;
        if !output.status.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let value = text
            .lines()
            .find_map(|line| {
                let mut fields = line.split_whitespace();
                if fields.next()? != name || fields.next()? != "REG_DWORD" {
                    return None;
                }
                let raw = fields.next()?;
                raw.strip_prefix("0x")
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| raw.parse().ok())
            })
            .ok_or(())?;
        Ok(Some(value))
    }

    fn set_rdp_lock(&self, locked: bool) -> Result<(), ()> {
        let rule_name = format!("name={RDP_LOCK_RULE_NAME}");
        // Delete is idempotent: an absent rule is the desired unlocked state.
        let _ = Self::run(
            "netsh.exe",
            &["advfirewall", "firewall", "delete", "rule", &rule_name],
        );
        if locked {
            Self::require_success(
                "netsh.exe",
                &[
                    "advfirewall",
                    "firewall",
                    "add",
                    "rule",
                    &rule_name,
                    "dir=in",
                    "action=block",
                    "protocol=TCP",
                    "localport=3389",
                    "profile=any",
                    "enable=yes",
                ],
            )?;
        }
        Ok(())
    }

    fn rdp_lock_exists(&self) -> Result<bool, ()> {
        let output = Self::run(
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$rule = Get-NetFirewallRule -Name 'WC-LockRDP' -ErrorAction SilentlyContinue; $port = $rule | Get-NetFirewallPortFilter -ErrorAction SilentlyContinue; if ($rule -and $rule.Direction -eq 'Inbound' -and $rule.Action -eq 'Block' -and $rule.Enabled -eq 'True' -and $port.Protocol -eq 'TCP' -and $port.LocalPort -eq '3389') { '1' } else { '0' }",
            ],
        )?;
        if !output.status.success() {
            return Ok(false);
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim() == "1")
    }
}

pub(crate) fn apply(
    request: ApplyMachineSettingRequest,
) -> Result<MachineSettingObserved, &'static str> {
    apply_with(&LiveWindowsState, request)
}

fn apply_with<S: WindowsState>(
    state: &S,
    request: ApplyMachineSettingRequest,
) -> Result<MachineSettingObserved, &'static str> {
    request
        .validate()
        .map_err(|_| "machine setting request is invalid")?;
    match request.value {
        MachineSettingValue::RdpIncoming {
            enabled,
            idle_timeout_seconds,
        } => {
            let timeout_ms = if enabled {
                idle_timeout_seconds.saturating_mul(1000)
            } else {
                0
            };
            state
                .set_dword("fDenyTSConnections", u32::from(!enabled))
                .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?;
            state
                .set_dword("MaxIdleTime", timeout_ms)
                .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?;
            state
                .set_dword("MaxDisconnectionTime", timeout_ms)
                .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?;
            state
                .set_dword("MaxConnectionTime", timeout_ms)
                .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?;
            state
                .set_dword("fResetBroken", u32::from(enabled))
                .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?;
            read_rdp_incoming(state)
        }
        MachineSettingValue::RdpLock { locked } => {
            state
                .set_rdp_lock(locked)
                .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?;
            Ok(MachineSettingObserved::RdpLock {
                locked: state
                    .rdp_lock_exists()
                    .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?,
            })
        }
    }
}

fn read_rdp_incoming<S: WindowsState>(state: &S) -> Result<MachineSettingObserved, &'static str> {
    let deny_connections = state
        .read_dword("fDenyTSConnections")
        .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?
        .map(|v| v != 0);
    let max_idle_time_ms = state
        .read_dword("MaxIdleTime")
        .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?;
    let max_disconnection_time_ms = state
        .read_dword("MaxDisconnectionTime")
        .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?;
    let max_connection_time_ms = state
        .read_dword("MaxConnectionTime")
        .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?;
    let reset_broken = state
        .read_dword("fResetBroken")
        .map_err(|_| MACHINE_SETTING_APPLY_FAILED)?
        .map(|v| v != 0);
    let enabled = deny_connections == Some(false)
        && max_idle_time_ms.unwrap_or_default() > 0
        && max_disconnection_time_ms == max_idle_time_ms
        && max_connection_time_ms == max_idle_time_ms
        && reset_broken == Some(true);
    Ok(MachineSettingObserved::RdpIncoming {
        enabled,
        deny_connections,
        idle_timeout_seconds: max_idle_time_ms.map(|ms| ms / 1000),
        max_idle_time_ms,
        max_disconnection_time_ms,
        max_connection_time_ms,
        reset_broken,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;
    use wincmd_shared::svc::MachineSettingId;

    #[derive(Default)]
    struct FakeWindowsState {
        values: RefCell<BTreeMap<&'static str, u32>>,
        lock: RefCell<bool>,
    }

    impl WindowsState for FakeWindowsState {
        fn set_dword(&self, name: &'static str, value: u32) -> Result<(), ()> {
            self.values.borrow_mut().insert(name, value);
            Ok(())
        }

        fn read_dword(&self, name: &'static str) -> Result<Option<u32>, ()> {
            Ok(self.values.borrow().get(name).copied())
        }

        fn set_rdp_lock(&self, locked: bool) -> Result<(), ()> {
            *self.lock.borrow_mut() = locked;
            Ok(())
        }

        fn rdp_lock_exists(&self) -> Result<bool, ()> {
            Ok(*self.lock.borrow())
        }
    }

    #[test]
    fn rdp_incoming_is_applied_then_returned_from_read_back() {
        let state = FakeWindowsState::default();
        let observed = apply_with(
            &state,
            ApplyMachineSettingRequest {
                setting: MachineSettingId::RdpIncoming,
                value: MachineSettingValue::RdpIncoming {
                    enabled: true,
                    idle_timeout_seconds: 900,
                },
            },
        )
        .unwrap();
        assert_eq!(
            observed,
            MachineSettingObserved::RdpIncoming {
                enabled: true,
                deny_connections: Some(false),
                idle_timeout_seconds: Some(900),
                max_idle_time_ms: Some(900_000),
                max_disconnection_time_ms: Some(900_000),
                max_connection_time_ms: Some(900_000),
                reset_broken: Some(true),
            }
        );
    }

    #[test]
    fn rdp_lock_is_verified_after_service_mutation() {
        let state = FakeWindowsState::default();
        let observed = apply_with(
            &state,
            ApplyMachineSettingRequest {
                setting: MachineSettingId::RdpLock,
                value: MachineSettingValue::RdpLock { locked: true },
            },
        )
        .unwrap();
        assert_eq!(observed, MachineSettingObserved::RdpLock { locked: true });
    }
}
