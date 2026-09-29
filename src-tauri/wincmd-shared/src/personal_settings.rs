// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opaque personal preferences; the service derives ownership from the pipe token.

pub const READ_PERSONAL_SETTINGS_VERB: &str = "svc.personal_settings.read";
pub const WRITE_PERSONAL_SETTINGS_VERB: &str = "svc.personal_settings.write";
// A legacy 1 MiB profile can expand when its secrets become nested base64 ciphertext.
pub const MAX_PERSONAL_SETTINGS_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadPersonalSettingsRequest {}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalSettingsRecord {
    pub revision: u64,
    pub value: Option<serde_json::Value>,
    pub legacy_recovery_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WritePersonalSettingsRequest {
    pub expected_revision: u64,
    pub value: serde_json::Value,
    pub legacy_recovery_required: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personal_preferences_have_an_explicit_owner_scoped_capability() {
        for verb in [READ_PERSONAL_SETTINGS_VERB, WRITE_PERSONAL_SETTINGS_VERB] {
            assert!(crate::svc::is_known_verb(verb));
            assert_eq!(
                crate::svc::classify_verb(verb),
                crate::svc::CapabilityClass::UserScoped
            );
        }
    }

    #[test]
    fn caller_cannot_supply_an_owner_or_path() {
        for field in ["sid", "callerSid", "path", "key", "admin"] {
            assert!(serde_json::from_value::<ReadPersonalSettingsRequest>(
                serde_json::json!({field: "forged"})
            )
            .is_err());
            assert!(serde_json::from_value::<WritePersonalSettingsRequest>(
                serde_json::json!({"expectedRevision": 0, "value": {},
                    "legacyRecoveryRequired": false, field: "forged"})
            )
            .is_err());
        }
    }
}
