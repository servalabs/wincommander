// SPDX-License-Identifier: AGPL-3.0-or-later
//! Canonical JSON strings preserve integer precision for every consumer.
use serde::{Deserialize, Deserializer, Serializer};

fn digits(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit())
        && (value.len() == 1 || !value.starts_with('0'))
}

pub(super) mod unsigned {
    use super::*;
    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(decoder: D) -> Result<u64, D::Error> {
        let value = String::deserialize(decoder)?;
        if !digits(&value) {
            return Err(serde::de::Error::custom(
                "expected canonical unsigned decimal string",
            ));
        }
        value
            .parse()
            .map_err(|_| serde::de::Error::custom("decimal counter exceeds u64"))
    }
}

pub(super) mod epoch {
    use super::*;
    pub fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(decoder: D) -> Result<i64, D::Error> {
        let value = String::deserialize(decoder)?;
        let canonical = match value.strip_prefix('-') {
            Some(magnitude) => magnitude != "0" && digits(magnitude),
            None => digits(&value),
        };
        if !canonical {
            return Err(serde::de::Error::custom(
                "expected canonical signed epoch decimal string",
            ));
        }
        value
            .parse()
            .map_err(|_| serde::de::Error::custom("epoch exceeds i64"))
    }
}
