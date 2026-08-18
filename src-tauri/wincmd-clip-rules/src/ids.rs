// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rule identifier.
//!
//! `fleet-proto` has zero `uuid`/`chrono` dependency by design — its crate
//! doc comment keeps the dependency closure small for the AV-scanned Free
//! binary, and every id/timestamp on its wire is a plain `String` (see
//! `DeviceId`, `OrgId`). `fleet-proto` depends on this crate and re-exports
//! its rule types to the fleet console, so `RuleId` must stay usable there
//! without dragging the `uuid` crate into that closure. Hence: a validated
//! newtype over `String`, not `uuid::Uuid` — and the validation below is
//! hand-rolled (a hex/hyphen shape check) rather than borrowed from a crate,
//! for the same dependency-budget reason.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A rule identifier: a canonical UUID rendered as either the hyphenated
/// form (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`, 36 chars) or the hyphenless
/// form (32 lowercase hex chars) — matching D-5's `IR-<uuid-simple>` ticket
/// id convention's own alphabet, minus the `IR-` prefix (rules aren't
/// tickets). Lowercase only: accepting both cases would let the same UUID
/// round-trip through two textually-different `RuleId`s, which would let a
/// rule dodge `CooldownLedger`'s per-id cooldown by re-casing its id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts", type = "string"))]
pub struct RuleId(String);

/// Why a candidate string was rejected as a `RuleId`. Carries no copy of the
/// rejected text — this type's `Display` can end up in a log line or a
/// `BadRequest` body, and the crate-wide reverse-leak rule (plan §8) says
/// never echo caller-controlled text back out, so the shape of the mistake
/// is reported, never the mistake itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleIdError {
    /// Length wasn't 32 (hyphenless) or 36 (hyphenated).
    WrongLength,
    /// Right length, but a char outside lowercase `[0-9a-f]` (plus hyphens
    /// at the four fixed 8-4-4-4-12 positions for the 36-char form).
    InvalidShape,
}

impl fmt::Display for RuleIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleIdError::WrongLength => {
                write!(f, "rule id must be 32 (hyphenless) or 36 (hyphenated) characters")
            }
            RuleIdError::InvalidShape => write!(
                f,
                "rule id must be a lowercase hex UUID, hyphenated or hyphenless"
            ),
        }
    }
}

impl std::error::Error for RuleIdError {}

/// `true` for `0-9` / lowercase `a-f` only — deliberately excludes
/// uppercase hex so `RuleId::new` can enforce the "lowercase only" rule
/// documented on the type.
fn is_lower_hex(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f')
}

impl RuleId {
    /// Validate and construct. Accepts canonical lowercase UUID text,
    /// hyphenated (`8-4-4-4-12`) or hyphenless (32 hex chars); rejects
    /// everything else, including uppercase hex — canonicalize first if the
    /// source might be mixed-case.
    pub fn new(s: impl Into<String>) -> Result<Self, RuleIdError> {
        let s = s.into();
        let bytes = s.as_bytes();
        match bytes.len() {
            32 => {
                if bytes.iter().all(|&b| is_lower_hex(b)) {
                    Ok(Self(s))
                } else {
                    Err(RuleIdError::InvalidShape)
                }
            }
            36 => {
                let hyphens_ok =
                    bytes[8] == b'-' && bytes[13] == b'-' && bytes[18] == b'-' && bytes[23] == b'-';
                let hex_ok = bytes.iter().enumerate().all(|(i, &b)| {
                    matches!(i, 8 | 13 | 18 | 23) || is_lower_hex(b)
                });
                if hyphens_ok && hex_ok {
                    Ok(Self(s))
                } else {
                    Err(RuleIdError::InvalidShape)
                }
            }
            _ => Err(RuleIdError::WrongLength),
        }
    }

    /// Borrow the underlying text — e.g. to hand to a `fleet-proto`
    /// `String`-typed wire field.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// Deserialize is hand-written (rather than derived) so a `RuleId` can never
// come into existence, even off the wire, without passing `RuleId::new`'s
// shape check. `Serialize` is plain derive — emitting the inner string back
// out never needs validation.
impl<'de> Deserialize<'de> for RuleId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        RuleId::new(s).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_hyphenated_lowercase() {
        assert!(RuleId::new("0e8f1a2b-3c4d-5e6f-7a8b-9c0d1e2f3a4b").is_ok());
    }

    #[test]
    fn accepts_hyphenless_lowercase() {
        assert!(RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").is_ok());
    }

    #[test]
    fn rejects_uppercase() {
        assert_eq!(
            RuleId::new("0E8F1A2B-3C4D-5E6F-7A8B-9C0D1E2F3A4B"),
            Err(RuleIdError::InvalidShape)
        );
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(RuleId::new("not-a-uuid"), Err(RuleIdError::WrongLength));
    }

    #[test]
    fn rejects_misplaced_hyphens() {
        // Right length and alphabet, hyphens shifted by one — the hyphen
        // positions are load-bearing, not just "36 chars of hex-or-hyphen".
        assert_eq!(
            RuleId::new("0e8f1a2b3-c4d-5e6f-7a8b-9c0d1e2f3a4b"),
            Err(RuleIdError::InvalidShape)
        );
    }

    #[test]
    fn deserialize_rejects_invalid_shape() {
        let result: Result<RuleId, _> = serde_json::from_str("\"not-a-uuid\"");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_error_never_echoes_the_bad_string() {
        // Sentinel that would be unmistakable if leaked into the error text.
        const SENTINEL: &str = "SENTINEL_MARKER_zzz_not_a_uuid";
        let result: Result<RuleId, _> = serde_json::from_str(&format!("\"{SENTINEL}\""));
        let err = result.expect_err("sentinel is not a valid RuleId");
        let rendered = err.to_string();
        assert!(!rendered.contains(SENTINEL), "error echoed the rejected text: {rendered}");
    }

    #[test]
    fn round_trips_through_json() {
        let id = RuleId::new("0e8f1a2b-3c4d-5e6f-7a8b-9c0d1e2f3a4b").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        let back: RuleId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
