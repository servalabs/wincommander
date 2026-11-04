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
#[path = "licence_device_wire.rs"]
mod wire;

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
    let _process_lock = crate::licence_security_lock().map_err(|_| INVALID)?;
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

pub(crate) struct RefreshProof {
    pub body: Vec<u8>,
    pub tag: [u8; 32],
}

fn sign_refresh_wire(
    credential: &ConfirmedCredential,
    body: Vec<u8>,
    credential_id: &[u8; 32],
    nonce: &[u8; 32],
    digest: &[u8; 32],
    expires_at: u64,
    now: u64,
) -> Result<RefreshProof, &'static str> {
    use subtle::ConstantTimeEq;
    if credential.id != *credential_id
        || expires_at <= now
        // Server lifetime is 120 seconds; allow one minute for clock skew.
        || expires_at > now.saturating_add(180)
        || body.is_empty()
        || body.len() > 16 * 1024
    {
        return Err(INVALID);
    }
    let actual: [u8; 32] = Sha256::digest(&body).into();
    if !bool::from(actual.ct_eq(digest)) {
        return Err(INVALID);
    }
    let challenge = wincmd_pq_experiment::licence_device_proof::Challenge {
        operation: wincmd_pq_experiment::licence_device_proof::Operation::Refresh,
        credential_id,
        nonce,
        request_digest: digest,
        expires_at,
    };
    let tag = wincmd_pq_experiment::licence_device_proof::sign(credential.secret(), &challenge);
    Ok(RefreshProof { body, tag })
}

/// Existing signed licence and cache evidence; callers cannot choose trust or paths through IPC.
pub(crate) struct RefreshLicence<'a> {
    pub payload: &'a str,
    pub signature: &'a str,
    pub hybrid: &'a str,
    pub last_verified_at: u64,
}

fn checked_refresh_wire(
    binding: &Binding<'_>,
    licence: &RefreshLicence<'_>,
) -> Result<Vec<u8>, &'static str> {
    crate::license_cache::verified_refresh_wire(
        licence.payload,
        licence.signature,
        licence.hybrid,
        licence.last_verified_at,
        binding.licence_id,
        binding.device_hash,
    )
}

/// Prepare a challenge request from the service's verified cache and confirmed identity.
pub(crate) fn prepare_challenge_request(
    binding: &Binding<'_>,
    receipt_trust: &wincmd_pq_experiment::jws::Trust<'_>,
    licence: &RefreshLicence<'_>,
    now: u64,
) -> Result<Vec<u8>, &'static str> {
    let credential = load_confirmed(binding, receipt_trust, now)?;
    let body = checked_refresh_wire(binding, licence)?;
    wire::challenge_request(&credential, &body)
}

/// Revalidate the cache before using a device secret for a received challenge.
pub(crate) fn prepare_renewal_request(
    binding: &Binding<'_>,
    receipt_trust: &wincmd_pq_experiment::jws::Trust<'_>,
    licence: &RefreshLicence<'_>,
    challenge: &[u8],
    now: u64,
) -> Result<Vec<u8>, &'static str> {
    let credential = load_confirmed(binding, receipt_trust, now)?;
    let body = checked_refresh_wire(binding, licence)?;
    wire::renewal_request(&credential, body, challenge, now)
}

/// Perform one bounded renewal exchange. The caller must authenticate the caller,
/// independently provision trust, and recheck protected cache state before saving.
pub(crate) async fn request_renewal(
    binding: &Binding<'_>,
    receipt_trust: &wincmd_pq_experiment::jws::Trust<'_>,
    licence: &RefreshLicence<'_>,
) -> Result<serde_json::Value, &'static str> {
    if binding.issuer_origin != wire::LICENCE_ORIGIN {
        return Err(INVALID);
    }
    let current_time = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|_| INVALID)
    };
    let body = checked_refresh_wire(binding, licence)?;
    let old: serde_json::Value = serde_json::from_slice(&body).map_err(|_| INVALID)?;
    let portable = old
        .get("isPortable")
        .and_then(serde_json::Value::as_bool)
        .ok_or(INVALID)?;
    let request = prepare_challenge_request(binding, receipt_trust, licence, current_time()?)?;
    let challenge = wire::post(wire::Endpoint::Challenge, request).await?;
    let request =
        prepare_renewal_request(binding, receipt_trust, licence, &challenge, current_time()?)?;
    let response = wire::post(wire::Endpoint::Renewal, request).await?;
    crate::license_cache::validate_device_renewal(
        &response,
        binding.licence_id,
        binding.device_hash,
        portable,
    )
}

/// A future authenticated service caller can renew without supplying cache,
/// binding, issuer or trust data. This function is not exposed over IPC.
pub(crate) async fn renew_from_machine_cache() -> Result<(), &'static str> {
    crate::license_cache::recover_pending_device_renewal()?;
    let cache = crate::license_cache::load_device_licence_snapshot()?;
    let trust = crate::license_cache::configured_device_receipt_trust()?;
    let mut receipt_policy = trust.policy();
    receipt_policy.purpose = wincmd_pq_experiment::Purpose::LicenceEnrolment;
    let binding = Binding { licence_id: &cache.licence_id, device_hash: &cache.device_hash,
        issuer_origin: wire::LICENCE_ORIGIN };
    let licence = RefreshLicence { payload: &cache.payload, signature: &cache.signature,
        hybrid: &cache.hybrid, last_verified_at: cache.last_verified_at };
    renew_and_save(&binding, &receipt_policy, &licence).await
}

/// Save only if the verified cache did not change while the request was in flight.
pub(crate) async fn renew_and_save(
    binding: &Binding<'_>,
    receipt_trust: &wincmd_pq_experiment::jws::Trust<'_>,
    licence: &RefreshLicence<'_>,
) -> Result<(), &'static str> {
    if crate::license_cache::recover_pending_device_renewal()? {
        return Err("license cache recovered; reload before retrying");
    }
    let renewal = request_renewal(binding, receipt_trust, licence).await?;
    crate::license_cache::save_device_renewal(
        licence.payload,
        licence.signature,
        licence.hybrid,
        licence.last_verified_at,
        renewal,
    )
}

/// No IPC verb is exposed. Any future caller must authenticate the user and
/// supply independent receipt trust and service time, not caller-chosen policy.
pub(crate) struct RefreshInput<'a> {
    pub payload: &'a str,
    pub signature: &'a str,
    pub hybrid: &'a str,
    pub last_verified_at: u64,
    pub credential_id: &'a [u8; 32],
    pub nonce: &'a [u8; 32],
    pub digest: &'a [u8; 32],
    pub expires_at: u64,
    pub now: u64,
}
pub(crate) fn sign_refresh(
    binding: &Binding<'_>,
    receipt_trust: &wincmd_pq_experiment::jws::Trust<'_>,
    input: &RefreshInput<'_>,
) -> Result<RefreshProof, &'static str> {
    let credential = load_confirmed(binding, receipt_trust, input.now)?;
    let wire = crate::license_cache::verified_refresh_wire(
        input.payload,
        input.signature,
        input.hybrid,
        input.last_verified_at,
        binding.licence_id,
        binding.device_hash,
    )?;
    sign_refresh_wire(
        &credential,
        wire,
        input.credential_id,
        input.nonce,
        input.digest,
        input.expires_at,
        input.now,
    )
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
    let historical = crate::license_cache::historical_device_receipt_trust(trust)?;
    decode_confirmed_with_history(protector, binding, trust, historical.as_ref(), bytes, now)
}

fn decode_confirmed_with_history(
    protector: &dyn Protector,
    binding: &Binding<'_>,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    historical: Option<&crate::license_cache::DeviceReceiptHistory>,
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
    if receipt::verify(receipt, trust, binding, &credential, accepted_at).is_err() {
        let old = historical.ok_or(INVALID)?;
        let epoch = receipt::verify_epoch(receipt, &old.receipt_policy(), binding, &credential, accepted_at)?;
        if epoch > old.maximum_epoch() { return Err(INVALID); }
    }
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
    let _process_lock = crate::licence_security_lock().map_err(|_| INVALID)?;
    confirm_locked(anchor, protector, binding, trust, signed_receipt, now)
}

// Caller must hold the credential lock and the cross-process security lock.
fn confirm_locked(
    anchor: &anchor::Anchor,
    protector: &dyn Protector,
    binding: &Binding<'_>,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    signed_receipt: &str,
    now: u64,
) -> Result<ConfirmedCredential, &'static str> {
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

fn enrolment_state(
    anchor: &anchor::Anchor,
    protector: &dyn Protector,
    binding: &Binding<'_>,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    now: u64,
) -> Result<EnrolmentState, &'static str> {
    let state = {
        let _guard = LOCK.lock().map_err(|_| INVALID)?;
        let _process_lock = crate::licence_security_lock().map_err(|_| INVALID)?;
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
                        protector, binding, trust, &bytes, now,
                    )?),
                    _ => return Err(INVALID),
                }
            }
        }
    };
    Ok(state)
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
    let state = enrolment_state(anchor, protector, binding, trust, clock())?;
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

/// Use only service-build trust for registration. Authorization is still the
/// responsibility of the service caller; no desktop-facing verb exists yet.
pub(crate) async fn enrol_with_configured_trust(
    binding: &Binding<'_>,
    licence: &RefreshLicence<'_>,
    license_key: &str,
) -> Result<(), &'static str> {
    let trust = crate::license_cache::configured_device_receipt_trust()?;
    let mut receipt_policy = trust.policy();
    receipt_policy.purpose = wincmd_pq_experiment::Purpose::LicenceEnrolment;
    enrol_over_network(binding, &receipt_policy, licence, license_key).await
}

/// Register over the fixed hybrid-only connection using the service policy.
async fn enrol_over_network(
    binding: &Binding<'_>,
    receipt_trust: &wincmd_pq_experiment::jws::Trust<'_>,
    licence: &RefreshLicence<'_>,
    license_key: &str,
) -> Result<(), &'static str> {
    if binding.issuer_origin != wire::LICENCE_ORIGIN {
        return Err(INVALID);
    }
    // Never transmit a device secret for an unverified or superseded cache.
    checked_refresh_wire(binding, licence)?;
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|_| INVALID)
    };
    enrol_network_with(
        binding,
        license_key,
        || {
            let anchor = anchor::Anchor::machine_named(
                r"SOFTWARE\ServaLabs\WinCommander\ExperimentalLicenceDevice",
            );
            enrolment_state(&anchor, &MachineDpapi, binding, receipt_trust, now()?)
        },
        wire::post_enrolment,
        |signed| {
            // Lock order: credential, cache, then cross-process. Verification
            // and confirmation must be one operation with no network I/O.
            let _guard = LOCK.lock().map_err(|_| INVALID)?;
            crate::license_cache::with_verified_refresh_wire(
                licence.payload, licence.signature, licence.hybrid, licence.last_verified_at,
                binding.licence_id, binding.device_hash, |_| {
                    let anchor = anchor::Anchor::machine_named(
                        r"SOFTWARE\ServaLabs\WinCommander\ExperimentalLicenceDevice",
                    );
                    confirm_locked(&anchor, &MachineDpapi, binding, receipt_trust, signed, now()?)?;
                    Ok(())
                },
            )
        },
    ).await
}

// The production caller supplies the fixed transport and atomic cache check.
// Injectable operations exercise the same asynchronous sequence in tests.
async fn enrol_network_with<State, SendRequest, Response, Confirm>(
    binding: &Binding<'_>,
    license_key: &str,
    state: State,
    send: SendRequest,
    confirm: Confirm,
) -> Result<(), &'static str>
where
    State: FnOnce() -> Result<EnrolmentState, &'static str>,
    SendRequest: FnOnce(Zeroizing<Vec<u8>>) -> Response,
    Response: std::future::Future<Output = Result<Vec<u8>, &'static str>>,
    Confirm: FnOnce(&str) -> Result<(), &'static str>,
{
    match state()? {
        EnrolmentState::Confirmed(_) => Ok(()),
        EnrolmentState::Pending(pending) => {
            let body = wire::enrolment_request(binding, &pending, license_key)?;
            let response = send(body).await?;
            let signed = wire::enrolment_receipt(&response)?;
            confirm(&signed)
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
    let _process_lock = crate::licence_security_lock().map_err(|_| INVALID)?;
    let store =
        anchor::Anchor::machine_named(r"SOFTWARE\ServaLabs\WinCommander\ExperimentalLicenceDevice");
    let bytes = store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    decode_confirmed(&MachineDpapi, binding, trust, &bytes, now)
}

#[cfg(test)]
#[path = "licence_device_credential_tests.rs"]
mod tests;
