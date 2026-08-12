// SPDX-License-Identifier: AGPL-3.0-or-later
//! F6 Phase-1, Piece 2 — Reboot-to-USB arming predicate.
//!
//! This module provides the gate predicate that the distress-wipe assembly
//! (F6 Phase-1 Piece 3, not yet implemented) will consult before arming the
//! UEFI BootNext + reboot path.
//!
//! # Arming gates (spec §6 gate 2)
//!
//! All three must be true before the USB reboot path is eligible:
//!   1. `self_destruct.enabled == Some(true)` — user has armed self-destruct.
//!   2. `self_destruct.reboot_to_usb_enabled == Some(true)` — user has armed
//!      the USB-boot extension.
//!   3. A `reboot_usb` distress mode is configured — wired in Piece 3.
//!
//! This function covers gates 1 and 2.  Gate 3 is NOT checked here; it is the
//! assembly piece's responsibility so this predicate stays pure + testable.
//!
//! # Safety
//!
//! This function is **pure** — no I/O, no side effects, no destructive calls.
//! It does NOT invoke `BootNext`, trigger a reboot, or call any crypto-erase.

/// Minimal view of `SelfDestructSettings` that this predicate needs.
/// Both `commander-free` (settings.rs) and `commander-pro` can pass their
/// own `SelfDestructSettings` value into this function after converting it.
pub struct SelfDestructRef<'a> {
    /// Mirrors `SelfDestructSettings::enabled`.
    pub enabled: Option<bool>,
    /// Mirrors `SelfDestructSettings::reboot_to_usb_enabled`.
    pub reboot_to_usb_enabled: Option<bool>,
    // Future fields (e.g. provisioning key fingerprint) will go here when
    // Piece 3 wires the third gate.
    pub _phantom: std::marker::PhantomData<&'a ()>,
}

/// Return `true` when gates 1 and 2 are satisfied — self-destruct is armed
/// and the reboot-to-USB extension is explicitly enabled.
///
/// Gate 3 ("a `reboot_usb` distress mode is configured") is the assembly
/// piece's responsibility and is NOT checked here.
pub fn reboot_to_usb_armed(sd: &SelfDestructRef<'_>) -> bool {
    sd.enabled == Some(true) && sd.reboot_to_usb_enabled == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(
        enabled: Option<bool>,
        reboot_to_usb_enabled: Option<bool>,
    ) -> SelfDestructRef<'static> {
        SelfDestructRef {
            enabled,
            reboot_to_usb_enabled,
            _phantom: std::marker::PhantomData,
        }
    }

    #[test]
    fn both_flags_true_returns_true() {
        let sd = make(Some(true), Some(true));
        assert!(reboot_to_usb_armed(&sd));
    }

    #[test]
    fn self_destruct_disabled_returns_false() {
        // reboot_to_usb_enabled is true but self_destruct is off
        let sd = make(Some(false), Some(true));
        assert!(!reboot_to_usb_armed(&sd));
    }

    #[test]
    fn reboot_to_usb_disabled_returns_false() {
        // self_destruct is on but reboot_to_usb_enabled is off
        let sd = make(Some(true), Some(false));
        assert!(!reboot_to_usb_armed(&sd));
    }

    #[test]
    fn both_none_returns_false() {
        let sd = make(None, None);
        assert!(!reboot_to_usb_armed(&sd));
    }

    #[test]
    fn self_destruct_none_returns_false() {
        let sd = make(None, Some(true));
        assert!(!reboot_to_usb_armed(&sd));
    }

    #[test]
    fn reboot_to_usb_none_returns_false() {
        let sd = make(Some(true), None);
        assert!(!reboot_to_usb_armed(&sd));
    }
}
