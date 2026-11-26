//! The steps a password probe and a password change share: resolve the container
//! in the caller's token, refuse a Vault-Permissions or mounted Vault, hold the
//! files, then call the Pro broker under the mount lock.

use std::collections::HashSet;
use std::time::Instant;

use wincmd_shared::vault_access::VaultMountReason;

use super::{AuthenticatedPipePeer, VerbError, PERSONAL_VAULT_UNAUTHORIZED};
use crate::vault_access::{FileFault, PasswordChangeTarget, TargetLookupError, VaultAccessStore, VaultError};
use crate::vault_mount::VaultMountBroker;

/// Records a refusal under one fixed diagnostic code; never a path or secret.
pub(super) struct Refusals {
    pub(super) diagnostic_id: String,
    pub(super) operation: &'static str,
    pub(super) started: Instant,
}

impl Refusals {
    pub(super) fn new(operation_id: u64, operation: &'static str) -> Self {
        Self { diagnostic_id: format!("VLT-{operation_id}"), operation, started: Instant::now() }
    }

    pub(super) fn refuse(&self, (code, error): (&'static str, VerbError)) -> VerbError {
        crate::diagnostics::record_vault_failure(&self.diagnostic_id, self.operation, code, "review_selected_target", false, self.started);
        error
    }

    pub(super) fn record(&self, failure: Option<VaultMountReason>) {
        crate::diagnostics::record_vault_password_change(&self.diagnostic_id, self.operation, failure, self.started);
    }

    /// The operation itself succeeded; only the hidden-part note was not saved.
    pub(super) fn note_not_saved(&self) {
        crate::diagnostics::record_vault_failure(&self.diagnostic_id, self.operation, "VLT.PASSWORD.NOTE_NOT_SAVED", "none", false, self.started);
    }
}

pub(super) fn real_password_required() -> (&'static str, VerbError) {
    (
        "VLT.PASSWORD.REAL_PASSWORD_REQUIRED",
        VerbError::new("real_password_required", "this vault needs its real password"),
    )
}

pub(super) fn note_unreadable() -> (&'static str, VerbError) {
    (
        "VLT.PASSWORD.NOTE_UNREADABLE",
        VerbError::new("vault_policy_unavailable", "the selected Vault record could not be verified"),
    )
}

/// A noted Vault answers only to its real password; the note is read only for an outer answer.
pub(super) fn probe_outcome(
    header: &'static str,
    noted: impl FnOnce() -> Result<bool, VaultError>,
) -> Result<&'static str, (&'static str, VerbError)> {
    if header == "outer" && noted().map_err(|_| note_unreadable())? {
        return Err(real_password_required());
    }
    Ok(header)
}

/// The hidden credentials sent with a decoy change must open the hidden header themselves.
pub(super) fn hidden_proof_refusal(probed: Result<&'static str, VaultMountReason>) -> Result<(), (&'static str, VerbError)> {
    match probed {
        Ok("hidden") => Ok(()),
        Ok(_) | Err(VaultMountReason::WrongPassword | VaultMountReason::EngineUnlockFailed) => Err(real_password_required()),
        Err(reason) => Err((
            "VLT.PASSWORD.HIDDEN_PROOF_FAILED",
            VerbError::new(failure_code(reason), "the real password was not checked"),
        )),
    }
}

/// Only the two header names leave the service.
pub(super) fn opened_header(reply: &serde_json::Value) -> Option<&'static str> {
    if reply.get("status").and_then(serde_json::Value::as_str) != Some("password_opened") {
        return None;
    }
    match reply.get("header").and_then(serde_json::Value::as_str)? {
        "outer" => Some("outer"),
        "hidden" => Some("hidden"),
        _ => None,
    }
}

pub(super) fn file_refusal(fault: FileFault) -> (&'static str, VerbError) {
    let (diagnostic, message) = match fault {
        FileFault::NotFound => ("VLT.PASSWORD.FILE_NOT_FOUND", "the Vault file could not be found"),
        FileFault::AccessDenied => ("VLT.PASSWORD.FILE_ACCESS_DENIED", "Windows refused this account access to the Vault file"),
        FileFault::InUse => ("VLT.PASSWORD.FILE_IN_USE", "another program is using the Vault file"),
    };
    (diagnostic, VerbError::new(fault.code(), message))
}

pub(super) fn identity_changed() -> (&'static str, VerbError) {
    (
        "VLT.PASSWORD.IDENTITY_CHANGED",
        VerbError::new("vault_policy_identity_changed", "the selected container is not the registered Vault"),
    )
}

pub(super) fn target_refusal(error: TargetLookupError) -> (&'static str, VerbError) {
    match error {
        TargetLookupError::File(fault) => file_refusal(fault),
        TargetLookupError::Vault(VaultError::ContainerIdentity) => identity_changed(),
        TargetLookupError::Vault(VaultError::Forbidden) => (
            "VLT.PASSWORD.NOT_AUTHORIZED",
            VerbError::new(PERSONAL_VAULT_UNAUTHORIZED, "the selected Vault belongs to another account"),
        ),
        TargetLookupError::Vault(VaultError::Validation) => (
            "VLT.PASSWORD.INVALID_TARGET",
            VerbError::new("vault_validation_failed", "the selected container path is invalid"),
        ),
        TargetLookupError::Vault(_) => (
            "VLT.PASSWORD.POLICY_UNAVAILABLE",
            VerbError::new("vault_policy_unavailable", "the selected Vault policy could not be verified"),
        ),
    }
}

/// A Vault-Permissions Vault's owner or an administrator may change it; another
/// grantee learns only that it is managed, a caller without a grant only that it may not.
pub(super) fn policy_refusal(
    target: &PasswordChangeTarget,
    caller_privileged: bool,
    authorized: impl FnOnce(&str) -> bool,
) -> Result<(), (&'static str, VerbError)> {
    let Some(entry_id) = &target.entry_id else {
        return Ok(());
    };
    if target.caller_owns_entry || caller_privileged {
        return Ok(());
    }
    Err(if authorized(entry_id) {
        (
            "VLT.PASSWORD.POLICY_MANAGED",
            VerbError::new("vault_policy_managed", "this Vault is managed in Vault Permissions"),
        )
    } else {
        (
            "VLT.PASSWORD.NOT_AUTHORIZED",
            VerbError::new(PERSONAL_VAULT_UNAUTHORIZED, "caller is not authorized for this Vault"),
        )
    })
}

/// The registry keys a mount by container identity, so an outer and a hidden mount both refuse.
pub(super) fn mount_refusal(vault_mount: &VaultMountBroker, target: &PasswordChangeTarget) -> Result<(), VaultMountReason> {
    let entry_ids: HashSet<String> = target.entry_id.iter().cloned().collect();
    let identities = HashSet::from([target.container_identity.clone()]);
    vault_mount.reject_policy_changes_while_mounted_locked(&entry_ids, &identities)
}

/// Bare codes are the shared contract the app maps to its own wording.
pub(super) fn failure_code(reason: VaultMountReason) -> &'static str {
    match reason {
        VaultMountReason::WrongPassword | VaultMountReason::EngineUnlockFailed => "wrong_password",
        VaultMountReason::NewPasswordCollides => "new_password_collides",
        VaultMountReason::WeakNewPassword => "weak_new_password",
        VaultMountReason::AlreadyMounted => "volume_mounted",
        VaultMountReason::EngineFailed => "engine_failed",
        other => VaultMountBroker::personal_mount_failure_code(other),
    }
}

/// Runs `run` on the resolved container, held and dismounted, under the mount lock.
#[allow(clippy::too_many_arguments)]
pub(super) fn on_held_target<T>(
    log: &Refusals,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    peer: &AuthenticatedPipePeer,
    caller_privileged: bool,
    volume_path: &str,
    keyfiles: &[String],
    run: impl FnOnce(&PasswordChangeTarget) -> Result<T, VerbError>,
) -> Result<T, VerbError> {
    let resolve = || {
        vault_access.password_change_target_as_caller(volume_path, peer.caller_sid(), peer.session_id(), peer.token())
    };
    let target = resolve().map_err(|error| log.refuse(target_refusal(error)))?;
    policy_refusal(&target, caller_privileged, |entry_id| {
        crate::vault_access::authorize_mount_for_token(vault_access, entry_id, peer.token()).allowed
    })
    .map_err(|refusal| log.refuse(refusal))?;
    // Held under the mount lock so no mount can start while the header is read or rewritten.
    vault_mount.with_exclusive_operation(|| {
        if resolve().ok().as_ref() != Some(&target) {
            return Err(log.refuse(identity_changed()));
        }
        if let Err(reason) = mount_refusal(vault_mount, &target) {
            log.record(Some(reason));
            return Err(VerbError::new(failure_code(reason), "dismount the Vault first"));
        }
        let _held = hold_for_broker(&target, keyfiles, peer).map_err(|refusal| log.refuse(refusal))?;
        run(&target)
    })
}

/// Pins the container and keyfiles in the caller's token so the broker cannot be pointed at a substitute.
pub(super) fn hold_for_broker(
    target: &PasswordChangeTarget,
    keyfiles: &[String],
    peer: &AuthenticatedPipePeer,
) -> Result<crate::vault_access::UnmanagedMountFileGuard, (&'static str, VerbError)> {
    crate::vault_access::hold_password_change_files(target, keyfiles, peer.token()).map_err(|error| match error {
        VaultError::ContainerIdentity => identity_changed(),
        _ => match crate::vault_access::container_fault_as_caller(&target.container_path, peer.token()) {
            Some(fault) => file_refusal(fault),
            None => (
                "VLT.PASSWORD.CALLER_ACCESS_DENIED",
                VerbError::new(
                    VaultMountBroker::personal_mount_failure_code(VaultMountReason::CallerAccessDenied),
                    "this account cannot write the container or read its keyfiles",
                ),
            ),
        },
    })
}

/// One call to the caller's session broker on `target`; `args` gets the resolved path
/// and is moved out, so the caller's scrub of its own copy still covers every secret.
pub(super) fn call_broker(
    operation_id: u64,
    peer: &AuthenticatedPipePeer,
    target: &PasswordChangeTarget,
    feature: &'static str,
    args: &mut serde_json::Value,
) -> Result<serde_json::Value, VaultMountReason> {
    args["VolumePath"] = serde_json::Value::String(target.container_path.clone());
    args["TargetSessionId"] = serde_json::Value::from(peer.session_id());
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(crate::pro_broker::VaultCall {
            request_id: operation_id,
            target_session_id: peer.session_id(),
            caller_sid: peer.caller_sid(),
            caller_token: Some(peer.token()),
            caller_authentication_id: Some(peer.authentication_id()),
            presentation: wincmd_shared::vault_access::VaultPresentation::PerUser,
            feature_id: feature,
            args: args.take(),
        }))
    })
}

/// Stores the note when a header answer proved a hidden part; a failed save only logs.
pub(super) fn note_if_hidden(log: &Refusals, vault_access: &VaultAccessStore, target: &PasswordChangeTarget, hidden: bool) {
    if hidden && vault_access.note_hidden_part(target).is_err() {
        log.note_not_saved();
    }
}

#[cfg(test)]
#[path = "vault_password_target_tests.rs"]
mod tests;
