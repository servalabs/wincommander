// SPDX-License-Identifier: AGPL-3.0-or-later
//! Neutral data contracts for locally selected encrypted file packages.
//! Tokens identify selections and sessions; the receiving process must still
//! enforce ownership, expiry, entitlement and native filesystem authorization.
use crate::CanonicalDecimal;
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use zeroize::Zeroize;

/// Native-only identity for an object selected by a trusted picker. This is
/// exchanged only over the authenticated backend connection, never returned to
/// the renderer as a substitute for a selection token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageObjectIdentity {
    pub volume: u32,
    pub index: u64,
    pub created: u64,
    pub directory: bool,
    pub parent: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackageToken(String);

impl PackageToken {
    pub fn new(value: String) -> Result<Self, &'static str> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("invalid package token");
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for PackageToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Serialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackagePassword(String);
impl PackagePassword {
    pub fn new(mut value: String) -> Result<Self, &'static str> {
        if value.len() > 1024 {
            value.zeroize();
            return Err("password exceeds byte limit");
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for PackagePassword {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}
impl Drop for PackagePassword {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
impl std::fmt::Debug for PackagePassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub enum PackageCarrier {
    Mp4,
    Png,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub enum PackageEntryKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub enum PackageJobPhase {
    Queued,
    Preparing,
    Writing,
    Finishing,
    Opening,
    Extracting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub enum PackageErrorCode {
    UnsupportedCarrier,
    CapacityExceeded,
    UnsafePath,
    SourceChanged,
    AccessDenied,
    InsufficientSpace,
    SessionExpired,
    UnsupportedElevation,
    Cancelled,
    IncompleteCleanup,
    UnknownOutcome,
    AuthenticationOrDamage,
    UnsupportedVersion,
    LegacyVault,
    MalformedCarrier,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackagePrepareRequest {
    pub source_tokens: Vec<PackageToken>,
    pub cover_token: PackageToken,
}
impl PackagePrepareRequest {
    pub fn valid(&self) -> bool {
        !self.source_tokens.is_empty() && self.source_tokens.len() <= 10_000
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackagePrepared {
    pub plan_id: PackageToken,
    pub carrier: PackageCarrier,
    pub entry_count: u32,
    pub source_bytes: CanonicalDecimal,
    pub envelope_bytes: CanonicalDecimal,
    pub carrier_capacity_bytes: Option<CanonicalDecimal>,
    pub excluded_pixels: Option<CanonicalDecimal>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackageCreateRequest {
    pub plan_id: PackageToken,
    pub output_token: PackageToken,
    pub password: PackagePassword,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackageOpenRequest {
    pub carrier_token: PackageToken,
    pub password: PackagePassword,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackageListRequest {
    pub session_id: PackageToken,
    pub folder_id: Option<PackageToken>,
    pub cursor: Option<PackageToken>,
    pub filter: Option<String>,
    pub limit: u16,
}
impl PackageListRequest {
    pub fn valid(&self) -> bool {
        (1..=200).contains(&self.limit)
            && self
                .filter
                .as_ref()
                .is_none_or(|value| value.chars().count() <= 128)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackageEntry {
    pub id: PackageToken,
    pub name: String,
    pub kind: PackageEntryKind,
    pub size_bytes: CanonicalDecimal,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackagePage {
    pub entries: Vec<PackageEntry>,
    pub next_cursor: Option<PackageToken>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub enum PackageSelection {
    All,
    Entries { ids: Vec<PackageToken> },
}
impl PackageSelection {
    pub fn valid(&self) -> bool {
        match self {
            Self::All => true,
            Self::Entries { ids } => !ids.is_empty() && ids.len() <= 10_000,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackageExtractRequest {
    pub session_id: PackageToken,
    pub destination_token: PackageToken,
    pub selection: PackageSelection,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct PackageJobProgress {
    pub job_id: PackageToken,
    pub phase: PackageJobPhase,
    pub completed_bytes: CanonicalDecimal,
    pub total_bytes: Option<CanonicalDecimal>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_identity_round_trip_keeps_full_width_and_rejects_unknown_fields() {
        let identity = PackageObjectIdentity {
            volume: 7,
            index: u64::MAX,
            created: u64::MAX - 1,
            directory: true,
            parent: true,
        };
        let wire = serde_json::to_string(&identity).unwrap();
        assert_eq!(
            serde_json::from_str::<PackageObjectIdentity>(&wire).unwrap(),
            identity
        );
        assert!(serde_json::from_str::<PackageObjectIdentity>(
            r#"{"volume":7,"index":1,"created":2,"directory":true,"parent":true,"extra":1}"#
        )
        .is_err());
    }
    #[test]
    fn tokens_reject_unbounded_or_noncanonical_values() {
        assert!(PackageToken::new("a0".repeat(32)).is_ok());
        for value in [
            "".to_string(),
            "A".repeat(64),
            "a".repeat(63),
            "z".repeat(64),
        ] {
            assert!(PackageToken::new(value).is_err());
        }
    }
    #[test]
    fn password_round_trip_preserves_unicode_and_redacts_debug() {
        let input = "  e\u{301}密码 ";
        let password: PackagePassword =
            serde_json::from_str(&serde_json::to_string(input).unwrap()).unwrap();
        assert_eq!(password.as_str(), input);
        assert_eq!(format!("{password:?}"), "[redacted]");
        assert!(PackagePassword::new("x".repeat(1025)).is_err());
    }
    #[test]
    fn requests_bound_listing_and_selected_entries() {
        let token = PackageToken::new("ab".repeat(32)).unwrap();
        let mut request = PackageListRequest {
            session_id: token.clone(),
            folder_id: None,
            cursor: None,
            filter: Some("x".repeat(128)),
            limit: 200,
        };
        assert!(request.valid());
        request.limit = 201;
        assert!(!request.valid());
        request.limit = 1;
        request.filter = Some("x".repeat(129));
        assert!(!request.valid());
        assert!(PackageSelection::Entries { ids: vec![token] }.valid());
        assert!(!PackageSelection::Entries { ids: vec![] }.valid());
        assert!(serde_json::from_str::<PackageListRequest>(
            r#"{"session_id":"bad","folder_id":null,"cursor":null,"filter":null,"limit":1}"#
        )
        .is_err());
    }

    #[test]
    fn selection_uses_tagged_wire_shape() {
        assert_eq!(
            serde_json::to_string(&PackageSelection::All).unwrap(),
            r#"{"kind":"all"}"#
        );
        let token = PackageToken::new("ab".repeat(32)).unwrap();
        let encoded = serde_json::to_value(PackageSelection::Entries { ids: vec![token] }).unwrap();
        assert_eq!(encoded["kind"], "entries");
        assert_eq!(encoded["ids"][0], "ab".repeat(32));
    }
}
