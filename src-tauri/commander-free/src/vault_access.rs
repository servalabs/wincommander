//! Tauri adapter for the service-owned Vault Access policy.
//!
//! `serde_json::Value` is intentional until generated shared policy types are
//! available in this crate. This module preserves the frozen snake_case wire
//! and contains that temporary adaptation at one boundary.

use serde_json::{json, Value};
use zeroize::Zeroize;

const GET_POLICY: &str = "svc.vault.get_policy";
const APPLY_POLICY: &str = "svc.vault.apply_policy";
const GET_STATUS: &str = "svc.vault.get_status";
const UNMOUNT: &str = "svc.vault.unmount";
const LIST_AUTHORIZED: &str = "svc.vault.list_authorized";
const CAPABILITIES: &str = "svc.vault.capabilities";

#[tauri::command]
pub async fn get_vault_access_policy() -> Result<Value, String> {
    crate::svc_client::call(GET_POLICY, json!({})).await
}

#[tauri::command]
pub async fn apply_vault_access_policy(policy: Value) -> Result<Value, String> {
    // The service performs full shape/principal/identity/ACL validation. The
    // renderer never supplies SIDs, a DACL, caller identity, or observations.
    crate::svc_client::call(APPLY_POLICY, policy).await
}

#[tauri::command]
pub async fn get_vault_access_status() -> Result<Value, String> {
    crate::svc_client::call(GET_STATUS, json!({})).await
}

/// Tauri maps the UI's mount arguments to this snake_case Rust signature.
/// `volume_role` remains optional so installed standard-container clients keep
/// their two-field request contract. Secrets are sent once and cleared from
/// both owned representations before this command returns.
#[tauri::command]
pub async fn vault_mount_entry(
    entry_id: String,
    password: String,
    volume_role: Option<wincmd_shared::vault_access::VaultVolumeRole>,
    hidden_protection_password: Option<String>,
) -> Result<Value, String> {
    let mut request = wincmd_shared::vault_access::VaultMountRequest {
        entry_id,
        password,
        volume_role: volume_role.unwrap_or_default(),
        hidden_protection_password,
    };
    let payload = serde_json::to_value(&request);
    // The service-call payload now owns the only remaining copy. Clear the
    // command-local copy before the potentially long engine wait begins.
    request.password.zeroize();
    crate::svc_client::call_vault_mount(
        payload.map_err(|_| "mount request could not be encoded".to_string())?,
    )
    .await
}

#[tauri::command]
pub async fn vault_unmount_entry(entry_id: String) -> Result<Value, String> {
    crate::svc_client::call(
        UNMOUNT,
        serde_json::to_value(wincmd_shared::vault_access::VaultUnmountRequest { entry_id })
            .map_err(|_| "unmount request could not be encoded".to_string())?,
    )
    .await
}

#[tauri::command]
pub async fn vault_list_authorized_entries() -> Result<Value, String> {
    crate::svc_client::call(LIST_AUTHORIZED, json!({})).await
}

#[tauri::command]
pub async fn get_vault_access_capabilities() -> Result<Value, String> {
    crate::svc_client::call(CAPABILITIES, json!({})).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_verbs_stay_on_the_frozen_service_wire() {
        assert_eq!(GET_POLICY, "svc.vault.get_policy");
        assert_eq!(APPLY_POLICY, "svc.vault.apply_policy");
        assert_eq!(GET_STATUS, "svc.vault.get_status");
        assert_eq!(UNMOUNT, "svc.vault.unmount");
        assert_eq!(LIST_AUTHORIZED, "svc.vault.list_authorized");
        assert_eq!(CAPABILITIES, "svc.vault.capabilities");
    }

    #[test]
    fn mount_command_payload_has_no_service_owned_fields() {
        let value = serde_json::to_value(wincmd_shared::vault_access::VaultMountRequest {
            entry_id: "shared".into(),
            password: "canary".into(),
            volume_role: wincmd_shared::vault_access::VaultVolumeRole::Outer,
            hidden_protection_password: None,
        })
        .unwrap();
        assert_eq!(value.as_object().unwrap().len(), 3);
        assert!(value.get("container_path").is_none());
        assert!(value.get("sid").is_none());
        assert_eq!(
            value.get("volume_role"),
            Some(&serde_json::Value::String("outer".into()))
        );
    }
}
