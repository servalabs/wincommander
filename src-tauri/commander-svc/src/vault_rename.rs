//! `svc.vault.rename` and `svc.vault.display_names`: a Vault's name lives only in
//! the service's records; the container file itself is never renamed.

use std::collections::HashSet;
use std::path::Path;

use wincmd_shared::vault_access::{is_supported_vault_device_path, normalize_vault_display_name, VaultAccess};

use super::vault_password_change::{failure_code, policy_change_allowed};
use super::{
    peer_has_active_interactive_session, AuthenticatedPipePeer, VerbError, PERSONAL_VAULT_SESSION_ABSENT,
    PERSONAL_VAULT_UNAUTHORIZED,
};
use crate::vault_access::{VaultAccessStore, VaultError};
use crate::vault_mount::VaultMountBroker;

pub(super) const VAULT_NOT_REGISTERED: &str = "vault_not_registered";

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RenameRequest {
    pub(super) container_path: String,
    pub(super) name: String,
}

pub(super) fn validate_request(args: &serde_json::Value) -> Option<RenameRequest> {
    let object = args.as_object()?;
    if object.keys().any(|key| key != "container_path" && key != "name") {
        return None;
    }
    let container_path = object.get("container_path")?.as_str()?.trim();
    if !Path::new(container_path).is_absolute() && !is_supported_vault_device_path(container_path) {
        return None;
    }
    Some(RenameRequest {
        container_path: container_path.to_owned(),
        name: normalize_vault_display_name(object.get("name")?.as_str()?)?,
    })
}

fn unauthorized() -> VerbError {
    VerbError::new(PERSONAL_VAULT_UNAUTHORIZED, "caller cannot rename this Vault")
}

fn identity_changed() -> VerbError {
    VerbError::new("vault_policy_identity_changed", "the selected container is not the registered Vault")
}

fn refusal(error: VaultError) -> VerbError {
    match error {
        VaultError::ContainerIdentity => identity_changed(),
        VaultError::Forbidden => unauthorized(),
        VaultError::Validation => VerbError::new("vault_validation_failed", "the selected container could not be found"),
        VaultError::Persistence => VerbError::new("vault_rename_not_saved", "the Vault name was not saved"),
        _ => VerbError::new("vault_policy_unavailable", "the selected Vault record could not be verified"),
    }
}

/// `write_grant` reports the caller's (allowed, access) for a policy entry.
pub(super) fn rename_for_caller(
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    caller_sid: &str,
    session_id: u32,
    request: &RenameRequest,
    write_grant: &dyn Fn(&str) -> (bool, Option<VaultAccess>),
) -> Result<serde_json::Value, VerbError> {
    let resolve = || vault_access.rename_target(&request.container_path, caller_sid, session_id);
    let target = resolve().map_err(refusal)?.ok_or_else(|| {
        VerbError::new(VAULT_NOT_REGISTERED, "WinCommander has no record of this container to hold a name")
    })?;
    if let Some(entry_id) = &target.entry_id {
        let (granted, access) = write_grant(entry_id);
        if !policy_change_allowed(&target, granted, access) {
            return Err(unauthorized());
        }
    }
    // Held under the mount lock so no mount can start between the check and the save.
    vault_mount.with_exclusive_operation(|| {
        if resolve().ok().flatten().as_ref() != Some(&target) {
            return Err(identity_changed());
        }
        let entry_ids: HashSet<String> = target.entry_id.iter().cloned().collect();
        let identities = HashSet::from([target.container_identity.clone()]);
        vault_mount
            .reject_policy_changes_while_mounted_locked(&entry_ids, &identities)
            .map_err(|reason| VerbError::new(failure_code(reason), "dismount the Vault before renaming it"))?;
        match &target.entry_id {
            Some(entry_id) => vault_access.rename_policy_entry(entry_id, &request.name),
            None => vault_access.rename_personal_vault(&target, caller_sid, &request.name),
        }
        .map_err(refusal)?;
        Ok(serde_json::json!({ "status": "renamed", "name": request.name }))
    })
}

pub(super) fn handle(
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let peer = match peer {
        Some(peer) if !peer.caller_sid().is_empty() && peer_has_active_interactive_session(peer) => peer,
        _ => return Err(VerbError::new(PERSONAL_VAULT_SESSION_ABSENT, "no interactive Windows session")),
    };
    let request = validate_request(&args)
        .ok_or_else(|| VerbError::new("vault_validation_failed", "rename request is invalid"))?;
    rename_for_caller(vault_access, vault_mount, peer.caller_sid(), peer.session_id(), &request, &|entry_id| {
        let authorization = crate::vault_access::authorize_mount_for_token(vault_access, entry_id, peer.token());
        (authorization.allowed, authorization.mode)
    })
}

pub(super) fn handle_display_names(
    vault_access: &VaultAccessStore,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let peer = peer
        .filter(|peer| !peer.caller_sid().is_empty())
        .ok_or_else(|| VerbError::new(PERSONAL_VAULT_SESSION_ABSENT, "no interactive Windows session"))?;
    let names = vault_access.display_names_for_caller(peer.caller_sid(), |entry_id| {
        let authorization = crate::vault_access::authorize_mount_for_token(vault_access, entry_id, peer.token());
        authorization.allowed && authorization.mode == Some(VaultAccess::Write)
    });
    Ok(serde_json::json!({
        "names": names
            .into_iter()
            .map(|(path, name)| serde_json::json!({ "path": path, "name": name }))
            .collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
#[path = "vault_rename_tests.rs"]
mod tests;
