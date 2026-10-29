// SPDX-License-Identifier: AGPL-3.0-or-later
//! Machine-bound Fleet trust and signed policy. Never trust a signer supplied
//! only by the policy sender: an administrator must provision it first.
#![allow(dead_code)]

use crate::policy_store::PolicyFs;
use base64::{engine::general_purpose::STANDARD, Engine};
use fleet_proto::{EpochSigningInput, PolicyEnvelope};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Mutex};

#[cfg(windows)]
#[path = "peer_auth_deployment_acl.rs"]
mod protected_acl;

/// Separate trust store. Existing directories and files are checked before
/// any ACL change or read, preventing adoption of pre-planted trust.
#[cfg(windows)]
pub struct WindowsFleetPolicyFs;

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

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    trust: FleetTrust,
    policy: Option<PolicyEnvelope>,
    audit: Vec<TrustAudit>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingEnrollment {
    device_id: String,
    signer_key: String,
    challenge: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    record: Option<Record>,
    pending: Option<PendingEnrollment>,
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
        || !valid_uuid(&trust.device_id)
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
    ) -> Result<String, &'static str> {
        use rand::RngCore;
        let placeholder = FleetTrust {
            org_id: "pending".into(),
            device_id: device_id.clone(),
            signer_key: signer_key.clone(),
            enrollment_generation: "00000000-0000-4000-8000-000000000000".into(),
        };
        validate_trust(&placeholder)?;
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
        let mut next = guard.clone();
        next.pending = Some(PendingEnrollment {
            device_id,
            signer_key,
            challenge: challenge.clone(),
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
        if pending.device_id != trust.device_id || pending.signer_key != trust.signer_key {
            return Err("enrollment_binding_mismatch");
        }
        // Server canonicalizes UUIDs before signing; require canonical inputs.
        if trust.device_id != trust.device_id.to_ascii_lowercase()
            || trust.enrollment_generation != trust.enrollment_generation.to_ascii_lowercase()
        {
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
        let next = State {
            record: Some(Record {
                trust,
                policy: None,
                audit,
            }),
            pending: None,
        };
        self.persist(&next)?;
        *guard = next;
        Ok(())
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
        let next = State {
            record: Some(Record {
                trust: replacement,
                policy: None,
                audit,
            }),
            pending: None,
        };
        self.persist(&next)?;
        *guard = next;
        Ok(())
    }

    pub fn install(&self, policy: PolicyEnvelope) -> Result<(), &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
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
                policy: Some(policy),
                audit: current.audit.clone(),
            }),
            pending: guard.pending.clone(),
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

    fn persist(&self, state: &State) -> Result<(), &'static str> {
        let bytes = serde_json::to_vec(state).map_err(|_| "invalid_store")?;
        self.fs
            .atomic_write(&self.path, &bytes)
            .map_err(|_| "store_unavailable")
    }
}

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
        let challenge = store
            .begin_enrollment(trust.device_id.clone(), trust.signer_key.clone())
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
