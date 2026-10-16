// SPDX-License-Identifier: AGPL-3.0-or-later
//! Only fixed Bluetooth policy stages cross the service/desktop boundary.

pub fn describe(source: &str) -> &'static str {
    for (code, message) in [
        ("[BT_POLICY_UNSUPPORTED]", "[BT_POLICY_UNSUPPORTED] Windows does not provide a supported Bluetooth policy on this system. No adapter was changed."),
        ("[BT_POLICY_MANAGED]", "[BT_POLICY_MANAGED] Another authority manages Bluetooth policy. Ask its administrator to change it; WinCommander has left it unchanged."),
        ("[BT_POLICY_PENDING]", "[BT_POLICY_PENDING] Windows has not confirmed the Bluetooth policy result. Check policy status before another change."),
        ("[BT_POLICY_REJECTED]", "[BT_POLICY_REJECTED] Windows rejected the Bluetooth policy request. No completed change is assumed."),
        ("[BT_POLICY_UNVERIFIED]", "[BT_POLICY_UNVERIFIED] Windows Bluetooth policy could not be verified. WinCommander will check again automatically."),
        ("[BT_LEGACY_RECOVERY]", "[BT_LEGACY_RECOVERY] A previous device-based Bluetooth change needs administrator review. Policy control will not change or restore adapters."),
        ("[BT_STATE_UNVERIFIED]", "[BT_STATE_UNVERIFIED] WinCommander could not verify its protected Bluetooth policy record. Ask your administrator to review it."),
        ("[BT_OPERATION_BUSY]", "[BT_OPERATION_BUSY] Another Bluetooth policy operation is finishing. Wait for the automatic status check before retrying."),
        ("[BT_ACCESS_DENIED]", "[BT_ACCESS_DENIED] Administrator approval is required to change Bluetooth policy for this PC."),
    ] {
        if source.contains(code) {
            return message;
        }
    }
    "[BT_SERVICE_UNAVAILABLE] The local service could not verify Bluetooth policy. WinCommander will check again automatically."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_stages_survive_transport_without_exposing_raw_details() {
        for code in [
            "BT_POLICY_UNSUPPORTED",
            "BT_POLICY_MANAGED",
            "BT_POLICY_PENDING",
            "BT_POLICY_REJECTED",
            "BT_POLICY_UNVERIFIED",
            "BT_LEGACY_RECOVERY",
            "BT_STATE_UNVERIFIED",
            "BT_OPERATION_BUSY",
            "BT_ACCESS_DENIED",
        ] {
            let message = describe(&format!(
                "service rejected request: [{code}] PRIVATE ID PATH"
            ));
            assert!(message.starts_with(&format!("[{code}]")));
            assert!(!message.contains("PRIVATE"));
        }
    }

    #[test]
    fn unknown_service_and_obsolete_device_stages_stay_bounded() {
        for source in [
            "PRIVATE ID PATH",
            "",
            "[BT_DISABLE_REJECTED]",
            "Windows code 0x00000017",
        ] {
            assert!(describe(source).starts_with("[BT_SERVICE_UNAVAILABLE]"));
            assert!(!describe(source).contains("PRIVATE"));
        }
    }
}
