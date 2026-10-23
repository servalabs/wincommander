// SPDX-License-Identifier: AGPL-3.0-or-later
//! Stable digest of an out-of-band deployment bundle bound by a signed command.

use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn canonical_json_sha256(value: &Value) -> String {
    let digest = Sha256::digest(crate::canonical_json_bytes(value));
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_binds_nested_values_independently_of_object_order() {
        let first: Value = serde_json::from_str(r#"{"z":{"b":2,"a":1},"a":[true,"x"]}"#).unwrap();
        let reordered: Value =
            serde_json::from_str(r#"{"a":[true,"x"],"z":{"a":1,"b":2}}"#).unwrap();
        assert_eq!(
            canonical_json_sha256(&first),
            canonical_json_sha256(&reordered)
        );
        let mut changed = reordered;
        changed["z"]["b"] = serde_json::json!(3);
        assert_ne!(
            canonical_json_sha256(&first),
            canonical_json_sha256(&changed)
        );
    }
}
