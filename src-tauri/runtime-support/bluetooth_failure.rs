// SPDX-License-Identifier: AGPL-3.0-or-later
//! Only fixed Bluetooth stages cross the service/desktop error boundary.

pub fn describe(source: &str) -> &'static str {
    if source.contains("[BT_OPERATION_BUSY]") {
        "[BT_OPERATION_BUSY] Another Bluetooth operation is finishing. State will be checked automatically; retry when it finishes."
    } else if source.contains("ACCESS_DENIED]") || source.contains("[BT_ADMIN_REQUIRED]") {
        "[BT_ACCESS_DENIED] Windows denied the Bluetooth change. Check administrator approval and device policy."
    } else if source.contains("RESTART_REQUIRED]") {
        "[BT_RESTART_REQUIRED] Windows requires an adapter restart. The Bluetooth change is not confirmed; check device status."
    } else if source.contains("TRANSITION_TIMEOUT]") {
        "[BT_TRANSITION_TIMEOUT] Windows has not confirmed the adapter transition. State will be checked automatically."
    } else if source.contains("[BT_STATE_CHANGED]")
        || source.contains("[BT_DEVICE_UNAVAILABLE]")
        || source.contains("DEVICE_CHANGED]")
    {
        "[BT_DEVICE_CHANGED] The Bluetooth adapter changed or disconnected. Reconnect it and retry after its state is checked."
    } else if source.contains("[BT_DISABLE_") {
        "[BT_DISABLE_REJECTED] Windows rejected the Bluetooth block. Check the adapter in Windows Device Manager before retrying."
    } else if source.contains("[BT_ENABLE_") {
        "[BT_ENABLE_REJECTED] Windows rejected Bluetooth restoration. Check the adapter in Windows Device Manager before retrying."
    } else if source.contains("[BT_STATE_UNVERIFIED]") || source.contains("[BT_POLICY_UNVERIFIED]")
    {
        "[BT_STATE_UNVERIFIED] Bluetooth recovery state or device policy could not be verified. No uncertain adapter will be enabled; contact your administrator."
    } else {
        "[BT_SERVICE_UNAVAILABLE] The local service could not confirm Bluetooth state. WinCommander will check again automatically; no completed change is assumed."
    }
}

pub fn support_reference(source: &str) -> String {
    let stage = describe(source)
        .split(']')
        .next()
        .unwrap_or("[BT_SERVICE_UNAVAILABLE");
    let native_code = source.split("Windows code 0x").nth(1).and_then(|tail| {
        let code = tail.get(..8)?;
        code.bytes()
            .all(|byte| byte.is_ascii_hexdigit())
            .then_some(code)
    });
    match native_code {
        Some(code) => format!("{stage}] Windows code 0x{code}"),
        None => format!("{stage}]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_stage_survives_transport_without_exposing_raw_details() {
        let message = describe("service rejected request: bluetooth_unverified ([BT_DISABLE_ACCESS_DENIED] private-details 0x00000033) [operation 1]");
        assert!(message.starts_with("[BT_ACCESS_DENIED]"));
        assert!(!message.contains("private-details"));
        assert!(!message.contains("0x00000033"));
        assert!(describe("[BT_ENABLE_TRANSITION_TIMEOUT]").starts_with("[BT_TRANSITION_TIMEOUT]"));
        assert!(describe("[BT_DISABLE_DEVICE_CHANGED]").starts_with("[BT_DEVICE_CHANGED]"));
    }

    #[test]
    fn unknown_transport_and_raw_errors_stay_bounded() {
        for source in ["private-path-or-identity", "", "[BT_UNRECOGNIZED]"] {
            assert!(describe(source).starts_with("[BT_SERVICE_UNAVAILABLE]"));
            assert!(!describe(source).contains("private-path"));
        }
        assert_eq!(
            support_reference("[BT_DISABLE_ACCESS_DENIED] private-path (Windows code 0x00000033.)"),
            "[BT_ACCESS_DENIED] Windows code 0x00000033"
        );
        assert_eq!(
            support_reference("private-details Windows code 0xNOT-A-CODE"),
            "[BT_SERVICE_UNAVAILABLE]"
        );
    }
}
