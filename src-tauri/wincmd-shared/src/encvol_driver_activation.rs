// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local activation receipt shared by the driver stager and service guard.
use serde::{Deserialize, Serialize};

#[cfg(windows)]
mod boot_session;
#[cfg(windows)]
pub use boot_session::current_boot_id;

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
            schema_version: 2,
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

    pub fn is_current_boot(&self, boot_id: &str) -> bool {
        self.schema_version == 2
            && parse_boot_session(&self.boot_id).is_some()
            && self.boot_id == boot_id
    }

    pub fn confirms_later_boot(&self, boot_id: &str) -> bool {
        if self.schema_version != 2 {
            return false;
        }
        match (
            parse_boot_session(&self.boot_id),
            parse_boot_session(boot_id),
        ) {
            (Some((previous_loader, previous)), Some((loader, current))) => {
                previous_loader == loader && current > previous
            }
            _ => false,
        }
    }

    fn validate(&self) -> Result<(), String> {
        let hex = |value: &str| {
            value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        // Schema 1 is readable only so protected stores can replace untrusted history with Pending.
        let valid_boot = match self.schema_version {
            1 => valid_loader_id(&self.boot_id),
            2 => parse_boot_session(&self.boot_id).is_some(),
            _ => false,
        };
        if self.sha256.len() != 64 || !hex(&self.sha256) || !valid_boot {
            return Err("driver activation record is invalid".into());
        }
        Ok(())
    }
}

fn valid_loader_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, b)| {
            if [8, 13, 18, 23].contains(&index) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
        && value.bytes().any(|b| b != b'0' && b != b'-')
}

fn parse_boot_session(value: &str) -> Option<(&str, u32)> {
    let (loader, sequence) = value.split_once('/')?;
    if !valid_loader_id(loader)
        || sequence.len() != 8
        || !sequence
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let sequence = u32::from_str_radix(sequence, 16).ok()?;
    (sequence != 0).then_some((loader, sequence))
}

#[cfg(test)]
mod tests {
    use super::*;
    const BOOT: &str = "12345678-1234-5678-9abc-def012345678/00000039";

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
        assert_eq!(json["schemaVersion"], 2);
        assert_eq!(json["sha256"], "ab".repeat(32));
    }

    #[test]
    fn rejects_unknown_duplicate_missing_and_noncanonical_authority() {
        let record =
            ActivationRecord::new(&"ab".repeat(32), BOOT, ActivationStatus::Active).unwrap();
        let original = serde_json::to_value(record).unwrap();
        for (field, value) in [
            ("schemaVersion", serde_json::json!(3)),
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

    #[test]
    fn only_same_loader_and_strictly_increased_sequence_confirms_a_later_boot() {
        let record =
            ActivationRecord::new(&"ab".repeat(32), BOOT, ActivationStatus::PendingReboot).unwrap();
        assert!(record.is_current_boot(BOOT));
        assert!(!record.confirms_later_boot(BOOT));
        assert!(record.confirms_later_boot("12345678-1234-5678-9abc-def012345678/0000003a"));
        for invalid in [
            "12345678-1234-5678-9abc-def012345678/00000038",
            "12345678-1234-5678-9abc-def012345679/0000003a",
            "12345678-1234-5678-9abc-def012345678/00000000",
            "12345678-1234-5678-9abc-def012345678",
        ] {
            assert!(!record.is_current_boot(invalid));
            assert!(!record.confirms_later_boot(invalid));
        }
        let maximum = ActivationRecord::new(
            &"ab".repeat(32),
            "12345678-1234-5678-9abc-def012345678/ffffffff",
            ActivationStatus::PendingReboot,
        )
        .unwrap();
        assert!(!maximum.confirms_later_boot("12345678-1234-5678-9abc-def012345678/00000001"));
    }

    #[test]
    fn legacy_receipts_are_readable_history_but_never_boot_authority() {
        for status in [ActivationStatus::PendingReboot, ActivationStatus::Active] {
            let legacy = ActivationRecord {
                schema_version: 1,
                sha256: "ab".repeat(32),
                boot_id: BOOT.split_once('/').unwrap().0.into(),
                status,
            };
            let parsed = ActivationRecord::parse(&legacy.encode().unwrap()).unwrap();
            assert!(!parsed.is_current_boot(BOOT));
            assert!(!parsed.confirms_later_boot(BOOT));
            let mut wrong_schema = serde_json::to_value(&legacy).unwrap();
            wrong_schema["schemaVersion"] = serde_json::json!(2);
            assert!(ActivationRecord::parse(&serde_json::to_vec(&wrong_schema).unwrap()).is_err());
        }
    }

    #[test]
    fn malformed_or_ambiguous_session_tokens_are_rejected() {
        for suffix in [
            "0",
            "00000000",
            "0000000A",
            "000000001",
            "0000003a/0000003b",
            "-0000001",
        ] {
            let mut record = serde_json::to_value(
                ActivationRecord::new(&"ab".repeat(32), BOOT, ActivationStatus::Active).unwrap(),
            )
            .unwrap();
            record["bootId"] =
                serde_json::json!(format!("12345678-1234-5678-9abc-def012345678/{suffix}"));
            assert!(ActivationRecord::parse(&serde_json::to_vec(&record).unwrap()).is_err());
        }
    }
}
