//! `svc.vault.probe_password`: which header the current password opens. The
//! engine tries the hidden and the outer header every time, so the file alone
//! decides and a wrong password costs the same on every Vault.

use wincmd_shared::vault_access::VaultMountReason;

use super::vault_password_change::{credential_keyfiles, interactive_peer, valid_pim, volume_path, SecretArgs};
use super::vault_password_target::{broker_on_held_target, failure_code, Refusals};
use super::{AuthenticatedPipePeer, VerbError};
use crate::vault_access::VaultAccessStore;
use crate::vault_mount::VaultMountBroker;

const BROKER_FEATURE: &str = "vault.broker.probe_password_personal";
const ALLOWED_KEYS: [&str; 5] = ["VolumePath", "TargetKind", "Password", "Pim", "Keyfiles"];

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ProbeRequest {
    pub(super) volume_path: String,
    pub(super) keyfiles: Vec<String>,
}

impl Drop for ProbeRequest {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.keyfiles.iter_mut().for_each(Zeroize::zeroize);
    }
}

/// Checks the request shape only; it never copies the password out of `args`.
pub(super) fn validate_request(args: &serde_json::Value) -> Option<ProbeRequest> {
    let object = args.as_object()?;
    if object.keys().any(|key| !ALLOWED_KEYS.contains(&key.as_str())) || !valid_pim(object.get("Pim")) {
        return None;
    }
    Some(ProbeRequest {
        volume_path: volume_path(object)?.to_owned(),
        keyfiles: credential_keyfiles(object, "Password", "Keyfiles")?,
    })
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

pub(super) async fn handle(
    operation_id: u64,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let mut args = SecretArgs(args);
    let log = Refusals::new(operation_id, "probe_password");
    let peer = interactive_peer(peer, &log)?;
    let Some(request) = validate_request(&args.0) else {
        return Err(log.refuse((
            "VLT.PASSWORD.REQUEST_INVALID",
            VerbError::new("vault_validation_failed", "password check request is invalid"),
        )));
    };
    let (_, result) = broker_on_held_target(
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
    let header = match &result {
        Ok(reply) => opened_header(reply).ok_or(VaultMountReason::BrokerReplyRejected),
        Err(reason) => Err(*reason),
    };
    log.record(header.err());
    match header {
        Ok(header) => Ok(serde_json::json!({ "header": header })),
        Err(reason) => Err(VerbError::new(failure_code(reason), "the password was not checked")),
    }
}

#[cfg(test)]
#[path = "vault_password_probe_tests.rs"]
mod tests;
