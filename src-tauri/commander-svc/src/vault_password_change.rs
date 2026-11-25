//! `svc.vault.change_password_personal`: the service picks the container; the Pro broker, in the caller's token, rewrites one header.

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use wincmd_shared::vault_access::{VaultAccess, VaultMountReason};

use super::{
    peer_has_active_interactive_session, zeroize_json, AuthenticatedPipePeer, VerbError,
    PERSONAL_VAULT_SESSION_ABSENT, PERSONAL_VAULT_UNAUTHORIZED,
};
use crate::vault_access::{PasswordChangeTarget, VaultAccessStore, VaultError};
use crate::vault_mount::VaultMountBroker;

const BROKER_FEATURE: &str = "vault.broker.change_password_personal";
const ALLOWED_KEYS: [&str; 9] = [
    "VolumePath",
    "TargetKind",
    "Target",
    "OldPassword",
    "NewPassword",
    "OldPim",
    "NewPim",
    "OldKeyfiles",
    "NewKeyfiles",
];
const MAX_PIM: u64 = 2_147_468;

/// Overwrites every credential string however the handler returns.
struct SecretArgs(serde_json::Value);

impl Drop for SecretArgs {
    fn drop(&mut self) {
        zeroize_json(&mut self.0);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PasswordChangeRequest {
    pub(super) volume_path: String,
    pub(super) target: &'static str,
    pub(super) keyfiles: Vec<String>,
}

impl Drop for PasswordChangeRequest {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.keyfiles.iter_mut().for_each(Zeroize::zeroize);
    }
}

/// Checks the request shape only; it never copies a password out of `args`.
pub(super) fn validate_request(args: &serde_json::Value) -> Option<PasswordChangeRequest> {
    let object = args.as_object()?;
    if object.keys().any(|key| !ALLOWED_KEYS.contains(&key.as_str())) {
        return None;
    }
    let volume_path = object.get("VolumePath")?.as_str()?.trim();
    let is_device = wincmd_shared::vault_access::is_supported_vault_device_path(volume_path);
    let valid_path = match object.get("TargetKind").map(|kind| (kind, kind.as_str())) {
        None | Some((serde_json::Value::Null, _)) | Some((_, Some("" | "file"))) => {
            !is_device && Path::new(volume_path).is_absolute()
        }
        Some((_, Some("device"))) => is_device,
        Some(_) => false,
    };
    if !valid_path {
        return None;
    }
    let target = match object.get("Target") {
        None | Some(serde_json::Value::Null) => "outer",
        Some(serde_json::Value::String(value)) if value == "outer" => "outer",
        Some(serde_json::Value::String(value)) if value == "hidden" => "hidden",
        Some(_) => return None,
    };
    if !valid_pim(object.get("OldPim")) || !valid_pim(object.get("NewPim")) {
        return None;
    }
    let mut request = PasswordChangeRequest {
        volume_path: volume_path.to_owned(),
        target,
        keyfiles: Vec::new(),
    };
    for (password_key, keyfiles_key) in [("OldPassword", "OldKeyfiles"), ("NewPassword", "NewKeyfiles")] {
        let password = object.get(password_key)?.as_str()?;
        let keyfiles = keyfile_paths(object.get(keyfiles_key))?;
        // VeraCrypt accepts keyfiles alone, but never an empty credential.
        if password.is_empty() && keyfiles.is_empty() {
            return None;
        }
        request.keyfiles.extend(keyfiles);
    }
    Some(request)
}

fn valid_pim(value: Option<&serde_json::Value>) -> bool {
    match value {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(text)) if text.is_empty() => true,
        Some(serde_json::Value::String(text)) => text
            .parse::<u64>()
            .is_ok_and(|pim| (1..=MAX_PIM).contains(&pim)),
        Some(serde_json::Value::Number(number)) => number
            .as_u64()
            .is_some_and(|pim| (1..=MAX_PIM).contains(&pim)),
        Some(_) => false,
    }
}

/// The desktop transport sends lists as JSON text; the Pro sidecar may send arrays.
fn keyfile_paths(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let paths: Vec<String> = match value {
        None | Some(serde_json::Value::Null) => return Some(Vec::new()),
        Some(serde_json::Value::String(text)) if text.trim().is_empty() => return Some(Vec::new()),
        Some(serde_json::Value::String(text)) => serde_json::from_str(text).ok()?,
        Some(array @ serde_json::Value::Array(_)) => serde_json::from_value(array.clone()).ok()?,
        Some(_) => return None,
    };
    paths
        .iter()
        .all(|path| Path::new(path).is_absolute())
        .then_some(paths)
}

/// Bare codes are the shared contract the app maps to its own wording.
pub(super) fn failure_code(reason: VaultMountReason) -> &'static str {
    match reason {
        VaultMountReason::WrongPassword | VaultMountReason::EngineUnlockFailed => "wrong_password",
        VaultMountReason::WrongHeader => "wrong_header",
        VaultMountReason::NewPasswordCollides => "new_password_collides",
        VaultMountReason::WeakNewPassword => "weak_new_password",
        VaultMountReason::AlreadyMounted => "volume_mounted",
        VaultMountReason::EngineFailed => "engine_failed",
        other => VaultMountBroker::personal_mount_failure_code(other),
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

fn target_refusal(error: VaultError) -> (&'static str, VerbError) {
    match error {
        VaultError::ContainerIdentity => (
            "VLT.PASSWORD.IDENTITY_CHANGED",
            VerbError::new(
                "vault_policy_identity_changed",
                "the selected container is not the registered Vault",
            ),
        ),
        VaultError::Forbidden => (
            "VLT.PASSWORD.NOT_AUTHORIZED",
            VerbError::new(PERSONAL_VAULT_UNAUTHORIZED, "the selected Vault belongs to another account"),
        ),
        VaultError::Validation => (
            "VLT.PASSWORD.INVALID_TARGET",
            VerbError::new("vault_validation_failed", "the selected container could not be found"),
        ),
        _ => (
            "VLT.PASSWORD.POLICY_UNAVAILABLE",
            VerbError::new("vault_policy_unavailable", "the selected Vault policy could not be verified"),
        ),
    }
}

pub(super) async fn handle(
    operation_id: u64,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let mut args = SecretArgs(args);
    let started = Instant::now();
    let diagnostic_id = format!("VLT-{operation_id}");
    let refuse = |code: &'static str, next: &'static str, error: VerbError| {
        crate::diagnostics::record_vault_failure(&diagnostic_id, "change_password", code, next, false, started);
        error
    };
    let peer = match peer {
        Some(peer) if !peer.caller_sid().is_empty() && peer_has_active_interactive_session(peer) => peer,
        _ => {
            return Err(refuse(
                "VLT.PASSWORD.SESSION_UNAVAILABLE",
                "sign_in_interactively",
                VerbError::new(PERSONAL_VAULT_SESSION_ABSENT, "no interactive Windows session"),
            ))
        }
    };
    let Some(request) = validate_request(&args.0) else {
        return Err(refuse(
            "VLT.PASSWORD.REQUEST_INVALID",
            "review_request",
            VerbError::new("vault_validation_failed", "password change request is invalid"),
        ));
    };
    let resolve = || {
        vault_access.password_change_target(&request.volume_path, peer.caller_sid(), peer.session_id())
    };
    let target = resolve().map_err(|error| {
        let (code, verb_error) = target_refusal(error);
        refuse(code, "review_selected_target", verb_error)
    })?;
    if let Some(entry_id) = &target.entry_id {
        let authorization =
            crate::vault_access::authorize_mount_for_token(vault_access, entry_id, peer.token());
        if !policy_change_allowed(&target, authorization.allowed, authorization.mode) {
            return Err(refuse(
                "VLT.PASSWORD.NOT_AUTHORIZED",
                "request_authorization",
                VerbError::new(PERSONAL_VAULT_UNAUTHORIZED, "caller cannot change this Vault's password"),
            ));
        }
    }
    // Held under the mount lock so no mount can start while the header is rewritten.
    vault_mount.with_exclusive_operation(|| {
        if resolve().ok().as_ref() != Some(&target) {
            return Err(refuse(
                "VLT.PASSWORD.IDENTITY_CHANGED",
                "review_selected_target",
                VerbError::new("vault_policy_identity_changed", "the selected container changed"),
            ));
        }
        let entry_ids: HashSet<String> = target.entry_id.iter().cloned().collect();
        let identities = HashSet::from([target.container_identity.clone()]);
        if let Err(reason) = vault_mount.reject_policy_changes_while_mounted_locked(&entry_ids, &identities) {
            crate::diagnostics::record_vault_password_change(&diagnostic_id, Some(reason), started);
            return Err(VerbError::new(failure_code(reason), "dismount the Vault before changing its password"));
        }
        let _held = crate::vault_access::hold_password_change_files(&target, &request.keyfiles, peer.token())
            .map_err(|error| match error {
                VaultError::ContainerIdentity => refuse(
                    "VLT.PASSWORD.IDENTITY_CHANGED",
                    "review_selected_target",
                    VerbError::new("vault_policy_identity_changed", "the selected container changed"),
                ),
                _ => refuse(
                    "VLT.PASSWORD.CALLER_ACCESS_DENIED",
                    "check_container_permissions",
                    VerbError::new(
                        VaultMountBroker::personal_mount_failure_code(VaultMountReason::CallerAccessDenied),
                        "this account cannot write the container or read its keyfiles",
                    ),
                ),
            })?;
        args.0["VolumePath"] = serde_json::Value::String(target.container_path.clone());
        args.0["TargetSessionId"] = serde_json::Value::from(peer.session_id());
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
                crate::pro_broker::VaultCall {
                    request_id: operation_id,
                    target_session_id: peer.session_id(),
                    caller_sid: peer.caller_sid(),
                    caller_token: Some(peer.token()),
                    caller_authentication_id: Some(peer.authentication_id()),
                    presentation: wincmd_shared::vault_access::VaultPresentation::PerUser,
                    feature_id: BROKER_FEATURE,
                    args: args.0.take(),
                },
            ))
        });
        let failure = match &result {
            Ok(reply) if reply.get("status").and_then(serde_json::Value::as_str) == Some("password_changed") => None,
            Ok(_) => Some(VaultMountReason::BrokerReplyRejected),
            Err(reason) => Some(*reason),
        };
        crate::diagnostics::record_vault_password_change(&diagnostic_id, failure, started);
        match failure {
            None => Ok(serde_json::json!({
                "status": "password_changed",
                "path": target.container_path,
                "target": request.target,
            })),
            Some(reason) => Err(VerbError::new(failure_code(reason), "the Vault password was not changed")),
        }
    })
}

#[cfg(test)]
#[path = "vault_password_change_tests.rs"]
mod tests;
