// SPDX-License-Identifier: AGPL-3.0-or-later
//! The SYSTEM service owns the fixed Bluetooth policy request boundary.

use serde_json::Value;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use wincmd_shared::svc::BluetoothSetBlockedRequest;

type CachedStatus = Option<(Instant, Result<Value, String>)>;

fn status_cache() -> &'static Mutex<CachedStatus> {
    static CACHE: OnceLock<Mutex<CachedStatus>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

async fn observed_status() -> Result<Value, String> {
    let mut cache = status_cache().lock().await;
    if let Some((time, result)) = cache.as_ref() {
        if time.elapsed() < Duration::from_secs(2) {
            return result.clone();
        }
    }
    // Multiple open dashboards share one bounded read. Mutations never use it.
    let result = tokio::task::spawn_blocking(crate::bluetooth_control_core::get)
        .await
        .map_err(|_| "[BT_POLICY_UNVERIFIED] Bluetooth policy checking did not finish.".to_string())
        .and_then(|result| result)
        .and_then(|status| {
            serde_json::to_value(status).map_err(|_| {
                "[BT_POLICY_UNVERIFIED] Bluetooth policy status could not be reported.".to_string()
            })
        });
    *cache = Some((Instant::now(), result.clone()));
    result
}

pub(crate) async fn handle(
    verb: &str,
    args: Value,
    caller_privileged: bool,
) -> Result<Value, String> {
    let blocked = validate_request(verb, args, caller_privileged)?;
    let mut result = if let Some(value) = blocked {
        let result =
            tokio::task::spawn_blocking(move || crate::bluetooth_control_core::set_internal(value))
                .await
                .map_err(|_| {
                    "[BT_POLICY_PENDING] Bluetooth policy change did not confirm.".to_string()
                })
                .and_then(|result| result);
        // Even failed requests may have reached Windows. Discard older reads.
        *status_cache().lock().await = None;
        serde_json::to_value(result?).map_err(|_| {
            "[BT_POLICY_UNVERIFIED] Bluetooth policy status could not be reported.".to_string()
        })?
    } else {
        observed_status().await?
    };
    let can_change = can_change(&result, caller_privileged);
    result["canChange"] = Value::Bool(can_change);
    Ok(result)
}

fn can_change(status: &Value, caller_privileged: bool) -> bool {
    status["canChange"].as_bool().unwrap_or(false) && caller_privileged
}

fn validate_request(
    verb: &str,
    args: Value,
    caller_privileged: bool,
) -> Result<Option<bool>, String> {
    match verb {
        "svc.bluetooth.policy.status" if args.as_object().is_some_and(|value| value.is_empty()) => {
            Ok(None)
        }
        "svc.bluetooth.policy.set_blocked" if caller_privileged => {
            serde_json::from_value::<BluetoothSetBlockedRequest>(args)
                .map(|request| Some(request.blocked))
                .map_err(|_| "[BT_POLICY_REJECTED] Bluetooth policy request is invalid.".into())
        }
        "svc.bluetooth.policy.set_blocked" => {
            Err("[BT_ACCESS_DENIED] Administrator approval is required to change Bluetooth policy for this PC.".into())
        }
        _ => Err("Bluetooth request is invalid.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn caller_privilege_cannot_override_policy_eligibility() {
        let allowed = json!({"schemaVersion":2,"canChange":true,"blocked":false,"state":"allowed"});
        assert!(can_change(&allowed, true));
        assert!(!can_change(&allowed, false));
        let unsupported = json!({"schemaVersion":2,"canChange":false,"state":"unsupported"});
        assert!(!can_change(&unsupported, true));
        assert!(!can_change(&json!({}), true));
    }

    #[test]
    fn normal_users_cannot_mutate_even_if_called_after_dispatch() {
        assert!(
            validate_request("svc.bluetooth.policy.set_blocked", json!({"blocked":true}), false).is_err()
        );
        assert!(validate_request("svc.bluetooth.policy.status", json!({}), false).is_ok());
    }

    #[test]
    fn callers_cannot_select_arbitrary_devices_or_supply_coerced_values() {
        assert!(validate_request("svc.bluetooth.set_blocked", json!({"blocked":true}), true).is_err());
        assert!(validate_request("svc.bluetooth.status", json!({}), true).is_err());
        for args in [
            json!({"blocked":true,"instanceId":"anything"}),
            json!({"blocked":"true"}),
            json!({}),
        ] {
            assert!(validate_request("svc.bluetooth.policy.set_blocked", args, true).is_err());
        }
        assert_eq!(
            validate_request("svc.bluetooth.policy.set_blocked", json!({"blocked":false}), true).unwrap(),
            Some(false)
        );
        assert!(validate_request("svc.bluetooth.policy.status", json!({"blocked":true}), true).is_err());
    }
}
