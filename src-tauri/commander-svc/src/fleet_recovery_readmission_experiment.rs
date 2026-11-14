// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only readmission authority; no IPC or production transport uses this.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::{rngs::SysRng, TryRng};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use zeroize::Zeroize;

const INVALID: &str = "recovery readmission unavailable";
const TTL: Duration = Duration::from_secs(30);

pub(super) struct Offer {
    id: String,
    snapshot: CredentialSnapshot,
    challenge: String,
    request: Vec<u8>,
    expires: Instant,
}

pub(super) struct Claim(Offer);
impl Claim {
    pub(super) fn request(&self) -> &[u8] { &self.0.request }
}

#[derive(Default)]
pub(super) struct Registry { pending: Option<Offer>, claimed: Option<String> }

#[derive(Deserialize)]
struct Reply {
    org_id: String,
    device_id: String,
    shield_progress_generation: String,
    shield_progress_generation_signature: String,
    outcome_pubkey_b64: String,
    server_signing_key_b64: String,
    checkin_secret_b64: String,
}
impl Drop for Reply {
    fn drop(&mut self) { self.checkin_secret_b64.zeroize(); }
}

impl FleetMachinePolicy {
    /// Experiment only: the claim is bound to the protected active journal,
    /// actual candidate key and the exact bytes that will be sent.
    pub(super) fn offer_readmission(&self, protector: &dyn crate::fleet_credential::Protector, now: i64)
        -> Result<String, &'static str> {
        let mut snapshot = self.credential_snapshot(protector)?.ok_or(INVALID)?;
        if !snapshot.is_recovered { return Err(INVALID); }
        let guard = self.state.lock().map_err(|_| INVALID)?;
        let record = guard.record.as_ref().ok_or(INVALID)?;
        if !Self::credential_matches(record, &snapshot) { return Err(INVALID); }
        let commit = record.recovery_commit.as_ref().ok_or(INVALID)?;
        let candidate = crate::fleet_recovery_candidate::CandidateStore::open(
            self.fs.as_ref(), self.path.parent().ok_or(INVALID)?.to_path_buf(), protector)?;
        let public = candidate.public_key(&commit.old_trust, &commit.recovery_id)?;
        let fingerprint = Sha256::digest(public).iter().map(|b| format!("{b:02x}")).collect::<String>();
        if fingerprint != commit.candidate_fingerprint { return Err(INVALID); }
        let request = candidate.sign_fixture_readmission(&commit.old_trust, &record.trust, &commit.recovery_id, now)?;
        let challenge = request.get("shield_progress_challenge").and_then(|v| v.as_str()).ok_or(INVALID)?.to_owned();
        let bytes = serde_json::to_vec(&request).map_err(|_| INVALID)?;
        let mut random = [0u8; 32];
        SysRng.try_fill_bytes(&mut random).map_err(|_| INVALID)?;
        let id = STANDARD.encode(random);
        snapshot.secret.zeroize();
        let offer = Offer { id: id.clone(), snapshot, challenge, request: bytes, expires: Instant::now() + TTL };
        *self.recovery_readmission.lock().map_err(|_| INVALID)? = Registry { pending: Some(offer), claimed: None };
        Ok(id)
    }

    /// Consumes before sending. A failed attempt cannot reuse the offer.
    pub(super) fn claim_readmission(&self, id: &str) -> Result<Claim, &'static str> {
        let guard = self.state.lock().map_err(|_| INVALID)?;
        let mut offers = self.recovery_readmission.lock().map_err(|_| INVALID)?;
        if !offers.pending.as_ref().is_some_and(|offer| offer.id == id) { return Err(INVALID); }
        let offer = offers.pending.take().ok_or(INVALID)?;
        let record = guard.record.as_ref().ok_or(INVALID)?;
        if Instant::now() >= offer.expires || !Self::credential_matches(record, &offer.snapshot) { return Err(INVALID); }
        offers.claimed = Some(offer.id.clone());
        Ok(Claim(offer))
    }

    /// Build only the protected origin/profile transport. This check does not
    /// cancel an HTTP request already in flight after a concurrent revocation.
    pub(super) fn readmission_transport(&self, claim: &Claim) -> Result<(String, reqwest::Client), &'static str> {
        let offer=&claim.0;
        let guard=self.state.lock().map_err(|_| INVALID)?;
        let registry=self.recovery_readmission.lock().map_err(|_| INVALID)?;
        let record=guard.record.as_ref().ok_or(INVALID)?;
        if registry.claimed.as_deref()!=Some(&offer.id) || Instant::now()>=offer.expires
            || !Self::credential_matches(record,&offer.snapshot)
            || crate::fleet_runtime_env::fingerprint().as_deref()!=Some(offer.snapshot.profile.environment_fingerprint.as_str()) {
            return Err(INVALID);
        }
        let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&offer.snapshot.profile.spki_pins);
        if !offer.snapshot.profile.spki_pins.trim().is_empty() && pins.is_none() { return Err(INVALID); }
        let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_| INVALID)?;
        Ok((offer.snapshot.origin.clone(),client))
    }

    /// Verify exact transport bytes, then hold the policy lock through the
    /// final snapshot check and publication. The test-only publisher must be
    /// bounded and non-reentrant because both policy and registry locks are held.
    /// No secret or proof is persisted.
    pub(super) fn finish_readmission<T>(&self, claim: Claim, status: u16, body: &[u8], header: &str,
        publish: impl FnOnce() -> T) -> Result<T, &'static str> {
        let offer = claim.0;
        if status != 200 || body.len() > 65536 || Instant::now() >= offer.expires { return Err(INVALID); }
        let signature = header.strip_prefix("v2.").ok_or(INVALID)?;
        let signature_bytes = STANDARD.decode(signature).map_err(|_| INVALID)?;
        if signature_bytes.len() != 64 || STANDARD.encode(&signature_bytes) != signature { return Err(INVALID); }
        let proof = fleet_proto::canonical_json_bytes(&serde_json::json!([
            "fleet.windows.enrollment.v2", offer.challenge,
            STANDARD.encode(Sha256::digest(&offer.request)), status, STANDARD.encode(Sha256::digest(body))
        ]));
        if !fleet_proto::verify_signature_b64(&offer.snapshot.trust.signer_key, &proof, signature) { return Err(INVALID); }
        let reply: Reply = fleet_proto::windows_v2::decode_windows_enrollment_safe(body).map_err(|_| INVALID)?;
        let mut secret = zeroize::Zeroizing::new(STANDARD.decode(&reply.checkin_secret_b64).map_err(|_| INVALID)?);
        if secret.len() != 32 || STANDARD.encode(&*secret) != reply.checkin_secret_b64 { return Err(INVALID); }
        let digest = Sha256::digest(&*secret).iter().map(|b| format!("{b:02x}")).collect::<String>();
        secret.zeroize();
        let commit = offer.snapshot.recovery_commit.as_ref().ok_or(INVALID)?;
        let key = STANDARD.decode(&reply.outcome_pubkey_b64).map_err(|_| INVALID)?;
        if key.len() != 32 || STANDARD.encode(&key) != reply.outcome_pubkey_b64
            || Sha256::digest(&key).iter().map(|b| format!("{b:02x}")).collect::<String>() != commit.candidate_fingerprint
            || reply.org_id != offer.snapshot.trust.org_id || reply.device_id != offer.snapshot.trust.device_id
            || reply.shield_progress_generation != offer.snapshot.trust.enrollment_generation
            || reply.server_signing_key_b64 != offer.snapshot.trust.signer_key || digest != commit.credential_sha256 { return Err(INVALID); }
        let generation_signature=STANDARD.decode(&reply.shield_progress_generation_signature).map_err(|_| INVALID)?;
        if generation_signature.len()!=64 || STANDARD.encode(&generation_signature)!=reply.shield_progress_generation_signature { return Err(INVALID); }
        let generation_proof=serde_json::to_vec(&["wincommander:shield-progress:enrollment:v1", &reply.org_id,
            &reply.device_id, &reply.shield_progress_generation, &offer.challenge]).map_err(|_| INVALID)?;
        if !fleet_proto::verify_signature_b64(&offer.snapshot.trust.signer_key,&generation_proof,&reply.shield_progress_generation_signature) { return Err(INVALID); }
        let guard = self.state.lock().map_err(|_| INVALID)?;
        let record = guard.record.as_ref().ok_or(INVALID)?;
        let mut registry = self.recovery_readmission.lock().map_err(|_| INVALID)?;
        if registry.claimed.as_deref() != Some(&offer.id) { return Err(INVALID); }
        registry.claimed = None;
        if Instant::now() >= offer.expires
            || !Self::credential_matches(record, &offer.snapshot) { return Err(INVALID); }
        Ok(publish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{contact_test_support::{key, trust}, credential_integration_tests::{submit, TestProtector}};
    use crate::policy_store::{PolicyFs, PolicyStoreError};
    use ed25519_dalek::Signer;
    use std::{collections::HashMap, io, path::{Path, PathBuf}, sync::{Arc, Mutex}};

    #[derive(Clone, Default)]
    struct Fs(Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>);
    impl PolicyFs for Fs {
        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            self.0.lock().unwrap().get(path).cloned().ok_or(io::ErrorKind::NotFound.into())
        }
        fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
            self.0.lock().unwrap().insert(path.to_owned(), data.to_vec()); Ok(())
        }
        fn ensure_dir_secure(&self, _: &Path) -> Result<(), PolicyStoreError> { Ok(()) }
    }
    fn setup() -> FleetMachinePolicy {
        let fs = Fs::default();
        let store = FleetMachinePolicy::open(Box::new(fs), "readmission".into()).unwrap();
        let binding = trust();
        let profile = TransportProfile { spki_pins: String::new(), environment_fingerprint: STANDARD.encode([9; 32]) };
        let challenge = store.begin_enrollment(binding.device_id.clone(), binding.signer_key.clone(),
            vec!["https://fleet.test".into()], Some(profile)).unwrap();
        let bytes = serde_json::to_vec(&["wincommander:shield-progress:enrollment:v1", &binding.org_id,
            &binding.device_id, &binding.enrollment_generation, &challenge]).unwrap();
        store.provision(binding.clone(), &STANDARD.encode(key().sign(&bytes).to_bytes()), "S-1-5-18").unwrap();
        let retirement = "11111111-1111-4111-8111-111111111111";
        let signed = serde_json::to_vec(&("fleet.windows.admin-retirement.v2", &binding.org_id, &binding.device_id,
            &binding.enrollment_generation, retirement, "loopback", 1000i64, &binding.signer_key)).unwrap();
        let receipt = serde_json::json!({"org_id":binding.org_id,"old_device_id":binding.device_id,
            "old_generation":binding.enrollment_generation,"retirement_id":retirement,"issued_at":1000,
            "version":2,"device_hash":"loopback","signer_key":binding.signer_key,"signature":STANDARD.encode(key().sign(&signed).to_bytes())});
        let candidates = crate::fleet_recovery_candidate::CandidateStore::open(store.fs.as_ref(), "readmission".into(), &TestProtector).unwrap();
        let public = candidates.stage(&binding, &serde_json::to_vec(&receipt).unwrap(), 1001).unwrap();
        submit(&store, "https://fleet.test", &[4;32]).unwrap();
        let mut state = store.state.lock().unwrap();
        let record = state.record.as_mut().unwrap();
        record.transport_profile.as_mut().unwrap().environment_fingerprint=crate::fleet_runtime_env::fingerprint().unwrap();
        record.recovery_commit = Some(RecoveryCommit { recovery_id: retirement.into(), old_trust: binding.clone(),
            new_generation: binding.enrollment_generation.clone(), decision_digest: "11".repeat(32), archive_sha256:"22".repeat(32),
            credential_sha256:Sha256::digest([4;32]).iter().map(|b| format!("{b:02x}")).collect(),
            candidate_fingerprint:Sha256::digest(STANDARD.decode(public).unwrap()).iter().map(|b| format!("{b:02x}")).collect(),
            origin:"https://fleet.test".into(), local_commit_digest:"44".repeat(32), activation_pending:false, server_status:None, rollback_sha256:None });
        drop(state); store
    }
    fn response(request: &[u8], secret: &[u8], invalid_generation_signature: bool) -> (Vec<u8>, String) {
        let request_value: serde_json::Value = serde_json::from_slice(request).unwrap();
        let binding = trust();
        let challenge=request_value["shield_progress_challenge"].as_str().unwrap();
        let signed=serde_json::to_vec(&["wincommander:shield-progress:enrollment:v1",&binding.org_id,
            &binding.device_id,&binding.enrollment_generation,challenge]).unwrap();
        let generation_signature=if invalid_generation_signature { STANDARD.encode([0;64]) } else { STANDARD.encode(key().sign(&signed).to_bytes()) };
        let body = serde_json::to_vec(&serde_json::json!({"org_id":binding.org_id,"device_id":binding.device_id,
            "shield_progress_generation":binding.enrollment_generation,"server_signing_key_b64":binding.signer_key,
            "shield_progress_generation_signature":generation_signature,
            "outcome_pubkey_b64":request_value["outcome_pubkey_b64"],"checkin_secret_b64":STANDARD.encode(secret)})).unwrap();
        let proof = fleet_proto::canonical_json_bytes(&serde_json::json!(["fleet.windows.enrollment.v2",
            request_value["shield_progress_challenge"],STANDARD.encode(Sha256::digest(request)),200,STANDARD.encode(Sha256::digest(&body))]));
        let header = format!("v2.{}",STANDARD.encode(key().sign(&proof).to_bytes())); (body,header)
    }
    #[test]
    fn signed_readmission_is_one_use_and_fenced() {
        let store = setup();
        let id = store.offer_readmission(&TestProtector, 1001).unwrap();
        let claim = store.claim_readmission(&id).unwrap();
        assert!(store.claim_readmission(&id).is_err());
        let (origin, _) = store.readmission_transport(&claim).unwrap();
        assert_eq!(origin,"https://fleet.test");
        let (body, header) = response(claim.request(), &[4;32], false);
        assert_eq!(store.finish_readmission(claim, 200, &body, &header, || 7).unwrap(), 7);
        for scenario in 0..5 {
            let id = store.offer_readmission(&TestProtector, 1001).unwrap();
            let claim = store.claim_readmission(&id).unwrap();
            let (mut body, header) = response(claim.request(), if scenario == 0 { &[5;32] } else { &[4;32] }, scenario == 4);
            if scenario == 1 { body.push(b' '); }
            if scenario == 2 { store.state.lock().unwrap().record.as_mut().unwrap().connection_revision += 1; }
            if scenario == 3 { store.offer_readmission(&TestProtector, 1001).unwrap(); }
            if scenario == 2 || scenario == 3 { assert!(store.readmission_transport(&claim).is_err()); }
            assert!(store.finish_readmission(claim, 200, &body, &header, || ()).is_err());
        }
    }
}
