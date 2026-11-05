// SPDX-License-Identifier: AGPL-3.0-or-later
//! Machine-bound Fleet trust and signed policy. Never trust a signer supplied
//! only by the policy sender: an administrator must provision it first.
#![allow(dead_code)]

use crate::policy_store::PolicyFs;
use base64::{engine::general_purpose::STANDARD, Engine};
use fleet_proto::{EpochSigningInput, PolicyEnvelope};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Mutex};

#[path = "fleet_origins.rs"]
mod origins;

#[path = "fleet_contact.rs"]
mod contact;
pub use contact::{ContactChallenge, ContactObservation};

#[path = "fleet_hybrid_enrollment.rs"]
mod hybrid_enrollment;

#[cfg(windows)]
use crate::peer_auth_deployment_acl as protected_acl;

/// Separate trust store. Existing directories and files are checked before
/// any ACL change or read, preventing adoption of pre-planted trust.
#[cfg(windows)]
pub struct WindowsFleetPolicyFs;

#[cfg(windows)]
pub(crate) fn system_store() -> Result<&'static FleetMachinePolicy, &'static str> {
    static STORE: std::sync::OnceLock<FleetMachinePolicy> = std::sync::OnceLock::new();
    static INIT: Mutex<()> = Mutex::new(());
    if let Some(store) = STORE.get() { return Ok(store); }
    let _guard = INIT.lock().map_err(|_| "store_unavailable")?;
    if let Some(store) = STORE.get() { return Ok(store); }
    let base = std::env::var_os("ProgramData").ok_or("store_unavailable")?;
    let dir = PathBuf::from(base).join("WinCommander").join("fleet-machine");
    let store = FleetMachinePolicy::open(Box::new(WindowsFleetPolicyFs), dir)?;
    let _ = STORE.set(store);
    STORE.get().ok_or("store_unavailable")
}

#[cfg(windows)]
impl PolicyFs for WindowsFleetPolicyFs {
    fn ensure_dir_secure(
        &self,
        dir: &std::path::Path,
    ) -> Result<(), crate::policy_store::PolicyStoreError> {
        use crate::policy_store::{PolicyStoreError, WindowsPolicyFs};
        let parent = dir.parent().ok_or(PolicyStoreError::InsecurePermissions)?;
        protected_acl::verify_component(parent)
            .map_err(|_| PolicyStoreError::InsecurePermissions)?;
        match std::fs::create_dir(dir) {
            Ok(()) => WindowsPolicyFs.ensure_dir_secure(dir)?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(PolicyStoreError::InsecurePermissions),
        }
        protected_acl::verify_component(dir).map_err(|_| PolicyStoreError::InsecurePermissions)
    }

    fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>> {
        if !path.exists() {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        protected_acl::verify_component(path)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
        std::fs::read(path)
    }

    fn atomic_write(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        let tmp = path.with_extension("next");
        if tmp.exists() {
            protected_acl::verify_component(&tmp)
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
            std::fs::remove_file(&tmp)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        protected_acl::verify_component(&tmp)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
        if path.exists() {
            protected_acl::verify_component(path)
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
        }
        std::fs::rename(tmp, path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetTrust {
    pub org_id: String,
    pub device_id: String,
    pub enrollment_generation: String,
    pub signer_key: String,
}

/// Independently administrator-provisioned public keys. This does not itself
/// prove that the remote server controls the corresponding private keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetHybridTrust {
    pub minimum_epoch: u64,
    pub ed_key_id: String,
    pub pq_key_id: String,
    pub ml_dsa_65_public_b64: String,
    /// Independently approved local outcome-signing key for this device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_key_b64: Option<String>,
}

fn validate_hybrid_trust(binding: &FleetTrust, hybrid: &FleetHybridTrust) -> Result<(), &'static str> {
    let valid_id = |id: &str| !id.is_empty() && id.len() <= 128
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if hybrid.minimum_epoch == 0 || !valid_id(&hybrid.ed_key_id) || !valid_id(&hybrid.pq_key_id)
        || hybrid.ed_key_id == hybrid.pq_key_id || hybrid.ml_dsa_65_public_b64.len() != 2604 {
        return Err("invalid_hybrid_trust");
    }
    let pq = STANDARD.decode(&hybrid.ml_dsa_65_public_b64).map_err(|_| "invalid_hybrid_trust")?;
    let ed = STANDARD.decode(&binding.signer_key).map_err(|_| "invalid_hybrid_trust")?;
    let ed: [u8; 32] = ed.try_into().map_err(|_| "invalid_hybrid_trust")?;
    if pq.len() != 1952 || pq.iter().all(|b| *b == 0) || STANDARD.encode(&pq) != hybrid.ml_dsa_65_public_b64
        || STANDARD.encode(ed) != binding.signer_key
        || !ed25519_dalek::VerifyingKey::from_bytes(&ed).is_ok_and(|key| !key.is_weak()) {
        return Err("invalid_hybrid_trust");
    }
    if let Some(value) = &hybrid.outcome_key_b64 {
        let outcome = STANDARD.decode(value).map_err(|_| "invalid_hybrid_trust")?;
        let outcome: [u8; 32] = outcome.try_into().map_err(|_| "invalid_hybrid_trust")?;
        if STANDARD.encode(outcome) != *value
            || !ed25519_dalek::VerifyingKey::from_bytes(&outcome).is_ok_and(|key| !key.is_weak()) {
            return Err("invalid_hybrid_trust");
        }
    }
    Ok(())
}

/// Nonsecret transport configuration authorized along with the origin set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportProfile {
    pub spki_pins: String,
    pub environment_fingerprint: String,
}

type TrustWithTransport = (FleetTrust, Vec<TrustAudit>, Vec<String>, Option<TransportProfile>, bool);
type ContactState = (FleetTrust, Option<ContactObservation>, Option<String>, String, u64);

fn default_service_enabled() -> bool { true }

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCredential {
    origin: String,
    ciphertext: String,
}

pub(crate) struct CredentialSnapshot {
    pub trust: FleetTrust,
    pub origins: Vec<String>,
    pub profile: TransportProfile,
    pub revision: u64,
    pub secret: zeroize::Zeroizing<Vec<u8>>,
}

#[derive(Clone)]
pub(crate) struct CandidateContext {
    pub trust: FleetTrust,
    pub origins: Vec<String>,
    pub profile: TransportProfile,
    pub revision: u64,
    pub connection_revision: u64,
    pub origin: String,
}

fn validate_transport_profile(profile: &TransportProfile) -> Result<(), &'static str> {
    if profile.spki_pins.len() > 4096 { return Err("invalid_transport_profile"); }
    let digest = STANDARD.decode(&profile.environment_fingerprint).map_err(|_| "invalid_transport_profile")?;
    if digest.len() != 32 || STANDARD.encode(digest) != profile.environment_fingerprint {
        return Err("invalid_transport_profile");
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    trust: FleetTrust,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hybrid_trust: Option<FleetHybridTrust>,
    #[serde(skip)]
    hybrid_unconfirmed: bool,
    policy: Option<PolicyEnvelope>,
    audit: Vec<TrustAudit>,
    #[serde(default)]
    contact: Option<ContactObservation>,
    #[serde(default)]
    origins: Vec<String>,
    #[serde(default)]
    transport_profile: Option<TransportProfile>,
    #[serde(default)]
    credential: Option<StoredCredential>,
    #[serde(default)]
    credential_revision: u64,
    #[serde(default)]
    connection_revision: u64,
    #[serde(default)]
    cleared: Option<ClearAcknowledgement>,
    #[serde(default)]
    runtime_profile_changed: bool,
    #[serde(default = "default_service_enabled")]
    service_enabled: bool,
}

fn validate_hybrid_state(record: &Record) -> Result<(), &'static str> {
    if let Some(hybrid) = &record.hybrid_trust {
        validate_hybrid_trust(&record.trust, hybrid)?;
        if record.service_enabled || record.credential.is_some() { return Err("invalid_hybrid_state"); }
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClearAcknowledgement {
    token: String,
    connection_token: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingEnrollment {
    device_id: String,
    signer_key: String,
    challenge: String,
    #[serde(default)]
    origins: Vec<String>,
    #[serde(default)]
    transport_profile: Option<TransportProfile>,
}

/// An administrator's short-lived grant. Deliberately never serialized: a
/// service restart invalidates every in-flight enrollment authorization.
#[derive(Clone)]
struct PendingHybridEnrollment {
    id: String,
    challenge: String,
    origin: String,
    caller_sid: String,
    session_id: u32,
    trust: FleetTrust,
    connection_revision: u64,
    expires: std::time::Instant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HybridEnrollmentOffer {
    pub authorization_id: String,
    pub challenge: String,
    pub origin: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    record: Option<Record>,
    pending: Option<PendingEnrollment>,
    #[serde(skip)]
    contact_challenge: Option<contact::PendingContact>,
    #[serde(skip)]
    hybrid_enrollment: Option<PendingHybridEnrollment>,
    #[serde(skip)]
    hybrid_claim: Option<hybrid_enrollment::ClaimMarker>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustAudit {
    action: String,
    caller_sid: String,
    generation: String,
    unix_time: u64,
}

fn trust_audit(action: &str, actor: &str, generation: &str) -> Result<TrustAudit, &'static str> {
    if !actor.starts_with("S-1-") || actor.len() > 184 {
        return Err("invalid_actor");
    }
    let unix_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "clock_unavailable")?
        .as_secs();
    Ok(TrustAudit {
        action: action.into(),
        caller_sid: actor.into(),
        generation: generation.into(),
        unix_time,
    })
}

pub struct FleetMachinePolicy {
    fs: Box<dyn PolicyFs>,
    path: PathBuf,
    state: Mutex<State>,
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value != "00000000-0000-0000-0000-000000000000"
        && value == value.to_ascii_lowercase()
        && value.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn validate_trust(trust: &FleetTrust) -> Result<(), &'static str> {
    if trust.org_id.is_empty()
        || trust.org_id.len() > 256
        || trust.device_id.is_empty()
        || trust.device_id.len() > 128
        || !valid_uuid(&trust.enrollment_generation)
    {
        return Err("invalid_binding");
    }
    let raw = STANDARD
        .decode(&trust.signer_key)
        .map_err(|_| "invalid_key")?;
    let bytes: [u8; 32] = raw.try_into().map_err(|_| "invalid_key")?;
    ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(|_| "invalid_key")?;
    Ok(())
}

fn verify_policy(trust: &FleetTrust, policy: &PolicyEnvelope) -> Result<(), &'static str> {
    if policy.org_id.0 != trust.org_id
        || policy.device_id.0 != trust.device_id
        || policy.signer_key != trust.signer_key
        || policy.shield_revision < 0
    {
        return Err("binding_mismatch");
    }
    // Both values must be inside the verified preimage; never accept a
    // generation or revision merely attached outside the signature.
    let signed = policy.preimage();
    let preimage: serde_json::Value =
        serde_json::from_slice(&signed).map_err(|_| "invalid_policy")?;
    if preimage
        .get("enrollment_generation")
        .and_then(|v| v.as_str())
        != Some(trust.enrollment_generation.as_str())
    {
        return Err("generation_mismatch");
    }
    if preimage
        .get("config_revision")
        .and_then(|v| v.as_i64())
        .filter(|v| *v >= 0)
        .is_none()
    {
        return Err("missing_config_revision");
    }
    if preimage
        .get("resolution_revision")
        .and_then(|v| v.as_i64())
        .filter(|v| *v >= 0)
        .is_none()
    {
        return Err("missing_resolution_revision");
    }
    if !fleet_proto::verify_signature_b64(&trust.signer_key, &signed, &policy.signature) {
        return Err("invalid_signature");
    }
    if let Some(config) = &policy.config {
        if config.org_id.0 != trust.org_id
            || config.version <= 0
            || config.signer_key != trust.signer_key
        {
            return Err("invalid_config");
        }
        let target_valid = match config.target_kind.as_str() {
            "org" => config.target_id.is_none(),
            "device" => config.target_id.as_deref() == Some(trust.device_id.as_str()),
            "group" => config.target_id.as_ref().is_some_and(|s| !s.is_empty()),
            _ => false,
        };
        let bytes = fleet_proto::epoch_preimage(&EpochSigningInput {
            version: config.version,
            config: &config.config_json,
            locked_paths: &config.locked_paths,
            managed: config.managed,
            target_kind: &config.target_kind,
            target_id: config.target_id.as_deref(),
        });
        if !target_valid
            || !fleet_proto::verify_signature_b64(&trust.signer_key, &bytes, &config.signature)
        {
            return Err("invalid_config");
        }
    }
    if policy
        .shield
        .as_ref()
        .is_some_and(|s| s.revision > policy.shield_revision || s.revision < 0)
    {
        return Err("invalid_shield_revision");
    }
    Ok(())
}

impl FleetMachinePolicy {
    /// The supplied filesystem must validate existing owner/DACL and reject
    /// reparse points *before* loading any existing trust record.
    pub fn open(fs: Box<dyn PolicyFs>, dir: PathBuf) -> Result<Self, &'static str> {
        fs.ensure_dir_secure(&dir).map_err(|_| "insecure_store")?;
        let path = dir.join("fleet-machine-policy.json");
        let state: State = match fs.read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "invalid_store")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(_) => return Err("store_unavailable"),
        };
        if let Some(record) = &state.record {
            validate_trust(&record.trust)?;
            validate_hybrid_state(record)?;
            if let Some(policy) = &record.policy {
                verify_policy(&record.trust, policy)?;
            }
        }
        Ok(Self {
            fs,
            path,
            state: Mutex::new(state),
        })
    }

    pub fn begin_enrollment(
        &self,
        device_id: String,
        signer_key: String,
        allowed_origins: Vec<String>,
        transport_profile: Option<TransportProfile>,
    ) -> Result<String, &'static str> {
        use rand::RngCore;
        let placeholder = FleetTrust {
            org_id: "pending".into(),
            device_id: device_id.clone(),
            signer_key: signer_key.clone(),
            enrollment_generation: "00000000-0000-4000-8000-000000000000".into(),
        };
        validate_trust(&placeholder)?;
        let allowed_origins = origins::normalize(&allowed_origins)?;
        if let Some(profile) = &transport_profile { validate_transport_profile(profile)?; }
        let mut bytes = [0u8; 16];
        rand::rngs::OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| "random_unavailable")?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let challenge = format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        );
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        guard.hybrid_enrollment = None;
        guard.hybrid_claim = None;
        let mut next = guard.clone();
        next.pending = Some(PendingEnrollment {
            device_id,
            signer_key,
            challenge: challenge.clone(),
            origins: allowed_origins,
            transport_profile,
        });
        self.persist(&next)?;
        *guard = next;
        Ok(challenge)
    }

    fn verify_enrollment(
        state: &State,
        trust: &FleetTrust,
        signature: &str,
    ) -> Result<(), &'static str> {
        let pending = state.pending.as_ref().ok_or("enrollment_not_started")?;
        if origins::normalize(&pending.origins)? != pending.origins {
            return Err("invalid_origin");
        }
        if let Some(profile) = &pending.transport_profile { validate_transport_profile(profile)?; }
        if pending.device_id != trust.device_id || pending.signer_key != trust.signer_key {
            return Err("enrollment_binding_mismatch");
        }
        // The generation is a UUID; device identifiers retain exact case.
        if trust.enrollment_generation != trust.enrollment_generation.to_ascii_lowercase() {
            return Err("invalid_binding");
        }
        let bytes = serde_json::to_vec(&[
            "wincommander:shield-progress:enrollment:v1",
            &trust.org_id,
            &trust.device_id,
            &trust.enrollment_generation,
            &pending.challenge,
        ])
        .map_err(|_| "invalid_proof")?;
        if !fleet_proto::verify_signature_b64(&trust.signer_key, &bytes, signature) {
            return Err("invalid_proof");
        }
        Ok(())
    }

    /// Caller authorization is enforced by the service pipe; this function
    /// deliberately refuses any replacement of an existing binding.
    pub fn provision(
        &self,
        trust: FleetTrust,
        signature: &str,
        actor: &str,
    ) -> Result<(), &'static str> {
        validate_trust(&trust)?;
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        if guard.record.is_some() {
            return Err("already_provisioned");
        }
        Self::verify_enrollment(&guard, &trust, signature)?;
        let audit = vec![trust_audit(
            "provision",
            actor,
            &trust.enrollment_generation,
        )?];
        let approved_origins = guard.pending.as_ref().ok_or("enrollment_not_started")?.origins.clone();
        let approved_profile = guard.pending.as_ref().ok_or("enrollment_not_started")?.transport_profile.clone();
        let next = State {
            record: Some(Record {
                trust,
                hybrid_trust: None,
                hybrid_unconfirmed: false,
                policy: None,
                audit,
                contact: None,
                origins: approved_origins,
                transport_profile: approved_profile,
                credential: None,
                credential_revision: 0,
                connection_revision: 0,
                cleared: None,
                runtime_profile_changed: false,
                service_enabled: true,
            }),
            pending: None,
            contact_challenge: None,
            hybrid_enrollment: None,
            hybrid_claim: None,
        };
        self.persist(&next)?;
        *guard = next;
        Ok(())
    }

    /// Only an authenticated administrator may call this through the service
    /// pipe. Once set, these keys cannot be replaced by the legacy path.
    /// Legacy service connections are disabled until hybrid integration exists.
    pub fn provision_hybrid_trust(
        &self,
        expected: FleetTrust,
        hybrid: FleetHybridTrust,
        actor: &str,
    ) -> Result<(), &'static str> {
        validate_trust(&expected)?;
        validate_hybrid_trust(&expected, &hybrid)?;
        if hybrid.outcome_key_b64.is_none() { return Err("outcome_key_required"); }
        let audit = trust_audit("hybrid_provision", actor, &expected.enrollment_generation)?;
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        if current.trust != expected { return Err("binding_changed"); }
        if let Some(existing) = &current.hybrid_trust {
            if *existing == hybrid {
                if !current.hybrid_unconfirmed { return Ok(()); }
                let mut next = guard.clone();
                next.record.as_mut().ok_or("not_provisioned")?.hybrid_unconfirmed = false;
                self.persist(&next)?;
                *guard = next;
                return Ok(());
            }
            let completing_legacy_pin = existing.outcome_key_b64.is_none()
                && existing.minimum_epoch == hybrid.minimum_epoch && existing.ed_key_id == hybrid.ed_key_id
                && existing.pq_key_id == hybrid.pq_key_id && existing.ml_dsa_65_public_b64 == hybrid.ml_dsa_65_public_b64;
            if !completing_legacy_pin || current.hybrid_unconfirmed { return Err("hybrid_rotation_required"); }
        }
        let mut next = guard.clone();
        let record = next.record.as_mut().ok_or("not_provisioned")?;
        record.credential_revision = record.credential_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.connection_revision = record.connection_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.credential = None;
        record.service_enabled = false;
        record.cleared = None;
        record.hybrid_trust = Some(hybrid);
        if record.audit.len() >= 64 { record.audit.remove(0); }
        record.audit.push(audit);
        next.pending = None;
        next.contact_challenge = None;
        next.hybrid_enrollment = None;
        next.hybrid_claim = None;
        match self.persist(&next) {
            Ok(()) => { *guard = next; Ok(()) }
            Err(error) => {
                // A failed write can have reached disk. Fence legacy use in
                // memory until the exact hybrid record has been persisted.
                next.record.as_mut().ok_or("not_provisioned")?.hybrid_unconfirmed = true;
                *guard = next;
                Err(error)
            }
        }
    }

    pub fn hybrid_trust_snapshot(&self) -> Result<Option<(FleetTrust, FleetHybridTrust)>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        if guard.record.as_ref().is_some_and(|record| record.hybrid_unconfirmed) { return Err("store_unavailable"); }
        Ok(guard.record.as_ref().and_then(|record| record.hybrid_trust.as_ref()
            .map(|hybrid| (record.trust.clone(), hybrid.clone()))))
    }

    /// An explicit administrator grant is scoped to an actual Windows user,
    /// session, existing trust, and already-authorized HTTPS origin.
    pub fn begin_hybrid_enrollment(&self, expected: &FleetTrust, origin: &str, actor: &str, session: u32)
        -> Result<HybridEnrollmentOffer, &'static str> {
        use rand::RngCore;
        validate_trust(expected)?;
        let audit = trust_audit("hybrid_enrollment_begin", actor, &expected.enrollment_generation)?;
        if session == 0 { return Err("invalid_session"); }
        let normalized = origins::normalize(&[origin.to_owned()])?;
        let origin = normalized.first().filter(|value| value.starts_with("https://"))
            .ok_or("invalid_origin")?.clone();
        let mut random = [0u8; 32];
        rand::rngs::OsRng.try_fill_bytes(&mut random).map_err(|_| "random_unavailable")?;
        random[6] = (random[6] & 0x0f) | 0x40;
        random[8] = (random[8] & 0x3f) | 0x80;
        random[22] = (random[22] & 0x0f) | 0x40;
        random[24] = (random[24] & 0x3f) | 0x80;
        let id = uuid::Uuid::from_slice(&random[..16]).map_err(|_| "random_unavailable")?.to_string();
        let challenge = uuid::Uuid::from_slice(&random[16..]).map_err(|_| "random_unavailable")?.to_string();
        let expires = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        if current.trust != *expected { return Err("binding_changed"); }
        if current.hybrid_unconfirmed || current.hybrid_trust.as_ref().and_then(|trust| trust.outcome_key_b64.as_ref()).is_none() { return Err("hybrid_trust_unavailable"); }
        if current.service_enabled || current.credential.is_some() || !current.origins.contains(&origin) {
            return Err("origin_not_authorized");
        }
        // A failed replacement or persistence never leaves the old grant live.
        guard.hybrid_enrollment = None;
        guard.hybrid_claim = None;
        let mut next = guard.clone();
        let record = next.record.as_mut().ok_or("not_provisioned")?;
        record.connection_revision = record.connection_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.credential_revision = record.credential_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.cleared = None;
        if record.audit.len() >= 64 { record.audit.remove(0); }
        record.audit.push(audit);
        let revision = record.connection_revision;
        next.pending = None;
        next.contact_challenge = None;
        self.persist(&next)?;
        if std::time::Instant::now() >= expires {
            *guard = next;
            return Err("hybrid_enrollment_expired");
        }
        next.hybrid_enrollment = Some(PendingHybridEnrollment { id: id.clone(), challenge: challenge.clone(),
            origin: origin.clone(), caller_sid: actor.into(), session_id: session,
            trust: expected.clone(), connection_revision: revision, expires });
        *guard = next;
        Ok(HybridEnrollmentOffer { authorization_id: id, challenge, origin })
    }

    pub fn cancel_hybrid_enrollment(&self, id: &str, actor: &str, session: u32) -> Result<bool, &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        if guard.hybrid_enrollment.as_ref().is_some_and(|pending|
            pending.id == id && pending.caller_sid == actor && pending.session_id == session) {
            guard.hybrid_enrollment = None;
            return Ok(true);
        }
        if guard.hybrid_claim.as_ref().is_some_and(|claim| claim.matches_owner(id, actor, session)) {
            guard.hybrid_claim = None;
            return Ok(true);
        }
        Ok(false)
    }

    /// The helper reads only the live grant for its captured SID and session.
    pub fn hybrid_trust_for_helper(&self, actor: &str, session: u32) -> Result<Option<(FleetTrust, FleetHybridTrust, HybridEnrollmentOffer)>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let Some(record) = guard.record.as_ref() else { return Ok(None); };
        if record.hybrid_unconfirmed { return Err("store_unavailable"); }
        let pending = guard.hybrid_enrollment.as_ref().ok_or("hybrid_enrollment_not_authorized")?;
        if pending.caller_sid != actor || session == 0 || pending.session_id != session
            || pending.trust != record.trust || pending.connection_revision != record.connection_revision
            || std::time::Instant::now() >= pending.expires || record.service_enabled || record.credential.is_some() {
            return Err("hybrid_enrollment_not_authorized");
        }
        Ok(record.hybrid_trust.as_ref().map(|hybrid| (record.trust.clone(), hybrid.clone(),
            HybridEnrollmentOffer { authorization_id: pending.id.clone(), challenge: pending.challenge.clone(), origin: pending.origin.clone() })))
    }

    /// Explicit administrator-authorized re-enrollment. Comparing the full
    /// previous binding prevents a delayed confirmation replacing a newer one.
    pub fn replace_trust(
        &self,
        previous: FleetTrust,
        replacement: FleetTrust,
        signature: &str,
        actor: &str,
    ) -> Result<(), &'static str> {
        validate_trust(&previous)?;
        validate_trust(&replacement)?;
        if previous
            .enrollment_generation
            .eq_ignore_ascii_case(&replacement.enrollment_generation)
        {
            return Err("generation_must_change");
        }
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        if current.trust != previous {
            return Err("binding_changed");
        }
        if current.hybrid_trust.is_some() { return Err("hybrid_replacement_required"); }
        Self::verify_enrollment(&guard, &replacement, signature)?;
        let mut audit = current.audit.clone();
        if audit.len() >= 64 {
            audit.remove(0);
        }
        audit.push(trust_audit(
            "reenroll",
            actor,
            &replacement.enrollment_generation,
        )?);
        let approved_origins = guard.pending.as_ref().ok_or("enrollment_not_started")?.origins.clone();
        let approved_profile = guard.pending.as_ref().ok_or("enrollment_not_started")?.transport_profile.clone();
        let next = State {
            record: Some(Record {
                trust: replacement,
                hybrid_trust: None,
                hybrid_unconfirmed: false,
                policy: None,
                audit,
                contact: None,
                origins: approved_origins,
                transport_profile: approved_profile,
                credential: None,
                // Authorized replacement may return to a previously used
                // trust tuple. Never recreate that tuple's old local tokens.
                credential_revision: current.credential_revision.checked_add(1).ok_or("revision_exhausted")?,
                connection_revision: current.connection_revision.checked_add(1).ok_or("revision_exhausted")?,
                cleared: None,
                runtime_profile_changed: false,
                service_enabled: true,
            }),
            pending: None,
            contact_challenge: None,
            hybrid_enrollment: None,
            hybrid_claim: None,
        };
        self.persist(&next)?;
        *guard = next;
        Ok(())
    }

    /// Complete a privileged, freshly challenged enrollment for unchanged
    /// machine trust. This can authorize its configured transport origins
    /// without changing an otherwise valid signed policy or generation.
    pub fn bind_origins(
        &self,
        expected: FleetTrust,
        signature: &str,
        actor: &str,
    ) -> Result<(), &'static str> {
        validate_trust(&expected)?;
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        if current.trust != expected { return Err("binding_changed"); }
        if current.hybrid_trust.is_some() { return Err("hybrid_enrollment_required"); }
        Self::verify_enrollment(&guard, &expected, signature)?;
        let approved = guard.pending.as_ref().ok_or("enrollment_not_started")?.origins.clone();
        let approved_profile = guard.pending.as_ref().ok_or("enrollment_not_started")?.transport_profile.clone();
        let mut next = guard.clone();
        let record = next.record.as_mut().ok_or("not_provisioned")?;
        if !record.service_enabled || record.origins != approved || record.transport_profile != approved_profile {
            if record.audit.len() >= 64 { record.audit.remove(0); }
            record.audit.push(trust_audit("origins", actor, &expected.enrollment_generation)?);
            record.origins = approved;
            record.transport_profile = approved_profile;
            record.credential = None;
            record.credential_revision = record.credential_revision.checked_add(1).ok_or("revision_exhausted")?;
            record.runtime_profile_changed = false;
            record.service_enabled = true;
            record.connection_revision = record.connection_revision.checked_add(1).ok_or("revision_exhausted")?;
            record.cleared = None;
        }
        next.pending = None;
        next.contact_challenge = None;
        self.persist(&next)?;
        *guard = next;
        Ok(())
    }

    pub fn authorized_origins(&self) -> Result<Option<Vec<String>>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        Ok(guard.record.as_ref().map(|record| record.origins.clone()))
    }

    /// A running pinned helper may report a mismatch only to suspend service
    /// transport. It cannot authorize new destinations or security settings.
    pub(crate) fn observe_runtime_transport(&self, expected: &FleetTrust, observed_origins: &[String], observed_profile: Option<&TransportProfile>, connection_token: &str) -> Result<bool, &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        if current.trust != *expected || Self::connection_token_for(current)? != connection_token { return Err("binding_changed"); }
        if !current.service_enabled { return Err("service_disabled"); }
        let matches = origins::normalize(observed_origins).ok().as_deref() == Some(current.origins.as_slice())
            && observed_profile.is_some() && current.transport_profile.as_ref() == observed_profile;
        if matches { return Ok(true); }
        if current.runtime_profile_changed { return Ok(false); }
        let mut next = guard.clone();
        let record = next.record.as_mut().ok_or("not_provisioned")?;
        record.runtime_profile_changed = true;
        record.credential_revision = record.credential_revision.checked_add(1).ok_or("revision_exhausted")?;
        self.persist(&next)?;
        *guard = next;
        Ok(false)
    }

    pub(crate) fn credential_missing_reason(&self) -> &'static str {
        let Ok(guard) = self.state.lock() else { return "service_store_unavailable"; };
        match &guard.record {
            None => "not_configured",
            Some(record) if !record.service_enabled => "not_configured",
            Some(record) if record.runtime_profile_changed => "network_configuration_differs",
            Some(record) if record.origins.is_empty() || record.transport_profile.is_none() => "not_configured",
            Some(_) => "credential_unavailable",
        }
    }

    pub fn install(&self, policy: PolicyEnvelope) -> Result<(), &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        if current.hybrid_trust.is_some() { return Err("hybrid_policy_required"); }
        verify_policy(&current.trust, &policy)?;
        if let Some(old) = &current.policy {
            if policy.shield_revision < old.shield_revision {
                return Err("rollback");
            }
            let old_revision = old.config_revision.ok_or("missing_config_revision")?;
            let revision = policy.config_revision.ok_or("missing_config_revision")?;
            if revision < old_revision {
                return Err("rollback");
            }
            let old_resolution = old
                .resolution_revision
                .ok_or("missing_resolution_revision")?;
            let resolution = policy
                .resolution_revision
                .ok_or("missing_resolution_revision")?;
            if resolution < old_resolution {
                return Err("rollback");
            }
            if resolution == old_resolution
                && ((revision == old_revision && old.config != policy.config)
                    || (policy.shield_revision == old.shield_revision
                        && old.shield != policy.shield))
            {
                return Err("conflicting_revision");
            }
            if old == &policy {
                return Ok(());
            }
        }
        let next = State {
            record: Some(Record {
                trust: current.trust.clone(),
                hybrid_trust: current.hybrid_trust.clone(),
                hybrid_unconfirmed: current.hybrid_unconfirmed,
                policy: Some(policy),
                audit: current.audit.clone(),
                contact: current.contact.clone(),
                origins: current.origins.clone(),
                transport_profile: current.transport_profile.clone(),
                credential: current.credential.clone(),
                credential_revision: current.credential_revision,
                connection_revision: current.connection_revision,
                cleared: current.cleared.clone(),
                runtime_profile_changed: current.runtime_profile_changed,
                service_enabled: current.service_enabled,
            }),
            pending: guard.pending.clone(),
            contact_challenge: guard.contact_challenge.clone(),
            hybrid_enrollment: guard.hybrid_enrollment.clone(),
            hybrid_claim: guard.hybrid_claim.clone(),
        };
        self.persist(&next)?;
        *guard = next;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Option<PolicyEnvelope>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        Ok(guard
            .record
            .as_ref()
            .ok_or("not_provisioned")?
            .policy
            .clone())
    }

    /// A missing trust record is distinct from a provisioned machine whose
    /// first signed policy has not arrived. Never map store failure to false.
    pub fn read_current<T>(&self,read:impl FnOnce(bool,Option<&PolicyEnvelope>)->Result<T,&'static str>)->Result<T,&'static str> {
        let guard=self.state.lock().map_err(|_|"store_unavailable")?;
        read(guard.record.is_some(),guard.record.as_ref().and_then(|record|record.policy.as_ref()))
    }

    pub fn trust_and_policy(&self) -> Result<(bool, Option<PolicyEnvelope>), &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        Ok(match &guard.record {
            None => (false, None),
            Some(record) => (true, record.policy.clone()),
        })
    }

    pub fn trust_snapshot(&self) -> Result<Option<(FleetTrust, Vec<TrustAudit>)>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        Ok(guard
            .record
            .as_ref()
            .map(|record| (record.trust.clone(), record.audit.clone())))
    }

    pub fn trust_with_origins(&self) -> Result<Option<TrustWithTransport>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        Ok(guard.record.as_ref().map(|record| (record.trust.clone(), record.audit.clone(), record.origins.clone(), record.transport_profile.clone(), record.service_enabled)))
    }

    fn persist(&self, state: &State) -> Result<(), &'static str> {
        if let Some(record) = &state.record { validate_hybrid_state(record)?; }
        let bytes = serde_json::to_vec(state).map_err(|_| "invalid_store")?;
        self.fs
            .atomic_write(&self.path, &bytes)
            .map_err(|_| "store_unavailable")
    }

    pub fn begin_contact(&self) -> Result<ContactChallenge, &'static str> {
        self.begin_contact_with_clock(std::time::Instant::now)
    }

    fn begin_contact_with_clock(
        &self,
        now: impl FnOnce() -> std::time::Instant,
    ) -> Result<ContactChallenge, &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let trust = guard
            .record
            .as_ref()
            .ok_or("not_provisioned")?
            .trust
            .clone();
        let pending = contact::PendingContact::issue(trust, now())?;
        let challenge = ContactChallenge {
            nonce: pending.nonce.clone(),
        };
        guard.contact_challenge = Some(pending);
        Ok(challenge)
    }

    pub fn contact_pending(&self, nonce: &str) -> bool {
        let Ok(guard) = self.state.lock() else {
            return false;
        };
        guard.contact_challenge.as_ref().is_some_and(|pending| {
            pending.nonce == nonce
                && pending.fresh(std::time::Instant::now())
                && guard
                    .record
                    .as_ref()
                    .is_some_and(|record| record.trust == pending.trust)
        })
    }

    pub fn record_contact(
        &self,
        nonce: &str,
        request: &[u8],
        response: &[u8],
        receipt: &str,
    ) -> Result<ContactObservation, &'static str> {
        self.record_contact_with_clock(nonce, request, response, receipt, || {
            (std::time::Instant::now(), contact::observed_time())
        })
    }

    fn record_contact_with_clock(
        &self,
        nonce: &str,
        request: &[u8],
        response: &[u8],
        receipt: &str,
        mut clock: impl FnMut() -> (std::time::Instant, Result<u64, &'static str>),
    ) -> Result<ContactObservation, &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        if guard
            .contact_challenge
            .as_ref()
            .map_or(true, |pending| pending.nonce != nonce)
        {
            return Err("contact_not_pending");
        }
        // Consume matching attempts even when verification or persistence fails.
        let pending = guard
            .contact_challenge
            .take()
            .ok_or("contact_not_pending")?;
        if !pending.fresh(clock().0) {
            return Err("contact_expired");
        }
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        if current.trust != pending.trust {
            return Err("binding_changed");
        }
        pending.verify(request, response, receipt)?;
        let time = clock();
        if !pending.fresh(time.0) {
            return Err("contact_expired");
        }
        let observation = ContactObservation {
            observed_unix_ms: time.1?,
        };
        if current
            .contact
            .as_ref()
            .is_some_and(|old| old.observed_unix_ms > observation.observed_unix_ms)
        {
            return Err("contact_clock_rollback");
        }
        let mut next = guard.clone();
        next.record.as_mut().ok_or("not_provisioned")?.contact = Some(observation.clone());
        self.persist(&next)?;
        *guard = next;
        Ok(observation)
    }

    pub fn contact_observation(
        &self,
    ) -> Result<Option<(String, ContactObservation)>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        Ok(guard.record.as_ref().and_then(|record| {
            record
                .contact
                .clone()
                .map(|observation| (record.trust.device_id.clone(), observation))
        }))
    }

    pub(crate) fn credential_snapshot(
        &self,
        protector: &dyn crate::fleet_credential::Protector,
    ) -> Result<Option<CredentialSnapshot>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let Some(record) = &guard.record else { return Ok(None); };
        if !record.service_enabled { return Ok(None); }
        if record.runtime_profile_changed { return Err("network_configuration_differs"); }
        let (Some(profile), Some(credential)) = (&record.transport_profile, &record.credential) else { return Ok(None); };
        validate_transport_profile(profile)?;
        if origins::normalize(&record.origins)? != record.origins
            || !record.origins.contains(&credential.origin) {
            return Err("origin_not_authorized");
        }
        let ciphertext = STANDARD.decode(&credential.ciphertext).map_err(|_| "invalid_fleet_credential")?;
        if STANDARD.encode(&ciphertext) != credential.ciphertext { return Err("invalid_fleet_credential"); }
        let binding = crate::fleet_credential::CredentialBinding {
            org_id: &record.trust.org_id, device_id: &record.trust.device_id,
            enrollment_generation: &record.trust.enrollment_generation, origin: &credential.origin,
        };
        let secret = crate::fleet_credential::unseal(protector, &binding, &ciphertext)?;
        Ok(Some(CredentialSnapshot {
            trust: record.trust.clone(), origins: record.origins.clone(), profile: profile.clone(),
            revision: record.credential_revision, secret,
        }))
    }

    pub(crate) fn credential_is_current(&self, snapshot: &CredentialSnapshot) -> bool {
        let Ok(guard) = self.state.lock() else { return false; };
        guard.record.as_ref().is_some_and(|record| {
            record.service_enabled && record.trust == snapshot.trust && record.credential.is_some()
                && record.credential_revision == snapshot.revision
                && record.origins == snapshot.origins && record.transport_profile.as_ref() == Some(&snapshot.profile)
        })
    }

    pub(crate) fn credential_revision_is_current(&self, trust: &FleetTrust, revision: u64) -> bool {
        let Ok(guard) = self.state.lock() else { return false; };
        guard.record.as_ref().is_some_and(|record| {
            record.service_enabled && record.trust == *trust && record.credential.is_some() && record.credential_revision == revision
        })
    }

    pub(crate) fn candidate_is_current(&self, context: &CandidateContext) -> bool {
        let Ok(guard) = self.state.lock() else { return false; };
        guard.record.as_ref().is_some_and(|record| {
            record.service_enabled && record.trust == context.trust && record.origins == context.origins
                && record.transport_profile.as_ref() == Some(&context.profile)
                && record.credential_revision == context.revision && record.connection_revision == context.connection_revision
                && record.origins.contains(&context.origin)
        })
    }

    pub(crate) fn prepare_candidate(
        &self, expected_connection: (&FleetTrust, &str), observed_origins: &[String], profile: &TransportProfile,
        origin: &str, secret: &[u8], protector: &dyn crate::fleet_credential::Protector,
    ) -> Result<Option<CandidateContext>, &'static str> {
        use subtle::ConstantTimeEq;
        let (trust, connection_token) = expected_connection;
        if secret.len() != 32 { return Err("invalid_fleet_credential"); }
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let record = guard.record.as_ref().ok_or("not_provisioned")?;
        if Self::connection_token_for(record)? != connection_token { return Err("binding_changed"); }
        if !record.service_enabled { return Err("service_disabled"); }
        if record.trust != *trust || record.transport_profile.as_ref() != Some(profile)
            || origins::normalize(observed_origins)? != record.origins
            || !record.origins.iter().any(|allowed| allowed == origin) {
            return Err("transport_not_authorized");
        }
        validate_transport_profile(profile)?;
        if !record.runtime_profile_changed {
            if let Some(old) = record.credential.as_ref().filter(|old| old.origin == origin) {
                let binding = crate::fleet_credential::CredentialBinding { org_id:&trust.org_id, device_id:&trust.device_id,
                    enrollment_generation:&trust.enrollment_generation, origin };
                if let Ok(ciphertext) = STANDARD.decode(&old.ciphertext) {
                    if let Ok(existing) = crate::fleet_credential::unseal(protector, &binding, &ciphertext) {
                        if bool::from(existing.as_slice().ct_eq(secret)) { return Ok(None); }
                    }
                }
            }
        }
        Ok(Some(CandidateContext { trust:trust.clone(), origins:record.origins.clone(), profile:profile.clone(),
            revision:record.credential_revision, connection_revision:record.connection_revision, origin:origin.to_owned() }))
    }

    /// Called only after the service has verified a fresh signed presence
    /// response for this exact origin and candidate secret.
    pub(crate) fn commit_candidate(
        &self, context: &CandidateContext, secret: &[u8], protector: &dyn crate::fleet_credential::Protector,
    ) -> Result<u64, &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        if !current.service_enabled || current.trust != context.trust || current.origins != context.origins
            || current.transport_profile.as_ref() != Some(&context.profile)
            || current.credential_revision != context.revision || current.connection_revision != context.connection_revision
            || !current.origins.contains(&context.origin) {
            return Err("binding_changed");
        }
        let binding = crate::fleet_credential::CredentialBinding { org_id:&context.trust.org_id,
            device_id:&context.trust.device_id, enrollment_generation:&context.trust.enrollment_generation, origin:&context.origin };
        let ciphertext = crate::fleet_credential::seal(protector, &binding, secret)?;
        let mut next = guard.clone();
        let record = next.record.as_mut().ok_or("not_provisioned")?;
        record.credential_revision = record.credential_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.credential = Some(StoredCredential { origin:context.origin.clone(), ciphertext:STANDARD.encode(ciphertext) });
        record.runtime_profile_changed = false;
        let revision = record.credential_revision;
        self.persist(&next)?;
        *guard = next;
        Ok(revision)
    }

    fn clear_token(record: &Record) -> Result<String, &'static str> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(&["wincommander:fleet-service-clear:v1", &record.trust.org_id,
            &record.trust.device_id, &record.trust.enrollment_generation, &record.trust.signer_key,
            &record.credential_revision.to_string()]).map_err(|_| "invalid_store")?;
        Ok(STANDARD.encode(Sha256::digest(bytes)))
    }

    fn connection_token_for(record: &Record) -> Result<String, &'static str> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(&["wincommander:fleet-service-connection:v1", &record.trust.org_id,
            &record.trust.device_id, &record.trust.enrollment_generation, &record.trust.signer_key,
            &record.connection_revision.to_string()]).map_err(|_| "invalid_store")?;
        Ok(STANDARD.encode(Sha256::digest(bytes)))
    }

    #[cfg(test)]
    pub(crate) fn connection_token(&self) -> Result<String, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        Self::connection_token_for(guard.record.as_ref().ok_or("not_provisioned")?)
    }

    /// A new local connection intent fences old clears and offers, but grants
    /// neither enrollment nor permission to reenable a disabled service.
    pub(crate) fn start_connection(&self, expected: &FleetTrust, token: &str, connection_token: &str) -> Result<(String, String), &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        if current.trust != *expected || Self::clear_token(current)? != token
            || Self::connection_token_for(current)? != connection_token {
            return Err("binding_changed");
        }
        guard.hybrid_enrollment = None;
        guard.hybrid_claim = None;
        let mut next = guard.clone();
        let record = next.record.as_mut().ok_or("not_provisioned")?;
        record.credential_revision = record.credential_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.connection_revision = record.connection_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.cleared = None;
        let result = (Self::connection_token_for(record)?, Self::clear_token(record)?);
        self.persist(&next)?;
        *guard = next;
        Ok(result)
    }

    pub(crate) fn credential_clear_token(&self) -> Result<Option<String>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        guard.record.as_ref().map(Self::clear_token).transpose()
    }

    pub(crate) fn contact_state(&self) -> Result<Option<ContactState>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        guard.record.as_ref().map(|record| {
            let token = Some(Self::clear_token(record)?);
            Ok((record.trust.clone(), record.contact.clone(), token, Self::connection_token_for(record)?, record.credential_revision))
        }).transpose()
    }

    pub(crate) fn clear_credential(&self, token: &str, connection_token: &str) -> Result<bool, &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let Some(record) = &guard.record else { return Ok(false); };
        if !record.service_enabled && record.credential.is_none()
            && record.cleared.as_ref().is_some_and(|ack| ack.token == token && ack.connection_token == connection_token) {
            // Read-only acknowledgement of the exact committed operation. New
            // connection intent, rebind or replacement discards this receipt.
            return Ok(true);
        }
        if Self::clear_token(record)? != token || Self::connection_token_for(record)? != connection_token {
            return Ok(false);
        }
        guard.hybrid_enrollment = None;
        guard.hybrid_claim = None;
        let mut next = guard.clone();
        let record = next.record.as_mut().ok_or("not_provisioned")?;
        record.credential = None;
        record.service_enabled = false;
        record.connection_revision = record.connection_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.cleared = Some(ClearAcknowledgement { token:token.to_owned(), connection_token:connection_token.to_owned() });
        record.credential_revision = record.credential_revision.checked_add(1).ok_or("revision_exhausted")?;
        next.pending = None;
        self.persist(&next)?;
        *guard = next;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "fleet_contact_test_support.rs"]
mod contact_test_support;
#[cfg(test)]
#[path = "fleet_contact_tests.rs"]
mod contact_tests;
#[cfg(test)]
#[path = "fleet_contact_binding_tests.rs"]
mod contact_binding_tests;
#[cfg(test)]
#[path = "fleet_contact_v2_tests.rs"]
mod contact_v2_tests;
#[cfg(test)]
#[path = "fleet_contact_bridge_tests.rs"]
mod contact_bridge_tests;
#[cfg(test)]
#[path = "fleet_credential_integration_tests.rs"]
pub(crate) mod credential_integration_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_store::PolicyStoreError;
    use ed25519_dalek::{Signer, SigningKey};
    use std::{io, path::Path, sync::Arc};

    #[derive(Clone, Default)]
    struct MemoryFs(Arc<Mutex<Option<Vec<u8>>>>);
    impl PolicyFs for MemoryFs {
        fn read(&self, _: &Path) -> io::Result<Vec<u8>> {
            self.0
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| io::ErrorKind::NotFound.into())
        }
        fn atomic_write(&self, _: &Path, bytes: &[u8]) -> io::Result<()> {
            *self.0.lock().unwrap() = Some(bytes.to_vec());
            Ok(())
        }
        fn ensure_dir_secure(&self, _: &Path) -> Result<(), PolicyStoreError> {
            Ok(())
        }
    }
    struct InsecureFs;
    impl PolicyFs for InsecureFs {
        fn read(&self, _: &Path) -> io::Result<Vec<u8>> {
            panic!("insecure storage must never be read")
        }
        fn atomic_write(&self, _: &Path, _: &[u8]) -> io::Result<()> {
            panic!("insecure storage must never be written")
        }
        fn ensure_dir_secure(&self, _: &Path) -> Result<(), PolicyStoreError> {
            Err(PolicyStoreError::InsecurePermissions)
        }
    }
    #[derive(Clone, Default)]
    struct FailingFs { inner: MemoryFs, fail: Arc<std::sync::atomic::AtomicBool>, commit_first: Arc<std::sync::atomic::AtomicBool> }
    impl PolicyFs for FailingFs {
        fn read(&self, path: &Path) -> io::Result<Vec<u8>> { self.inner.read(path) }
        fn atomic_write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                if self.commit_first.load(std::sync::atomic::Ordering::SeqCst) { self.inner.atomic_write(path, bytes)?; }
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            self.inner.atomic_write(path, bytes)
        }
        fn ensure_dir_secure(&self, path: &Path) -> Result<(), PolicyStoreError> { self.inner.ensure_dir_secure(path) }
    }
    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[31; 32])
    }
    fn trust() -> FleetTrust {
        FleetTrust {
            org_id: "test-org".into(),
            device_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
            enrollment_generation: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
            signer_key: STANDARD.encode(signing_key().verifying_key().as_bytes()),
        }
    }
    fn policy(revision: i64) -> PolicyEnvelope {
        let binding = trust();
        let mut result = PolicyEnvelope {
            org_id: fleet_proto::OrgId(binding.org_id),
            device_id: fleet_proto::DeviceId(binding.device_id),
            enrollment_generation: Some(binding.enrollment_generation),
            config_revision: Some(revision),
            resolution_revision: Some(revision),
            config: None,
            shield: None,
            shield_revision: 0,
            clipboard_guard_enabled: false,
            signature: String::new(),
            signer_key: binding.signer_key,
        };
        sign(&mut result);
        result
    }
    fn sign(policy: &mut PolicyEnvelope) {
        policy.signature = STANDARD.encode(signing_key().sign(&policy.preimage()).to_bytes());
    }
    fn store(fs: MemoryFs) -> FleetMachinePolicy {
        FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap()
    }
    fn proof(store: &FleetMachinePolicy, trust: &FleetTrust) -> String {
        proof_with_origins(store, trust, Vec::new())
    }
    fn proof_with_origins(store: &FleetMachinePolicy, trust: &FleetTrust, origins: Vec<String>) -> String {
        let challenge = store
            .begin_enrollment(trust.device_id.clone(), trust.signer_key.clone(), origins, None)
            .unwrap();
        let bytes = serde_json::to_vec(&[
            "wincommander:shield-progress:enrollment:v1",
            &trust.org_id,
            &trust.device_id,
            &trust.enrollment_generation,
            &challenge,
        ])
        .unwrap();
        STANDARD.encode(signing_key().sign(&bytes).to_bytes())
    }
    fn provision(store: &FleetMachinePolicy) {
        let t = trust();
        let signature = proof(store, &t);
        store.provision(t, &signature, "S-1-5-18").unwrap();
    }

    fn hybrid() -> FleetHybridTrust {
        FleetHybridTrust { minimum_epoch: 7, ed_key_id: "fleet-ed".into(), pq_key_id: "fleet-pq".into(),
            ml_dsa_65_public_b64: STANDARD.encode([42u8; 1952]),
            outcome_key_b64: Some(STANDARD.encode(SigningKey::from_bytes(&[7; 32]).verifying_key().as_bytes())) }
    }

    fn hybrid_request(offer: &HybridEnrollmentOffer) -> Vec<u8> {
        hybrid_request_with_key(offer, 7)
    }

    fn hybrid_request_with_key(offer: &HybridEnrollmentOffer, seed: u8) -> Vec<u8> {
        let outcome = SigningKey::from_bytes(&[seed; 32]);
        let mut body = serde_json::json!({"device_id":trust().device_id, "device_hash":"device-hash",
            "device_kind":"wincommander", "agent_version":"3.6.7", "protocol_version":1,
            "capabilities":["hmac_body_v2"], "outcome_pubkey_b64":STANDARD.encode(outcome.verifying_key().as_bytes()),
            "shield_progress_challenge":offer.challenge, "enrollment_proof_version":2,
            "enrollment_server_key_b64":trust().signer_key,
            "enrollment_nonce":"44444444-4444-4444-8444-444444444444", "enrollment_ts":42 });
        let mut preimage = b"WinCommander Fleet enrollment proof v2\nPOST\n/v1/windows/enroll-v2\n".to_vec();
        preimage.extend(fleet_proto::canonical_json_bytes(&body));
        body["enrollment_signature_b64"] = STANDARD.encode(outcome.sign(&preimage).to_bytes()).into();
        serde_json::to_vec(&body).unwrap()
    }

    fn signed_request_with_extra(offer: &HybridEnrollmentOffer, extra: serde_json::Value) -> Vec<u8> {
        let mut body: serde_json::Value = serde_json::from_slice(&hybrid_request(offer)).unwrap();
        body.as_object_mut().unwrap().remove("enrollment_signature_b64");
        body["extra"] = extra;
        let mut preimage = b"WinCommander Fleet enrollment proof v2\nPOST\n/v1/windows/enroll-v2\n".to_vec();
        preimage.extend(fleet_proto::canonical_json_bytes(&body));
        body["enrollment_signature_b64"] = STANDARD.encode(SigningKey::from_bytes(&[7;32]).sign(&preimage).to_bytes()).into();
        serde_json::to_vec(&body).unwrap()
    }

    fn hybrid_ready() -> (MemoryFs, FleetMachinePolicy) {
        let fs = MemoryFs::default();
        let store = store(fs.clone());
        let signature = proof_with_origins(&store, &trust(), vec!["https://fleet.example".into()]);
        store.provision(trust(), &signature, "S-1-5-18").unwrap();
        store.provision_hybrid_trust(trust(), hybrid(), "S-1-5-18").unwrap();
        (fs, store)
    }

    #[test]
    fn hybrid_claim_is_one_use_exact_and_revocable() {
        let (fs, store) = hybrid_ready();
        let offer = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let bytes = hybrid_request(&offer);
        assert!(store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-8", 7, &bytes).is_err());
        let claim = store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &bytes).unwrap();
        assert_eq!(claim.request(), bytes);
        assert_eq!(claim.origin(), "https://fleet.example");
        assert_eq!(claim.challenge(), offer.challenge);
        assert_eq!(claim.outcome_key(), STANDARD.encode(SigningKey::from_bytes(&[7; 32]).verifying_key().as_bytes()));
        assert!(store.hybrid_claim_is_current(&claim));
        assert!(store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &bytes).is_err());
        assert_eq!(store.hybrid_trust_for_helper("S-1-5-21-7", 7), Err("hybrid_enrollment_not_authorized"));
        assert!(!store.cancel_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-8", 7).unwrap());
        let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        assert!(!reopened.hybrid_claim_is_current(&claim));
        assert!(store.cancel_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7).unwrap());
        assert!(!store.hybrid_claim_is_current(&claim));
    }

    #[test]
    fn hybrid_claim_is_revoked_by_clear_and_replacement() {
        let (_, store) = hybrid_ready();
        let offer = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let claim = store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &hybrid_request(&offer)).unwrap();
        let (_, _, token, connection, _) = store.contact_state().unwrap().unwrap();
        assert!(store.clear_credential(&token.unwrap(), &connection).unwrap());
        assert!(!store.hybrid_claim_is_current(&claim));

        let offer = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let claim = store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &hybrid_request(&offer)).unwrap();
        assert!(store.hybrid_claim_is_current(&claim));
        store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        assert!(!store.hybrid_claim_is_current(&claim));
    }

    #[test]
    fn hybrid_claim_rejects_malformed_or_tampered_requests_and_burns_the_grant() {
        let (_, store) = hybrid_ready();
        for mode in 0..10 {
            let offer = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
            let original = hybrid_request(&offer);
            let bad = match mode {
                0 => vec![b'x'; 32 * 1024 + 1],
                1 => b"{\"device_id\":\"a\",\"device_id\":\"b\"}".to_vec(),
                6 => hybrid_request_with_key(&offer, 8),
                7 => signed_request_with_extra(&offer, serde_json::json!({"$serde_json::private::Number":"1"})),
                8 => signed_request_with_extra(&offer, serde_json::json!(9007199254740992u64)),
                9 => signed_request_with_extra(&offer, serde_json::json!(1.0)),
                _ => {
                    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
                    match mode {
                        2 => value["shield_progress_challenge"] = "other".into(),
                        3 => value["device_id"] = "other".into(),
                        4 => value["enrollment_server_key_b64"] = STANDARD.encode([9u8;32]).into(),
                        _ => value["enrollment_signature_b64"] = STANDARD.encode([0u8;64]).into(),
                    }
                    serde_json::to_vec(&value).unwrap()
                }
            };
            assert!(store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &bad).is_err());
            assert!(store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &original).is_err());
        }
    }

    #[test]
    fn hybrid_claim_has_only_one_winner_when_callers_race() {
        use std::sync::Barrier;
        let (_, store) = hybrid_ready();
        let store = Arc::new(store);
        let offer = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let bytes = Arc::new(hybrid_request(&offer));
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0..8).map(|_| {
            let store = Arc::clone(&store); let bytes = Arc::clone(&bytes); let barrier = Arc::clone(&barrier);
            let id = offer.authorization_id.clone();
            std::thread::spawn(move || { barrier.wait(); store.claim_hybrid_enrollment(&id, "S-1-5-21-7", 7, &bytes).is_ok() })
        }).collect();
        assert_eq!(workers.into_iter().map(|worker| worker.join().unwrap()).filter(|success| *success).count(), 1);
    }

    #[test]
    fn hybrid_trust_is_administrator_pinned_and_disables_legacy_replacement() {
        let fs = MemoryFs::default();
        let store = store(fs.clone());
        provision(&store);
        store.install(policy(1)).unwrap();
        let old = store.trust_with_origins().unwrap().unwrap();
        assert!(old.4);
        let mut wrong = trust(); wrong.device_id = "other".into();
        assert_eq!(store.provision_hybrid_trust(wrong, hybrid(), "S-1-5-18"), Err("binding_changed"));
        assert_eq!(store.provision_hybrid_trust(trust(), hybrid(), "bad"), Err("invalid_actor"));
        assert_eq!(store.provision_hybrid_trust(trust(), hybrid(), "S-1-5-18"), Ok(()));
        assert!(!store.trust_with_origins().unwrap().unwrap().4);
        assert_eq!(store.hybrid_trust_for_helper("S-1-5-18", 1), Err("hybrid_enrollment_not_authorized"));
        assert_eq!(store.snapshot().unwrap(), Some(policy(1)));
        assert_eq!(store.provision_hybrid_trust(trust(), hybrid(), "S-1-5-18"), Ok(()));
        let mut changed = hybrid(); changed.minimum_epoch = 8;
        assert_eq!(store.provision_hybrid_trust(trust(), changed, "S-1-5-18"), Err("hybrid_rotation_required"));
        let mut replacement = trust(); replacement.enrollment_generation = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
        assert_eq!(store.replace_trust(trust(), replacement, "forged", "S-1-5-18"), Err("hybrid_replacement_required"));
        assert_eq!(store.bind_origins(trust(), "forged", "S-1-5-18"), Err("hybrid_enrollment_required"));
        assert_eq!(store.install(policy(1)), Err("hybrid_policy_required"));
        let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        assert_eq!(reopened.hybrid_trust_snapshot().unwrap(), Some((trust(), hybrid())));
        assert!(!reopened.trust_with_origins().unwrap().unwrap().4);
        assert_eq!(reopened.hybrid_trust_for_helper("S-1-5-18", 1), Err("hybrid_enrollment_not_authorized"));
        assert_eq!(reopened.snapshot().unwrap(), Some(policy(1)));
    }

    #[test]
    fn hybrid_trust_rejects_invalid_or_corrupt_keys_on_write_and_restart() {
        let fs = MemoryFs::default();
        let store = store(fs.clone());
        provision(&store);
        for bad in [FleetHybridTrust { minimum_epoch: 0, ..hybrid() },
            FleetHybridTrust { pq_key_id: "fleet-ed".into(), ..hybrid() },
            FleetHybridTrust { ml_dsa_65_public_b64: STANDARD.encode([0u8; 1952]), ..hybrid() },
            FleetHybridTrust { ml_dsa_65_public_b64: "invalid".into(), ..hybrid() }] {
            assert_eq!(store.provision_hybrid_trust(trust(), bad, "S-1-5-18"), Err("invalid_hybrid_trust"));
        }
        assert!(store.hybrid_trust_snapshot().unwrap().is_none());
        store.provision_hybrid_trust(trust(), hybrid(), "S-1-5-18").unwrap();
        let mut bytes = fs.0.lock().unwrap();
        let mut state: serde_json::Value = serde_json::from_slice(bytes.as_ref().unwrap()).unwrap();
        let mut enabled = state.clone();
        enabled["record"]["service_enabled"] = true.into();
        *bytes = Some(serde_json::to_vec(&enabled).unwrap());
        drop(bytes);
        assert!(matches!(FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")), Err("invalid_hybrid_state")));
        let mut bytes = fs.0.lock().unwrap();
        let mut credential = state.clone();
        credential["record"]["credential"] = serde_json::json!({"origin":"https://fleet.example", "ciphertext":"AA=="});
        *bytes = Some(serde_json::to_vec(&credential).unwrap());
        drop(bytes);
        assert!(matches!(FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")), Err("invalid_hybrid_state")));
        let mut bytes = fs.0.lock().unwrap();
        state["record"]["hybrid_trust"]["minimum_epoch"] = 0.into();
        *bytes = Some(serde_json::to_vec(&state).unwrap());
        drop(bytes);
        assert!(matches!(FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")), Err("invalid_hybrid_trust")));
    }

    #[test]
    fn legacy_hybrid_pin_requires_administrator_to_complete_outcome_key() {
        let (fs, store) = hybrid_ready();
        let mut bytes = fs.0.lock().unwrap();
        let mut state: serde_json::Value = serde_json::from_slice(bytes.as_ref().unwrap()).unwrap();
        state["record"]["hybrid_trust"].as_object_mut().unwrap().remove("outcome_key_b64");
        *bytes = Some(serde_json::to_vec(&state).unwrap());
        drop(bytes);
        drop(store);
        let reopened = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
        assert_eq!(reopened.hybrid_trust_snapshot().unwrap().unwrap().1.outcome_key_b64, None);
        assert_eq!(reopened.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7), Err("hybrid_trust_unavailable"));
        let mut changed = hybrid(); changed.pq_key_id = "changed".into();
        assert_eq!(reopened.provision_hybrid_trust(trust(), changed, "S-1-5-18"), Err("hybrid_rotation_required"));
        let mut missing = hybrid(); missing.outcome_key_b64 = None;
        assert_eq!(reopened.provision_hybrid_trust(trust(), missing, "S-1-5-18"), Err("outcome_key_required"));
        reopened.provision_hybrid_trust(trust(), hybrid(), "S-1-5-18").unwrap();
        let saved = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        assert_eq!(saved.hybrid_trust_snapshot().unwrap(), Some((trust(), hybrid())));
        assert!(saved.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).is_ok());
    }

    #[test]
    fn hybrid_trust_ambiguous_persistence_fences_legacy_until_confirmed() {
        for commit_first in [false, true] {
            let fs = FailingFs::default();
            let store = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
            provision(&store);
            fs.commit_first.store(commit_first, std::sync::atomic::Ordering::SeqCst);
            fs.fail.store(true, std::sync::atomic::Ordering::SeqCst);
            assert!(store.provision_hybrid_trust(trust(), hybrid(), "S-1-5-18").is_err());
            assert_eq!(store.hybrid_trust_snapshot(), Err("store_unavailable"));
            assert!(!store.trust_with_origins().unwrap().unwrap().4);
            assert!(store.provision_hybrid_trust(trust(), hybrid(), "S-1-5-18").is_err());
            fs.fail.store(false, std::sync::atomic::Ordering::SeqCst);
            let before_retry = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
            assert_eq!(before_retry.hybrid_trust_snapshot().unwrap().is_some(), commit_first);
            assert_eq!(before_retry.trust_with_origins().unwrap().unwrap().4, !commit_first);
            store.provision_hybrid_trust(trust(), hybrid(), "S-1-5-18").unwrap();
            let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
            assert_eq!(reopened.hybrid_trust_snapshot().unwrap(), Some((trust(), hybrid())));
            assert!(!reopened.trust_with_origins().unwrap().unwrap().4);
        }
    }

    #[test]
    fn hybrid_enrollment_grant_is_bound_revocable_and_lost_on_restart() {
        let fs = MemoryFs::default();
        let store = store(fs.clone());
        let signature = proof_with_origins(&store, &trust(), vec!["https://fleet.example".into()]);
        store.provision(trust(), &signature, "S-1-5-18").unwrap();
        assert_eq!(store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7), Err("hybrid_trust_unavailable"));
        store.provision_hybrid_trust(trust(), hybrid(), "S-1-5-18").unwrap();
        assert_eq!(store.hybrid_trust_for_helper("S-1-5-21-7", 7), Err("hybrid_enrollment_not_authorized"));
        assert_eq!(store.begin_hybrid_enrollment(&trust(), "http://fleet.example", "S-1-5-21-7", 7), Err("invalid_origin"));
        assert_eq!(store.begin_hybrid_enrollment(&trust(), "https://wrong.example", "S-1-5-21-7", 7), Err("origin_not_authorized"));
        assert_eq!(store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 0), Err("invalid_session"));
        let first = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        assert_eq!(store.hybrid_trust_for_helper("S-1-5-21-8", 7), Err("hybrid_enrollment_not_authorized"));
        assert_eq!(store.hybrid_trust_for_helper("S-1-5-21-7", 8), Err("hybrid_enrollment_not_authorized"));
        assert_eq!(store.hybrid_trust_for_helper("S-1-5-21-7", 7).unwrap().unwrap().2, first);
        assert!(!store.cancel_hybrid_enrollment(&first.authorization_id, "S-1-5-21-8", 7).unwrap());
        assert!(store.cancel_hybrid_enrollment(&first.authorization_id, "S-1-5-21-7", 7).unwrap());
        assert_eq!(store.hybrid_trust_for_helper("S-1-5-21-7", 7), Err("hybrid_enrollment_not_authorized"));
        let second = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        assert_ne!(first.authorization_id, second.authorization_id);
        assert!(!store.cancel_hybrid_enrollment(&first.authorization_id, "S-1-5-21-7", 7).unwrap());
        {
            let mut state = store.state.lock().unwrap();
            state.hybrid_enrollment.as_mut().unwrap().expires = std::time::Instant::now();
        }
        assert_eq!(store.hybrid_trust_for_helper("S-1-5-21-7", 7), Err("hybrid_enrollment_not_authorized"));
        store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        assert_eq!(reopened.hybrid_trust_for_helper("S-1-5-21-7", 7), Err("hybrid_enrollment_not_authorized"));
    }

    #[test]
    fn hybrid_enrollment_grant_is_cancelled_by_disconnect_and_failed_replacement() {
      for commit_first in [false, true] {
        let fs = FailingFs::default();
        let store = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
        let signature = proof_with_origins(&store, &trust(), vec!["https://fleet.example".into()]);
        store.provision(trust(), &signature, "S-1-5-18").unwrap();
        store.provision_hybrid_trust(trust(), hybrid(), "S-1-5-18").unwrap();
        store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let (_, _, clear, connection, _) = store.contact_state().unwrap().unwrap();
        assert!(store.clear_credential(&clear.unwrap(), &connection).unwrap());
        assert_eq!(store.hybrid_trust_for_helper("S-1-5-21-7", 7), Err("hybrid_enrollment_not_authorized"));
        store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        fs.commit_first.store(commit_first, std::sync::atomic::Ordering::SeqCst);
        fs.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).is_err());
        assert_eq!(store.hybrid_trust_for_helper("S-1-5-21-7", 7), Err("hybrid_enrollment_not_authorized"));
        fs.fail.store(false, std::sync::atomic::Ordering::SeqCst);
        let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        assert_eq!(reopened.hybrid_trust_for_helper("S-1-5-21-7", 7), Err("hybrid_enrollment_not_authorized"));
      }
    }

    #[test]
    fn provision_requires_one_use_challenge_and_rejects_replacement() {
        let store = store(MemoryFs::default());
        let signature = proof(&store, &trust());
        assert_eq!(
            store.provision(trust(), "forged", "S-1-5-18"),
            Err("invalid_proof")
        );
        assert_eq!(store.provision(trust(), &signature, "S-1-5-18"), Ok(()));
        assert_eq!(
            store.provision(trust(), &signature, "S-1-5-18"),
            Err("already_provisioned")
        );
        let mut replacement = trust();
        replacement.org_id = "other".into();
        assert_eq!(
            store.provision(replacement, &signature, "S-1-5-18"),
            Err("already_provisioned")
        );
    }

    #[test]
    fn privileged_enrollment_binds_only_staged_origins_and_fences_stale_updates() {
        let fs = MemoryFs::default();
        let store = store(fs.clone());
        let binding = trust();
        let initial = vec!["http://Fleet.Example:80".into(), "https://mesh.example".into()];
        let signature = proof_with_origins(&store, &binding, initial);
        store.provision(binding.clone(), &signature, "S-1-5-18").unwrap();
        assert_eq!(store.authorized_origins().unwrap().unwrap(), vec!["http://fleet.example", "https://mesh.example"]);
        let old = proof_with_origins(&store, &binding, vec!["https://old.example".into()]);
        let fresh = proof_with_origins(&store, &binding, vec!["https://new.example".into()]);
        assert_eq!(store.bind_origins(binding.clone(), &old, "S-1-5-18"), Err("invalid_proof"));
        let mut wrong = binding.clone(); wrong.org_id = "other".into();
        assert_eq!(store.bind_origins(wrong, &fresh, "S-1-5-18"), Err("binding_changed"));
        store.bind_origins(binding.clone(), &fresh, "S-1-5-18").unwrap();
        assert_eq!(store.authorized_origins().unwrap().unwrap(), vec!["https://new.example"]);
        assert_eq!(store.bind_origins(binding, &fresh, "S-1-5-18"), Err("enrollment_not_started"));
        let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        assert_eq!(reopened.authorized_origins().unwrap().unwrap(), vec!["https://new.example"]);
    }

    #[test]
    fn transport_profile_is_validated_and_persisted_only_with_enrollment_proof() {
        let fs = MemoryFs::default();
        let store = store(fs.clone());
        let binding = trust();
        let profile = TransportProfile { spki_pins: "a".repeat(64), environment_fingerprint: STANDARD.encode([7u8; 32]) };
        let challenge = store.begin_enrollment(binding.device_id.clone(), binding.signer_key.clone(), vec!["https://fleet.example".into()], Some(profile.clone())).unwrap();
        let bytes = serde_json::to_vec(&["wincommander:shield-progress:enrollment:v1", &binding.org_id, &binding.device_id, &binding.enrollment_generation, &challenge]).unwrap();
        let signature = STANDARD.encode(signing_key().sign(&bytes).to_bytes());
        store.provision(binding.clone(), &signature, "S-1-5-18").unwrap();
        let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        assert_eq!(reopened.trust_with_origins().unwrap().unwrap().3, Some(profile));
        let invalid = TransportProfile { spki_pins: String::new(), environment_fingerprint: "invalid".into() };
        assert_eq!(reopened.begin_enrollment(binding.device_id, binding.signer_key, Vec::new(), Some(invalid)), Err("invalid_transport_profile"));
    }

    #[test]
    fn policy_status_distinguishes_unbound_bound_and_signed_policy() {
        let store = store(MemoryFs::default());
        assert_eq!(store.trust_and_policy().unwrap(), (false, None));
        let signature = proof(&store, &trust());
        assert_eq!(store.trust_and_policy().unwrap(), (false, None));
        store.provision(trust(), &signature, "S-1-5-18").unwrap();
        assert_eq!(store.trust_and_policy().unwrap(), (true, None));
        store.install(policy(1)).unwrap();
        assert_eq!(store.trust_and_policy().unwrap(), (true, Some(policy(1))));
    }

    #[test]
    fn refuses_insecure_store_before_reading_any_trust() {
        assert!(matches!(
            FleetMachinePolicy::open(Box::new(InsecureFs), PathBuf::from("test")),
            Err("insecure_store")
        ));
    }

    #[test]
    fn enrollment_challenge_survives_restart_and_superseded_proofs_fail() {
        let fs = MemoryFs::default();
        let first = store(fs.clone());
        let stale = proof(&first, &trust());
        let current = proof(&first, &trust());
        let reopened = store(fs);
        assert_eq!(
            reopened.provision(trust(), &stale, "S-1-5-18"),
            Err("invalid_proof")
        );
        assert_eq!(reopened.provision(trust(), &current, "S-1-5-18"), Ok(()));
        assert_eq!(
            reopened.provision(trust(), &current, "S-1-5-18"),
            Err("already_provisioned")
        );
    }

    #[test]
    fn enrollment_proof_is_bound_to_device_key_and_org() {
        let store = store(MemoryFs::default());
        let signature = proof(&store, &trust());
        let mut different_device = trust();
        different_device.device_id = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
        assert_eq!(
            store.provision(different_device, &signature, "S-1-5-18"),
            Err("enrollment_binding_mismatch")
        );
        let mut different_org = trust();
        different_org.org_id = "other-org".into();
        assert_eq!(
            store.provision(different_org, &signature, "S-1-5-18"),
            Err("invalid_proof")
        );
        assert_eq!(store.provision(trust(), &signature, "S-1-5-18"), Ok(()));
    }

    #[test]
    fn device_binding_preserves_existing_fleet_identifier_semantics() {
        let store = store(MemoryFs::default());
        let mut binding = trust();
        binding.device_id = "Device-Alpha_42".into();
        let signature = proof(&store, &binding);
        store.provision(binding.clone(), &signature, "S-1-5-18").unwrap();
        assert_eq!(store.trust_snapshot().unwrap().unwrap().0.device_id, "Device-Alpha_42");
        binding.device_id.clear();
        assert_eq!(validate_trust(&binding), Err("invalid_binding"));
        binding.device_id = "x".repeat(129);
        assert_eq!(validate_trust(&binding), Err("invalid_binding"));
    }

    #[test]
    fn explicit_reenrollment_requires_current_binding_and_new_generation() {
        let store = store(MemoryFs::default());
        let old = trust();
        provision(&store);
        store.install(policy(1)).unwrap();
        let mut next = old.clone();
        next.enrollment_generation = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
        assert_eq!(
            store.replace_trust(old.clone(), old.clone(), "", "S-1-5-18"),
            Err("generation_must_change")
        );
        let mut wrong = old.clone();
        wrong.org_id = "other".into();
        let signature = proof(&store, &next);
        assert_eq!(
            store.replace_trust(wrong, next.clone(), &signature, "S-1-5-18"),
            Err("binding_changed")
        );
        store
            .replace_trust(old.clone(), next, &signature, "S-1-5-18")
            .unwrap();
        let (_, audit) = store.trust_snapshot().unwrap().unwrap();
        assert_eq!(audit.len(), 2);
        assert_eq!(audit[1].action, "reenroll");
        assert_eq!(audit[1].caller_sid, "S-1-5-18");
        assert_eq!(store.snapshot().unwrap(), None);
        assert_eq!(store.install(policy(1)), Err("generation_mismatch"));
        assert_eq!(
            store.replace_trust(old, trust(), &signature, "S-1-5-18"),
            Err("generation_must_change")
        );
    }

    #[test]
    fn signed_binding_and_both_revision_counters_are_required() {
        let store = store(MemoryFs::default());
        provision(&store);
        let mut value = policy(1);
        value.enrollment_generation = None;
        sign(&mut value);
        assert_eq!(store.install(value), Err("generation_mismatch"));
        let mut value = policy(1);
        value.config_revision = None;
        sign(&mut value);
        assert_eq!(store.install(value), Err("missing_config_revision"));
        let mut value = policy(1);
        value.resolution_revision = None;
        sign(&mut value);
        assert_eq!(store.install(value), Err("missing_resolution_revision"));
        let mut value = policy(1);
        value.device_id.0 = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
        sign(&mut value);
        assert_eq!(store.install(value), Err("binding_mismatch"));
        let mut value = policy(1);
        value.config_revision = Some(2);
        assert_eq!(store.install(value), Err("invalid_signature"));
    }

    #[test]
    fn inner_epoch_is_independently_verified_and_device_target_is_checked() {
        let store = store(MemoryFs::default());
        provision(&store);
        let mut value = policy(2);
        let mut config = fleet_proto::ConfigEpoch {
            org_id: fleet_proto::OrgId(trust().org_id),
            version: 2,
            config_json: serde_json::json!({"privacy":{"privacyShield":{"blurOpacity":80}}}),
            target_kind: "device".into(),
            target_id: Some(trust().device_id),
            locked_paths: vec![],
            managed: false,
            signature: String::new(),
            signer_key: trust().signer_key,
        };
        let preimage = fleet_proto::epoch_preimage(&EpochSigningInput {
            version: config.version,
            config: &config.config_json,
            locked_paths: &config.locked_paths,
            managed: config.managed,
            target_kind: &config.target_kind,
            target_id: config.target_id.as_deref(),
        });
        config.signature = STANDARD.encode(signing_key().sign(&preimage).to_bytes());
        value.config = Some(config.clone());
        sign(&mut value);
        store.install(value).unwrap();
        let mut next = policy(3);
        config.target_id = Some("cccccccc-cccc-4ccc-8ccc-cccccccccccc".into());
        next.config = Some(config);
        sign(&mut next);
        assert_eq!(store.install(next), Err("invalid_config"));
    }

    #[test]
    fn resolution_revision_advances_when_group_changes_effective_state() {
        let store = store(MemoryFs::default());
        provision(&store);
        store.install(policy(2)).unwrap();
        let mut changed = policy(2);
        changed.resolution_revision = Some(3);
        changed.shield = Some(fleet_proto::ShieldDesiredState {
            enabled: true,
            mode: "notify_only".into(),
            revision: 0,
            updated_at: "2026-01-01T00:00:00Z".into(),
            command_id: None,
        });
        sign(&mut changed);
        store.install(changed).unwrap();
        assert_eq!(store.install(policy(2)), Err("rollback"));
    }

    #[test]
    fn signed_policy_heads_may_advance_without_a_membership_change() {
        let store = store(MemoryFs::default());
        provision(&store);
        store.install(policy(2)).unwrap();
        let mut changed = policy(2);
        changed.shield_revision = 1;
        changed.shield = Some(fleet_proto::ShieldDesiredState {
            enabled: true,
            mode: "notify_only".into(),
            revision: 1,
            updated_at: "2026-01-01T00:00:00Z".into(),
            command_id: None,
        });
        sign(&mut changed);
        store.install(changed.clone()).unwrap();
        changed.config_revision = Some(3);
        let mut config = fleet_proto::ConfigEpoch {
            org_id: fleet_proto::OrgId(trust().org_id),
            version: 3,
            config_json: serde_json::json!({"privacy":{"privacyShield":{"blurOpacity":50}}}),
            target_kind: "org".into(),
            target_id: None,
            locked_paths: vec![],
            managed: false,
            signature: String::new(),
            signer_key: trust().signer_key,
        };
        let preimage = fleet_proto::epoch_preimage(&EpochSigningInput {
            version: config.version,
            config: &config.config_json,
            locked_paths: &config.locked_paths,
            managed: config.managed,
            target_kind: &config.target_kind,
            target_id: None,
        });
        config.signature = STANDARD.encode(signing_key().sign(&preimage).to_bytes());
        changed.config = Some(config);
        sign(&mut changed);
        store.install(changed).unwrap();
    }

    #[test]
    fn persisted_policy_is_reverified_and_replays_rejected_after_restart() {
        let fs = MemoryFs::default();
        let first = store(fs.clone());
        provision(&first);
        first.install(policy(2)).unwrap();
        let reopened = store(fs.clone());
        assert_eq!(reopened.snapshot().unwrap(), Some(policy(2)));
        assert_eq!(reopened.install(policy(1)), Err("rollback"));
        let mut changed = policy(2);
        changed.shield = Some(fleet_proto::ShieldDesiredState {
            enabled: true,
            mode: "notify_only".into(),
            revision: 0,
            updated_at: "2026-01-01T00:00:00Z".into(),
            command_id: None,
        });
        sign(&mut changed);
        assert_eq!(reopened.install(changed), Err("conflicting_revision"));
        reopened.install(policy(3)).unwrap();
        let mut bytes = fs.0.lock().unwrap();
        let mut state: State = serde_json::from_slice(bytes.as_ref().unwrap()).unwrap();
        state
            .record
            .as_mut()
            .unwrap()
            .policy
            .as_mut()
            .unwrap()
            .signature = "forged".into();
        *bytes = Some(serde_json::to_vec(&state).unwrap());
        drop(bytes);
        assert!(FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).is_err());
    }
}
