// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local activation receipt shared by the driver stager and service guard.
use serde::{Deserialize, Serialize};

pub const ACTIVATION_RECORD_PATH: &str =
    r"C:\ProgramData\WinCommander\bin\engine\driver-activation.json";
pub const ACTIVATION_LOCK_PATH: &str =
    r"C:\ProgramData\WinCommander\bin\engine\driver-activation.lock";
pub const MAX_RECORD_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivationStatus {
    PendingReboot,
    Active,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationRecord {
    pub schema_version: u32,
    pub sha256: String,
    pub boot_id: String,
    pub status: ActivationStatus,
}

impl ActivationRecord {
    pub fn new(sha256: &str, boot_id: &str, status: ActivationStatus) -> Result<Self, String> {
        let record = Self {
            schema_version: 1,
            sha256: sha256.to_ascii_lowercase(),
            boot_id: boot_id.to_ascii_lowercase(),
            status,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err("driver activation record is oversized".into());
        }
        let record: Self = serde_json::from_slice(bytes)
            .map_err(|_| "driver activation record is invalid".to_string())?;
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|_| "driver activation record is invalid".into())
    }

    fn validate(&self) -> Result<(), String> {
        let hex = |value: &str| {
            value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        let valid_boot = self.boot_id.len() == 36
            && self.boot_id.bytes().enumerate().all(|(index, b)| {
                if [8, 13, 18, 23].contains(&index) {
                    b == b'-'
                } else {
                    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
                }
            })
            && self.boot_id.bytes().any(|b| b != b'0' && b != b'-');
        if self.schema_version != 1 || self.sha256.len() != 64 || !hex(&self.sha256) || !valid_boot
        {
            return Err("driver activation record is invalid".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const BOOT: &str = "12345678-1234-5678-9abc-def012345678";

    #[test]
    fn strict_record_roundtrips_and_normalizes_constructor_inputs() {
        let record = ActivationRecord::new(
            &"AB".repeat(32),
            &BOOT.to_uppercase(),
            ActivationStatus::PendingReboot,
        )
        .unwrap();
        let bytes = record.encode().unwrap();
        assert_eq!(ActivationRecord::parse(&bytes).unwrap(), record);
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], "pendingReboot");
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["sha256"], "ab".repeat(32));
    }

    #[test]
    fn rejects_unknown_duplicate_missing_and_noncanonical_authority() {
        let record =
            ActivationRecord::new(&"ab".repeat(32), BOOT, ActivationStatus::Active).unwrap();
        let original = serde_json::to_value(record).unwrap();
        for (field, value) in [
            ("schemaVersion", serde_json::json!(2)),
            (
                "bootId",
                serde_json::json!("00000000-0000-0000-0000-000000000000"),
            ),
            ("bootId", serde_json::json!(BOOT.to_uppercase())),
            ("sha256", serde_json::json!("ab")),
            ("status", serde_json::json!("ready")),
            ("driverPath", serde_json::json!("C:\\other.sys")),
        ] {
            let mut altered = original.clone();
            altered[field] = value;
            assert!(ActivationRecord::parse(&serde_json::to_vec(&altered).unwrap()).is_err());
        }
        let mut missing = original.clone();
        missing.as_object_mut().unwrap().remove("bootId");
        assert!(ActivationRecord::parse(&serde_json::to_vec(&missing).unwrap()).is_err());
        let duplicate =
            serde_json::to_string(&original)
                .unwrap()
                .replacen('{', "{\"schemaVersion\":1,", 1);
        assert!(ActivationRecord::parse(duplicate.as_bytes()).is_err());
        assert!(ActivationRecord::parse(&vec![b' '; MAX_RECORD_BYTES + 1]).is_err());
    }
}
