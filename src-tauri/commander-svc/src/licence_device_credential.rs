// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unwired experimental credential custody. Enrolment, recovery, caller
//! authorization and transport are not implemented by this module.
use crate::fleet_credential::{MachineDpapi, Protector};
use crate::license_cache::licence_device_anchor as anchor;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use zeroize::Zeroizing;

const INVALID: &str = "device credential is invalid or unavailable";
const LIMIT: usize = 64 * 1024;
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
pub(crate) struct Binding<'a> {
    pub licence_id: &'a str,
    pub device_hash: &'a str,
    /// Must be the exact independently authorized issuer origin.
    pub issuer_origin: &'a str,
}

fn entropy(binding: &Binding<'_>, credential_id: &[u8; 32]) -> Result<[u8; 32], &'static str> {
    if [
        binding.licence_id,
        binding.device_hash,
        binding.issuer_origin,
    ]
    .iter()
    .any(|field| field.is_empty() || field.len() > 512)
    {
        return Err(INVALID);
    }
    let encoded = serde_json::to_vec(&(
        "wincommander:licence-device-credential:pending:v2",
        binding.licence_id,
        binding.device_hash,
        binding.issuer_origin,
        credential_id,
    ))
    .map_err(|_| INVALID)?;
    Ok(Sha256::digest(encoded).into())
}

pub(crate) struct PendingCredential {
    pub id: [u8; 32],
    secret: Zeroizing<[u8; 32]>,
}
impl PendingCredential {
    pub(crate) fn generate() -> Result<Self, &'static str> {
        let mut id = [0u8; 32];
        let mut secret = Zeroizing::new([0u8; 32]);
        rand::rngs::OsRng
            .try_fill_bytes(&mut id)
            .map_err(|_| INVALID)?;
        rand::rngs::OsRng
            .try_fill_bytes(secret.as_mut())
            .map_err(|_| INVALID)?;
        Ok(Self { id, secret })
    }
    pub(crate) fn secret(&self) -> &[u8; 32] {
        &self.secret
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    phase: String,
    id: [u8; 32],
    sealed: Vec<u8>,
}

fn decode(
    protector: &dyn Protector,
    binding: &Binding<'_>,
    bytes: &[u8],
) -> Result<PendingCredential, &'static str> {
    if bytes.is_empty() || bytes.len() > LIMIT {
        return Err(INVALID);
    }
    let record: Record = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    if record.version != 2
        || record.phase != "pending"
        || record.sealed.is_empty()
        || record.sealed.len() > LIMIT
    {
        return Err(INVALID);
    }
    let secret = protector
        .unprotect(&record.sealed, &entropy(binding, &record.id)?)
        .map_err(|_| INVALID)?;
    if secret.len() != 32 {
        return Err(INVALID);
    }
    let mut fixed = Zeroizing::new([0u8; 32]);
    fixed.copy_from_slice(&secret);
    Ok(PendingCredential {
        id: record.id,
        secret: fixed,
    })
}

fn save_locked(
    anchor: &anchor::Anchor,
    protector: &dyn Protector,
    binding: &Binding<'_>,
    credential: &PendingCredential,
) -> Result<(), &'static str> {
    // Never silently replace an existing or damaged identity. Recovery and
    // cross-process coordination must be designed before this is connected.
    if anchor.read().map_err(|_| INVALID)?.is_some() {
        return Err(INVALID);
    }
    let sealed = protector
        .protect(credential.secret(), &entropy(binding, &credential.id)?)
        .map_err(|_| INVALID)?;
    if sealed.is_empty() || sealed.len() > LIMIT {
        return Err(INVALID);
    }
    let bytes = serde_json::to_vec(&Record {
        version: 2,
        phase: "pending".into(),
        id: credential.id,
        sealed,
    })
    .map_err(|_| INVALID)?;
    if bytes.len() > LIMIT {
        return Err(INVALID);
    }
    anchor.write(&bytes).map_err(|_| INVALID)?;
    let actual = anchor.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    let reopened = decode(protector, binding, &actual)?;
    use subtle::ConstantTimeEq;
    if reopened.id != credential.id || !bool::from(reopened.secret().ct_eq(credential.secret())) {
        return Err(INVALID);
    }
    Ok(())
}

fn get_or_create_with(
    anchor: &anchor::Anchor,
    protector: &dyn Protector,
    binding: &Binding<'_>,
) -> Result<PendingCredential, &'static str> {
    let _guard = LOCK.lock().map_err(|_| INVALID)?;
    if let Some(bytes) = anchor.read().map_err(|_| INVALID)? {
        return decode(protector, binding, &bytes);
    }
    let candidate = PendingCredential::generate()?;
    save_locked(anchor, protector, binding, &candidate)?;
    Ok(candidate)
}

/// Persist before contacting the issuer. This never establishes an active
/// credential; a future confirmation path must verify a bound issuer receipt.
pub(crate) fn get_or_create_pending(
    binding: &Binding<'_>,
) -> Result<PendingCredential, &'static str> {
    get_or_create_with(
        &anchor::Anchor::machine_named(
            r"SOFTWARE\ServaLabs\WinCommander\ExperimentalLicenceDevice",
        ),
        &MachineDpapi,
        binding,
    )
}

#[cfg(test)]
#[path = "licence_device_credential_tests.rs"]
mod tests;
