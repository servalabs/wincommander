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

/// Decode signed enrollment JSON only when its numeric and object-key
/// semantics are independent of serde_json's optional precision features.
/// Original bytes must still be retained for the transport receipt.
pub fn decode_windows_enrollment_safe<T: DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, serde_json::Error> {
    let value = parse_lossless(bytes)?;
    let mut pending = vec![&value];
    while let Some(node) = pending.pop() {
        match node {
            LosslessValue::Object(object) => {
                for (key, child) in object.iter() {
                    if key.as_str() == "$serde_json::private::Number" {
                        return Err(serde_json::Error::custom(
                            "reserved JSON key in signed enrollment",
                        ));
                    }
                    pending.push(child);
                }
            }
            LosslessValue::Array(array) => pending.extend(array.iter()),
            LosslessValue::Number(number) => {
                let lexical = number.as_str();
                let integer = lexical
                    .parse::<i64>()
                    .map_err(|_| serde_json::Error::custom("unsafe signed enrollment number"))?;
                if !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&integer)
                    || integer.to_string() != lexical
                {
                    return Err(serde_json::Error::custom("unsafe signed enrollment number"));
                }
            }
            _ => {}
        }
    }
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

#[cfg(test)]
mod enrollment_tests {
    use super::*;

    #[test]
    fn signed_enrollment_rejects_feature_dependent_json_values() {
        for input in [
            r#"{"x":{"$serde_json::private::Number":"1"}}"#,
            r#"{"x":{"\u0024serde_json::private::Number":"1"}}"#,
            r#"{"x":1.0}"#,
            r#"{"x":1e0}"#,
            r#"{"x":-0}"#,
            r#"{"x":9007199254740992}"#,
            r#"{"x":-9007199254740992}"#,
            r#"{"x":1,"x":2}"#,
        ] {
            assert!(
                decode_windows_enrollment_safe::<serde_json::Value>(input.as_bytes()).is_err(),
                "{input}"
            );
        }
        for input in [
            r#"{"x":0}"#,
            r#"{"x":-42}"#,
            r#"{"x":9007199254740991}"#,
            r#"{"x":[true,null,{"value":"1.0"}]}"#,
        ] {
            assert!(
                decode_windows_enrollment_safe::<serde_json::Value>(input.as_bytes()).is_ok(),
                "{input}"
            );
        }
    }
}
