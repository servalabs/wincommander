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
        || result.as_str().is_some_and(has_legacy_failure)
        || result
            .get("stdout")
            .and_then(Value::as_str)
            .is_some_and(has_legacy_failure)
        || (result.get("success").and_then(Value::as_bool) == Some(true)
            && result
                .get("data")
                .and_then(Value::as_str)
                .is_some_and(has_legacy_failure))
}

fn has_legacy_failure(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.trim_start().starts_with("fail:"))
}

pub(super) fn check_flow_command_receipt(result: Value) -> Result<(), String> {
    if is_explicit_failure(&result) {
        return Err("Backend command reported a failed or partial outcome".into());
    }
    if matches!(
        result.get("status").and_then(Value::as_str),
        Some(
            "pending"
                | "reboot_required"
                | "preference_set_reboot_needed"
                | "unsupported"
                | "not_supported"
                | "partially_applied"
        )
    ) {
        return Err("Backend command did not report a completed outcome".into());
    }
    // Unclassified legacy data retains its existing behavior; this is not OS read-back.
    Ok(())
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

    #[test]
    fn flow_cannot_complete_from_an_explicit_failed_receipt() {
        for value in [
            json!({"error":true}),
            json!({"ok":false, "stdout":"fail: access denied"}),
            json!({"success":false}),
            json!({"status":"blocked"}),
            json!({"status":"partial"}),
        ] {
            assert!(
                check_flow_command_receipt(value.clone()).is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn flow_cannot_complete_from_pending_or_unsupported_receipts() {
        for status in [
            "pending",
            "reboot_required",
            "preference_set_reboot_needed",
            "unsupported",
            "not_supported",
            "partially_applied",
        ] {
            let value = json!({"status":status, "success":true});
            assert!(
                check_flow_command_receipt(value.clone()).is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn legacy_failure_output_cannot_become_applied_state_or_flow_completion() {
        for value in [
            json!("fail: access denied"),
            json!({"ok":true, "stdout":"ok: one item\n  fail: another item"}),
            json!({"success":true, "data":"fail: incomplete"}),
        ] {
            assert!(is_explicit_failure(&value), "{value}");
            assert!(
                check_flow_command_receipt(value.clone()).is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn flow_retains_legacy_success_and_unclassified_domain_data() {
        for value in [
            json!({"status":"enabled"}),
            json!({"success":true}),
            json!({"ok":true, "stdout":"ok: done"}),
            json!({"ok":true, "stdout":"diagnostic mentions fail: without reporting failure"}),
            json!({"status":"authorized_dismounted", "state":"unmounted"}),
            json!({"description":"fail: is diagnostic text"}),
            json!({"data":"fail: is inventory data"}),
            json!([1, 2]),
            json!(null),
        ] {
            assert!(check_flow_command_receipt(value.clone()).is_ok(), "{value}");
        }
    }

    #[test]
    fn flow_failure_does_not_copy_backend_output_into_execution_errors() {
        assert_eq!(
            check_flow_command_receipt(json!({"ok":false, "stdout":"private fixture output"})),
            Err("Backend command reported a failed or partial outcome".into())
        );
    }
}
