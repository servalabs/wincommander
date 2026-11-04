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

#[path = "licence_device_receipt.rs"]
mod receipt;

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
    #[serde(default)]
    receipt: Option<String>,
    #[serde(default)]
    accepted_at: Option<u64>,
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
        || record.receipt.is_some()
        || record.accepted_at.is_some()
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
        receipt: None,
        accepted_at: None,
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

pub(crate) struct ConfirmedCredential {
    pub id: [u8; 32],
    secret: Zeroizing<[u8; 32]>,
}
impl ConfirmedCredential {
    pub(crate) fn secret(&self) -> &[u8; 32] {
        &self.secret
    }
}

fn confirmed_entropy(
    binding: &Binding<'_>,
    id: &[u8; 32],
    receipt: &str,
    accepted_at: u64,
) -> Result<[u8; 32], &'static str> {
    // Reuse pending validation for bounded identity fields before encoding.
    entropy(binding, id)?;
    let receipt_hash: [u8; 32] = Sha256::digest(receipt.as_bytes()).into();
    let encoded = serde_json::to_vec(&(
        "wincommander:licence-device-credential:confirmed:v2",
        binding.licence_id,
        binding.device_hash,
        binding.issuer_origin,
        id,
        receipt_hash,
        accepted_at,
    ))
    .map_err(|_| INVALID)?;
    Ok(Sha256::digest(encoded).into())
}

fn decode_confirmed(
    protector: &dyn Protector,
    binding: &Binding<'_>,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    bytes: &[u8],
    now: u64,
) -> Result<ConfirmedCredential, &'static str> {
    if bytes.is_empty() || bytes.len() > LIMIT {
        return Err(INVALID);
    }
    let record: Record = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    let receipt = record.receipt.as_deref().ok_or(INVALID)?;
    let accepted_at = record.accepted_at.ok_or(INVALID)?;
    if record.version != 2
        || record.phase != "confirmed"
        || record.sealed.is_empty()
        || record.sealed.len() > LIMIT
        || receipt.len() > 32 * 1024
        || accepted_at > now
    {
        return Err(INVALID);
    }
    let unsealed = protector
        .unprotect(
            &record.sealed,
            &confirmed_entropy(binding, &record.id, receipt, accepted_at)?,
        )
        .map_err(|_| INVALID)?;
    if unsealed.len() != 32 {
        return Err(INVALID);
    }
    let mut secret = Zeroizing::new([0u8; 32]);
    secret.copy_from_slice(&unsealed);
    let credential = PendingCredential {
        id: record.id,
        secret,
    };
    // A receipt must have been valid when accepted, but does not expire the
    // already-confirmed local identity; later status checks remain separate.
    receipt::verify(receipt, trust, binding, &credential, accepted_at)?;
    Ok(ConfirmedCredential {
        id: credential.id,
        secret: credential.secret,
    })
}

fn confirm_with(
    anchor: &anchor::Anchor,
    protector: &dyn Protector,
    binding: &Binding<'_>,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    signed_receipt: &str,
    now: u64,
) -> Result<ConfirmedCredential, &'static str> {
    let _guard = LOCK.lock().map_err(|_| INVALID)?;
    let original = anchor.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    let record: Record = serde_json::from_slice(&original).map_err(|_| INVALID)?;
    if record.phase == "confirmed" {
        let confirmed = decode_confirmed(protector, binding, trust, &original, now)?;
        if record.receipt.as_deref() != Some(signed_receipt) {
            // Concurrent exact enrolment retries can receive different valid
            // signatures or issue times. They must bind the same secret and
            // still be fresh; never replace the retained confirmation.
            let mut secret = Zeroizing::new([0u8; 32]);
            secret.copy_from_slice(confirmed.secret());
            let pending = PendingCredential {
                id: confirmed.id,
                secret,
            };
            receipt::verify(signed_receipt, trust, binding, &pending, now)?;
        }
        return Ok(confirmed);
    }
    let pending = decode(protector, binding, &original)?;
    receipt::verify(signed_receipt, trust, binding, &pending, now)?;
    let sealed = protector
        .protect(
            pending.secret(),
            &confirmed_entropy(binding, &pending.id, signed_receipt, now)?,
        )
        .map_err(|_| INVALID)?;
    if sealed.is_empty() || sealed.len() > LIMIT {
        return Err(INVALID);
    }
    let bytes = serde_json::to_vec(&Record {
        version: 2,
        phase: "confirmed".into(),
        id: pending.id,
        sealed,
        receipt: Some(signed_receipt.into()),
        accepted_at: Some(now),
    })
    .map_err(|_| INVALID)?;
    if bytes.len() > LIMIT {
        return Err(INVALID);
    }
    anchor.write(&bytes).map_err(|_| INVALID)?;
    let written = anchor.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    decode_confirmed(protector, binding, trust, &written, now)
}

enum EnrolmentState {
    Pending(PendingCredential),
    Confirmed(ConfirmedCredential),
}

fn enrol_with<F, C>(
    anchor: &anchor::Anchor,
    protector: &dyn Protector,
    binding: &Binding<'_>,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    clock: C,
    send: F,
) -> Result<ConfirmedCredential, &'static str>
where
    F: FnOnce(&[u8; 32], &[u8; 32]) -> Result<String, &'static str>,
    C: Fn() -> u64,
{
    let state = {
        let _guard = LOCK.lock().map_err(|_| INVALID)?;
        match anchor.read().map_err(|_| INVALID)? {
            None => {
                let pending = PendingCredential::generate()?;
                save_locked(anchor, protector, binding, &pending)?;
                EnrolmentState::Pending(pending)
            }
            Some(bytes) => {
                let record: Record = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
                match record.phase.as_str() {
                    "pending" => EnrolmentState::Pending(decode(protector, binding, &bytes)?),
                    "confirmed" => EnrolmentState::Confirmed(decode_confirmed(
                        protector,
                        binding,
                        trust,
                        &bytes,
                        clock(),
                    )?),
                    _ => return Err(INVALID),
                }
            }
        }
    };
    match state {
        EnrolmentState::Confirmed(confirmed) => Ok(confirmed),
        EnrolmentState::Pending(pending) => {
            // The caller is responsible for an authenticated, hybrid-protected
            // transport. Never log or expose this secret to a desktop caller.
            let signed_receipt = send(&pending.id, pending.secret())?;
            confirm_with(anchor, protector, binding, trust, &signed_receipt, clock())
        }
    }
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

/// The service must supply trusted time, trust, caller authorization and a
/// hybrid-protected transport; no desktop-facing verb is provided here.
pub(crate) fn enrol<F, C>(
    binding: &Binding<'_>,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    clock: C,
    send: F,
) -> Result<ConfirmedCredential, &'static str>
where
    F: FnOnce(&[u8; 32], &[u8; 32]) -> Result<String, &'static str>,
    C: Fn() -> u64,
{
    enrol_with(
        &anchor::Anchor::machine_named(
            r"SOFTWARE\ServaLabs\WinCommander\ExperimentalLicenceDevice",
        ),
        &MachineDpapi,
        binding,
        trust,
        clock,
        send,
    )
}

/// `now` and trust must come from the service's independent local policy.
pub(crate) fn confirm_pending(
    binding: &Binding<'_>,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    signed_receipt: &str,
    now: u64,
) -> Result<ConfirmedCredential, &'static str> {
    confirm_with(
        &anchor::Anchor::machine_named(
            r"SOFTWARE\ServaLabs\WinCommander\ExperimentalLicenceDevice",
        ),
        &MachineDpapi,
        binding,
        trust,
        signed_receipt,
        now,
    )
}

pub(crate) fn load_confirmed(
    binding: &Binding<'_>,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    now: u64,
) -> Result<ConfirmedCredential, &'static str> {
    let _guard = LOCK.lock().map_err(|_| INVALID)?;
    let store =
        anchor::Anchor::machine_named(r"SOFTWARE\ServaLabs\WinCommander\ExperimentalLicenceDevice");
    let bytes = store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    decode_confirmed(&MachineDpapi, binding, trust, &bytes, now)
}

#[cfg(test)]
#[path = "licence_device_credential_tests.rs"]
mod tests;
