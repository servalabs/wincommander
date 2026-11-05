// SPDX-License-Identifier: AGPL-3.0-or-later
//! Staged service-owned trial custody. There is intentionally no transport or
//! IPC entry point until the Worker reservation and settlement are paired.
use crate::fleet_credential::{MachineDpapi, Protector};
use crate::license_cache::{licence_device_anchor::Anchor, release_gate};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const INVALID: &str = "trial security state is unavailable";
const PATH: &str = r"SOFTWARE\ServaLabs\WinCommander\HybridTrialActivation";
const ENTROPY: &[u8] = b"wincommander:licence-trial-intent:v1";
const LIMIT: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope { version: u8, sealed: String }

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingTrial {
    operation_id: String,
    device_hash: String,
    credential_id: [u8; 32],
    credential_secret: [u8; 32],
}

impl Drop for PendingTrial {
    fn drop(&mut self) { self.credential_secret.zeroize(); }
}

impl PendingTrial {
    /// The proposed v4 reservation commits to this credential before any
    /// secret is sent; the transport remains deliberately disconnected.
    fn credential_commitment(&self) -> String {
        crate::licence_activation_intent::credential_commitment(&self.credential_id, &self.credential_secret)
    }
}

fn valid_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn machine() -> Anchor { Anchor::machine_named(PATH) }

fn load_at(store: &Anchor, protector: &dyn Protector) -> Result<Option<PendingTrial>, &'static str> {
    if !store.exists().map_err(|_| INVALID)? { return Ok(None); }
    let raw = store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    if raw.len() > LIMIT { return Err(INVALID); }
    let envelope: Envelope = serde_json::from_slice(&raw).map_err(|_| INVALID)?;
    if envelope.version != 1 || envelope.sealed.len() > LIMIT { return Err(INVALID); }
    let sealed = URL_SAFE_NO_PAD.decode(&envelope.sealed).map_err(|_| INVALID)?;
    if sealed.is_empty() || URL_SAFE_NO_PAD.encode(&sealed) != envelope.sealed { return Err(INVALID); }
    let plaintext = protector.unprotect(&sealed, ENTROPY).map_err(|_| INVALID)?;
    let pending: PendingTrial = serde_json::from_slice(&plaintext).map_err(|_| INVALID)?;
    if !valid_hex(&pending.operation_id) || !valid_hex(&pending.device_hash) { return Err(INVALID); }
    Ok(Some(pending))
}

/// Called only while holding the shared cross-process licence security lock.
pub(crate) fn ensure_idle_locked() -> Result<(), &'static str> {
    ensure_idle_at(&machine(), &MachineDpapi)
}

fn ensure_idle_at(store: &Anchor, protector: &dyn Protector) -> Result<(), &'static str> {
    if load_at(store, protector)?.is_some() { Err(INVALID) } else { Ok(()) }
}

/// This scaffold cannot be exposed yet: the eventual entry point must also
/// check the protected licence cache and retained paid history before starting.
pub(crate) fn begin_or_resume() -> Result<PendingTrial, &'static str> {
    let identity = crate::licence_machine_identity::read_machine_identity()?;
    if identity.portable { return Err(INVALID); }
    begin_at(&machine(), &release_gate::machine(), &MachineDpapi, &identity.device_hash,
        crate::licence_activation_intent::ensure_no_paid_intent_locked)
}

/// A resumed trial always retains the exact operation and device credential.
/// No caller can replace a damaged or unfinished intent.
fn begin_at(store: &Anchor, gate: &Anchor, protector: &dyn Protector, device_hash: &str,
    check_paid: impl FnOnce() -> Result<(), &'static str>) -> Result<PendingTrial, &'static str> {
    if !valid_hex(device_hash) { return Err(INVALID); }
    let _lock = crate::licence_security_lock()?;
    release_gate::ensure_idle_at(gate)?;
    check_paid()?;
    if let Some(saved) = load_at(store, protector)? {
        return if saved.device_hash == device_hash { Ok(saved) } else { Err(INVALID) };
    }
    let mut operation = [0u8; 32];
    let mut credential_id = [0u8; 32];
    let mut credential_secret = [0u8; 32];
    rand::rngs::OsRng.try_fill_bytes(&mut operation).map_err(|_| INVALID)?;
    rand::rngs::OsRng.try_fill_bytes(&mut credential_id).map_err(|_| INVALID)?;
    rand::rngs::OsRng.try_fill_bytes(&mut credential_secret).map_err(|_| INVALID)?;
    let operation_id = operation.iter().map(|byte| format!("{byte:02x}")).collect();
    operation.zeroize();
    let pending = PendingTrial { operation_id, device_hash: device_hash.into(), credential_id, credential_secret };
    credential_secret.zeroize();
    let plain = Zeroizing::new(serde_json::to_vec(&pending).map_err(|_| INVALID)?);
    let sealed = protector.protect(&plain, ENTROPY).map_err(|_| INVALID)?;
    if sealed.is_empty() || sealed.len() > LIMIT / 2 { return Err(INVALID); }
    let bytes = serde_json::to_vec(&Envelope { version: 1, sealed: URL_SAFE_NO_PAD.encode(sealed) }).map_err(|_| INVALID)?;
    store.write(&bytes).map_err(|_| INVALID)?;
    let saved = load_at(store, protector)?.ok_or(INVALID)?;
    if saved.operation_id != pending.operation_id || saved.device_hash != pending.device_hash
        || saved.credential_id != pending.credential_id
        || !bool::from(saved.credential_secret.ct_eq(&pending.credential_secret)) { return Err(INVALID); }
    Ok(saved)
}

#[cfg(test)]
#[path = "licence_trial_intent_tests.rs"]
mod tests;
