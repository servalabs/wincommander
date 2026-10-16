// SPDX-License-Identifier: AGPL-3.0-or-later
//! Machine-owned Bluetooth CSP policy. No adapter or radio mutation exists here.

#[path = "bluetooth_control/engine.rs"]
mod engine;
#[cfg(windows)]
#[path = "bluetooth_control/journal.rs"]
mod journal;
#[path = "bluetooth_control/legacy.rs"]
mod legacy;
#[cfg(windows)]
#[path = "bluetooth_control/provider.rs"]
mod provider;
#[cfg(windows)]
#[path = "bluetooth_control/security.rs"]
mod security;

pub use engine::BluetoothStatus;

/// Observe configured and resultant policy; never saves or applies a setting.
pub fn get() -> Result<BluetoothStatus, String> {
    #[cfg(windows)]
    {
        engine::status(&mut provider::Platform::default())
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
        provider::require_admin()?;
        let _guard = security::MachineLock::acquire()?;
        engine::set(&mut provider::Platform::default(), blocked)
    }
    #[cfg(not(windows))]
    {
        let _ = blocked;
        Err("Bluetooth control is available only on Windows.".into())
    }
}
