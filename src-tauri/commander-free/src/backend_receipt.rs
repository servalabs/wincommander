// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::Value;

pub(super) fn is_explicit_failure(result: &Value) -> bool {
    result.get("error").and_then(Value::as_bool) == Some(true)
        || ["ok", "success"]
            .iter()
            .any(|key| result.get(key).and_then(Value::as_bool) == Some(false))
        || matches!(
            result.get("status").and_then(Value::as_str),
            Some("failed" | "error" | "blocked" | "partial")
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn failure_flags_override_apparent_success() {
        for value in [
            json!({"status":"enabled", "error":true}),
            json!({"status":"applied", "ok":false}),
            json!({"status":"disabled", "success":false}),
            json!({"status":"partial"}),
        ] {
            assert!(is_explicit_failure(&value));
        }
    }

    #[test]
    fn preserves_legacy_success_and_domain_data() {
        for value in [
            json!({"status":"enabled"}),
            json!({"success":true}),
            json!({"error":false}),
            json!({"error":"diagnostic text"}),
            json!([1, 2]),
            json!(null),
        ] {
            assert!(!is_explicit_failure(&value));
        }
    }
}
