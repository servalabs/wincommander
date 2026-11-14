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
#[path = "fleet_hybrid_reply.rs"]
mod hybrid_reply;
#[path = "fleet_hybrid_store.rs"]
mod hybrid_store;
#[path = "fleet_hybrid_bootstrap.rs"]
mod hybrid_bootstrap;

#[cfg(windows)]
use crate::peer_auth_deployment_acl as protected_acl;
#[cfg(windows)]
#[path = "fleet_machine_owner.rs"]
mod machine_owner;

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
    let store = open_system_store(machine_store_dir()?)?;
    let _ = STORE.set(store);
    STORE.get().ok_or("store_unavailable")
}

#[cfg(windows)]
fn machine_store_dir() -> Result<PathBuf, &'static str> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
    };
    struct AllocatedPath(*mut u16);
    impl Drop for AllocatedPath {
        fn drop(&mut self) {
            unsafe { CoTaskMemFree(self.0.cast()) };
        }
    }
    let mut value = std::ptr::null_mut();
    // Windows allocates this buffer; the guard frees it even on failure.
    let result = unsafe {
        SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, std::ptr::null_mut(), &mut value)
    };
    let value = AllocatedPath(value);
    if result < 0 || value.0.is_null() {
        return Err("store_unavailable");
    }
    // The Windows API returns a NUL-terminated buffer valid until it is freed.
    let length = (0..32768)
        .find(|&offset| unsafe { *value.0.add(offset) } == 0)
        .filter(|&length| length != 0)
        .ok_or("store_unavailable")?;
    let base = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(value.0, length)
    }));
    if !base.is_absolute() {
        return Err("store_unavailable");
    }
    Ok(base.join("WinCommander").join("fleet-machine"))
}

#[cfg(windows)]
fn open_system_store(dir: PathBuf) -> Result<FleetMachinePolicy, &'static str> {
    let parent = machine_owner::protect_parent(&dir)?;
    let fs = WindowsFleetPolicyFs;
    fs.ensure_dir_secure(&dir).map_err(|_| "insecure_store")?;
    let lease = machine_owner::acquire(&dir)?.with_parent(parent);
    let mut store = FleetMachinePolicy::open(Box::new(fs), dir)?;
    store._owner = Some(lease);
    Ok(store)
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
        std::fs::symlink_metadata(path)?;
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
fn is_zero(value: &u64) -> bool { *value == 0 }
fn is_false(value: &bool) -> bool { !*value }

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCredential {
    origin: String,
    ciphertext: String,
}

pub(crate) struct CredentialSnapshot {
    pub trust: FleetTrust,
    pub origins: Vec<String>,
    pub origin: String,
    pub profile: TransportProfile,
    pub revision: u64,
    pub secret: zeroize::Zeroizing<Vec<u8>>,
    pub operator_key: Option<String>,
    pub is_recovered: bool,
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
struct RecoveryCommit {
    recovery_id:String, old_trust:FleetTrust, new_generation:String, decision_digest:String,
    archive_sha256:String, credential_sha256:String, candidate_fingerprint:String,
    origin:String, local_commit_digest:String, activation_pending:bool,
    #[serde(default)] server_status:Option<String>,
    #[serde(default)] rollback_sha256:Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    trust: FleetTrust,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hybrid_trust: Option<FleetHybridTrust>,
    #[serde(default, skip_serializing_if = "is_zero")]
    hybrid_epoch_floor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hybrid_credential: Option<StoredHybridCredential>,
    #[serde(default, skip_serializing_if = "is_false")]
    hybrid_initial_pending: bool,
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
    #[serde(default)]
    recovery_commit: Option<RecoveryCommit>,
    #[serde(default)]
    operator_key: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredHybridCredential {
    origin: String,
    ciphertext: String,
    epoch: u64,
}

fn effective_hybrid_trust(record: &Record) -> Option<FleetHybridTrust> {
    record.hybrid_trust.as_ref().map(|trust| {
        let mut trust = trust.clone();
        trust.minimum_epoch = trust.minimum_epoch.max(record.hybrid_epoch_floor);
        trust
    })
}

fn validate_hybrid_state(record: &Record) -> Result<(), &'static str> {
    if record.hybrid_initial_pending
        && (record.hybrid_trust.as_ref().and_then(|trust| trust.outcome_key_b64.as_ref()).is_none() || record.hybrid_epoch_floor != 0
            || record.hybrid_credential.is_some() || record.policy.is_some()
            || record.contact.is_some() || record.service_enabled || record.credential.is_some()) {
        return Err("invalid_hybrid_initial_state");
    }
    if let Some(hybrid) = &record.hybrid_trust {
        validate_hybrid_trust(&record.trust, hybrid)?;
        if record.service_enabled || record.credential.is_some() { return Err("invalid_hybrid_state"); }
        if let Some(credential) = &record.hybrid_credential {
            let sealed = STANDARD.decode(&credential.ciphertext).map_err(|_| "invalid_hybrid_state")?;
            if hybrid.outcome_key_b64.is_none() || credential.epoch == 0
                || credential.epoch != record.hybrid_epoch_floor
                || credential.epoch < hybrid.minimum_epoch || sealed.is_empty() || sealed.len() > 64 * 1024
                || STANDARD.encode(&sealed) != credential.ciphertext
                || !record.origins.contains(&credential.origin) {
                return Err("invalid_hybrid_state");
            }
        }
    } else if record.hybrid_epoch_floor != 0 || record.hybrid_credential.is_some() {
        return Err("invalid_hybrid_state");
    }
    Ok(())
}

fn ensure_completed(record: Option<&Record>) -> Result<(), &'static str> {
    if record.is_some_and(|record| record.hybrid_unconfirmed) { return Err("store_unavailable"); }
    if record.is_some_and(|record| record.hybrid_initial_pending) { return Err("hybrid_enrollment_pending"); }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClearAcknowledgement {
    token: String,
    connection_token: String,
    #[serde(default)] leave_evidence: Option<crate::fleet_recovery_leave::TerminalEvidence>,
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
    #[cfg(windows)]
    _owner: Option<machine_owner::OwnerLease>,
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

pub(crate) fn validate_operator_key(trust:&FleetTrust, value:&str) -> Result<[u8;32],&'static str> {
    let raw=STANDARD.decode(value).map_err(|_| "invalid_operator_key")?;
    if STANDARD.encode(&raw)!=value { return Err("invalid_operator_key"); }
    let bytes:[u8;32]=raw.try_into().map_err(|_| "invalid_operator_key")?;
    let key=ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(|_| "invalid_operator_key")?;
    let online:[u8;32]=STANDARD.decode(&trust.signer_key).map_err(|_| "invalid_operator_key")?.try_into().map_err(|_| "invalid_operator_key")?;
    if key.is_weak() || bytes==online { return Err("invalid_operator_key"); }
    Ok(bytes)
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
    /// Restore a prior protected state only for a still-pending local commit.
    /// The verifier must authenticate the server's irreversible cancellation
    /// and a fresh local administrator while the state lock is held.
    pub(crate) fn rollback_cancelled_prepare(&self,verify:impl FnOnce(&FleetTrust,&str,&str,&str,&str)->Result<(),&'static str>) -> Result<(),&'static str> {
        use sha2::{Digest,Sha256};
        let mut guard=self.state.lock().map_err(|_| "store_unavailable")?;
        let current=guard.record.as_ref().ok_or("not_provisioned")?;
        let commit=current.recovery_commit.as_ref().ok_or("recovery_not_prepared")?;
        if !commit.activation_pending || current.service_enabled || current.trust.enrollment_generation!=commit.new_generation
            || current.trust.org_id!=commit.old_trust.org_id || current.trust.device_id!=commit.old_trust.device_id
            || current.trust.signer_key!=commit.old_trust.signer_key { return Err("recovery rollback unavailable"); }
        let hash=commit.rollback_sha256.as_deref().ok_or("recovery rollback snapshot missing")?;
        let id=uuid::Uuid::parse_str(&commit.recovery_id).map_err(|_| "recovery rollback unavailable")?;
        if id.is_nil() || id.to_string()!=commit.recovery_id { return Err("recovery rollback unavailable"); }
        let path=self.path.parent().ok_or("store_unavailable")?.join(format!("recovery-rollback-{id}.json"));
        let bytes=self.fs.read(&path).map_err(|_| "recovery rollback snapshot missing")?;
        if bytes.len()>16*1024*1024 || Sha256::digest(&bytes).iter().map(|b|format!("{b:02x}")).collect::<String>()!=hash
            { return Err("recovery rollback snapshot changed"); }
        let mut previous:State=serde_json::from_slice(&bytes).map_err(|_| "recovery rollback snapshot changed")?;
        let record=previous.record.as_mut().ok_or("recovery rollback snapshot changed")?;
        let prior=record.recovery_commit.as_ref().ok_or("recovery rollback snapshot changed")?;
        if record.trust!=commit.old_trust || !record.service_enabled || record.credential.is_none()
            || prior.activation_pending || prior.new_generation!=record.trust.enrollment_generation
            || prior.old_trust.org_id!=record.trust.org_id || prior.old_trust.device_id!=record.trust.device_id
            || prior.old_trust.signer_key!=record.trust.signer_key { return Err("recovery rollback snapshot changed"); }
        verify(&record.trust,&prior.recovery_id,&commit.recovery_id,&commit.new_generation,&commit.decision_digest)?;
        record.credential_revision=record.credential_revision.max(current.credential_revision).checked_add(1).ok_or("revision_exhausted")?;
        record.connection_revision=record.connection_revision.max(current.connection_revision).checked_add(1).ok_or("revision_exhausted")?;
        previous.pending=None;previous.contact_challenge=None;
        self.persist(&previous)?;*guard=previous;Ok(())
    }
    /// The supplied filesystem must validate existing owner/DACL and reject
    /// reparse points *before* loading any existing trust record.
    fn open(fs: Box<dyn PolicyFs>, dir: PathBuf) -> Result<Self, &'static str> {
        fs.ensure_dir_secure(&dir).map_err(|_| "insecure_store")?;
        let path = dir.join("fleet-machine-policy.json");
        let state: State = match fs.read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "invalid_store")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(_) => return Err("store_unavailable"),
        };
        if let Some(record) = &state.record {
            if record.recovery_commit.as_ref().is_some_and(|commit| commit.activation_pending && record.service_enabled) { return Err("invalid_store"); }
            validate_trust(&record.trust)?;
            validate_hybrid_state(record)?;
            if let Some(key)=&record.operator_key { validate_operator_key(&record.trust,key)?; }
            if let Some(policy) = &record.policy {
                verify_policy(&record.trust, policy)?;
            }
        }
        Ok(Self {
            fs,
            path,
            state: Mutex::new(state),
            #[cfg(windows)]
            _owner: None,
        })
    }

    /// Test fixtures use in-memory storage and do not claim process ownership.
    #[cfg(test)]
    pub(crate) fn open_test(fs: Box<dyn PolicyFs>, dir: PathBuf) -> Result<Self, &'static str> {
        Self::open(fs, dir)
    }

    pub fn begin_enrollment(
        &self,
        device_id: String,
        signer_key: String,
        allowed_origins: Vec<String>,
        transport_profile: Option<TransportProfile>,
    ) -> Result<String, &'static str> {
        use rand::TryRng;
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
        rand::rngs::SysRng
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
        if guard.record.as_ref().and_then(|r|r.recovery_commit.as_ref()).is_some_and(|c|c.activation_pending) { return Err("recovery_pending"); }
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
                hybrid_epoch_floor: 0,
                hybrid_credential: None,
                hybrid_initial_pending: false,
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
                recovery_commit: None,
                operator_key: None,
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
        ensure_completed(guard.record.as_ref())?;
        Ok(guard.record.as_ref().and_then(|record| effective_hybrid_trust(record)
            .map(|hybrid| (record.trust.clone(), hybrid))))
    }

    /// An explicit administrator grant is scoped to an actual Windows user,
    /// session, existing trust, and already-authorized HTTPS origin.
    pub fn begin_hybrid_enrollment(&self, expected: &FleetTrust, origin: &str, actor: &str, session: u32)
        -> Result<HybridEnrollmentOffer, &'static str> {
        use rand::TryRng;
        validate_trust(expected)?;
        let audit = trust_audit("hybrid_enrollment_begin", actor, &expected.enrollment_generation)?;
        if session == 0 { return Err("invalid_session"); }
        let normalized = origins::normalize(&[origin.to_owned()])?;
        let origin = normalized.first().filter(|value| value.starts_with("https://"))
            .ok_or("invalid_origin")?.clone();
        let mut random = [0u8; 32];
        rand::rngs::SysRng.try_fill_bytes(&mut random).map_err(|_| "random_unavailable")?;
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
        Ok(effective_hybrid_trust(record).map(|hybrid| (record.trust.clone(), hybrid,
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
        if current.recovery_commit.as_ref().is_some_and(|commit|commit.activation_pending) { return Err("recovery_pending"); }
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
                hybrid_epoch_floor: 0,
                hybrid_credential: None,
                hybrid_initial_pending: false,
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
                recovery_commit: None,
                operator_key: None,
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
        if current.recovery_commit.as_ref().is_some_and(|commit|commit.activation_pending) { return Err("recovery_pending"); }
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
        self.install_with_snapshot(policy,None)
    }

    pub(crate) fn install_recovery_policy(&self,policy:PolicyEnvelope,snapshot:&CredentialSnapshot)->Result<(),&'static str> {
        self.install_with_snapshot(policy,Some(snapshot))
    }

    fn install_with_snapshot(&self, policy:PolicyEnvelope, snapshot:Option<&CredentialSnapshot>) -> Result<(), &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        if current.hybrid_trust.is_some() { return Err("hybrid_policy_required"); }
        if let Some(snapshot)=snapshot {
            if !snapshot.is_recovered || !current.service_enabled || current.trust!=snapshot.trust
                || current.credential_revision!=snapshot.revision || current.origins!=snapshot.origins
                || current.transport_profile.as_ref()!=Some(&snapshot.profile) || current.operator_key!=snapshot.operator_key
                || !current.credential.as_ref().is_some_and(|credential|credential.origin==snapshot.origin)
                || !current.recovery_commit.as_ref().is_some_and(|commit|!commit.activation_pending) { return Err("recovery_credential_changed"); }
        }
        if current.recovery_commit.as_ref().is_some_and(|commit|commit.activation_pending) { return Err("recovery_pending"); }
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
                hybrid_epoch_floor: current.hybrid_epoch_floor,
                hybrid_credential: current.hybrid_credential.clone(),
                hybrid_initial_pending: current.hybrid_initial_pending,
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
                recovery_commit: current.recovery_commit.clone(),
                operator_key: current.operator_key.clone(),
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
        ensure_completed(guard.record.as_ref())?;
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
        ensure_completed(guard.record.as_ref())?;
        read(guard.record.is_some(),guard.record.as_ref().and_then(|record|record.policy.as_ref()))
    }

    pub fn trust_and_policy(&self) -> Result<(bool, Option<PolicyEnvelope>), &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        ensure_completed(guard.record.as_ref())?;
        Ok(match &guard.record {
            None => (false, None),
            Some(record) => (true, record.policy.clone()),
        })
    }

    pub fn trust_snapshot(&self) -> Result<Option<(FleetTrust, Vec<TrustAudit>)>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        ensure_completed(guard.record.as_ref())?;
        Ok(guard
            .record
            .as_ref()
            .map(|record| (record.trust.clone(), record.audit.clone())))
    }

    /// Holds protected trust stable for a short non-reentrant operation. The
    /// trust lock must always precede the handoff grant lock.
    pub(crate) fn with_handoff_trust<T>(&self, f: impl FnOnce(&FleetTrust) -> Result<T, &'static str>) -> Result<T, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        f(&guard.record.as_ref().ok_or("protected Fleet trust is unavailable")?.trust)
    }

    /// An initial enrollment has no trust yet. Keep the pending identity and
    /// the absence of an existing binding stable while checking recovery state.
    pub(crate) fn with_handoff_or_pending<T>(&self, device: &str, trusted: impl FnOnce(&FleetTrust) -> Result<T, &'static str>, pending: impl FnOnce() -> Result<T, &'static str>) -> Result<T, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        match &guard.record {
            Some(record) => trusted(&record.trust),
            None if guard.pending.as_ref().is_some_and(|value| value.device_id == device) => pending(),
            None => Err("protected Fleet trust is unavailable"),
        }
    }

    /// Hold trust stable while matching a prior, fully activated recovery.
    pub(crate) fn with_handoff_history<T>(&self, f: impl FnOnce(&FleetTrust, Option<(&str, &FleetTrust, &str, &str)>) -> Result<T, &'static str>) -> Result<T, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let record = guard.record.as_ref().ok_or("protected Fleet trust is unavailable")?;
        let previous = record.recovery_commit.as_ref().filter(|c| !c.activation_pending && record.service_enabled
            && record.credential.is_some() && c.new_generation == record.trust.enrollment_generation)
            .map(|c| (c.recovery_id.as_str(), &c.old_trust, c.decision_digest.as_str(),c.candidate_fingerprint.as_str()));
        f(&record.trust, previous)
    }

    pub fn trust_with_origins(&self) -> Result<Option<TrustWithTransport>, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        ensure_completed(guard.record.as_ref())?;
        Ok(guard.record.as_ref().map(|record| (record.trust.clone(), record.audit.clone(), record.origins.clone(), record.transport_profile.clone(), record.service_enabled)))
    }

    fn persist(&self, state: &State) -> Result<(), &'static str> {
        if let Some(record) = &state.record {
            // Only a deliberate exact-state confirmation may clear this fence.
            if record.hybrid_unconfirmed { return Err("store_unavailable"); }
            validate_hybrid_state(record)?;
        }
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
        ensure_completed(guard.record.as_ref())?;
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
        if ensure_completed(guard.record.as_ref()).is_err() { return false; }
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
        ensure_completed(guard.record.as_ref())?;
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
        ensure_completed(guard.record.as_ref())?;
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
            trust: record.trust.clone(), origins: record.origins.clone(), origin:credential.origin.clone(), profile: profile.clone(),
            revision: record.credential_revision, secret, operator_key:record.operator_key.clone(),
            is_recovered:record.recovery_commit.as_ref().is_some_and(|commit|!commit.activation_pending),
        }))
    }

    pub(crate) fn credential_is_current(&self, snapshot: &CredentialSnapshot) -> bool {
        let Ok(guard) = self.state.lock() else { return false; };
        guard.record.as_ref().is_some_and(|record| {
            record.service_enabled && record.trust == snapshot.trust && record.credential.as_ref().is_some_and(|credential|credential.origin==snapshot.origin)
                && record.credential_revision == snapshot.revision
                && record.origins == snapshot.origins && record.transport_profile.as_ref() == Some(&snapshot.profile)
                && record.operator_key == snapshot.operator_key
                && record.recovery_commit.as_ref().is_some_and(|commit|!commit.activation_pending)==snapshot.is_recovered
        })
    }

    pub(crate) fn credential_revision_is_current(&self, trust: &FleetTrust, revision: u64) -> bool {
        let Ok(guard) = self.state.lock() else { return false; };
        guard.record.as_ref().is_some_and(|record| {
            record.service_enabled && record.trust == *trust && record.credential.is_some() && record.credential_revision == revision
        })
    }

    /// Expose only the protected current recovery identity to service-owned leave signing.
    pub(crate) fn with_leave_identity<T>(&self, snapshot: &CredentialSnapshot, f: impl FnOnce(&FleetTrust, &FleetTrust, &str, &str) -> Result<T, &'static str>) -> Result<T, &'static str> {
        let guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let record = guard.record.as_ref().ok_or("not_provisioned")?;
        let commit = record.recovery_commit.as_ref().ok_or("recovery_not_prepared")?;
        if commit.activation_pending || !snapshot.is_recovered || !record.service_enabled || record.credential.is_none()
            || record.trust != snapshot.trust || record.credential_revision != snapshot.revision
            || record.origins != snapshot.origins || record.transport_profile.as_ref() != Some(&snapshot.profile)
            || !record.credential.as_ref().is_some_and(|v| v.origin == snapshot.origin)
            || record.trust.org_id != commit.old_trust.org_id || record.trust.device_id != commit.old_trust.device_id
            || record.trust.signer_key != commit.old_trust.signer_key || record.trust.enrollment_generation != commit.new_generation {
            return Err("recovery_credential_changed");
        }
        f(&record.trust, &commit.old_trust, &commit.recovery_id, &commit.candidate_fingerprint)
    }

    /// Keep protected active trust locked while a caller validates and uses
    /// historical recovery evidence; this does not authorize new recovery.
    pub(crate) fn with_active_recovery<T>(&self, expected:Option<&CredentialSnapshot>,f:impl FnOnce(&FleetTrust,&FleetTrust,&str,&str,&str,&str,&str,i64,&PolicyEnvelope)->Result<T,&'static str>) -> Result<T,&'static str> {
        let guard=self.state.lock().map_err(|_| "store_unavailable")?;
        let record=guard.record.as_ref().ok_or("not_provisioned")?;
        let commit=record.recovery_commit.as_ref().ok_or("recovery_not_prepared")?;
        if commit.activation_pending || !record.service_enabled || record.credential.is_none()
            || record.trust.org_id!=commit.old_trust.org_id || record.trust.device_id!=commit.old_trust.device_id
            || record.trust.signer_key!=commit.old_trust.signer_key || record.trust.enrollment_generation!=commit.new_generation {
            return Err("recovery_not_active");
        }
        if let Some(snapshot)=expected {
            if !snapshot.is_recovered || record.trust!=snapshot.trust || record.credential_revision!=snapshot.revision
                || record.origins!=snapshot.origins || record.transport_profile.as_ref()!=Some(&snapshot.profile)
                || record.operator_key!=snapshot.operator_key
                || !record.credential.as_ref().is_some_and(|credential|credential.origin==snapshot.origin) { return Err("recovery_credential_changed"); }
        }
        let operator=record.operator_key.as_deref().ok_or("operator_key_not_pinned")?;
        validate_operator_key(&record.trust,operator)?;
        let policy=record.policy.as_ref().ok_or("policy_unavailable")?;
        verify_policy(&record.trust,policy)?;
        let epoch=policy.config.as_ref().map(|config|config.version).ok_or("policy_unavailable")?;
        f(&record.trust,&commit.old_trust,&commit.recovery_id,&commit.decision_digest,&commit.archive_sha256,&commit.candidate_fingerprint,operator,epoch,policy)
    }

    /// Fleet posture's held-epoch signal is the signed configuration held by
    /// the service; it does not assert native enforcement or Windows readback.
    pub(crate) fn held_recovery_epoch(&self,snapshot:&CredentialSnapshot)->Result<i64,&'static str> {
        self.with_active_recovery(Some(snapshot),|_,_,_,_,_,_,_,epoch,_|Ok(epoch))
    }
    /// Distinguish ordinary, prepared, and committed service-owned recovery
    /// without releasing a credential to the helper.
    pub(crate) fn recovery_execution_mode(&self)->Result<(String,&'static str),&'static str> {
        let guard=self.state.lock().map_err(|_| "store_unavailable")?;
        let Some(record)=guard.record.as_ref() else {
            return guard.pending.as_ref().map(|pending| (pending.device_id.clone(), "enrolling")).ok_or("not_provisioned");
        };
        let mode=match &record.recovery_commit {
            None=>"legacy",
            Some(commit) if commit.activation_pending && !record.service_enabled=>"pending",
            Some(commit) if !commit.activation_pending && record.service_enabled && record.credential.is_some()
                && record.trust.org_id==commit.old_trust.org_id && record.trust.device_id==commit.old_trust.device_id
                && record.trust.signer_key==commit.old_trust.signer_key && record.trust.enrollment_generation==commit.new_generation=>"recovered",
            _=>return Err("recovery_unavailable"),
        };
        Ok((record.trust.device_id.clone(),mode))
    }
    pub(crate) fn held_recovery_policy(&self,snapshot:&CredentialSnapshot)->Result<fleet_proto::RecoveryHeldPolicy,&'static str> {
        self.with_active_recovery(Some(snapshot),|current,_,_,_,_,_,_,epoch,policy| {
            let value=serde_json::to_value(policy).map_err(|_| "invalid_policy")?;
            Ok(fleet_proto::RecoveryHeldPolicy { enrollment_generation:current.enrollment_generation.clone(),
                config_revision:policy.config_revision.ok_or("missing_config_revision")?,
                resolution_revision:policy.resolution_revision.ok_or("missing_resolution_revision")?,epoch_version:epoch,
                policy_sha256:fleet_proto::canonical_json_sha256(&value),policy:policy.clone() })
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
        ensure_completed(Some(record))?;
        let bytes = serde_json::to_vec(&["wincommander:fleet-service-clear:v1", &record.trust.org_id,
            &record.trust.device_id, &record.trust.enrollment_generation, &record.trust.signer_key,
            &record.credential_revision.to_string()]).map_err(|_| "invalid_store")?;
        Ok(STANDARD.encode(Sha256::digest(bytes)))
    }

    fn connection_token_for(record: &Record) -> Result<String, &'static str> {
        use sha2::{Digest, Sha256};
        ensure_completed(Some(record))?;
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
        ensure_completed(guard.record.as_ref())?;
        guard.record.as_ref().map(|record| {
            let token = Some(Self::clear_token(record)?);
            Ok((record.trust.clone(), record.contact.clone(), token, Self::connection_token_for(record)?, record.credential_revision))
        }).transpose()
    }

    pub(crate) fn clear_credential(&self, token: &str, connection_token: &str) -> Result<bool, &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let Some(record) = &guard.record else { return Ok(false); };
        if record.recovery_commit.as_ref().is_some_and(|commit|commit.activation_pending) { return Err("recovery_pending"); }
        if !record.service_enabled && record.credential.is_none()
            && record.cleared.as_ref().is_some_and(|ack| ack.token == token && ack.connection_token == connection_token) {
            // An ambiguous prior hybrid write needs exact readback-by-write;
            // an in-memory acknowledgement alone cannot prove deletion.
            if record.hybrid_unconfirmed {
                let mut confirmed = guard.clone();
                confirmed.record.as_mut().ok_or("not_provisioned")?.hybrid_unconfirmed = false;
                self.persist(&confirmed)?;
                *guard = confirmed;
            }
            return Ok(true);
        }
        if !record.service_enabled && record.credential.is_none()
            && record.cleared.as_ref().and_then(|a|a.leave_evidence.as_ref()).is_some_and(|e|crate::fleet_recovery_leave::evidence_valid(&record.trust,e))
            && Self::clear_token(record)?==token && Self::connection_token_for(record)?==connection_token { return Ok(true); }
        if Self::clear_token(record)? != token || Self::connection_token_for(record)? != connection_token {
            return Ok(false);
        }
        guard.hybrid_enrollment = None;
        guard.hybrid_claim = None;
        let mut next = guard.clone();
        let record = next.record.as_mut().ok_or("not_provisioned")?;
        record.credential = None;
        record.hybrid_credential = None;
        record.service_enabled = false;
        record.connection_revision = record.connection_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.cleared = Some(ClearAcknowledgement { token:token.to_owned(), connection_token:connection_token.to_owned(), leave_evidence: None });
        record.credential_revision = record.credential_revision.checked_add(1).ok_or("revision_exhausted")?;
        next.pending = None;
        match self.persist(&next) {
            Ok(()) => { *guard = next; Ok(true) }
            Err(error) if next.record.as_ref().is_some_and(|record| record.hybrid_trust.is_some()) => {
                // A deletion may have reached disk. Do not let later writes
                // resurrect the old in-memory credential after an error.
                next.record.as_mut().ok_or("not_provisioned")?.hybrid_unconfirmed = true;
                *guard = next;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
    pub(crate) fn clear_after_verified_leave(&self, token: &str, connection_token: &str, proof: &crate::fleet_recovery_leave::VerifiedTerminal) -> Result<bool, &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let Some(record) = guard.record.as_ref() else { return Ok(false) };
        if !record.service_enabled || !proof.matches(&record.trust, record.credential_revision)
            || Self::clear_token(record)? != token || Self::connection_token_for(record)? != connection_token { return Ok(false) }
        let mut next=guard.clone(); let record=next.record.as_mut().ok_or("not_provisioned")?;
        record.credential=None; record.service_enabled=false;
        record.connection_revision=record.connection_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.credential_revision=record.credential_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.cleared=Some(ClearAcknowledgement { token:token.to_owned(), connection_token:connection_token.to_owned(), leave_evidence:Some(proof.evidence()) });
        next.pending=None; self.persist(&next)?; *guard=next; Ok(true)
    }
    pub(crate) fn confirmed_leave(&self) -> Result<Option<(FleetTrust,String)>, &'static str> {
        let guard=self.state.lock().map_err(|_| "store_unavailable")?;
        match guard.record.as_ref().filter(|record| !record.service_enabled && record.credential.is_none() && record.cleared.as_ref().and_then(|a|a.leave_evidence.as_ref()).is_some_and(|e|crate::fleet_recovery_leave::evidence_valid(&record.trust,e))) {
            Some(record)=>Ok(Some((record.trust.clone(),Self::connection_token_for(record)?))), None=>Ok(None),
        }
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
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        hybrid_request_at(offer, seed, now)
    }

    fn hybrid_request_at(offer: &HybridEnrollmentOffer, seed: u8, timestamp: i64) -> Vec<u8> {
        let outcome = SigningKey::from_bytes(&[seed; 32]);
        let mut body = serde_json::json!({"device_id":trust().device_id, "device_hash":"device-hash",
            "device_kind":"wincommander", "agent_version":"3.6.7", "protocol_version":1,
            "capabilities":["hmac_body_v2"], "outcome_pubkey_b64":STANDARD.encode(outcome.verifying_key().as_bytes()),
            "shield_progress_challenge":offer.challenge, "enrollment_proof_version":2,
            "enrollment_server_key_b64":trust().signer_key,
            "enrollment_nonce":"44444444-4444-4444-8444-444444444444", "enrollment_ts":timestamp });
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

    fn hybrid_store_fixture(fs: FailingFs) -> (FleetMachinePolicy, FleetHybridTrust,
        HybridEnrollmentOffer, hybrid_enrollment::ClaimedHybridEnrollment, hybrid_reply::VerifiedHybridReply) {
        use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use wincmd_pq_experiment::{jws, Purpose, VerifyError};
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let pq_public: [u8; 1952] = pq.public_key().as_ref().try_into().unwrap();
        let store = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        let proof = proof_with_origins(&store, &trust(), vec!["https://fleet.example".into()]);
        store.provision(trust(), &proof, "S-1-5-18").unwrap();
        let mut hybrid = hybrid();
        hybrid.ml_dsa_65_public_b64 = STANDARD.encode(pq_public);
        store.provision_hybrid_trust(trust(), hybrid.clone(), "S-1-5-18").unwrap();
        let offer = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let claim = store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &hybrid_request(&offer)).unwrap();
        let body = serde_json::to_vec(&serde_json::json!({"device_id":trust().device_id,
            "org_id":trust().org_id, "shield_progress_generation":trust().enrollment_generation,
            "outcome_pubkey_b64":claim.outcome_key(), "checkin_secret_b64":STANDARD.encode([6u8; 32]),
            "server_signing_key_b64":trust().signer_key, "capabilities":[]})).unwrap();
        let ed = signing_key();
        let ed_public = ed.verifying_key().to_bytes();
        let signer = jws::Trust { purpose: Purpose::FleetEnrollment, minimum_epoch: 7,
            ed_key_id:"fleet-ed", ed25519_public:&ed_public, pq_key_id:"fleet-pq", ml_dsa_65_public:&pq_public };
        let preimage = fleet_proto::windows_v2::windows_hybrid_enrollment_preimage(claim.challenge(), claim.request(), 200, &body);
        let classical = format!("v2.{}", STANDARD.encode(ed.sign(&preimage).to_bytes()));
        let proof = jws::sign::assemble(&preimage, 9, &signer,
            |message| Ok(ed.sign(message).to_bytes().to_vec()),
            |message| { let mut output=vec![0;3309];
                let len=pq.sign(message, &mut output).map_err(|_| VerifyError::InvalidSignature)?;
                output.truncate(len); Ok(output) }).unwrap();
        let verified = store.verify_hybrid_enrollment_reply(&claim, 200, &body, &classical, &URL_SAFE_NO_PAD.encode(proof)).unwrap();
        (store, hybrid, offer, claim, verified)
    }

    #[test]
    fn hybrid_bootstrap_write_failure_cannot_enable_an_unconfirmed_identity() {
        use std::sync::atomic::Ordering;
        for commit_first in [false, true] {
            let fs = FailingFs::default();
            let store = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
            fs.commit_first.store(commit_first, Ordering::SeqCst);
            fs.fail.store(true, Ordering::SeqCst);
            let bootstrap = || store.provision_hybrid_bootstrap(trust().org_id, trust().device_id,
                trust().signer_key, hybrid(), vec!["https://fleet.example".into()], None, "S-1-5-18");
            assert!(bootstrap().is_err());
            assert!(matches!(store.trust_snapshot(), Err("store_unavailable")));
            assert_eq!(store.hybrid_trust_snapshot(), Err("store_unavailable"));
            assert!(store.begin_contact().is_err());
            let reopened = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
            let state = reopened.state.lock().unwrap();
            assert_eq!(state.record.is_some(), commit_first);
            if let Some(record) = &state.record { assert!(record.hybrid_initial_pending && !record.service_enabled); }
            drop(state);
            fs.fail.store(false, Ordering::SeqCst);
            let pending = bootstrap().unwrap();
            assert_eq!(bootstrap().unwrap(), pending);
            assert!(store.provision_hybrid_bootstrap(trust().org_id, trust().device_id,
                trust().signer_key, hybrid(), vec!["https://other.example".into()], None, "S-1-5-18").is_err());
            assert!(matches!(store.trust_snapshot(), Err("hybrid_enrollment_pending")));
        }
    }

    #[test]
    fn first_hybrid_enrollment_installs_server_generation_with_credential() {
        first_hybrid_enrollment_with(&super::credential_integration_tests::TestProtector, None);
    }

    #[cfg(windows)]
    #[test]
    fn first_hybrid_enrollment_installs_server_generation_with_machine_dpapi() {
        first_hybrid_enrollment_with(&crate::fleet_credential::MachineDpapi, None);
    }

    #[test]
    fn first_hybrid_enrollment_ambiguous_write_never_publishes_an_unconfirmed_binding() {
        for commit_first in [false, true] {
            first_hybrid_enrollment_with(&super::credential_integration_tests::TestProtector, Some(commit_first));
        }
    }

    fn first_hybrid_enrollment_with(protector: &dyn crate::fleet_credential::Protector, fail_after: Option<bool>) {
        use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use wincmd_pq_experiment::{jws, Purpose, VerifyError};
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let pq_public: [u8; 1952] = pq.public_key().as_ref().try_into().unwrap();
        let fs = FailingFs::default();
        let store = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
        let mut keys = hybrid();
        keys.ml_dsa_65_public_b64 = STANDARD.encode(pq_public);
        let bootstrap = || store.provision_hybrid_bootstrap(trust().org_id, trust().device_id,
            trust().signer_key, keys.clone(), vec!["https://fleet.example".into()], None, "S-1-5-18");
        let pending = bootstrap().unwrap();
        assert_eq!(bootstrap().unwrap(), pending);
        assert!(matches!(store.trust_snapshot(), Err("hybrid_enrollment_pending")));
        assert!(store.begin_contact().is_err());
        let reopened = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
        assert_eq!(reopened.hybrid_trust_snapshot(), Err("hybrid_enrollment_pending"));
        let offer = store.begin_hybrid_enrollment(&pending, "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let mut stale: serde_json::Value = serde_json::from_slice(&hybrid_request(&offer)).unwrap();
        stale.as_object_mut().unwrap().remove("enrollment_signature_b64");
        stale["previous_generation"] = pending.enrollment_generation.clone().into();
        let mut stale_preimage = b"WinCommander Fleet enrollment proof v2\nPOST\n/v1/windows/enroll-v2\n".to_vec();
        stale_preimage.extend(fleet_proto::canonical_json_bytes(&stale));
        stale["enrollment_signature_b64"] = STANDARD.encode(SigningKey::from_bytes(&[7;32]).sign(&stale_preimage).to_bytes()).into();
        assert!(store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &serde_json::to_vec(&stale).unwrap()).is_err());
        let offer = store.begin_hybrid_enrollment(&pending, "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let claim = store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &hybrid_request(&offer)).unwrap();
        let generation = uuid::Uuid::new_v4().to_string();
        assert_ne!(generation, pending.enrollment_generation);
        let body = serde_json::to_vec(&serde_json::json!({"device_id":pending.device_id,
            "org_id":pending.org_id, "shield_progress_generation":generation,
            "outcome_pubkey_b64":claim.outcome_key(), "checkin_secret_b64":STANDARD.encode([6u8;32]),
            "server_signing_key_b64":pending.signer_key, "capabilities":[]})).unwrap();
        let ed = signing_key();
        let ed_public = ed.verifying_key().to_bytes();
        let signer = jws::Trust { purpose: Purpose::FleetEnrollment, minimum_epoch: 7,
            ed_key_id:"fleet-ed", ed25519_public:&ed_public, pq_key_id:"fleet-pq", ml_dsa_65_public:&pq_public };
        let preimage = fleet_proto::windows_v2::windows_hybrid_enrollment_preimage(claim.challenge(), claim.request(), 200, &body);
        let classical = format!("v2.{}", STANDARD.encode(ed.sign(&preimage).to_bytes()));
        let proof = jws::sign::assemble(&preimage, 9, &signer,
            |message| Ok(ed.sign(message).to_bytes().to_vec()),
            |message| { let mut output=vec![0;3309];
                let len=pq.sign(message, &mut output).map_err(|_| VerifyError::InvalidSignature)?;
                output.truncate(len); Ok(output) }).unwrap();
        let verified = store.verify_hybrid_enrollment_reply(&claim, 200, &body, &classical, &URL_SAFE_NO_PAD.encode(proof)).unwrap();
        if let Some(commit_first) = fail_after {
            use std::sync::atomic::Ordering;
            fs.commit_first.store(commit_first, Ordering::SeqCst);
            fs.fail.store(true, Ordering::SeqCst);
            assert!(store.stage_hybrid_credential(&claim, verified, protector).is_err());
            assert!(!store.hybrid_claim_is_current(&claim));
            assert_eq!(store.hybrid_trust_snapshot(), Err("store_unavailable"));
            assert!(matches!(store.trust_snapshot(), Err("store_unavailable")));
            assert!(matches!(store.trust_with_origins(), Err("store_unavailable")));
            assert!(matches!(store.trust_and_policy(), Err("store_unavailable")));
            assert!(matches!(store.contact_state(), Err("store_unavailable")));
            assert!(matches!(store.credential_clear_token(), Err("store_unavailable")));
            assert!(store.begin_contact().is_err());
            let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
            let guard = reopened.state.lock().unwrap();
            let record = guard.record.as_ref().unwrap();
            assert_eq!(record.hybrid_initial_pending, !commit_first);
            assert_eq!(record.hybrid_credential.is_some(), commit_first);
            assert_eq!(record.trust.enrollment_generation.as_str(), if commit_first { generation.as_str() } else { pending.enrollment_generation.as_str() });
            return;
        }
        store.stage_hybrid_credential(&claim, verified, protector).unwrap();
        let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        let guard = reopened.state.lock().unwrap();
        let record = guard.record.as_ref().unwrap();
        assert!(!record.hybrid_initial_pending);
        assert_eq!(record.trust.enrollment_generation, generation);
        assert_eq!(record.hybrid_epoch_floor, 9);
        let credential = record.hybrid_credential.as_ref().unwrap();
        let binding = crate::fleet_credential::CredentialBinding { org_id:&record.trust.org_id,
            device_id:&record.trust.device_id, enrollment_generation:&record.trust.enrollment_generation,
            origin:&credential.origin };
        let secret = crate::fleet_credential::unseal(protector,
            &binding, &STANDARD.decode(&credential.ciphertext).unwrap()).unwrap();
        assert_eq!(secret.as_slice(), &[6u8;32]);
    }

    #[test]
    fn hybrid_store_failed_write_is_fenced_and_reconciles_exact_state() {
        use std::sync::atomic::Ordering;
        for commit_first in [false, true] {
            let fs = FailingFs::default();
            let (store, hybrid, _, claim, verified) = hybrid_store_fixture(fs.clone());
            fs.commit_first.store(commit_first, Ordering::SeqCst);
            fs.fail.store(true, Ordering::SeqCst);
            assert!(store.stage_hybrid_credential(&claim, verified, &super::credential_integration_tests::TestProtector).is_err());
            assert_eq!(store.hybrid_trust_snapshot(), Err("store_unavailable"));
            let before = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
            let state = before.state.lock().unwrap();
            assert_eq!(state.record.as_ref().unwrap().hybrid_credential.is_some(), commit_first);
            drop(state);
            fs.fail.store(false, Ordering::SeqCst);
            assert!(store.begin_enrollment(trust().device_id, trust().signer_key, Vec::new(), None).is_err());
            let unchanged = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
            assert_eq!(unchanged.state.lock().unwrap().record.as_ref().unwrap().hybrid_credential.is_some(), commit_first);
            store.provision_hybrid_trust(trust(), hybrid, "S-1-5-18").unwrap();
            let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
            let state = reopened.state.lock().unwrap();
            let record = state.record.as_ref().unwrap();
            assert_eq!(record.hybrid_epoch_floor, 9);
            assert!(record.hybrid_credential.is_some());
            assert!(!record.service_enabled && record.credential.is_none());
        }
    }

    #[test]
    fn hybrid_clear_failed_write_does_not_resurrect_in_memory_credential() {
        use std::sync::atomic::Ordering;
        for commit_first in [false, true] {
            let fs = FailingFs::default();
            let (store, _hybrid, _, claim, verified) = hybrid_store_fixture(fs.clone());
            store.stage_hybrid_credential(&claim, verified, &super::credential_integration_tests::TestProtector).unwrap();
            let (_, _, token, connection, _) = store.contact_state().unwrap().unwrap();
            let token = token.unwrap();
            fs.commit_first.store(commit_first, Ordering::SeqCst);
            fs.fail.store(true, Ordering::SeqCst);
            assert!(store.clear_credential(&token, &connection).is_err());
            assert!(store.clear_credential(&token, &connection).is_err());
            assert_eq!(store.hybrid_trust_snapshot(), Err("store_unavailable"));
            assert!(store.state.lock().unwrap().record.as_ref().unwrap().hybrid_credential.is_none());
            let before = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
            assert_eq!(before.state.lock().unwrap().record.as_ref().unwrap().hybrid_credential.is_some(), !commit_first);
            fs.fail.store(false, Ordering::SeqCst);
            assert!(store.begin_enrollment(trust().device_id, trust().signer_key, Vec::new(), None).is_err());
            let unchanged = FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("test")).unwrap();
            assert_eq!(unchanged.state.lock().unwrap().record.as_ref().unwrap().hybrid_credential.is_some(), !commit_first);
            assert!(store.clear_credential(&token, &connection).unwrap());
            assert_eq!(store.hybrid_trust_snapshot().unwrap().unwrap().1.minimum_epoch, 9);
            let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
            let state = reopened.state.lock().unwrap();
            let record = state.record.as_ref().unwrap();
            assert!(record.hybrid_credential.is_none());
            assert_eq!(record.hybrid_epoch_floor, 9);
        }
    }

    #[cfg(windows)]
    #[test]
    fn hybrid_store_machine_dpapi_reopens_sealed_credential() {
        let fs = FailingFs::default();
        let (store, _, _, claim, verified) = hybrid_store_fixture(fs.clone());
        store.stage_hybrid_credential(&claim, verified, &crate::fleet_credential::MachineDpapi).unwrap();
        let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        let state = reopened.state.lock().unwrap();
        let record = state.record.as_ref().unwrap();
        let credential = record.hybrid_credential.as_ref().unwrap();
        let sealed = STANDARD.decode(&credential.ciphertext).unwrap();
        assert_ne!(sealed.as_slice(), &[6u8; 32]);
        let binding = crate::fleet_credential::CredentialBinding { org_id: &record.trust.org_id,
            device_id: &record.trust.device_id, enrollment_generation: &record.trust.enrollment_generation,
            origin: &credential.origin };
        let secret = crate::fleet_credential::unseal(&crate::fleet_credential::MachineDpapi, &binding, &sealed).unwrap();
        assert_eq!(secret.as_slice(), &[6u8; 32]);
        assert_eq!(record.hybrid_epoch_floor, 9);
    }

    #[test]
    fn hybrid_store_rejects_reply_from_superseded_claim() {
        let (store, _, _, _, verified) = hybrid_store_fixture(FailingFs::default());
        let offer = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let claim = store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &hybrid_request(&offer)).unwrap();
        assert!(store.stage_hybrid_credential(&claim, verified, &super::credential_integration_tests::TestProtector).is_err());
        assert!(store.hybrid_claim_is_current(&claim));
        assert!(store.state.lock().unwrap().record.as_ref().unwrap().hybrid_credential.is_none());
    }

    #[test]
    fn classical_record_omits_unused_hybrid_fields() {
        let fs = MemoryFs::default();
        let store = store(fs.clone());
        provision(&store);
        let value: serde_json::Value = serde_json::from_slice(fs.0.lock().unwrap().as_ref().unwrap()).unwrap();
        let record = value["record"].as_object().unwrap();
        assert!(!record.contains_key("hybrid_epoch_floor"));
        assert!(!record.contains_key("hybrid_credential"));
    }

    #[test]
    fn hybrid_store_and_cancel_race_has_one_order() {
        use std::sync::Barrier;
        let fs = FailingFs::default();
        let (store, _, offer, claim, verified) = hybrid_store_fixture(fs);
        let store = Arc::new(store);
        let claim = Arc::new(claim);
        let barrier = Arc::new(Barrier::new(2));
        let worker = { let store=Arc::clone(&store); let claim=Arc::clone(&claim); let barrier=Arc::clone(&barrier);
            std::thread::spawn(move || { barrier.wait();
                store.stage_hybrid_credential(&claim, verified, &super::credential_integration_tests::TestProtector).is_ok() }) };
        barrier.wait();
        let cancelled = store.cancel_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7).unwrap();
        let stored = worker.join().unwrap();
        assert_ne!(stored, cancelled);
        assert_eq!(store.state.lock().unwrap().record.as_ref().unwrap().hybrid_credential.is_some(), stored);
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
    fn hybrid_claim_rejects_stale_and_future_signed_requests() {
        let (_, store) = hybrid_ready();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        for timestamp in [now - 600, now + 600, i64::MAX] {
            let offer = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
            let bytes = hybrid_request_at(&offer, 7, timestamp);
            assert!(store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &bytes).is_err());
            assert!(store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &hybrid_request(&offer)).is_err());
        }
    }

    #[test]
    fn hybrid_reply_requires_both_pinned_signatures_and_current_claim() {
        use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use wincmd_pq_experiment::{jws, Purpose, VerifyError};
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let pq_public: [u8; 1952] = pq.public_key().as_ref().try_into().unwrap();
        let fs = MemoryFs::default();
        let store = store(fs.clone());
        let proof = proof_with_origins(&store, &trust(), vec!["https://fleet.example".into()]);
        store.provision(trust(), &proof, "S-1-5-18").unwrap();
        let mut hybrid = hybrid();
        hybrid.ml_dsa_65_public_b64 = STANDARD.encode(pq_public);
        store.provision_hybrid_trust(trust(), hybrid, "S-1-5-18").unwrap();
        let offer = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let claim = store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &hybrid_request(&offer)).unwrap();
        let body = serde_json::to_vec(&serde_json::json!({
            "device_id":trust().device_id, "org_id":trust().org_id,
            "shield_progress_generation":trust().enrollment_generation,
            "outcome_pubkey_b64":claim.outcome_key(), "checkin_secret_b64":STANDARD.encode([6u8; 32]),
            "server_signing_key_b64":trust().signer_key, "capabilities":["hmac_body_v2"]
        })).unwrap();
        let ed = signing_key();
        let ed_public = ed.verifying_key().to_bytes();
        let signer = jws::Trust { purpose: Purpose::FleetEnrollment, minimum_epoch: 7,
            ed_key_id: "fleet-ed", ed25519_public: &ed_public,
            pq_key_id: "fleet-pq", ml_dsa_65_public: &pq_public };
        let sign = |claim: &hybrid_enrollment::ClaimedHybridEnrollment, bytes: &[u8]| {
            let preimage = fleet_proto::windows_v2::windows_hybrid_enrollment_preimage(claim.challenge(), claim.request(), 200, bytes);
            let classical = format!("v2.{}", STANDARD.encode(ed.sign(&preimage).to_bytes()));
            let hybrid = jws::sign::assemble(&preimage, 9, &signer,
                |message| Ok(ed.sign(message).to_bytes().to_vec()),
                |message| { let mut output = vec![0; 3309];
                    let len = pq.sign(message, &mut output).map_err(|_| VerifyError::InvalidSignature)?;
                    output.truncate(len); Ok(output) }).unwrap();
            (classical, URL_SAFE_NO_PAD.encode(hybrid))
        };
        let (classical, proof) = sign(&claim, &body);
        let verified = store.verify_hybrid_enrollment_reply(&claim, 200, &body, &classical, &proof).unwrap();
        assert_eq!(verified.epoch(), 9);
        assert_eq!(verified.body(), &body);
        assert!(store.verify_hybrid_enrollment_reply(&claim, 201, &body, &classical, &proof).is_err());
        assert!(store.verify_hybrid_enrollment_reply(&claim, 200, b"{}", &classical, &proof).is_err());
        assert!(store.verify_hybrid_enrollment_reply(&claim, 200, &body, "v2.invalid", &proof).is_err());
        assert!(store.verify_hybrid_enrollment_reply(&claim, 200, &body, &"x".repeat(100_000), &proof).is_err());
        assert!(store.verify_hybrid_enrollment_reply(&claim, 200, &body, &classical, &"x".repeat(24 * 1024 + 1)).is_err());
        assert!(store.verify_hybrid_enrollment_reply(&claim, 200, &body, &classical, "invalid").is_err());
        let mut corrupt: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&proof).unwrap()).unwrap();
        let signature = corrupt["signatures"][1]["signature"].as_str().unwrap().to_owned();
        let first = if signature.starts_with('A') { "B" } else { "A" };
        corrupt["signatures"][1]["signature"] = format!("{}{}", first, &signature[1..]).into();
        let corrupt = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&corrupt).unwrap());
        assert!(store.verify_hybrid_enrollment_reply(&claim, 200, &body, &classical, &corrupt).is_err());
        let mut missing: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&proof).unwrap()).unwrap();
        missing["signatures"].as_array_mut().unwrap().pop();
        let missing = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&missing).unwrap());
        assert!(store.verify_hybrid_enrollment_reply(&claim, 200, &body, &classical, &missing).is_err());
        let mut wrong: serde_json::Value = serde_json::from_slice(&body).unwrap();
        wrong["org_id"] = "different-org".into();
        let wrong = serde_json::to_vec(&wrong).unwrap();
        let (wrong_classical, wrong_proof) = sign(&claim, &wrong);
        assert!(store.verify_hybrid_enrollment_reply(&claim, 200, &wrong, &wrong_classical, &wrong_proof).is_err());
        let mut wrong: serde_json::Value = serde_json::from_slice(&body).unwrap();
        wrong["shield_progress_generation"] = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
        let wrong = serde_json::to_vec(&wrong).unwrap();
        let (wrong_classical, wrong_proof) = sign(&claim, &wrong);
        assert!(store.verify_hybrid_enrollment_reply(&claim, 200, &wrong, &wrong_classical, &wrong_proof).is_err());
        assert!(store.cancel_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7).unwrap());
        assert!(store.verify_hybrid_enrollment_reply(&claim, 200, &body, &classical, &proof).is_err());
        assert!(store.stage_hybrid_credential(&claim, verified, &super::credential_integration_tests::TestProtector).is_err());
        let offer = store.begin_hybrid_enrollment(&trust(), "https://fleet.example", "S-1-5-21-7", 7).unwrap();
        let claim = store.claim_hybrid_enrollment(&offer.authorization_id, "S-1-5-21-7", 7, &hybrid_request(&offer)).unwrap();
        let (classical, proof) = sign(&claim, &body);
        let verified = store.verify_hybrid_enrollment_reply(&claim, 200, &body, &classical, &proof).unwrap();
        store.stage_hybrid_credential(&claim, verified, &super::credential_integration_tests::TestProtector).unwrap();
        assert!(!store.hybrid_claim_is_current(&claim));
        assert_eq!(store.hybrid_trust_snapshot().unwrap().unwrap().1.minimum_epoch, 9);
        assert!(store.credential_snapshot(&super::credential_integration_tests::TestProtector).unwrap().is_none());
        let reopened = FleetMachinePolicy::open(Box::new(fs), PathBuf::from("test")).unwrap();
        let snapshot = reopened.state.lock().unwrap();
        let record = snapshot.record.as_ref().unwrap();
        assert_eq!(record.hybrid_epoch_floor, 9);
        let stored = record.hybrid_credential.as_ref().unwrap();
        let binding = crate::fleet_credential::CredentialBinding { org_id: &record.trust.org_id,
            device_id: &record.trust.device_id, enrollment_generation: &record.trust.enrollment_generation,
            origin: &stored.origin };
        let secret = crate::fleet_credential::unseal(&super::credential_integration_tests::TestProtector,
            &binding, &STANDARD.decode(&stored.ciphertext).unwrap()).unwrap();
        assert_eq!(secret.as_slice(), &[6u8; 32]);
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
            assert!(matches!(store.trust_with_origins(), Err("store_unavailable")));
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
    fn pending_enrollment_never_bypasses_existing_trust_or_other_device() {
        let store = store(MemoryFs::default());
        let device = trust().device_id;
        assert!(store.recovery_execution_mode().is_err());
        assert!(store.with_handoff_or_pending(&device, |_| Ok("trusted"), || Ok("pending")).is_err());
        let _ = proof(&store, &trust());
        assert_eq!(store.recovery_execution_mode().unwrap(), (device.clone(), "enrolling"));
        assert_eq!(store.with_handoff_or_pending(&device, |_| Ok("trusted"), || Ok("pending")), Ok("pending"));
        assert!(store.with_handoff_or_pending("other", |_| Ok("trusted"), || Ok("pending")).is_err());
        provision(&store);
        assert_eq!(store.recovery_execution_mode().unwrap(), (device.clone(), "legacy"));
        assert_eq!(store.with_handoff_or_pending(&device, |_| Ok("trusted"), || Ok("pending")), Ok("trusted"));
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

// Recovery preparation is intentionally not exposed through the service pipe.
#[cfg(windows)]
impl FleetMachinePolicy {
    pub(crate) fn prepare_recovery_commit(
        &self, verified:&crate::fleet_identity_handoff::VerifiedHandoff,
        credential:&crate::fleet_recovery_credential::VerifiedCredential,
        archive:&str, origin:&str, protector:&dyn crate::fleet_credential::Protector, now:i64,
    ) -> Result<String,&'static str> {
        use sha2::{Digest,Sha256};
        if !crate::fleet_identity_handoff::recovery_timestamp_valid(verified.issued_at(),now)
            || !crate::fleet_identity_handoff::recovery_timestamp_valid(credential.issued_at(),now)
            || credential.recovery_id()!=verified.retirement_id() || credential.new_generation()!=verified.new_generation()
            || credential.decision_digest()!=verified.decision_digest().iter().map(|b|format!("{b:02x}")).collect::<String>()
            || credential.archive_sha256()!=archive { return Err("invalid recovery commit"); }
        let mut guard=self.state.lock().map_err(|_| "store_unavailable")?;
        let current=guard.record.as_ref().ok_or("not_provisioned")?;
        let retry_matches_old=current.recovery_commit.as_ref().is_some_and(|prepared| {
            prepared.activation_pending && prepared.recovery_id==verified.retirement_id()
                && verified.matches_trust(&prepared.old_trust)
        });
        if !verified.matches_trust(&current.trust) && !retry_matches_old { return Err("recovery trust changed"); }
        validate_operator_key(&current.trust,current.operator_key.as_deref().ok_or("operator_key_not_pinned")?)?;
        if let Some(prepared)=current.recovery_commit.as_ref() {
            if prepared.recovery_id==verified.retirement_id() && prepared.new_generation==verified.new_generation()
                && prepared.decision_digest==credential.decision_digest() && prepared.archive_sha256==archive
                && prepared.credential_sha256==credential.digest() && prepared.origin==origin
                && prepared.candidate_fingerprint==verified.fingerprint().iter().map(|b|format!("{b:02x}")).collect::<String>()
                && prepared.activation_pending && !current.service_enabled
                && current.trust.org_id==prepared.old_trust.org_id && current.trust.device_id==prepared.old_trust.device_id
                && current.trust.signer_key==prepared.old_trust.signer_key && current.trust.enrollment_generation==prepared.new_generation {
                use subtle::ConstantTimeEq;
                let stored=current.credential.as_ref().ok_or("invalid_store")?;
                if stored.origin!=origin { return Err("invalid_store"); }
                let bytes=STANDARD.decode(&stored.ciphertext).map_err(|_| "invalid_store")?;
                if STANDARD.encode(&bytes)!=stored.ciphertext { return Err("invalid_store"); }
                let binding=crate::fleet_credential::CredentialBinding { org_id:&current.trust.org_id,device_id:&current.trust.device_id,
                    enrollment_generation:&current.trust.enrollment_generation,origin };
                let recovered=crate::fleet_credential::unseal(protector,&binding,&bytes)?;
                if !bool::from(recovered.as_slice().ct_eq(credential.secret())) { return Err("invalid_store"); }
                return Ok(prepared.local_commit_digest.clone());
            }
            if prepared.activation_pending || !current.service_enabled || current.credential.is_none()
                || prepared.recovery_id==verified.retirement_id() || prepared.new_generation!=current.trust.enrollment_generation
                || prepared.old_trust.org_id!=current.trust.org_id || prepared.old_trust.device_id!=current.trust.device_id
                || prepared.old_trust.signer_key!=current.trust.signer_key { return Err("recovery commit changed"); }
            let dir=self.path.parent().ok_or("store_unavailable")?;
            let operator=current.operator_key.as_deref().ok_or("operator_key_not_pinned")?;
            crate::pipe::fleet_handoff_confirmation::check_previous_recovery_drained(&*self.fs,dir,protector,&current.trust,
                &prepared.old_trust,&prepared.recovery_id,&prepared.decision_digest,&prepared.archive_sha256,
                &prepared.candidate_fingerprint,operator,verified)?;
            let id=uuid::Uuid::parse_str(&prepared.recovery_id).map_err(|_| "invalid_store")?;
            if id.is_nil() || id.to_string()!=prepared.recovery_id { return Err("invalid_store"); }
            let archive_path=dir.join(format!("recovery-commit-{id}.json"));
            let prior=serde_json::to_vec(prepared).map_err(|_| "invalid_store")?;
            match self.fs.read(&archive_path) {
                Ok(existing) if existing==prior=>{},
                Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
                    self.fs.atomic_write(&archive_path,&prior).map_err(|_| "store_unavailable")?;
                    if self.fs.read(&archive_path).map_err(|_| "store_unavailable")?!=prior { return Err("store_unavailable"); }
                },
                _=>return Err("recovery history changed"),
            }
        }
        let old=current.trust.clone();
        if !current.origins.iter().any(|value|value==origin) || current.transport_profile.is_none() { return Err("invalid recovery origin"); }
        let dir=self.path.parent().ok_or("store_unavailable")?;
        crate::pipe::fleet_handoff_confirmation::initialize_replacement(&*self.fs,dir,&old,verified,archive)?;
        let new_trust=FleetTrust { org_id:old.org_id.clone(), device_id:old.device_id.clone(),
            enrollment_generation:verified.new_generation().into(), signer_key:old.signer_key.clone() };
        let credential_binding=crate::fleet_credential::CredentialBinding { org_id:&new_trust.org_id,device_id:&new_trust.device_id,
            enrollment_generation:&new_trust.enrollment_generation,origin };
        let sealed=crate::fleet_credential::seal(protector,&credential_binding,credential.secret())?;
        let decision=credential.decision_digest().to_owned();
        let local=serde_json::to_vec(&("wincommander:fleet:identity-handoff-local-commit:v1",&old.org_id,&old.device_id,
            verified.retirement_id(),verified.new_generation(),&decision,archive,STANDARD.encode(verified.public_key()),
            &old.signer_key,credential.digest(),verified.next_sequence(),verified.previous_hash())).map_err(|_| "invalid recovery commit")?;
        let local_commit_digest=Sha256::digest(&local).iter().map(|b|format!("{b:02x}")).collect::<String>();
        // Preserve the complete protected pre-transition state before the
        // credential is replaced. An incomplete write can only block retry.
        let rollback_bytes=serde_json::to_vec(&*guard).map_err(|_| "invalid_store")?;
        let rollback_hash=Sha256::digest(&rollback_bytes).iter().map(|b|format!("{b:02x}")).collect::<String>();
        let recovery_id=uuid::Uuid::parse_str(verified.retirement_id()).map_err(|_| "invalid_store")?;
        if recovery_id.is_nil() || recovery_id.to_string()!=verified.retirement_id() { return Err("invalid_store"); }
        let rollback_path=dir.join(format!("recovery-rollback-{recovery_id}.json"));
        match self.fs.read(&rollback_path) {
            Ok(existing) if existing==rollback_bytes=>{},
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
                self.fs.atomic_write(&rollback_path,&rollback_bytes).map_err(|_| "store_unavailable")?;
                if self.fs.read(&rollback_path).map_err(|_| "store_unavailable")?!=rollback_bytes { return Err("store_unavailable"); }
            },
            _=>return Err("recovery rollback snapshot changed"),
        }
        let prepared=RecoveryCommit { recovery_id:verified.retirement_id().into(),old_trust:old,
            new_generation:verified.new_generation().into(),decision_digest:decision,archive_sha256:archive.into(),
            credential_sha256:credential.digest().into(),candidate_fingerprint:verified.fingerprint().iter().map(|b|format!("{b:02x}")).collect(),
            origin:origin.into(),local_commit_digest:local_commit_digest.clone(),activation_pending:true,server_status:None,rollback_sha256:Some(rollback_hash) };
        let mut next=guard.clone();
        let record=next.record.as_mut().ok_or("not_provisioned")?;
        record.trust=new_trust; record.policy=None; record.contact=None;
        record.credential=Some(StoredCredential { origin:origin.into(),ciphertext:STANDARD.encode(sealed) });
        record.credential_revision=record.credential_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.connection_revision=record.connection_revision.checked_add(1).ok_or("revision_exhausted")?;
        record.cleared=None; record.service_enabled=false; record.recovery_commit=Some(prepared);
        next.pending=None; next.contact_challenge=None;
        self.persist(&next)?; *guard=next;
        Ok(local_commit_digest)
    }
}

#[cfg(windows)]
pub(crate) struct RecoveryExpected {
    old_trust:FleetTrust, recovery_id:String, new_generation:String, decision_digest:String,
    archive_sha256:String, credential_sha256:String, local_commit_digest:String, candidate_fingerprint:String,
    origin:String, profile:TransportProfile, credential_revision:u64, sealed_credential_sha256:[u8;32],
}
#[cfg(windows)]
fn pending_credential_matches(record:&Record, expected:&RecoveryExpected) -> bool {
    use sha2::{Digest,Sha256};
    record.credential.as_ref().is_some_and(|credential| {
        credential.origin==expected.origin && record.credential_revision==expected.credential_revision
            && <[u8;32]>::from(Sha256::digest(credential.ciphertext.as_bytes()))==expected.sealed_credential_sha256
    })
}
#[cfg(windows)]
impl RecoveryExpected {
    pub(crate) fn old_trust(&self)->&FleetTrust { &self.old_trust }
    pub(crate) fn recovery_id(&self)->&str { &self.recovery_id }
    pub(crate) fn new_generation(&self)->&str { &self.new_generation }
    pub(crate) fn decision_digest(&self)->&str { &self.decision_digest }
    pub(crate) fn archive_sha256(&self)->&str { &self.archive_sha256 }
    pub(crate) fn credential_sha256(&self)->&str { &self.credential_sha256 }
    pub(crate) fn local_commit_digest(&self)->&str { &self.local_commit_digest }
    pub(crate) fn candidate_fingerprint(&self)->&str { &self.candidate_fingerprint }
    pub(crate) fn origin(&self)->&str { &self.origin }
    pub(crate) fn profile(&self)->&TransportProfile { &self.profile }
}
#[cfg(windows)]
impl FleetMachinePolicy {
    pub(crate) fn pending_recovery(&self)->Result<Option<RecoveryExpected>,&'static str> {
        let guard=self.state.lock().map_err(|_| "store_unavailable")?;
        let Some(record)=guard.record.as_ref() else { return Ok(None) };
        let Some(commit)=record.recovery_commit.as_ref() else { return Ok(None) };
        validate_operator_key(&record.trust,record.operator_key.as_deref().ok_or("operator_key_not_pinned")?)?;
        if !commit.activation_pending { return Ok(None); }
        let credential=record.credential.as_ref().ok_or("invalid_store")?;
        let sealed=STANDARD.decode(&credential.ciphertext).map_err(|_| "invalid_store")?;
        if STANDARD.encode(&sealed)!=credential.ciphertext { return Err("invalid_store"); }
        if record.service_enabled || credential.origin!=commit.origin || record.trust.org_id!=commit.old_trust.org_id
            || record.trust.device_id!=commit.old_trust.device_id || record.trust.signer_key!=commit.old_trust.signer_key
            || record.trust.enrollment_generation!=commit.new_generation { return Err("invalid_store"); }
        Ok(Some(RecoveryExpected { old_trust:commit.old_trust.clone(),recovery_id:commit.recovery_id.clone(),
            new_generation:commit.new_generation.clone(),decision_digest:commit.decision_digest.clone(),
            archive_sha256:commit.archive_sha256.clone(),credential_sha256:commit.credential_sha256.clone(),
            local_commit_digest:commit.local_commit_digest.clone(),candidate_fingerprint:commit.candidate_fingerprint.clone(),
            origin:commit.origin.clone(),profile:record.transport_profile.clone().ok_or("invalid_store")?,
            credential_revision:record.credential_revision,
            sealed_credential_sha256:{ use sha2::{Digest,Sha256}; Sha256::digest(credential.ciphertext.as_bytes()).into() } }))
    }
}

#[cfg(windows)]
impl FleetMachinePolicy {
    pub(crate) fn activate_recovery(&self, expected:&RecoveryExpected,
        activation:&crate::fleet_recovery_credential::VerifiedActivation, now:i64) -> Result<(),&'static str> {
        if !crate::fleet_identity_handoff::recovery_timestamp_valid(activation.issued_at(),now)
            || activation.recovery_id()!=expected.recovery_id() || activation.local_commit_digest()!=expected.local_commit_digest() {
            return Err("invalid recovery activation");
        }
        let mut guard=self.state.lock().map_err(|_| "store_unavailable")?;
        let record=guard.record.as_ref().ok_or("not_provisioned")?;
        let commit=record.recovery_commit.as_ref().ok_or("recovery_not_prepared")?;
        if commit.recovery_id!=expected.recovery_id || commit.old_trust!=expected.old_trust
            || commit.new_generation!=expected.new_generation || commit.decision_digest!=expected.decision_digest
            || commit.archive_sha256!=expected.archive_sha256 || commit.credential_sha256!=expected.credential_sha256
            || commit.local_commit_digest!=expected.local_commit_digest || commit.candidate_fingerprint!=expected.candidate_fingerprint
            || commit.origin!=expected.origin || record.transport_profile.as_ref()!=Some(&expected.profile)
            || record.trust.org_id!=commit.old_trust.org_id || record.trust.device_id!=commit.old_trust.device_id
            || record.trust.signer_key!=commit.old_trust.signer_key || record.trust.enrollment_generation!=commit.new_generation
            || !pending_credential_matches(record,expected) { return Err("recovery commit changed"); }
        if !commit.activation_pending {
            return if record.service_enabled { Ok(()) } else { Err("invalid_store") };
        }
        if record.service_enabled { return Err("invalid_store"); }
        let mut next=guard.clone();
        let record=next.record.as_mut().ok_or("not_provisioned")?;
        let commit=record.recovery_commit.as_mut().ok_or("recovery_not_prepared")?;
        commit.activation_pending=false; commit.server_status=Some(activation.status().into());
        record.service_enabled=true;
        self.persist(&next)?; *guard=next;
        Ok(())
    }
}

#[cfg(all(test,windows))]
mod recovery_commit_tests {
    use super::*;
    use ed25519_dalek::{Signer,SigningKey};
    use std::{collections::HashMap,path::Path,sync::{Arc,atomic::{AtomicBool,Ordering}}};
    #[derive(Clone,Default)] struct Fs { files:Arc<Mutex<HashMap<PathBuf,Vec<u8>>>>, fail:Arc<AtomicBool> }
    impl PolicyFs for Fs {
        fn read(&self,p:&Path)->std::io::Result<Vec<u8>> { self.files.lock().unwrap().get(p).cloned().ok_or(std::io::ErrorKind::NotFound.into()) }
        fn atomic_write(&self,p:&Path,b:&[u8])->std::io::Result<()> {
            if p.file_name().is_some_and(|n|n=="fleet-machine-policy.json") && self.fail.swap(false,Ordering::SeqCst) { return Err(std::io::ErrorKind::Other.into()); }
            self.files.lock().unwrap().insert(p.into(),b.into()); Ok(())
        }
        fn ensure_dir_secure(&self,_:&Path)->Result<(),crate::policy_store::PolicyStoreError> { Ok(()) }
    }
    struct Protector;
    impl crate::fleet_credential::Protector for Protector {
        fn protect(&self,p:&[u8],e:&[u8])->Result<Vec<u8>,&'static str> { let mut v=e.to_vec(); v.extend(p.iter().zip(e).map(|(a,b)|a^b)); Ok(v) }
        fn unprotect(&self,p:&[u8],e:&[u8])->Result<zeroize::Zeroizing<Vec<u8>>,&'static str> {
            if p.len()!=64 || p[..32]!=*e { return Err("invalid"); }
            Ok(zeroize::Zeroizing::new(p[32..].iter().zip(e).map(|(a,b)|a^b).collect()))
        }
    }
    mod candidate_loopback_tests {
        use super::*;
        include!("fleet_recovery_candidate_loopback_tests.rs");
    }
    fn hex(bytes:&[u8])->String { bytes.iter().map(|b|format!("{b:02x}")).collect() }
    #[test]
    fn repeated_recovery_preserves_prior_commit_and_retries_after_failed_write() {
        use sha2::{Digest,Sha256};
        struct VariableProtector;
        impl crate::fleet_credential::Protector for VariableProtector {
            fn protect(&self,p:&[u8],e:&[u8])->Result<Vec<u8>,&'static str> { let mut v=e.to_vec();v.extend_from_slice(p);Ok(v) }
            fn unprotect(&self,p:&[u8],e:&[u8])->Result<zeroize::Zeroizing<Vec<u8>>,&'static str> {
                if !p.starts_with(e) { return Err("invalid"); } Ok(zeroize::Zeroizing::new(p[e.len()..].to_vec()))
            }
        }
        let fs=Fs::default();let dir=PathBuf::from("repeat-protected");let protector=VariableProtector;
        let signer=SigningKey::from_bytes(&[11;32]);
        let old=FleetTrust { org_id:"org-a".into(),device_id:"0480b2ca-f4c3-4599-81fc-4876f73e70fa".into(),
            enrollment_generation:"22222222-2222-4222-8222-222222222222".into(),signer_key:STANDARD.encode(signer.verifying_key().as_bytes()) };
        let first_id="11111111-1111-4111-8111-111111111111";let second_id="44444444-4444-4444-8444-444444444444";
        let first_generation="33333333-3333-4333-8333-333333333333";let second_generation="55555555-5555-4555-8555-555555555555";
        let candidate=crate::fleet_recovery_candidate::CandidateStore::open(&fs,dir.clone(),&protector).unwrap();
        let make=|trust:&FleetTrust,id:&str,generation:&str| {
            let receipt_preimage=serde_json::to_vec(&("fleet.windows.admin-retirement.v1",&trust.org_id,&trust.device_id,&trust.enrollment_generation,id,1000i64,&trust.signer_key)).unwrap();
            let receipt=serde_json::json!({"org_id":trust.org_id,"old_device_id":trust.device_id,"old_generation":trust.enrollment_generation,
                "retirement_id":id,"issued_at":1000,"signer_key":trust.signer_key,"signature":STANDARD.encode(signer.sign(&receipt_preimage).to_bytes())});
            let key=candidate.stage(trust,&serde_json::to_vec(&receipt).unwrap(),1001).unwrap();
            let hash=Some("ab".repeat(32));
            let preimage=serde_json::to_vec(&("wincommander:fleet:identity-outcome-handoff:v1",id,&trust.org_id,&trust.device_id,
                &trust.enrollment_generation,generation,&key,7u64,&hash,1000i64,&trust.signer_key)).unwrap();
            let handoff:crate::fleet_identity_handoff::Handoff=serde_json::from_value(serde_json::json!({"recovery_id":id,"org_id":trust.org_id,"device_id":trust.device_id,
                "old_generation":trust.enrollment_generation,"new_generation":generation,"new_key":key,"next_sequence":7,"previous_hash":hash,
                "issued_at":1000,"server_key":trust.signer_key,"signature":STANDARD.encode(signer.sign(&preimage).to_bytes())})).unwrap();
            let public:[u8;32]=STANDARD.decode(key).unwrap().try_into().unwrap();
            let verified=handoff.verify(trust,id,generation,&public,1001).unwrap();
            (handoff,public,verified)
        };
        let (first,key1,v1)=make(&old,first_id,first_generation);let archive="cd".repeat(32);
        crate::pipe::fleet_handoff_confirmation::test_seed_review(&fs,&dir,&old,&first,first_id,&key1,1001,&archive).unwrap();
        let current=FleetTrust { enrollment_generation:first_generation.into(),..old.clone() };
        let (second,key2,v2)=make(&current,second_id,second_generation);
        let snapshot="34".repeat(32);let proof_nonce="77777777-7777-4777-8777-777777777777";
        let proof=candidate.sign_drain(&old,&current,first_id,&v2,&snapshot,1001,proof_nonce).unwrap();
        let mut changed_current=current.clone();
        changed_current.enrollment_generation="bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into();
        assert!(candidate.sign_drain(&old,&changed_current,first_id,&v2,&snapshot,1001,proof_nonce).is_err());
        let signed=serde_json::to_vec(&("wincommander:fleet:recovery-drain:v1",&current.org_id,&current.device_id,&current.enrollment_generation,
            first_id,second_id,second_generation,hex(v2.fingerprint()),hex(v2.decision_digest()),7u64,Some("ab".repeat(32)),
            &snapshot,&current.signer_key,1001i64,proof_nonce)).unwrap();
        let retried=candidate.sign_drain(&old,&current,first_id,&v2,&snapshot,1002,"88888888-8888-4888-8888-888888888888").unwrap();
        assert_ne!(proof,retried);
        let signature:[u8;64]=STANDARD.decode(proof).unwrap().try_into().unwrap();
        assert!(ed25519_dalek::VerifyingKey::from_bytes(&key1).unwrap().verify_strict(&signed,&ed25519_dalek::Signature::from_bytes(&signature)).is_ok());
        let cancel_request=candidate.sign_cancel(&old,&current,first_id,second_id,second_generation,&hex(v2.decision_digest()),1001,proof_nonce).unwrap();
        let cancel_bytes=serde_json::to_vec(&("wincommander:fleet:recovery-cancel:v1","POST","/v1/agents/identity-handoff/cancel",
            &current.org_id,&current.device_id,&current.enrollment_generation,first_id,second_id,second_generation,hex(v2.decision_digest()),
            &current.signer_key,1001i64,proof_nonce)).unwrap();
        let cancel_signature:[u8;64]=STANDARD.decode(cancel_request).unwrap().try_into().unwrap();
        assert!(ed25519_dalek::VerifyingKey::from_bytes(&key1).unwrap().verify_strict(&cancel_bytes,&ed25519_dalek::Signature::from_bytes(&cancel_signature)).is_ok());
        crate::pipe::fleet_handoff_confirmation::test_seed_review(&fs,&dir,&current,&second,second_id,&key2,1001,&archive).unwrap();
        let decision=hex(v2.decision_digest());let nonce="66666666-6666-4666-8666-666666666666";let secret=[8u8;32];let credential_hash=hex(&Sha256::digest(secret));
        let response_preimage=serde_json::to_vec(&("wincommander:fleet:identity-handoff-prepare-response:v1","POST","/v1/agents/identity-handoff/confirm",
            &current.org_id,&current.device_id,second_id,second_generation,&decision,&archive,&credential_hash,&current.signer_key,nonce,1000i64)).unwrap();
        let response=serde_json::json!({"org_id":current.org_id,"device_id":current.device_id,"recovery_id":second_id,"new_generation":second_generation,
            "decision_digest":decision,"archive_sha256":archive,"credential_sha256":credential_hash,"server_key":current.signer_key,
            "request_nonce":nonce,"issued_at":1000,"checkin_secret_b64":STANDARD.encode(secret),"signature":STANDARD.encode(signer.sign(&response_preimage).to_bytes())});
        let credential=crate::fleet_recovery_credential::verify_response(&serde_json::to_vec(&response).unwrap(),&current,&v2,&archive,nonce,1001).unwrap();
        let prior=RecoveryCommit { recovery_id:first_id.into(),old_trust:old.clone(),new_generation:first_generation.into(),decision_digest:hex(v1.decision_digest()),
            archive_sha256:archive.clone(),credential_sha256:"ef".repeat(32),candidate_fingerprint:hex(v1.fingerprint()),origin:"https://fleet.example".into(),
            local_commit_digest:"12".repeat(32),activation_pending:false,server_status:Some("credential_committed_active".into()),rollback_sha256:None };
        let state=State { record:Some(Record { trust:current.clone(),hybrid_trust:None,hybrid_epoch_floor:0,hybrid_credential:None,hybrid_initial_pending:false,hybrid_unconfirmed:false,policy:None,audit:Vec::new(),contact:None,origins:vec!["https://fleet.example".into()],
            transport_profile:Some(TransportProfile { spki_pins:String::new(),environment_fingerprint:STANDARD.encode([1u8;32]) }),
            credential:Some(StoredCredential { origin:"https://fleet.example".into(),ciphertext:STANDARD.encode([1u8;32]) }),credential_revision:1,
            connection_revision:0,cleared:None,runtime_profile_changed:false,service_enabled:true,recovery_commit:Some(prior.clone()),
            operator_key:Some(STANDARD.encode(SigningKey::from_bytes(&[33;32]).verifying_key().as_bytes())) }),pending:None,contact_challenge:None,hybrid_enrollment:None,hybrid_claim:None };
        for variant in 0..4 {
            let mut changed=state.clone();
            let changed_trust=&mut changed.record.as_mut().unwrap().trust;
            match variant {
                0 => changed_trust.signer_key=STANDARD.encode(SigningKey::from_bytes(&[12;32]).verifying_key().as_bytes()),
                1 => changed_trust.org_id="other-org".into(),
                2 => changed_trust.device_id="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
                _ => changed_trust.enrollment_generation="bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
            }
            let isolated=Fs::default();
            isolated.atomic_write(&dir.join("fleet-machine-policy.json"),&serde_json::to_vec(&changed).unwrap()).unwrap();
            let changed_store=FleetMachinePolicy::open(Box::new(isolated.clone()),dir.clone()).unwrap();
            let before=isolated.files.lock().unwrap().clone();
            assert_eq!(changed_store.prepare_recovery_commit(&v2,&credential,&archive,"https://fleet.example",&protector,1001),Err("recovery trust changed"));
            assert_eq!(*isolated.files.lock().unwrap(),before);
        }
        let path=dir.join("fleet-machine-policy.json");fs.atomic_write(&path,&serde_json::to_vec(&state).unwrap()).unwrap();
        let store=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
        let journal_path=dir.join(format!("replacement-{first_id}.json"));
        let original_journal=fs.read(&journal_path).unwrap();let mut blocked:serde_json::Value=serde_json::from_slice(&original_journal).unwrap();
        blocked["base_sequence"]=6.into();fs.atomic_write(&journal_path,&serde_json::to_vec(&blocked).unwrap()).unwrap();
        assert!(store.prepare_recovery_commit(&v2,&credential,&archive,"https://fleet.example",&protector,1001).is_err());
        assert_eq!(store.trust_snapshot().unwrap().unwrap().0,current);
        fs.atomic_write(&journal_path,&original_journal).unwrap();
        let operator=STANDARD.encode(SigningKey::from_bytes(&[33;32]).verifying_key().as_bytes());
        crate::pipe::fleet_handoff_confirmation::test_freeze_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&v2).unwrap();
        // Lost proof responses are safe to retry; a different transition cannot reuse the freeze.
        crate::pipe::fleet_handoff_confirmation::test_freeze_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&v2).unwrap();
        assert!(crate::pipe::fleet_handoff_confirmation::test_freeze_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&v1).is_err());
        assert!(crate::pipe::fleet_handoff_confirmation::test_frozen_admission(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator).is_err());
        let cancel_nonce="99999999-9999-4999-8999-999999999999";let status="cancelled";
        let signed_cancel=serde_json::to_vec(&("wincommander:fleet:recovery-cancel-response:v1","POST","/v1/agents/identity-handoff/cancel",
            &current.org_id,&current.device_id,&current.enrollment_generation,first_id,second_id,second_generation,&decision,&current.signer_key,
            cancel_nonce,1001i64,status)).unwrap();
        let cancel=serde_json::json!({"org_id":current.org_id,"device_id":current.device_id,"current_generation":current.enrollment_generation,
            "previous_recovery_id":first_id,"next_recovery_id":second_id,"next_generation":second_generation,"decision_digest":decision,
            "server_key":current.signer_key,"request_nonce":cancel_nonce,"issued_at":1001,"status":status,
            "signature":STANDARD.encode(signer.sign(&signed_cancel).to_bytes())});let cancel=serde_json::to_vec(&cancel).unwrap();
        assert!(crate::pipe::fleet_handoff_confirmation::test_cancel_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&cancel,cancel_nonce,1002,false).is_err());
        assert!(crate::pipe::fleet_handoff_confirmation::test_cancel_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&cancel,"77777777-7777-4777-8777-777777777777",1002,true).is_err());
        crate::pipe::fleet_handoff_confirmation::test_cancel_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&cancel,cancel_nonce,1002,true).unwrap();
        let fresh_nonce="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let fresh_preimage=serde_json::to_vec(&("wincommander:fleet:recovery-cancel-response:v1","POST","/v1/agents/identity-handoff/cancel",
            &current.org_id,&current.device_id,&current.enrollment_generation,first_id,second_id,second_generation,&decision,&current.signer_key,
            fresh_nonce,1400i64,status)).unwrap();
        let mut fresh:serde_json::Value=serde_json::from_slice(&cancel).unwrap();fresh["request_nonce"]=fresh_nonce.into();fresh["issued_at"]=1400.into();
        fresh["signature"]=STANDARD.encode(signer.sign(&fresh_preimage).to_bytes()).into();let fresh=serde_json::to_vec(&fresh).unwrap();
        crate::pipe::fleet_handoff_confirmation::test_cancel_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&fresh,fresh_nonce,1401,true).unwrap();
        let after_cancel=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
        assert_eq!(after_cancel.trust_snapshot().unwrap().unwrap().0,current);
        crate::pipe::fleet_handoff_confirmation::test_cancel_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&cancel,cancel_nonce,1002,true).unwrap();
        assert!(crate::pipe::fleet_handoff_confirmation::test_freeze_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&v2).is_err());
        // Restore the independent pre-cancellation fixture to test a successful
        // commit; production permanently rejects a cancelled decision.
        fs.atomic_write(&journal_path,&original_journal).unwrap();
        crate::pipe::fleet_handoff_confirmation::test_freeze_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&v2).unwrap();
        fs.fail.store(true,Ordering::SeqCst);
        assert!(store.prepare_recovery_commit(&v2,&credential,&archive,"https://fleet.example",&protector,1001).is_err());
        assert_eq!(store.trust_snapshot().unwrap().unwrap().0,current);
        let archived=fs.read(&dir.join(format!("recovery-commit-{first_id}.json"))).unwrap();
        assert_eq!(archived,serde_json::to_vec(&prior).unwrap());
        let reopened=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
        assert!(crate::pipe::fleet_handoff_confirmation::test_frozen_admission(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator).is_err());
        let digest=reopened.prepare_recovery_commit(&v2,&credential,&archive,"https://fleet.example",&protector,1001).unwrap();
        let again=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
        assert_eq!(again.prepare_recovery_commit(&v2,&credential,&archive,"https://fleet.example",&protector,1001).unwrap(),digest);
        assert_eq!(fs.read(&dir.join(format!("recovery-commit-{first_id}.json"))).unwrap(),archived);
        assert_eq!(candidate.public_key(&old,first_id).unwrap(),key1);
        assert!(again.rollback_cancelled_prepare(|_,_,_,_,_|Err("denied")).is_err());
        assert_eq!(again.trust_snapshot().unwrap().unwrap().0.enrollment_generation,second_generation);
        let rollback_path=dir.join(format!("recovery-rollback-{second_id}.json"));
        let snapshot=fs.read(&rollback_path).unwrap();fs.atomic_write(&rollback_path,b"tampered").unwrap();
        assert!(again.rollback_cancelled_prepare(|_,_,_,_,_|Ok(())).is_err());
        fs.atomic_write(&rollback_path,&snapshot).unwrap();
        fs.fail.store(true,Ordering::SeqCst);
        assert!(again.rollback_cancelled_prepare(|_,_,_,_,_|Ok(())).is_err());
        let after_failed_rollback=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
        assert_eq!(after_failed_rollback.trust_snapshot().unwrap().unwrap().0.enrollment_generation,second_generation);
        after_failed_rollback.rollback_cancelled_prepare(|trust,previous,next,generation,seen_decision| {
            assert_eq!(trust,&current);assert_eq!(previous,first_id);assert_eq!(next,second_id);
            assert_eq!(generation,second_generation);assert_eq!(seen_decision,decision);Ok(())
        }).unwrap();
        let restored=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
        assert_eq!(restored.trust_snapshot().unwrap().unwrap().0,current);
        assert!(crate::pipe::fleet_handoff_confirmation::test_frozen_admission(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator).is_err());
        crate::pipe::fleet_handoff_confirmation::test_cancel_drain(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator,&fresh,fresh_nonce,1401,true).unwrap();
        crate::pipe::fleet_handoff_confirmation::test_frozen_admission(&fs,&dir,&protector,&current,&old,&v1,&archive,&operator).unwrap();
    }
    /// Opt-in cross-process test against the disposable server fixture only.
    /// Native consent and the Pro archive broker are deliberately simulated.
    #[tokio::test]
    #[ignore = "requires isolated recovery loopback fixture"]
    async fn recovery_loopback_prepare_commit_and_contact() {
        struct LoopProtector;
        impl crate::fleet_credential::Protector for LoopProtector {
            fn protect(&self,p:&[u8],e:&[u8])->Result<Vec<u8>,&'static str> {
                if e.is_empty() { return Err("invalid"); } let mut v=e.to_vec();v.extend(p.iter().enumerate().map(|(i,b)|b^e[i%e.len()]));Ok(v)
            }
            fn unprotect(&self,p:&[u8],e:&[u8])->Result<zeroize::Zeroizing<Vec<u8>>,&'static str> {
                if e.is_empty() || !p.starts_with(e) { return Err("invalid"); }
                Ok(zeroize::Zeroizing::new(p[e.len()..].iter().enumerate().map(|(i,b)|b^e[i%e.len()]).collect()))
            }
        }
        let base=std::env::var("FLEET_RECOVERY_LOOPBACK_BASE").expect("set isolated loopback fixture URL");
        let parsed=url::Url::parse(&base).unwrap();
        assert_eq!(parsed.scheme(),"http");
        assert!(matches!(parsed.host_str(),Some("127.0.0.1")|Some("localhost")));
        let http=reqwest::Client::new();
        let fixture:serde_json::Value=http.get(format!("{base}/test/fixture")).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        let field=|name:&str|fixture.get(name).and_then(serde_json::Value::as_str).unwrap().to_owned();
        let trust=FleetTrust { org_id:field("org_id"),device_id:field("device_id"),enrollment_generation:field("old_generation"),signer_key:field("server_key") };
        let recovery=field("recovery_id");let operator=field("operator_key");
        let receipt=serde_json::to_vec(fixture.get("retirement_receipt").unwrap()).unwrap();
        let fs=Fs::default();let dir=PathBuf::from("loopback-protected");
        let state=State { record:Some(Record { trust:trust.clone(),hybrid_trust:None,hybrid_epoch_floor:0,hybrid_credential:None,hybrid_initial_pending:false,hybrid_unconfirmed:false,policy:None,audit:Vec::new(),contact:None,
            origins:vec![base.clone()],transport_profile:Some(TransportProfile { spki_pins:String::new(),
                environment_fingerprint:crate::fleet_runtime_env::fingerprint().unwrap() }),credential:None,credential_revision:0,
            connection_revision:0,cleared:None,runtime_profile_changed:false,service_enabled:true,recovery_commit:None,operator_key:Some(operator) }),
            pending:None,contact_challenge:None,hybrid_enrollment:None,hybrid_claim:None };
        fs.atomic_write(&dir.join("fleet-machine-policy.json"),&serde_json::to_vec(&state).unwrap()).unwrap();
        let store=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
        assert_eq!(store.recovery_execution_mode().unwrap(),(trust.device_id.clone(),"legacy"));
        let protector=LoopProtector;let candidate=crate::fleet_recovery_candidate::CandidateStore::open(&fs,dir.clone(),&protector).unwrap();
        let now=||i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()).unwrap();
        let (public, conflict)=crate::fleet_recovery_transport::submit_candidate(&store,&candidate,&receipt,||true).await.unwrap();
        assert!(!conflict);
        assert_eq!(crate::fleet_recovery_transport::submit_candidate(&store,&candidate,&receipt,||true).await.unwrap(),(public.clone(),false));
        assert!(crate::fleet_recovery_transport::submit_candidate(&store,&candidate,&receipt,||false).await.is_err());
        http.post(format!("{base}/test/fixture/candidate")).json(&serde_json::json!({"candidate_key":public})).send().await.unwrap().error_for_status().unwrap();
        let handoff=crate::fleet_recovery_transport::request_handoff(&store,&candidate,&receipt,||true).await.unwrap();
        let key:[u8;32]=STANDARD.decode(&public).unwrap().try_into().unwrap();let archive="ab".repeat(32);
        crate::pipe::fleet_handoff_confirmation::test_seed_review(&fs,&dir,&trust,&handoff,&recovery,&key,now(),&archive).unwrap();
        let status=crate::fleet_recovery_transport::prepare_and_resume(&store,&candidate,&receipt,&handoff,&recovery,
            handoff.new_generation(),&archive,&protector,||true).await.unwrap();
        assert_eq!(status,"credential_committed_pending_approval");
        assert!(crate::pipe::fleet_handoff_confirmation::test_recovery_contact(&store,false,false,&protector).await.unwrap());
        http.post(format!("{base}/test/fixture/activate")).send().await.unwrap().error_for_status().unwrap();
        assert!(!crate::pipe::fleet_handoff_confirmation::test_recovery_contact(&store,false,false,&protector).await.unwrap());
        crate::pipe::fleet_handoff_confirmation::test_recovery_contact(&store,false,true,&protector).await.unwrap();
        crate::pipe::fleet_handoff_confirmation::test_recovery_contact(&store,true,false,&protector).await.unwrap();
        if std::env::var("FLEET_SIMPLE_LEAVE_LOOPBACK").ok().as_deref()==Some("1") {
            // Leave proof and cleanup need no separate operator key. The test
            // modifies only its in-memory mock store after recovery setup.
            let path=dir.join("fleet-machine-policy.json");
            let mut saved:State=serde_json::from_slice(&fs.read(&path).unwrap()).unwrap();
            saved.record.as_mut().unwrap().operator_key=None;
            fs.atomic_write(&path,&serde_json::to_vec(&saved).unwrap()).unwrap();
            let store=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
            assert!(crate::fleet_recovery_leave::request(&store,&protector,"wrong-device").await.is_err());
            let request=crate::fleet_recovery_leave::request(&store,&protector,&trust.device_id).await.unwrap();
            assert_eq!(request["status"],"pending");
            let pending=crate::pipe::fleet_handoff_confirmation::test_simple_leave_status(&store,&fs,&dir,&protector,&candidate).await.unwrap();
            assert_eq!(pending["approved"],false);
            http.post(format!("{base}/test/fixture/remove")).send().await.unwrap().error_for_status().unwrap();
            let approved=crate::pipe::fleet_handoff_confirmation::test_simple_leave_status(&store,&fs,&dir,&protector,&candidate).await.unwrap();
            assert_eq!(approved["approved"],true);
            assert!(store.credential_snapshot(&protector).unwrap().is_none());
            let reopened=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
            assert!(reopened.confirmed_leave().unwrap().is_some());
            let (_,_,token,connection,_)=reopened.contact_state().unwrap().unwrap();
            assert!(reopened.clear_credential(&token.unwrap(),&connection).unwrap());
            assert!(reopened.confirmed_leave().unwrap().is_some());
            return;
        }
        let results:serde_json::Value=http.get(format!("{base}/test/fixture/results")).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        let commands:Vec<fleet_proto::SignedCommand>=serde_json::from_value(results["commands"].clone()).unwrap();assert_eq!(commands.len(),2);
        for command in commands { crate::pipe::fleet_handoff_confirmation::test_seed_late_evidence(&store,&fs,&dir,&protector,&candidate,command).unwrap(); }
        assert_eq!(crate::pipe::fleet_handoff_confirmation::test_pending_result_count(&store,&fs,&dir,&candidate).unwrap(),2);
        assert!(crate::pipe::fleet_handoff_confirmation::test_recovery_results(&store,&fs,&dir,&protector,&candidate).await.is_err());
        assert_eq!(crate::pipe::fleet_handoff_confirmation::test_pending_result_count(&store,&fs,&dir,&candidate).unwrap(),2);
        http.post(format!("{base}/test/fixture/results/release")).send().await.unwrap().error_for_status().unwrap();
        crate::pipe::fleet_handoff_confirmation::test_recovery_results(&store,&fs,&dir,&protector,&candidate).await.unwrap();
        assert_eq!(crate::pipe::fleet_handoff_confirmation::test_pending_result_count(&store,&fs,&dir,&candidate).unwrap(),0);
        let command:fleet_proto::SignedCommand=http.get(format!("{base}/test/fixture/unknown")).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        let command_id=command.command_id.clone();
        let seeded=crate::pipe::fleet_handoff_confirmation::test_seed_late_evidence(&store,&fs,&dir,&protector,&candidate,command).unwrap();
        http.post(format!("{base}/test/fixture/unknown")).json(&seeded).send().await.unwrap().error_for_status().unwrap();
        crate::pipe::fleet_handoff_confirmation::test_upload_late(&store,&fs,&dir,&protector,&candidate,&command_id).await.unwrap();
        // A repeated upload returns from the retained verified receipt; it does not delete evidence.
        crate::pipe::fleet_handoff_confirmation::test_upload_late(&store,&fs,&dir,&protector,&candidate,&command_id).await.unwrap();
        let created:serde_json::Value=http.post(format!("{base}/test/fixture/vault")).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        let id=created["command_id"].as_str().unwrap();
        crate::pipe::fleet_handoff_confirmation::test_vault_work(&store,&fs,&dir,&protector,&candidate,id).await.unwrap();
        let report:serde_json::Value=http.get(format!("{base}/test/fixture/vault/{id}")).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        assert!(matches!(report["status"].as_str(),Some("applied"|"verified")));
        assert_eq!(report["result"],serde_json::json!({"installed":true,"scope":"agent_authorized","mounted_count":2}));
    }
    #[test]
    fn operator_pin_requires_approval_distinct_key_and_atomic_storage() {
        let signer=SigningKey::from_bytes(&[11;32]);
        let trust=FleetTrust { org_id:"org-a".into(),device_id:"0480b2ca-f4c3-4599-81fc-4876f73e70fa".into(),
            enrollment_generation:"22222222-2222-4222-8222-222222222222".into(),signer_key:STANDARD.encode(signer.verifying_key().as_bytes()) };
        let fs=Fs::default(); let dir=PathBuf::from("operator-test");
        let state=State { record:Some(Record { trust:trust.clone(),hybrid_trust:None,hybrid_epoch_floor:0,hybrid_credential:None,hybrid_initial_pending:false,hybrid_unconfirmed:false,policy:None,audit:Vec::new(),contact:None,origins:Vec::new(),transport_profile:None,
            credential:None,credential_revision:0,connection_revision:0,cleared:None,runtime_profile_changed:false,service_enabled:true,recovery_commit:None,operator_key:None }),pending:None,contact_challenge:None,hybrid_enrollment:None,hybrid_claim:None };
        let mut legacy=serde_json::to_value(&state).unwrap(); legacy["record"].as_object_mut().unwrap().remove("operator_key");
        fs.atomic_write(&dir.join("fleet-machine-policy.json"),&serde_json::to_vec(&legacy).unwrap()).unwrap();
        let store=FleetMachinePolicy::open(Box::new(fs.clone()),dir).unwrap(); assert!(store.operator_key().unwrap().is_none());
        let operator=STANDARD.encode(SigningKey::from_bytes(&[33;32]).verifying_key().as_bytes());
        assert!(store.commit_operator_key_with(trust.signer_key.clone(),|_,_| panic!("equal key must be rejected first")).is_err());
        assert!(validate_operator_key(&trust,&STANDARD.encode([0u8;32])).is_err());
        assert!(validate_operator_key(&trust,"not-canonical!").is_err());
        assert!(store.commit_operator_key_with(operator.clone(),|_,_| Err("denied")).is_err());
        fs.fail.store(true,Ordering::SeqCst);
        assert!(store.commit_operator_key_with(operator.clone(),|_,_| Ok("S-1-5-21-1".into())).is_err());
        assert!(store.operator_key().unwrap().is_none());
        store.commit_operator_key_with(operator.clone(),|seen,key| { assert_eq!(seen,&trust); assert_eq!(key,operator); Ok("S-1-5-21-1".into()) }).unwrap();
        assert_eq!(store.operator_key().unwrap(),Some(operator));
    }
    #[test]
    fn prepared_credential_is_atomic_restartable_and_inactive_until_signed_commit() {
        use sha2::{Digest,Sha256};
        let trust=FleetTrust { org_id:"org-a".into(),device_id:"0480b2ca-f4c3-4599-81fc-4876f73e70fa".into(),
            enrollment_generation:"22222222-2222-4222-8222-222222222222".into(),signer_key:"Zr5+Myx6RTMyvZ0Kf32wVfXF7xoGraZtmLOftoEMRzo=".into() };
        let h:crate::fleet_identity_handoff::Handoff=serde_json::from_value(serde_json::json!({"recovery_id":"11111111-1111-4111-8111-111111111111","org_id":trust.org_id,"device_id":trust.device_id,
            "old_generation":trust.enrollment_generation,"new_generation":"33333333-3333-4333-8333-333333333333","new_key":"kaKKC3Q4FZOk2UaVeSCJJq/IrYLIg5t2RDWbnrqaSzo=",
            "next_sequence":7,"previous_hash":"ab".repeat(32),"issued_at":1000,"server_key":trust.signer_key,
            "signature":"h97rXEpV9lskSg1jI2698m0urZAufIhNyq0qhcDFCNc2lVnOBaCWXIWxHgi6GDWP3rRyvbqdv/zhsv/TvjWZDQ=="})).unwrap();
        let key:[u8;32]=STANDARD.decode("kaKKC3Q4FZOk2UaVeSCJJq/IrYLIg5t2RDWbnrqaSzo=").unwrap().try_into().unwrap();
        let verified=h.verify(&trust,"11111111-1111-4111-8111-111111111111","33333333-3333-4333-8333-333333333333",&key,1001).unwrap();
        let archive="cd".repeat(32); let nonce="44444444-4444-4444-8444-444444444444"; let secret=[8u8;32];
        let decision=hex(verified.decision_digest()); let secret_hash=hex(&Sha256::digest(secret));
        let response_preimage=serde_json::to_vec(&("wincommander:fleet:identity-handoff-prepare-response:v1","POST","/v1/agents/identity-handoff/confirm",
            &trust.org_id,&trust.device_id,verified.retirement_id(),verified.new_generation(),&decision,&archive,&secret_hash,&trust.signer_key,nonce,1000i64)).unwrap();
        let response=serde_json::json!({"org_id":trust.org_id,"device_id":trust.device_id,"recovery_id":verified.retirement_id(),"new_generation":verified.new_generation(),
            "decision_digest":decision,"archive_sha256":archive,"credential_sha256":secret_hash,"server_key":trust.signer_key,"request_nonce":nonce,"issued_at":1000,
            "checkin_secret_b64":STANDARD.encode(secret),"signature":STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&response_preimage).to_bytes())});
        let credential=crate::fleet_recovery_credential::verify_response(&serde_json::to_vec(&response).unwrap(),&trust,&verified,&archive,nonce,1001).unwrap();
        let fs=Fs::default(); let dir=PathBuf::from("protected"); let path=dir.join("fleet-machine-policy.json");
        let state=State { record:Some(Record { trust:trust.clone(),hybrid_trust:None,hybrid_epoch_floor:0,hybrid_credential:None,hybrid_initial_pending:false,hybrid_unconfirmed:false,policy:None,audit:Vec::new(),contact:None,origins:vec!["https://fleet.example".into()],
            transport_profile:Some(TransportProfile { spki_pins:String::new(),environment_fingerprint:STANDARD.encode([1u8;32]) }),
            credential:None,credential_revision:0,connection_revision:0,cleared:None,runtime_profile_changed:false,service_enabled:true,recovery_commit:None,operator_key:Some(STANDARD.encode(SigningKey::from_bytes(&[33;32]).verifying_key().as_bytes())) }),pending:None,contact_challenge:None,hybrid_enrollment:None,hybrid_claim:None };
        fs.atomic_write(&path,&serde_json::to_vec(&state).unwrap()).unwrap();
        let review=serde_json::json!({"org_id":trust.org_id,"device_id":trust.device_id,"old_generation":trust.enrollment_generation,
            "recovery_id":verified.retirement_id(),"new_generation":verified.new_generation(),"handoff_digest":hex(verified.digest()),"decision_digest":decision,
            "candidate_fingerprint":hex(verified.fingerprint()),"archive_sha256":archive,"journal_present":false,"journal_sha256":null,
            "archived_actor_sid":"S-1-5-21-1","reviewing_actor_sid":"S-1-5-21-1","outcome":"unknown_acknowledged"});
        fs.atomic_write(&dir.join(format!("review-{}.json",verified.retirement_id())),&serde_json::to_vec(&review).unwrap()).unwrap();
        let store=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
        assert_eq!(store.recovery_execution_mode().unwrap(),(trust.device_id.clone(),"legacy"));
        fs.fail.store(true,Ordering::SeqCst);
        assert!(store.prepare_recovery_commit(&verified,&credential,&archive,"https://fleet.example",&Protector,1001).is_err());
        assert_eq!(store.trust_snapshot().unwrap().unwrap().0,trust);
        let digest=store.prepare_recovery_commit(&verified,&credential,&archive,"https://fleet.example",&Protector,1001).unwrap();
        assert_eq!(store.recovery_execution_mode().unwrap(),(trust.device_id.clone(),"pending"));
        assert!(store.credential_snapshot(&Protector).unwrap().is_none());
        let reopened=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
        let expected=reopened.pending_recovery().unwrap().unwrap(); assert_eq!(expected.local_commit_digest(),digest);
        assert!(reopened.begin_enrollment(trust.device_id.clone(),trust.signer_key.clone(),vec!["https://fleet.example".into()],None).is_err());
        let clear_token=reopened.credential_clear_token().unwrap().unwrap();
        let connection_token=reopened.contact_state().unwrap().unwrap().3;
        assert!(reopened.clear_credential(&clear_token,&connection_token).is_err());
        let status="credential_committed_pending_approval";
        let commit_preimage=serde_json::to_vec(&("wincommander:fleet:identity-handoff-commit-response:v1","POST","/v1/agents/identity-handoff/commit",
            &trust.org_id,&trust.device_id,verified.retirement_id(),verified.new_generation(),&decision,&archive,&digest,&secret_hash,&trust.signer_key,nonce,1000i64,status)).unwrap();
        let commit=serde_json::json!({"org_id":trust.org_id,"device_id":trust.device_id,"recovery_id":verified.retirement_id(),"new_generation":verified.new_generation(),
            "decision_digest":decision,"archive_sha256":archive,"local_commit_digest":digest,"credential_sha256":secret_hash,"server_key":trust.signer_key,
            "request_nonce":nonce,"issued_at":1000,"status":status,"signature":STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&commit_preimage).to_bytes())});
        let activation=crate::fleet_recovery_credential::verify_commit_response(&serde_json::to_vec(&commit).unwrap(),&expected,nonce,1001).unwrap();
        let unchanged=reopened.state.lock().unwrap().clone();
        let persisted=fs.read(&path).unwrap();
        for malformed in ["invalid!", "AQ"] {
            reopened.state.lock().unwrap().record.as_mut().unwrap().credential.as_mut().unwrap().ciphertext=malformed.into();
            assert!(reopened.pending_recovery().is_err());
            assert_eq!(fs.read(&path).unwrap(),persisted);
            *reopened.state.lock().unwrap()=unchanged.clone();
        }
        struct ResealProtector;
        impl crate::fleet_credential::Protector for ResealProtector {
            fn protect(&self,plain:&[u8],entropy:&[u8])->Result<Vec<u8>,&'static str> {
                let mut result=crate::fleet_credential::Protector::protect(&Protector,plain,entropy)?;
                result.push(1); Ok(result)
            }
            fn unprotect(&self,sealed:&[u8],entropy:&[u8])->Result<zeroize::Zeroizing<Vec<u8>>,&'static str> {
                let original=if sealed.len()==65 && sealed.last()==Some(&1) { &sealed[..64] } else { sealed };
                crate::fleet_credential::Protector::unprotect(&Protector,original,entropy)
            }
        }
        let binding=crate::fleet_credential::CredentialBinding { org_id:&trust.org_id,device_id:&trust.device_id,
            enrollment_generation:verified.new_generation(),origin:"https://fleet.example" };
        let resealed=crate::fleet_credential::seal(&ResealProtector,&binding,&secret).unwrap();
        assert_eq!(crate::fleet_credential::unseal(&ResealProtector,&binding,&resealed).unwrap().as_slice(),&secret);
        assert!(reopened.validate_pending_recovery_credential(&expected,&ResealProtector).is_ok());
        for variant in 0..3 {
            {
                let mut state=reopened.state.lock().unwrap();
                let record=state.record.as_mut().unwrap();
                match variant {
                    0 => record.credential.as_mut().unwrap().origin="https://other.example".into(),
                    1 => record.credential.as_mut().unwrap().ciphertext=STANDARD.encode(&resealed),
                    _ => record.credential_revision+=1,
                }
            }
            assert!(reopened.validate_pending_recovery_credential(&expected,&ResealProtector).is_err());
            assert_eq!(reopened.activate_recovery(&expected,&activation,1001),Err("recovery commit changed"));
            assert!(!reopened.state.lock().unwrap().record.as_ref().unwrap().service_enabled);
            assert_eq!(fs.read(&path).unwrap(),persisted);
            *reopened.state.lock().unwrap()=unchanged.clone();
        }
        reopened.activate_recovery(&expected,&activation,1001).unwrap();
        reopened.activate_recovery(&expected,&activation,1001).unwrap();
        assert_eq!(reopened.recovery_execution_mode().unwrap(),(trust.device_id.clone(),"recovered"));
        let mut snapshot=reopened.credential_snapshot(&Protector).unwrap().unwrap();
        assert_eq!(snapshot.secret.as_slice(),&secret);
        assert!(reopened.pending_recovery().unwrap().is_none());
        let signer=SigningKey::from_bytes(&[11;32]);
        let mut config=fleet_proto::ConfigEpoch { org_id:fleet_proto::OrgId(trust.org_id.clone()),version:7,
            config_json:serde_json::json!({}),target_kind:"org".into(),target_id:None,locked_paths:vec![],managed:true,
            signature:String::new(),signer_key:trust.signer_key.clone() };
        config.signature=STANDARD.encode(signer.sign(&fleet_proto::epoch_preimage(&fleet_proto::EpochSigningInput { version:config.version,
            config:&config.config_json,locked_paths:&config.locked_paths,managed:config.managed,target_kind:&config.target_kind,target_id:config.target_id.as_deref() })).to_bytes());
        let mut policy=fleet_proto::PolicyEnvelope { org_id:fleet_proto::OrgId(trust.org_id.clone()),device_id:fleet_proto::DeviceId(trust.device_id.clone()),
            enrollment_generation:Some(verified.new_generation().into()),config_revision:Some(1),resolution_revision:Some(1),config:Some(config),
            shield:None,shield_revision:0,clipboard_guard_enabled:false,signature:String::new(),signer_key:trust.signer_key.clone() };
        policy.signature=STANDARD.encode(signer.sign(&policy.preimage()).to_bytes());
        let mut forged=policy.clone(); forged.enrollment_generation=Some(trust.enrollment_generation.clone());
        assert!(reopened.install_recovery_policy(forged,&snapshot).is_err());
        let mut forged=policy.clone(); forged.config.as_mut().unwrap().config_json=serde_json::json!({"tampered":true});
        forged.signature=STANDARD.encode(signer.sign(&forged.preimage()).to_bytes());
        assert!(reopened.install_recovery_policy(forged,&snapshot).is_err());
        reopened.install_recovery_policy(policy.clone(),&snapshot).unwrap();
        assert_eq!(reopened.held_recovery_epoch(&snapshot).unwrap(),7);
        let held=reopened.held_recovery_policy(&snapshot).unwrap();
        assert_eq!(held.enrollment_generation,verified.new_generation());assert_eq!(held.epoch_version,7);
        assert_eq!(held.policy_sha256,fleet_proto::canonical_json_sha256(&serde_json::to_value(&policy).unwrap()));
        assert_eq!(held.policy,policy);
        let request=crate::pipe::fleet_handoff_confirmation::test_held_policy_request(&reopened,&snapshot).unwrap();
        assert_eq!(request.recovery_held_policy.as_ref(),Some(&held));
        assert!(request.capabilities.iter().any(|value|value=="windows_recovery_commands_only_v1"));
        assert_eq!(request.hmac,fleet_agent_core::compute_request_hmac_v2(&snapshot.secret,"POST","/v1/agents/checkin",&request).unwrap());
        let results=crate::pipe::fleet_handoff_confirmation::test_results_policy_request(&reopened,&snapshot).unwrap();
        assert_eq!(results.recovery_held_policy.as_ref(),Some(&held));
        assert!(results.capabilities.iter().any(|value|value=="windows_recovery_results_v1"));
        assert_eq!(results.hmac,fleet_agent_core::compute_request_hmac_v2(&snapshot.secret,"POST","/v1/agents/checkin",&results).unwrap());
        snapshot.revision+=1;
        assert!(reopened.held_recovery_epoch(&snapshot).is_err());
        assert!(reopened.held_recovery_policy(&snapshot).is_err());
        snapshot.revision-=1;
        let generation=snapshot.trust.enrollment_generation.clone();
        snapshot.trust.enrollment_generation=trust.enrollment_generation.clone();
        assert!(reopened.held_recovery_epoch(&snapshot).is_err());
        assert!(reopened.held_recovery_policy(&snapshot).is_err());
        snapshot.trust.enrollment_generation=generation;
        let after_restart=FleetMachinePolicy::open(Box::new(fs.clone()),dir.clone()).unwrap();
        assert_eq!(after_restart.snapshot().unwrap(),Some(policy));
        let snapshot=after_restart.credential_snapshot(&Protector).unwrap().unwrap();
        assert_eq!(after_restart.held_recovery_epoch(&snapshot).unwrap(),7);
    }
}

#[cfg(windows)]
impl FleetMachinePolicy {
    /// Revalidates the reviewed decision against current protected trust.
    pub(crate) fn validate_recovery_review(&self, verified:&crate::fleet_identity_handoff::VerifiedHandoff,
        archive:&str) -> Result<(),&'static str> {
        self.with_handoff_trust(|trust| {
            let dir=self.path.parent().ok_or("store_unavailable")?;
            crate::pipe::fleet_handoff_confirmation::initialize_replacement(&*self.fs,dir,trust,verified,archive)
        })
    }
}

#[cfg(windows)]
impl FleetMachinePolicy {
    pub(crate) fn validate_pending_recovery_credential(&self, expected:&RecoveryExpected,
        protector:&dyn crate::fleet_credential::Protector) -> Result<(),&'static str> {
        use sha2::{Digest,Sha256};
        let guard=self.state.lock().map_err(|_| "store_unavailable")?;
        let record=guard.record.as_ref().ok_or("not_provisioned")?;
        let commit=record.recovery_commit.as_ref().ok_or("recovery_not_prepared")?;
        if !commit.activation_pending || record.service_enabled || !pending_credential_matches(record,expected) || commit.recovery_id!=expected.recovery_id
            || commit.local_commit_digest!=expected.local_commit_digest || commit.credential_sha256!=expected.credential_sha256
            || commit.origin!=expected.origin || record.trust.org_id!=expected.old_trust.org_id
            || record.trust.device_id!=expected.old_trust.device_id || record.trust.signer_key!=expected.old_trust.signer_key
            || record.trust.enrollment_generation!=expected.new_generation { return Err("recovery commit changed"); }
        let credential=record.credential.as_ref().ok_or("invalid_store")?;
        if credential.origin!=commit.origin { return Err("invalid_store"); }
        let sealed=STANDARD.decode(&credential.ciphertext).map_err(|_| "invalid_store")?;
        if STANDARD.encode(&sealed)!=credential.ciphertext { return Err("invalid_store"); }
        let binding=crate::fleet_credential::CredentialBinding { org_id:&record.trust.org_id,device_id:&record.trust.device_id,
            enrollment_generation:&record.trust.enrollment_generation,origin:&credential.origin };
        let secret=crate::fleet_credential::unseal(protector,&binding,&sealed)?;
        let digest=Sha256::digest(secret.as_slice()).iter().map(|b|format!("{b:02x}")).collect::<String>();
        if digest!=commit.credential_sha256 { return Err("invalid_store"); }
        Ok(())
    }
}

#[cfg(windows)]
impl FleetMachinePolicy {
    pub(crate) fn operator_key(&self)->Result<Option<String>,&'static str> {
        let guard=self.state.lock().map_err(|_| "store_unavailable")?;
        let Some(record)=guard.record.as_ref() else { return Ok(None) };
        if let Some(key)=&record.operator_key { validate_operator_key(&record.trust,key)?; }
        Ok(record.operator_key.clone())
    }
    pub(crate) fn commit_operator_key(&self, operator:String,
        approval:crate::pipe::fleet_handoff_confirmation::OperatorApproval,
        peer:&crate::pipe::AuthenticatedPipePeer)->Result<(),&'static str> {
        self.commit_operator_key_with(operator, |trust,key| approval.validate(trust,key,peer))
    }
    fn commit_operator_key_with(&self,operator:String,
        verify:impl FnOnce(&FleetTrust,&str)->Result<String,&'static str>)->Result<(),&'static str> {
        let mut guard=self.state.lock().map_err(|_| "store_unavailable")?;
        let current=guard.record.as_ref().ok_or("not_provisioned")?;
        if current.recovery_commit.as_ref().is_some_and(|commit|commit.activation_pending) { return Err("recovery_pending"); }
        validate_operator_key(&current.trust,&operator)?;
        let actor=verify(&current.trust,&operator)?;
        let mut next=guard.clone(); let record=next.record.as_mut().ok_or("not_provisioned")?;
        if record.operator_key.as_deref()!=Some(&operator) {
            if record.audit.len()>=64 { record.audit.remove(0); }
            record.audit.push(trust_audit("operator_key_pin",&actor,&record.trust.enrollment_generation)?);
            record.operator_key=Some(operator);
        }
        self.persist(&next)?; *guard=next; Ok(())
    }
}
