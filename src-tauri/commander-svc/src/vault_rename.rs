//! `svc.vault.rename`: renames the container file in its own folder, in the
//! caller's token, once its current password opens it, then moves every record
//! naming the old path. `svc.vault.display_names` lists names older versions saved.

use std::path::Path;

use wincmd_shared::vault_access::{is_supported_vault_device_path, VaultAccess};

use super::vault_password_change::{credential_keyfiles, interactive_peer, selected_path, valid_pim, SecretArgs};
use super::vault_password_probe::probe_header;
use super::vault_password_target::{
    failure_code, file_refusal, hold_for_broker, identity_changed, mount_refusal, note_if_hidden, probe_outcome,
    target_refusal, Refusals,
};
use super::{AuthenticatedPipePeer, VerbError, PERSONAL_VAULT_SESSION_ABSENT, PERSONAL_VAULT_UNAUTHORIZED};
use crate::vault_access::{PasswordChangeTarget, RenameFault, TargetLookupError, VaultAccessStore};
use crate::vault_mount::VaultMountBroker;

#[path = "vault_rename_name.rs"]
mod name;

const ALLOWED_KEYS: [&str; 5] = ["container_path", "new_name", "password", "pim", "keyfiles"];

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RenameRequest {
    pub(super) container_path: String,
    pub(super) new_name: String,
    pub(super) keyfiles: Vec<String>,
}

impl Drop for RenameRequest {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.keyfiles.iter_mut().for_each(Zeroize::zeroize);
    }
}

/// Checks the request shape only; it never copies the password out of `args`.
pub(super) fn validate_request(args: &serde_json::Value) -> Option<RenameRequest> {
    let object = args.as_object()?;
    if object.keys().any(|key| !ALLOWED_KEYS.contains(&key.as_str())) || !valid_pim(object.get("pim")) {
        return None;
    }
    let container_path = selected_path(object.get("container_path")?.as_str()?);
    if !Path::new(container_path).is_absolute() || is_supported_vault_device_path(container_path) {
        return None;
    }
    Some(RenameRequest {
        container_path: container_path.to_owned(),
        new_name: object.get("new_name")?.as_str()?.to_owned(),
        keyfiles: credential_keyfiles(object, "password", "keyfiles")?,
    })
}

/// The rename's credential shaped as a password probe, moved (never copied) out of `args`.
fn probe_args(args: &mut serde_json::Value) -> SecretArgs {
    let mut take = |key: &str| args.get_mut(key).map(serde_json::Value::take).unwrap_or_default();
    SecretArgs(serde_json::json!({
        "TargetKind": "file",
        "Password": take("password"),
        "Pim": take("pim"),
        "Keyfiles": take("keyfiles"),
    }))
}

/// A Vault-Permissions Vault may be renamed by its owner, an administrator or a write grant.
pub(super) fn policy_rename_allowed(
    target: &PasswordChangeTarget,
    caller_privileged: bool,
    granted: bool,
    access: Option<VaultAccess>,
) -> bool {
    target.entry_id.is_none()
        || target.caller_owns_entry
        || caller_privileged
        || (granted && access == Some(VaultAccess::Write))
}

fn name_refusal(code: &'static str) -> (&'static str, VerbError) {
    match code {
        "name_unchanged" => ("VLT.RENAME.NAME_UNCHANGED", VerbError::new(code, "the new name is the current name")),
        _ => ("VLT.RENAME.NAME_INVALID", VerbError::new("name_invalid", "Windows cannot use that file name")),
    }
}

fn rename_refusal(fault: RenameFault) -> (&'static str, VerbError) {
    match fault {
        RenameFault::Exists => ("VLT.RENAME.NAME_EXISTS", VerbError::new("name_exists", "a file with that name already exists")),
        RenameFault::File(fault) => file_refusal(fault),
        RenameFault::Identity => identity_changed(),
        RenameFault::Failed => ("VLT.RENAME.FAILED", VerbError::new("vault_rename_not_saved", "Windows did not rename the Vault file")),
    }
}

/// `\\?\D:\x` reads as `D:\x` in the app; the records keep their own spelling.
fn display_path(path: &str) -> String {
    match path.strip_prefix(r"\\?\") {
        Some(unc) if unc.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"UNC\")) => {
            format!(r"\\{}", &unc[4..])
        }
        Some(dos) => dos.to_owned(),
        None => path.to_owned(),
    }
}

/// What a rename asks of Windows, run in the caller's token in production.
pub(super) struct RenameSteps<'a> {
    pub(super) resolve: &'a dyn Fn() -> Result<PasswordChangeTarget, TargetLookupError>,
    /// The caller's (allowed, access) for a policy entry.
    pub(super) grant: &'a dyn Fn(&str) -> (bool, Option<VaultAccess>),
    pub(super) prove_access: &'a dyn Fn(&PasswordChangeTarget) -> Result<(), RenameFault>,
    pub(super) exists: &'a dyn Fn(&str) -> Result<bool, RenameFault>,
    /// (from, identity, to)
    pub(super) rename: &'a dyn Fn(&str, &str, &str) -> Result<(), RenameFault>,
}

/// `probe` answers which header the request's password opens, under the mount lock.
pub(super) fn rename_for_caller(
    log: &Refusals,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    caller_privileged: bool,
    request: &RenameRequest,
    steps: &RenameSteps<'_>,
    probe: impl FnOnce(&PasswordChangeTarget) -> Result<&'static str, VerbError>,
) -> Result<serde_json::Value, VerbError> {
    let target = (steps.resolve)().map_err(|error| log.refuse(target_refusal(error)))?;
    if let Some(entry_id) = &target.entry_id {
        let (granted, access) = (steps.grant)(entry_id);
        if !policy_rename_allowed(&target, caller_privileged, granted, access) {
            return Err(log.refuse(if granted {
                ("VLT.RENAME.POLICY_MANAGED", VerbError::new("vault_policy_managed", "this Vault is managed in Vault Permissions"))
            } else {
                ("VLT.RENAME.NOT_AUTHORIZED", VerbError::new(PERSONAL_VAULT_UNAUTHORIZED, "caller cannot rename this Vault"))
            }));
        }
    }
    let old_name = Path::new(&target.container_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let new_name = name::target_file_name(&old_name, &request.new_name).map_err(|code| log.refuse(name_refusal(code)))?;
    let new_path = Path::new(&target.container_path).with_file_name(&new_name).to_string_lossy().into_owned();
    (steps.prove_access)(&target).map_err(|fault| log.refuse(rename_refusal(fault)))?;
    if (steps.exists)(&new_path).map_err(|fault| log.refuse(rename_refusal(fault)))? {
        return Err(log.refuse(rename_refusal(RenameFault::Exists)));
    }
    // Held under the mount lock so no mount can start between the password check and the rename.
    vault_mount.with_exclusive_operation(|| {
        if (steps.resolve)().ok().as_ref() != Some(&target) {
            return Err(log.refuse(identity_changed()));
        }
        mount_refusal(vault_mount, &target).map_err(|reason| {
            log.record(Some(reason));
            VerbError::new(failure_code(reason), "dismount the Vault before renaming it")
        })?;
        let header = probe(&target)?;
        let header = probe_outcome(header, || vault_access.has_hidden_part(&target.container_identity))
            .map_err(|refusal| log.refuse(refusal))?;
        note_if_hidden(log, vault_access, &target, header == "hidden");
        (steps.rename)(&target.container_path, &target.container_identity, &new_path)
            .map_err(|fault| log.refuse(rename_refusal(fault)))?;
        if vault_access.move_records_for_rename(&target, &new_name).is_err() {
            let restored = (steps.rename)(&new_path, &target.container_identity, &target.container_path).is_ok();
            return Err(log.refuse((
                if restored { "VLT.RENAME.NOT_SAVED" } else { "VLT.RENAME.NOT_SAVED_NOT_RESTORED" },
                VerbError::new("vault_rename_not_saved", "the Vault records could not be updated"),
            )));
        }
        log.record(None);
        Ok(serde_json::json!({ "status": "renamed", "path": display_path(&new_path), "name": new_name }))
    })
}

pub(super) fn handle(
    operation_id: u64,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
    caller_privileged: bool,
) -> Result<serde_json::Value, VerbError> {
    let mut args = SecretArgs(args);
    let log = Refusals::new(operation_id, "rename");
    let peer = interactive_peer(peer, &log)?;
    let Some(request) = validate_request(&args.0) else {
        return Err(log.refuse((
            "VLT.RENAME.REQUEST_INVALID",
            VerbError::new("vault_validation_failed", "rename request is invalid"),
        )));
    };
    let mut credential = probe_args(&mut args.0);
    let token = peer.token();
    let resolve = || {
        vault_access.password_change_target_as_caller(&request.container_path, peer.caller_sid(), peer.session_id(), token)
    };
    let grant = |entry_id: &str| {
        let authorization = crate::vault_access::authorize_mount_for_token(vault_access, entry_id, token);
        (authorization.allowed, authorization.mode)
    };
    let prove_access = |target: &PasswordChangeTarget| crate::vault_access::prove_rename_access_as_caller(target, token);
    let exists = |path: &str| crate::vault_access::target_exists_as_caller(path, token);
    let rename = |from: &str, identity: &str, to: &str| {
        crate::vault_access::rename_container_as_caller(from, identity, to, token)
    };
    let steps = RenameSteps { resolve: &resolve, grant: &grant, prove_access: &prove_access, exists: &exists, rename: &rename };
    rename_for_caller(&log, vault_access, vault_mount, caller_privileged, &request, &steps, |target| {
        // Released before the rename: the pin shares no delete access.
        let _held = hold_for_broker(target, &request.keyfiles, peer).map_err(|refusal| log.refuse(refusal))?;
        probe_header(operation_id, peer, target, &mut credential.0).map_err(|reason| {
            log.record(Some(reason));
            VerbError::new(failure_code(reason), "the password was not checked")
        })
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
