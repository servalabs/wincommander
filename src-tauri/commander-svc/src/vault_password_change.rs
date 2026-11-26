//! `svc.vault.change_password_personal`: the service picks the container; the Pro broker, in the caller's token, rewrites one header.

use std::path::Path;

use wincmd_shared::vault_access::VaultMountReason;

use super::vault_password_target::{broker_on_held_target, failure_code, Refusals};
use super::{
    peer_has_active_interactive_session, zeroize_json, AuthenticatedPipePeer, VerbError,
    PERSONAL_VAULT_SESSION_ABSENT,
};
use crate::vault_access::VaultAccessStore;
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
/// The proven current real password, with Target "outer" only: it alone lets the
/// engine refuse a new decoy password that opens the hidden header too.
const HIDDEN_KEYS: [&str; 3] = ["HiddenPassword", "HiddenPim", "HiddenKeyfiles"];
const MAX_PIM: u64 = 2_147_468;

/// Overwrites every credential string however the handler returns.
pub(super) struct SecretArgs(pub(super) serde_json::Value);

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
    pub(super) checks_collision: bool,
}

impl Drop for PasswordChangeRequest {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.keyfiles.iter_mut().for_each(Zeroize::zeroize);
    }
}

/// Explorer's "Copy as path" wraps the path in quotes; pasted, it still names the same file.
pub(super) fn selected_path(text: &str) -> &str {
    let trimmed = text.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .map_or(trimmed, str::trim)
}

/// An absolute file path, or a partition path with `TargetKind` "device".
pub(super) fn volume_path(object: &serde_json::Map<String, serde_json::Value>) -> Option<&str> {
    let volume_path = selected_path(object.get("VolumePath")?.as_str()?);
    let is_device = wincmd_shared::vault_access::is_supported_vault_device_path(volume_path);
    let valid = match object.get("TargetKind").map(|kind| (kind, kind.as_str())) {
        None | Some((serde_json::Value::Null, _)) | Some((_, Some("" | "file"))) => {
            !is_device && Path::new(volume_path).is_absolute()
        }
        Some((_, Some("device"))) => is_device,
        Some(_) => false,
    };
    valid.then_some(volume_path)
}

/// The keyfiles of one credential; `None` when the field is malformed or the credential is empty.
pub(super) fn credential_keyfiles(
    object: &serde_json::Map<String, serde_json::Value>,
    password_key: &str,
    keyfiles_key: &str,
) -> Option<Vec<String>> {
    let password = object.get(password_key)?.as_str()?;
    let keyfiles = keyfile_paths(object.get(keyfiles_key))?;
    // VeraCrypt accepts keyfiles alone, but never an empty credential.
    (!password.is_empty() || !keyfiles.is_empty()).then_some(keyfiles)
}

/// Checks the request shape only; it never copies a password out of `args`.
pub(super) fn validate_request(args: &serde_json::Value) -> Option<PasswordChangeRequest> {
    let object = args.as_object()?;
    if object
        .keys()
        .any(|key| !ALLOWED_KEYS.contains(&key.as_str()) && !HIDDEN_KEYS.contains(&key.as_str()))
    {
        return None;
    }
    let volume_path = volume_path(object)?;
    let target = match object.get("Target") {
        None | Some(serde_json::Value::Null) => "outer",
        Some(serde_json::Value::String(value)) if value == "outer" => "outer",
        Some(serde_json::Value::String(value)) if value == "hidden" => "hidden",
        Some(_) => return None,
    };
    let checks_collision = !blank(object.get("HiddenPassword")) || !blank(object.get("HiddenKeyfiles"));
    // A real password sent with anything but a decoy change would be ignored silently.
    if (target == "hidden" || !checks_collision) && HIDDEN_KEYS.iter().any(|key| !blank(object.get(*key))) {
        return None;
    }
    if !["OldPim", "NewPim", "HiddenPim"].into_iter().all(|key| valid_pim(object.get(key))) {
        return None;
    }
    let mut keyfiles = credential_keyfiles(object, "OldPassword", "OldKeyfiles")?;
    keyfiles.extend(credential_keyfiles(object, "NewPassword", "NewKeyfiles")?);
    if checks_collision {
        let password = match object.get("HiddenPassword") {
            None | Some(serde_json::Value::Null) => "",
            Some(value) => value.as_str()?,
        };
        let hidden = keyfile_paths(object.get("HiddenKeyfiles"))?;
        if password.is_empty() && hidden.is_empty() {
            return None;
        }
        keyfiles.extend(hidden);
    }
    Some(PasswordChangeRequest {
        volume_path: volume_path.to_owned(),
        target,
        keyfiles,
        checks_collision,
    })
}

fn blank(value: Option<&serde_json::Value>) -> bool {
    match value {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(text)) => matches!(text.trim(), "" | "[]"),
        Some(serde_json::Value::Array(items)) => items.is_empty(),
        Some(_) => false,
    }
}

pub(super) fn valid_pim(value: Option<&serde_json::Value>) -> bool {
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

pub(super) fn interactive_peer<'a>(
    peer: Option<&'a AuthenticatedPipePeer>,
    log: &Refusals,
) -> Result<&'a AuthenticatedPipePeer, VerbError> {
    match peer {
        Some(peer) if !peer.caller_sid().is_empty() && peer_has_active_interactive_session(peer) => Ok(peer),
        _ => Err(log.refuse((
            "VLT.PASSWORD.SESSION_UNAVAILABLE",
            VerbError::new(PERSONAL_VAULT_SESSION_ABSENT, "no interactive Windows session"),
        ))),
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
    let log = Refusals::new(operation_id, "change_password");
    let peer = interactive_peer(peer, &log)?;
    let Some(request) = validate_request(&args.0) else {
        return Err(log.refuse((
            "VLT.PASSWORD.REQUEST_INVALID",
            VerbError::new("vault_validation_failed", "password change request is invalid"),
        )));
    };
    if !request.checks_collision {
        if let Some(object) = args.0.as_object_mut() {
            for key in HIDDEN_KEYS {
                if let Some(mut value) = object.remove(key) {
                    zeroize_json(&mut value);
                }
            }
        }
    }
    let (path, result) = broker_on_held_target(
        &log,
        operation_id,
        vault_access,
        vault_mount,
        peer,
        &request.volume_path,
        &request.keyfiles,
        BROKER_FEATURE,
        &mut args.0,
    )?;
    let failure = match &result {
        Ok(reply) if reply.get("status").and_then(serde_json::Value::as_str) == Some("password_changed") => None,
        Ok(_) => Some(VaultMountReason::BrokerReplyRejected),
        Err(reason) => Some(*reason),
    };
    log.record(failure);
    match failure {
        None => Ok(serde_json::json!({ "status": "password_changed", "path": path, "target": request.target })),
        Some(reason) => Err(VerbError::new(failure_code(reason), "the Vault password was not changed")),
    }
}

#[cfg(test)]
#[path = "vault_password_change_tests.rs"]
mod tests;
