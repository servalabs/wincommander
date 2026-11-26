//! `svc.vault.rename` and `svc.vault.display_names`: a Vault's name lives only in
//! the service's records; the container file itself is never renamed.

use std::path::Path;

use wincmd_shared::vault_access::{
    is_supported_vault_device_path, normalize_vault_display_name, VaultAccess, VaultMountReason,
};

use super::vault_password_change::selected_path;
use super::vault_password_target::{failure_code, mount_refusal, target_refusal};
use super::{
    peer_has_active_interactive_session, AuthenticatedPipePeer, VerbError, PERSONAL_VAULT_SESSION_ABSENT,
    PERSONAL_VAULT_UNAUTHORIZED,
};
use crate::vault_access::{PasswordChangeTarget, RenameRecord, TargetLookupError, VaultAccessStore, VaultError};
use crate::vault_mount::VaultMountBroker;

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
    let container_path = selected_path(object.get("container_path")?.as_str()?);
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

fn lookup_refusal(error: TargetLookupError) -> VerbError {
    match error {
        TargetLookupError::File(_) => target_refusal(error).1,
        TargetLookupError::Vault(error) => refusal(error),
    }
}

fn write_refusal(error: TargetLookupError) -> VerbError {
    match error {
        TargetLookupError::Vault(VaultError::ContainerIdentity) => identity_changed(),
        TargetLookupError::File(_) => target_refusal(error).1,
        TargetLookupError::Vault(_) => VerbError::new(
            VaultMountBroker::personal_mount_failure_code(VaultMountReason::CallerAccessDenied),
            "this account cannot write the container",
        ),
    }
}

/// A policy Vault needs its owner or a write grant; a personal one is checked
/// by [`VaultAccessStore::password_change_target`].
pub(super) fn policy_change_allowed(
    target: &PasswordChangeTarget,
    granted: bool,
    access: Option<VaultAccess>,
) -> bool {
    target.entry_id.is_none()
        || target.caller_owns_entry
        || (granted && access == Some(VaultAccess::Write))
}

type RenameLookup<'a> = &'a dyn Fn() -> Result<(PasswordChangeTarget, RenameRecord), TargetLookupError>;

/// `resolve` finds the container in the caller's token; `write_grant` reports the
/// caller's (allowed, access) for a policy entry; `prove_write` opens an
/// unregistered container read/write as the caller.
pub(super) fn rename_for_caller(
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    caller_sid: &str,
    request: &RenameRequest,
    resolve: RenameLookup<'_>,
    write_grant: &dyn Fn(&str) -> (bool, Option<VaultAccess>),
    prove_write: &dyn Fn(&PasswordChangeTarget) -> Result<(), TargetLookupError>,
) -> Result<serde_json::Value, VerbError> {
    let (target, record) = resolve().map_err(lookup_refusal)?;
    if let Some(entry_id) = &target.entry_id {
        let (granted, access) = write_grant(entry_id);
        if !policy_change_allowed(&target, granted, access) {
            return Err(unauthorized());
        }
    }
    // Held under the mount lock so no mount can start between the check and the save.
    vault_mount.with_exclusive_operation(|| {
        if !matches!(resolve(), Ok((current, current_record)) if current == target && current_record == record) {
            return Err(identity_changed());
        }
        mount_refusal(vault_mount, &target)
            .map_err(|reason| VerbError::new(failure_code(reason), "dismount the Vault before renaming it"))?;
        match (record, &target.entry_id) {
            (RenameRecord::Policy, Some(entry_id)) => vault_access.rename_policy_entry(entry_id, &request.name),
            (RenameRecord::Personal, None) => vault_access.rename_personal_vault(&target, caller_sid, &request.name),
            (RenameRecord::Unregistered, None) => {
                prove_write(&target).map_err(write_refusal)?;
                vault_access.rename_unregistered_container(&target, caller_sid, &request.name)
            }
            _ => Err(VaultError::Validation),
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
    rename_for_caller(
        vault_access,
        vault_mount,
        peer.caller_sid(),
        &request,
        &|| {
            vault_access.rename_target_as_caller(&request.container_path, peer.caller_sid(), peer.session_id(), peer.token())
        },
        &|entry_id| {
            let authorization = crate::vault_access::authorize_mount_for_token(vault_access, entry_id, peer.token());
            (authorization.allowed, authorization.mode)
        },
        &|target| prove_write_as_caller(target, peer.token()),
    )
}

/// Opens the container read/write in the caller's token; a refusal keeps its OS reason.
pub(super) fn prove_write_as_caller(
    target: &PasswordChangeTarget,
    caller_token: windows_sys::Win32::Foundation::HANDLE,
) -> Result<(), TargetLookupError> {
    crate::vault_access::hold_password_change_files(target, &[], caller_token)
        .map(drop)
        .map_err(|error| match error {
            VaultError::ContainerIdentity => TargetLookupError::Vault(error),
            _ => crate::vault_access::container_fault_as_caller(&target.container_path, caller_token)
                .map_or(TargetLookupError::Vault(error), TargetLookupError::File),
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
        "names": names.into_iter().map(display_name_row).collect::<Vec<_>>(),
    }))
}

fn display_name_row(row: crate::vault_access::VaultDisplayName) -> serde_json::Value {
    serde_json::json!({ "path": row.path, "name": row.name })
}

#[cfg(test)]
#[path = "vault_rename_tests.rs"]
mod tests;
