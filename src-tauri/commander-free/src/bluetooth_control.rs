// SPDX-License-Identifier: AGPL-3.0-or-later
//! Machine Bluetooth control always uses the authenticated SYSTEM service.

use serde_json::{json, Value};

#[path = "../../runtime-support/bluetooth_failure.rs"]
mod failure;

pub(crate) fn confirmation_args(blocked: bool) -> String {
    format!("[\"bluetooth_control_set\",{blocked}]")
}

#[tauri::command]
pub async fn bluetooth_control_status() -> Result<Value, String> {
    crate::svc_client::call("svc.bluetooth.status", json!({}))
        .await
        .map_err(|error| failure::describe(&error).to_string())
}

#[tauri::command]
pub async fn bluetooth_control_set(
    blocked: bool,
    capability_token: Option<String>,
) -> Result<Value, String> {
    if !crate::process_privileges::current_process_elevation().unwrap_or(false) {
        return Err("Administrator approval is required to change Bluetooth for this PC.".into());
    }
    crate::authz::consume_required(
        capability_token.as_deref(),
        crate::authz::DestructiveAction::KillSwitch,
        &confirmation_args(blocked),
    )?;
    // A timeout is an unknown outcome. Never automatically replay a mutation.
    crate::svc_client::call("svc.bluetooth.set_blocked", json!({ "blocked": blocked }))
        .await
        .map_err(|error| {
            eprintln!("[bluetooth] {}", failure::support_reference(&error));
            failure::describe(&error).to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_confirmation_cannot_authorize_internet_or_reverse_action() {
        assert_ne!(confirmation_args(true), confirmation_args(false));
        assert_ne!(
            confirmation_args(true),
            crate::authz::kill_switch_args(true)
        );
        assert_eq!(confirmation_args(true), "[\"bluetooth_control_set\",true]");
    }

    #[test]
    fn bluetooth_rejects_missing_or_forged_confirmation() {
        use crate::authz::{consume_required, DestructiveAction};
        let request: crate::authz::DestructiveRequest =
            serde_json::from_value(json!({"command":"bluetooth_control_set","blocked":true}))
                .unwrap();
        assert!(matches!(
            request,
            crate::authz::DestructiveRequest::BluetoothControlSet { blocked: true }
        ));
        assert!(consume_required(
            None,
            DestructiveAction::KillSwitch,
            &confirmation_args(true)
        )
        .is_err());
        assert!(consume_required(
            Some("forged"),
            DestructiveAction::KillSwitch,
            &confirmation_args(true)
        )
        .is_err());
    }
}
