// SPDX-License-Identifier: AGPL-3.0-or-later
//! Machine-owned Bluetooth adapter restriction; never removes paired devices.
//! Reconciliation is reactive, not a guarantee against traffic before hotplug detection.

#[path = "bluetooth_control/engine.rs"]
mod engine;
#[cfg(windows)]
#[path = "bluetooth_control/journal.rs"]
mod journal;
#[cfg(windows)]
#[path = "bluetooth_control/security.rs"]
mod security;
#[cfg(windows)]
#[path = "bluetooth_control/windows.rs"]
mod windows;

pub use engine::BluetoothStatus;

/// Observe the current machine restriction and verified Windows adapter state.
pub fn get() -> Result<BluetoothStatus, String> {
    #[cfg(windows)]
    {
        engine::status(&mut windows::Platform)
    }
    #[cfg(not(windows))]
    {
        Err("Bluetooth control is available only on Windows.".into())
    }
}

/// Caller must also enforce interactive confirmation before this privileged boundary.
pub fn set_internal(blocked: bool) -> Result<BluetoothStatus, String> {
    #[cfg(windows)]
    {
        windows::require_admin()?;
        let _guard = security::MachineLock::acquire()?;
        engine::set(&mut windows::Platform, blocked)
    }
    #[cfg(not(windows))]
    {
        let _ = blocked;
        Err("Bluetooth control is available only on Windows.".into())
    }
}

/// Service-owned worker enforces only an already-persisted machine restriction.
pub fn reconcile() -> Result<BluetoothStatus, String> {
    #[cfg(windows)]
    {
        windows::require_admin()?;
        let _guard = security::MachineLock::acquire()?;
        engine::reconcile(&mut windows::Platform)
    }
    #[cfg(not(windows))]
    {
        Err("Bluetooth control is available only on Windows.".into())
    }
}
