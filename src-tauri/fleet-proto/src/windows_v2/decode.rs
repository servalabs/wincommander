// SPDX-License-Identifier: AGPL-3.0-or-later
use json_syntax::JsonParse;
use serde::de::{DeserializeOwned, Error};
use std::collections::HashSet;

/// Feature-independent JSON semantics. Numbers retain their original lexemes;
/// literal private serde property names remain ordinary object properties.
pub use json_syntax::JsonValue as LosslessValue;

/// Validate exact wire bytes before typed decoding. Opaque JSON fields remain
/// strings and are separately validated by LosslessJson, never parsed as Value.
/// Use strongly typed outer DTOs here: choosing serde_json::Value as T would
/// retain that type's feature-dependent semantics. Inspect embedded data using
/// LosslessJson::parse_value instead.
pub fn decode_windows_v2<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    parse_lossless(bytes)?;
    serde_json::from_slice(bytes)
}

pub(super) fn parse_lossless(bytes: &[u8]) -> Result<LosslessValue, serde_json::Error> {
    check_depth(bytes)?;
    let (value, _) = LosslessValue::parse_slice(bytes)
        .map_err(|_| serde_json::Error::custom("invalid JSON syntax"))?;
    let mut pending = vec![&value];
    while let Some(node) = pending.pop() {
        match node {
            LosslessValue::Object(object) => {
                let mut names = HashSet::new();
                for (key, child) in object.iter() {
                    if !names.insert(key.as_str()) {
                        return Err(serde_json::Error::custom("duplicate JSON object key"));
                    }
                    pending.push(child);
                }
            }
            LosslessValue::Array(array) => pending.extend(array.iter()),
            _ => {}
        }
    }
    Ok(value)
}

// Retain the existing serde decoder's nesting budget. The independent parser
// is iterative, but its owned AST drops recursively. This preflight only bounds
// structure; the maintained parser validates grammar, UTF-8 and escapes.
fn check_depth(bytes: &[u8]) -> Result<(), serde_json::Error> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth >= 128 {
                        return Err(serde_json::Error::custom("JSON nesting limit exceeded"));
                    }
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}
