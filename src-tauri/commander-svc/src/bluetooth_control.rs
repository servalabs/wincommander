// SPDX-License-Identifier: AGPL-3.0-or-later
//! The existing service owns Bluetooth enforcement even when the GUI is closed.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use wincmd_shared::svc::BluetoothSetBlockedRequest;

#[path = "../../runtime-support/bluetooth_failure.rs"]
mod failure;

static LAST_RECONCILE: Mutex<Option<Instant>> = Mutex::new(None);

fn monitoring() -> bool {
    LAST_RECONCILE
        .lock()
        .ok()
        .and_then(|time| *time)
        .is_some_and(|time| time.elapsed() < Duration::from_secs(15))
}

pub(crate) async fn run() {
    let mut last_failed = false;
    loop {
        let result = tokio::task::spawn_blocking(crate::bluetooth_control_core::reconcile).await;
        let failed = !matches!(result, Ok(Ok(_)));
        if !failed {
            if let Ok(mut time) = LAST_RECONCILE.lock() {
                *time = Some(Instant::now());
            }
        } else if !last_failed {
            // Never log device identities or the journal contents.
            let message = match &result {
                Ok(Err(error)) => failure::support_reference(error),
                _ => failure::support_reference(""),
            };
            eprintln!("[bluetooth] {message}");
        }
        last_failed = failed;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

pub(crate) async fn handle(
    verb: &str,
    args: Value,
    caller_privileged: bool,
) -> Result<Value, String> {
    let blocked = validate_request(verb, args, caller_privileged)?;
    if blocked == Some(true) && !monitoring() {
        return Err(
            "[BT_SERVICE_UNAVAILABLE] Bluetooth enforcement is not ready. Wait for the service before retrying."
                .into(),
        );
    }
    let status = tokio::task::spawn_blocking(move || match blocked {
        Some(value) => crate::bluetooth_control_core::set_internal(value),
        None => crate::bluetooth_control_core::get(),
    })
    .await
    .map_err(|_| "[BT_SERVICE_UNAVAILABLE] Bluetooth checking did not finish. State will be checked automatically.".to_string())??;
    let mut result = serde_json::to_value(status)
        .map_err(|_| "Bluetooth status could not be reported.".to_string())?;
    let monitoring = monitoring();
    let can_change = can_change(&result, caller_privileged, monitoring);
    result["canChange"] = Value::Bool(can_change);
    result["monitoring"] = Value::Bool(monitoring);
    Ok(result)
}

fn can_change(status: &Value, caller_privileged: bool, monitoring: bool) -> bool {
    status["canChange"].as_bool().unwrap_or(false)
        && caller_privileged
        && (monitoring || status["blocked"] == true || status["state"] == "recovery_required")
}

fn validate_request(
    verb: &str,
    args: Value,
    caller_privileged: bool,
) -> Result<Option<bool>, String> {
    match verb {
        "svc.bluetooth.status" if args.as_object().is_some_and(|value| value.is_empty()) => {
            Ok(None)
        }
        "svc.bluetooth.set_blocked" if caller_privileged => {
            serde_json::from_value::<BluetoothSetBlockedRequest>(args)
                .map(|request| Some(request.blocked))
                .map_err(|_| "Bluetooth request is invalid. Refresh before retrying.".into())
        }
        "svc.bluetooth.set_blocked" => {
            Err("Administrator approval is required to change Bluetooth for this PC.".into())
        }
        _ => Err("Bluetooth request is invalid.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn allow_recovery_stays_available_when_enforcement_heartbeat_is_stale() {
        let recovery = json!({"canChange":true,"blocked":false,"state":"recovery_required"});
        assert!(can_change(&recovery, true, false));
        assert!(!can_change(&recovery, false, false));
        let allowed = json!({"canChange":true,"blocked":false,"state":"allowed"});
        assert!(!can_change(&allowed, true, false));
        assert!(can_change(&allowed, true, true));
    }

    #[test]
    fn normal_users_cannot_mutate_even_if_called_after_dispatch() {
        assert!(
            validate_request("svc.bluetooth.set_blocked", json!({"blocked":true}), false).is_err()
        );
        assert!(validate_request("svc.bluetooth.status", json!({}), false).is_ok());
    }

    #[test]
    fn callers_cannot_select_arbitrary_devices_or_supply_coerced_values() {
        for args in [
            json!({"blocked":true,"instanceId":"anything"}),
            json!({"blocked":"true"}),
            json!({}),
        ] {
            assert!(validate_request("svc.bluetooth.set_blocked", args, true).is_err());
        }
        assert_eq!(
            validate_request("svc.bluetooth.set_blocked", json!({"blocked":false}), true).unwrap(),
            Some(false)
        );
        assert!(validate_request("svc.bluetooth.status", json!({"blocked":true}), true).is_err());
    }
}
