// SPDX-License-Identifier: AGPL-3.0-or-later
use std::cell::RefCell;
use std::collections::BTreeMap;

use super::*;
use wincmd_shared::svc::MachineSettingId;

#[derive(Default)]
struct FakeWindowsState {
    values: RefCell<BTreeMap<&'static str, u32>>,
    lock: RefCell<bool>,
    ignore_writes: bool,
}

impl WindowsState for FakeWindowsState {
    fn set_dword(&self, name: &'static str, value: u32) -> Result<(), ()> {
        if !self.ignore_writes {
            self.values.borrow_mut().insert(name, value);
        }
        Ok(())
    }

    fn read_dword(&self, name: &'static str) -> Result<Option<u32>, ()> {
        Ok(self.values.borrow().get(name).copied())
    }

    fn set_rdp_lock(&self, locked: bool) -> Result<(), ()> {
        if !self.ignore_writes {
            *self.lock.borrow_mut() = locked;
        }
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
#[test]
fn lock_must_not_succeed_when_windows_keeps_it_unlocked() {
    let state = FakeWindowsState {
        ignore_writes: true,
        ..Default::default()
    };
    assert!(apply_with(
        &state,
        ApplyMachineSettingRequest {
            setting: MachineSettingId::RdpLock,
            value: MachineSettingValue::RdpLock { locked: true },
        }
    )
    .is_err());
}

#[test]
fn unlock_must_not_succeed_when_a_block_rule_remains() {
    let state = FakeWindowsState {
        lock: RefCell::new(true),
        ignore_writes: true,
        ..Default::default()
    };
    assert!(apply_with(
        &state,
        ApplyMachineSettingRequest {
            setting: MachineSettingId::RdpLock,
            value: MachineSettingValue::RdpLock { locked: false },
        }
    )
    .is_err());
}

#[test]
fn missing_registry_readback_is_not_success_for_either_enable_or_disable() {
    for enabled in [true, false] {
        let state = FakeWindowsState {
            ignore_writes: true,
            ..Default::default()
        };
        assert!(
            apply_with(
                &state,
                ApplyMachineSettingRequest {
                    setting: MachineSettingId::RdpIncoming,
                    value: MachineSettingValue::RdpIncoming {
                        enabled,
                        idle_timeout_seconds: 900
                    },
                }
            )
            .is_err(),
            "enabled={enabled}"
        );
    }
}
