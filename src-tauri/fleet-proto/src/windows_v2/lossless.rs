// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{decode::parse_lossless, hash_hex, LosslessValue};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Exact JSON text, transported as a string to preserve numbers in every
/// language. Verify its enclosing signature before interpreting it; do not
/// parse/reserialize it to reconstruct signing bytes. Persist this exact text
/// separately from any parsed/JSONB validation view; storage round-trips must
/// not change whitespace, escaping, object order or numeric spelling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(transparent)]
pub struct LosslessJson(String);

impl LosslessJson {
    pub fn new(text: String) -> Result<Self, serde_json::Error> {
        parse_lossless(text.as_bytes())?;
        Ok(Self(text))
    }
    /// Only for newly constructed trusted values. Ingest external/signed JSON
    /// with new(raw_text); a previously parsed Value may already have changed
    /// numeric spelling or private-property semantics and cannot restore it.
    pub fn from_value(value: &Value) -> Result<Self, serde_json::Error> {
        Self::new(serde_json::to_string(value)?)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Inspect verified JSON using feature-independent semantics. Do not feed
    /// the text to serde_json::Value: its number/private-key interpretation can
    /// vary with other crates' enabled features. Use explicit checked domain
    /// conversion from this AST, retaining the original text for signatures.
    pub fn parse_value(&self) -> Result<LosslessValue, serde_json::Error> {
        parse_lossless(self.0.as_bytes())
    }
}

impl<'de> Deserialize<'de> for LosslessJson {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(decoder)?).map_err(serde::de::Error::custom)
    }
}

/// Digest the exact result representation, distinguishing absent from JSON null.
pub fn windows_result_hash(result: Option<&LosslessJson>) -> String {
    hash_hex(&crate::canonical_json_bytes(&serde_json::json!([
        "fleet.windows.result.v2",
        result
    ])))
}
