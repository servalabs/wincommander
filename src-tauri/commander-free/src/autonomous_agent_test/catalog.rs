use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) enum Scenario {
    #[serde(rename = "fleet.preflight")]
    FleetPreflight,
    #[serde(rename = "fleet.checkin.readback")]
    FleetCheckinReadback,
    #[serde(rename = "clipboard_guard.synthetic_marker")]
    ClipboardGuardSyntheticMarker,
    #[serde(rename = "privacy_shield.status")]
    PrivacyShieldStatus,
    #[serde(rename = "privacy_shield.start_stop")]
    PrivacyShieldStartStop,
}

impl Scenario {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::FleetPreflight => "fleet.preflight",
            Self::FleetCheckinReadback => "fleet.checkin.readback",
            Self::ClipboardGuardSyntheticMarker => "clipboard_guard.synthetic_marker",
            Self::PrivacyShieldStatus => "privacy_shield.status",
            Self::PrivacyShieldStartStop => "privacy_shield.start_stop",
        }
    }

    pub(super) fn command_binding(self) -> (&'static str, fleet_proto::ActionClass) {
        use fleet_proto::ActionClass::Safe;
        match self {
            Self::FleetPreflight | Self::FleetCheckinReadback => ("mesh.status", Safe),
            Self::ClipboardGuardSyntheticMarker => ("ink_receipt.status", Safe),
            Self::PrivacyShieldStatus => ("endpoint.security_snapshot", Safe),
            Self::PrivacyShieldStartStop => ("policy.reapply", Safe),
        }
    }
}
