// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fixed Bluetooth support stages and numeric Windows CONFIGRET only. Never
//! include device identities, paths, registry contents or Windows error strings.
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CR_ACCESS_DENIED, CR_INVALID_DEVNODE, CR_INVALID_FLAG, CR_NEED_RESTART, CR_NOT_DISABLEABLE,
    CR_NO_SUCH_DEVNODE,
};

pub(super) fn native_rejected(enabled: bool, result: u32) -> String {
    let operation = if enabled { "ENABLE" } else { "DISABLE" };
    let (reason, guidance) = match result {
        CR_ACCESS_DENIED => (
            "ACCESS_DENIED",
            "Windows denied the Bluetooth adapter change. Check administrator and device-policy permissions; no protection was bypassed.",
        ),
        CR_NOT_DISABLEABLE => (
            "NOT_DISABLEABLE",
            "Windows reports that this Bluetooth adapter cannot be changed now. Keep another input method available and review device status.",
        ),
        CR_NEED_RESTART => (
            "RESTART_REQUIRED",
            "Windows reports that this adapter change requires a device restart. WinCommander has not confirmed the change; review device status before taking further action.",
        ),
        CR_INVALID_FLAG => (
            "FLAGS_REJECTED",
            "Windows rejected the requested Bluetooth operation mode. Refresh status and use this reference when reporting the problem.",
        ),
        CR_NO_SUCH_DEVNODE | CR_INVALID_DEVNODE => (
            "DEVICE_CHANGED",
            "The Bluetooth adapter changed or disconnected. Reconnect it, then refresh status before retrying.",
        ),
        _ => (
            "REJECTED",
            "Windows rejected the Bluetooth adapter change. Refresh status and check device status before retrying.",
        ),
    };
    // A fixed-width integer from the native API is safe support evidence. No
    // native message or caller-controlled text is interpolated into this error.
    format!("[BT_{operation}_{reason}] {guidance} (Windows code 0x{result:08X}.)")
}

pub(super) fn transition_timeout(enabled: bool) -> String {
    let operation = if enabled { "ENABLE" } else { "DISABLE" };
    format!("[BT_{operation}_TRANSITION_TIMEOUT] Windows accepted the Bluetooth request but did not confirm the expected adapter state in time. Refresh status before retrying; no completed change is being claimed.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_denial_preserves_stage_and_numeric_reference_without_raw_errors() {
        for enabled in [false, true] {
            let message = native_rejected(enabled, CR_ACCESS_DENIED);
            let operation = if enabled { "ENABLE" } else { "DISABLE" };
            assert!(message.starts_with(&format!("[BT_{operation}_ACCESS_DENIED]")));
            assert!(message.contains("0x00000033"));
            assert!(message.len() < 400);
        }
    }

    #[test]
    fn known_native_failure_stages_have_distinct_safe_next_actions() {
        for (code, reference) in [
            (CR_NOT_DISABLEABLE, "NOT_DISABLEABLE"),
            (CR_NEED_RESTART, "RESTART_REQUIRED"),
            (CR_INVALID_FLAG, "FLAGS_REJECTED"),
            (CR_NO_SUCH_DEVNODE, "DEVICE_CHANGED"),
            (CR_INVALID_DEVNODE, "DEVICE_CHANGED"),
        ] {
            assert!(native_rejected(false, code).starts_with(&format!("[BT_DISABLE_{reference}]")));
        }
    }

    #[test]
    fn unrecognized_native_result_is_bounded_numeric_evidence() {
        let message = native_rejected(false, u32::MAX);
        assert!(message.starts_with("[BT_DISABLE_REJECTED]"));
        assert!(message.contains("0xFFFFFFFF"));
        assert!(message.len() < 300);
    }

    #[test]
    fn accepted_but_unverified_is_distinct_from_rejected_and_never_success() {
        for enabled in [false, true] {
            let message = transition_timeout(enabled);
            assert!(message.contains("TRANSITION_TIMEOUT]"));
            assert!(message.contains("did not confirm"));
            assert!(!message.contains("Windows code"));
        }
    }
}
