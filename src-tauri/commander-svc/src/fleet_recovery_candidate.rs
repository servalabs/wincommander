// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disabled service-owned candidate identity primitive. No IPC route invokes it.
#![allow(dead_code)]
use crate::{fleet_credential::Protector, fleet_machine_policy::FleetTrust, policy_store::PolicyFs};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signer, SigningKey, Signature, VerifyingKey};
use rand::{rngs::SysRng, TryRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Mutex};
use zeroize::Zeroizing;

const INVALID: &str = "invalid recovery candidate";
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    org_id: String, old_device_id: String, old_generation: String, retirement_id: String,
    issued_at: i64, signer_key: String, signature: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    trust: FleetTrust, retirement_id: String, public_key: String, sealed_seed: String,
}
fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).ok().is_some_and(|v| !v.is_nil() && v.to_string() == value)
}
fn decode<const N: usize>(s: &str) -> Result<[u8; N], &'static str> {
    let b = B64.decode(s).map_err(|_| INVALID)?;
    if B64.encode(&b) != s { return Err(INVALID); }
    b.try_into().map_err(|_| INVALID)
}
impl Receipt {
    fn verify(&self, trust: &FleetTrust, now: i64) -> Result<(), &'static str> {
        if self.org_id != trust.org_id || self.org_id.is_empty() || self.org_id.len() > 256
            || self.old_device_id != trust.device_id || self.old_generation != trust.enrollment_generation
            || self.signer_key != trust.signer_key || !uuid(&self.old_device_id)
            || !uuid(&self.old_generation) || !uuid(&self.retirement_id)
            || !crate::fleet_identity_handoff::recovery_timestamp_valid(self.issued_at, now) { return Err(INVALID); }
        let key = VerifyingKey::from_bytes(&decode::<32>(&trust.signer_key)?).map_err(|_| INVALID)?;
        if key.is_weak() { return Err(INVALID); }
        let bytes = serde_json::to_vec(&("fleet.windows.admin-retirement.v1", &self.org_id,
            &self.old_device_id, &self.old_generation, &self.retirement_id, self.issued_at,
            &self.signer_key)).map_err(|_| INVALID)?;
        key.verify_strict(&bytes, &Signature::from_bytes(&decode::<64>(&self.signature)?)).map_err(|_| INVALID)
    }
}
fn entropy(trust: &FleetTrust, retirement: &str) -> Result<[u8;32], &'static str> {
    let bytes = serde_json::to_vec(&("wincommander:fleet:recovery-candidate:v1", &trust.org_id,
        &trust.device_id, &trust.enrollment_generation, &trust.signer_key, retirement)).map_err(|_| INVALID)?;
    Ok(Sha256::digest(bytes).into())
}
/// A future caller must hold the protected trust lock throughout each operation.
pub(crate) struct CandidateStore<'a> { fs: &'a dyn PolicyFs, path: PathBuf, protector: &'a dyn Protector }
impl<'a> CandidateStore<'a> {
    /// Signs only a cancellation for the locally frozen decision after the
    /// caller has checked protected state and local administrator approval.
    pub(crate) fn sign_cancel(&self,old:&FleetTrust,current:&FleetTrust,previous:&str,next:&str,generation:&str,
        decision:&str,now:i64,nonce:&str)->Result<String,&'static str> {
        if !uuid(previous) || !uuid(next) || !uuid(generation) || !uuid(nonce) || decision.len()!=64
            || !decision.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || old.org_id!=current.org_id || old.device_id!=current.device_id || old.signer_key!=current.signer_key { return Err(INVALID); }
        let _guard=LOCK.lock().map_err(|_| INVALID)?;
        let candidate=self.read_for(old,previous)?;let key=self.signing_key(&candidate,old,previous)?;
        let bytes=serde_json::to_vec(&("wincommander:fleet:recovery-cancel:v1","POST","/v1/agents/identity-handoff/cancel",
            &current.org_id,&current.device_id,&current.enrollment_generation,previous,next,generation,decision,&current.signer_key,now,nonce)).map_err(|_| INVALID)?;
        Ok(B64.encode(key.sign(&bytes).to_bytes()))
    }
    /// Caller holds protected trust and the journal lease and has checked the
    /// next signed decision and the fully drained snapshot.
    pub(crate) fn sign_drain(&self,old:&FleetTrust,current:&FleetTrust,previous:&str,next:&crate::fleet_identity_handoff::VerifiedHandoff,
        snapshot:&str,now:i64,nonce:&str)->Result<String,&'static str> {
        if !uuid(previous) || !uuid(next.retirement_id()) || !uuid(next.new_generation()) || !uuid(nonce)
            || snapshot.len()!=64 || !snapshot.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || old.org_id!=current.org_id || old.device_id!=current.device_id || old.signer_key!=current.signer_key { return Err(INVALID); }
        let _guard=LOCK.lock().map_err(|_| INVALID)?;
        let candidate=self.read_for(old,previous)?;let key=self.signing_key(&candidate,old,previous)?;
        let fingerprint=next.fingerprint().iter().map(|b|format!("{b:02x}")).collect::<String>();
        let decision=next.decision_digest().iter().map(|b|format!("{b:02x}")).collect::<String>();
        let bytes=serde_json::to_vec(&("wincommander:fleet:recovery-drain:v1",&current.org_id,&current.device_id,
            &current.enrollment_generation,previous,next.retirement_id(),next.new_generation(),fingerprint,decision,
            next.next_sequence(),next.previous_hash(),snapshot,&current.signer_key,now,nonce)).map_err(|_| INVALID)?;
        Ok(B64.encode(key.sign(&bytes).to_bytes()))
    }
    pub(crate) fn sign_late_evidence(&self,trust:&FleetTrust,retirement:&str,fingerprint:&[u8;32],
        command:&str,evidence_id:&str,unknown_receipt:&str,unknown_hash:&str,result_hash:&str)->Result<String,&'static str> {
        let valid_hash=|value:&str|value.len()==64 && value.bytes().all(|byte|byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !uuid(retirement) || command.is_empty() || command.len()>256 || !uuid(evidence_id) || !uuid(unknown_receipt)
            || !valid_hash(unknown_hash) || !valid_hash(result_hash) { return Err(INVALID); }
        let _guard=LOCK.lock().map_err(|_| INVALID)?;
        let candidate=self.read_for(trust,retirement)?;
        let key=self.signing_key(&candidate,trust,retirement)?;
        if Sha256::digest(key.verifying_key().as_bytes()).as_slice()!=fingerprint { return Err(INVALID); }
        let bytes=serde_json::to_vec(&("wincommander:fleet:late-execution-evidence:v1",&trust.org_id,&trust.device_id,
            retirement,command,evidence_id,unknown_receipt,unknown_hash,result_hash)).map_err(|_| INVALID)?;
        Ok(B64.encode(key.sign(&bytes).to_bytes()))
    }
    pub(crate) fn public_key(&self,trust:&FleetTrust,retirement:&str)->Result<[u8;32],&'static str> {
        let _guard=LOCK.lock().map_err(|_| INVALID)?;
        let candidate=self.read_for(trust,retirement)?;
        Ok(*self.signing_key(&candidate,trust,retirement)?.verifying_key().as_bytes())
    }
    pub(crate) fn open(fs: &'a dyn PolicyFs, dir: PathBuf, protector: &'a dyn Protector) -> Result<Self, &'static str> {
        fs.ensure_dir_secure(&dir).map_err(|_| INVALID)?;
        Ok(Self { fs, path: dir.join("recovery-candidate.json"), protector })
    }
    fn read(&self) -> Result<Option<Candidate>, &'static str> {
        match self.fs.read(&self.path) {
            Ok(bytes) if bytes.len() <= 65536 => serde_json::from_slice(&bytes).map(Some).map_err(|_| INVALID),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            _ => Err(INVALID),
        }
    }
    fn archive_path(&self, retirement:&str)->Result<PathBuf,&'static str> {
        if !uuid(retirement) { return Err(INVALID); }
        Ok(self.path.parent().ok_or(INVALID)?.join(format!("recovery-candidate-{retirement}.json")))
    }
    fn read_for(&self,trust:&FleetTrust,retirement:&str)->Result<Candidate,&'static str> {
        if let Some(candidate)=self.read()? {
            if candidate.trust==*trust && candidate.retirement_id==retirement { return Ok(candidate); }
        }
        let bytes=self.fs.read(&self.archive_path(retirement)?).map_err(|_| INVALID)?;
        if bytes.len()>65536 { return Err(INVALID); }
        let candidate:Candidate=serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
        if candidate.trust!=*trust || candidate.retirement_id!=retirement { return Err(INVALID); }
        Ok(candidate)
    }
    /// Caller must hold protected trust and independently verify the previous
    /// active commit. This preserves the exact sealed key before any replacement.
    pub(crate) fn archive_activated(&self,old:&FleetTrust,retirement:&str,fingerprint:&[u8;32])->Result<(),&'static str> {
        let _guard=LOCK.lock().map_err(|_| INVALID)?;
        let bytes=self.fs.read(&self.path).map_err(|_| INVALID)?;
        if bytes.len()>65536 { return Err(INVALID); }
        let candidate:Candidate=serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
        let key=self.signing_key(&candidate,old,retirement)?;
        if Sha256::digest(key.verifying_key().as_bytes()).as_slice()!=fingerprint { return Err(INVALID); }
        let path=self.archive_path(retirement)?;
        match self.fs.read(&path) {
            Ok(existing) if existing==bytes=>Ok(()),
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
                self.fs.atomic_write(&path,&bytes).map_err(|_| INVALID)?;
                if self.fs.read(&path).map_err(|_| INVALID)?!=bytes { return Err(INVALID); }
                Ok(())
            },
            _=>Err(INVALID),
        }
    }
    fn signing_key(&self, candidate: &Candidate, trust: &FleetTrust, retirement: &str) -> Result<SigningKey, &'static str> {
        if candidate.trust != *trust || candidate.retirement_id != retirement { return Err(INVALID); }
        let sealed = B64.decode(&candidate.sealed_seed).map_err(|_| INVALID)?;
        if sealed.is_empty() || sealed.len() > 32768 || B64.encode(&sealed) != candidate.sealed_seed { return Err(INVALID); }
        let secret = self.protector.unprotect(&sealed, &entropy(trust, retirement)?)?;
        let seed: Zeroizing<[u8;32]> = Zeroizing::new(secret.as_slice().try_into().map_err(|_| INVALID)?);
        let key = SigningKey::from_bytes(&seed);
        if B64.encode(key.verifying_key().as_bytes()) != candidate.public_key { return Err(INVALID); }
        Ok(key)
    }
    pub(crate) fn stage(&self, trust: &FleetTrust, receipt_json: &[u8], now: i64) -> Result<String, &'static str> {
        if receipt_json.len() > 8192 { return Err(INVALID); }
        let receipt: Receipt = serde_json::from_slice(receipt_json).map_err(|_| INVALID)?;
        receipt.verify(trust, now)?;
        let _guard = LOCK.lock().map_err(|_| INVALID)?;
        let primary=self.read()?;
        if let Some(candidate)=primary.as_ref() {
            if candidate.trust==*trust && candidate.retirement_id==receipt.retirement_id {
                self.signing_key(candidate,trust,&receipt.retirement_id)?; return Ok(candidate.public_key.clone());
            }
        }
        let path=if primary.is_some() { self.archive_path(&receipt.retirement_id)? } else { self.path.clone() };
        match self.fs.read(&path) {
            Ok(bytes) if bytes.len()<=65536=>{
                let candidate:Candidate=serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
                self.signing_key(&candidate,trust,&receipt.retirement_id)?; return Ok(candidate.public_key);
            },
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
            _=>return Err(INVALID),
        }
        let mut seed = Zeroizing::new([0u8;32]);
        SysRng.try_fill_bytes(seed.as_mut()).map_err(|_| INVALID)?;
        let key = SigningKey::from_bytes(&seed);
        let sealed = self.protector.protect(seed.as_ref(), &entropy(trust, &receipt.retirement_id)?)?;
        if sealed.is_empty() || sealed.len() > 32768 { return Err(INVALID); }
        let candidate = Candidate { trust: trust.clone(), retirement_id: receipt.retirement_id,
            public_key: B64.encode(key.verifying_key().as_bytes()), sealed_seed: B64.encode(sealed) };
        let encoded = serde_json::to_vec(&candidate).map_err(|_| INVALID)?;
        self.fs.atomic_write(&path, &encoded).map_err(|_| INVALID)?;
        if self.fs.read(&path).map_err(|_| INVALID)?!=encoded { return Err(INVALID); }
        Ok(candidate.public_key)
    }
    pub(crate) fn sign_handoff_request(&self, trust: &FleetTrust, retirement: &str, now: i64) -> Result<serde_json::Value, &'static str> {
        let _guard = LOCK.lock().map_err(|_| INVALID)?;
        let candidate = self.read_for(trust,retirement)?;
        let key = self.signing_key(&candidate, trust, retirement)?;
        let mut nonce = [0u8; 16]; SysRng.try_fill_bytes(&mut nonce).map_err(|_| INVALID)?;
        nonce[6] = (nonce[6] & 15) | 64; nonce[8] = (nonce[8] & 63) | 128;
        let nonce = uuid::Uuid::from_bytes(nonce).to_string();
        let preimage = serde_json::to_vec(&("wincommander:fleet:identity-handoff-request:v1", "POST",
            "/v1/agents/identity-handoff", &trust.org_id, &trust.device_id, retirement,
            &trust.signer_key, now, &nonce)).map_err(|_| INVALID)?;
        Ok(serde_json::json!({"org_id":trust.org_id, "device_id":trust.device_id,
            "recovery_id":retirement, "server_key":trust.signer_key, "issued_at":now,
            "nonce":nonce, "signature":B64.encode(key.sign(&preimage).to_bytes())}))
    }
    /// Signs only a closed, recovery-bound confirmation after service review.
    pub(crate) fn sign_confirmation(&self, trust:&FleetTrust, retirement:&str, new_generation:&str,
        decision_digest:&str, archive_digest:&str, fingerprint:&[u8;32], now:i64) -> Result<serde_json::Value,&'static str> {
        let valid_hash=|s:&str| s.len()==64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !uuid(new_generation) || !valid_hash(decision_digest) || !valid_hash(archive_digest) { return Err(INVALID); }
        let _guard=LOCK.lock().map_err(|_| INVALID)?;
        let candidate=self.read_for(trust,retirement)?;
        let key=self.signing_key(&candidate,trust,retirement)?;
        if Sha256::digest(key.verifying_key().as_bytes()).as_slice()!=fingerprint { return Err(INVALID); }
        let mut nonce=[0u8;16]; SysRng.try_fill_bytes(&mut nonce).map_err(|_| INVALID)?;
        nonce[6]=(nonce[6]&15)|64; nonce[8]=(nonce[8]&63)|128;
        let nonce=uuid::Uuid::from_bytes(nonce).to_string();
        let bytes=serde_json::to_vec(&("wincommander:fleet:identity-handoff-confirmation:v1","POST",
            "/v1/agents/identity-handoff/confirm", &trust.org_id,&trust.device_id,retirement,
            new_generation,decision_digest,archive_digest,&trust.signer_key,now,&nonce)).map_err(|_| INVALID)?;
        Ok(serde_json::json!({"org_id":trust.org_id,"device_id":trust.device_id,"recovery_id":retirement,
            "new_generation":new_generation,"decision_digest":decision_digest,"archive_sha256":archive_digest,
            "server_key":trust.signer_key,"issued_at":now,"nonce":nonce,"signature":B64.encode(key.sign(&bytes).to_bytes())}))
    }
    /// A commit request can be signed only from a protected prepared record.
    #[cfg(windows)]
    pub(crate) fn sign_commit(&self, expected:&crate::fleet_machine_policy::RecoveryExpected, now:i64) -> Result<serde_json::Value,&'static str> {
        let trust=expected.old_trust();
        let _guard=LOCK.lock().map_err(|_| INVALID)?;
        let candidate=self.read_for(trust,expected.recovery_id())?;
        let key=self.signing_key(&candidate,trust,expected.recovery_id())?;
        let fingerprint=Sha256::digest(key.verifying_key().as_bytes()).iter().map(|b|format!("{b:02x}")).collect::<String>();
        if fingerprint!=expected.candidate_fingerprint() { return Err(INVALID); }
        let mut nonce=[0u8;16]; SysRng.try_fill_bytes(&mut nonce).map_err(|_| INVALID)?;
        nonce[6]=(nonce[6]&15)|64; nonce[8]=(nonce[8]&63)|128;
        let nonce=uuid::Uuid::from_bytes(nonce).to_string();
        let bytes=serde_json::to_vec(&("wincommander:fleet:identity-handoff-commit:v1","POST",
            "/v1/agents/identity-handoff/commit",&trust.org_id,&trust.device_id,expected.recovery_id(),
            expected.new_generation(),expected.decision_digest(),expected.archive_sha256(),expected.local_commit_digest(),
            expected.credential_sha256(),&trust.signer_key,now,&nonce)).map_err(|_| INVALID)?;
        Ok(serde_json::json!({"org_id":trust.org_id,"device_id":trust.device_id,"recovery_id":expected.recovery_id(),
            "new_generation":expected.new_generation(),"decision_digest":expected.decision_digest(),"archive_sha256":expected.archive_sha256(),
            "local_commit_digest":expected.local_commit_digest(),"credential_sha256":expected.credential_sha256(),
            "server_key":trust.signer_key,"issued_at":now,"nonce":nonce,"signature":B64.encode(key.sign(&bytes).to_bytes())}))
    }
    /// Internal service-only signing; no IPC accepts raw hashes or signing keys.
    pub(crate) fn sign_unenroll_status(&self,old:&FleetTrust,retirement:&str,current:&FleetTrust,fingerprint:&[u8;32],nonce:&str)->Result<serde_json::Value,&'static str> {
        let _guard=LOCK.lock().map_err(|_|INVALID)?;
        if old.org_id!=current.org_id || old.device_id!=current.device_id || old.signer_key!=current.signer_key
            || nonce.len()!=64 || !nonce.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)) {return Err(INVALID)}
        let candidate=self.read_for(old,retirement)?;let key=self.signing_key(&candidate,old,retirement)?;
        if Sha256::digest(key.verifying_key().as_bytes()).as_slice()!=fingerprint {return Err(INVALID)}
        let bytes=serde_json::to_vec(&serde_json::json!(["fleet.windows.unenroll-status.request.v1","POST","/v1/windows/unenroll-status",current.org_id,current.device_id,current.enrollment_generation,nonce])).map_err(|_|INVALID)?;
        Ok(serde_json::json!({"protocol_version":1,"org_id":current.org_id,"device_id":current.device_id,"enrollment_generation":current.enrollment_generation,"nonce":nonce,"signature_b64":B64.encode(key.sign(&bytes).to_bytes())}))
    }
    /// Internal service-only signing; no IPC accepts raw hashes or signing keys.
    pub(crate) fn sign_outcome(&self, trust: &FleetTrust, retirement: &str,
        fingerprint: &[u8;32], outcome: &mut fleet_proto::ActionOutcome) -> Result<(), &'static str> {
        let _guard = LOCK.lock().map_err(|_| INVALID)?;
        let candidate=self.read_for(trust,retirement)?;
        let key=self.signing_key(&candidate,trust,retirement)?;
        if Sha256::digest(key.verifying_key().as_bytes()).as_slice()!=fingerprint
            || outcome.device_id.0!=trust.device_id || outcome.version!=1
            || !outcome.signature.is_empty() || !outcome.record_hash.is_empty() { return Err(INVALID); }
        outcome.record_hash=fleet_proto::action_outcome_hash(outcome);
        let hash=Sha256::digest(fleet_proto::canonical_action_outcome_bytes(outcome));
        outcome.signature=B64.encode(key.sign(&hash).to_bytes());
        Ok(())
    }
    /// Signs only an enrollment bound to the already verified retirement and pinned server.
    pub(crate) fn sign_enrollment(&self, trust: &FleetTrust, retirement: &str,
        now: i64) -> Result<serde_json::Value, &'static str> {
        let _guard = LOCK.lock().map_err(|_| INVALID)?;
        let candidate = self.read_for(trust,retirement)?;
        let key = self.signing_key(&candidate, trust, retirement)?;
        let mut body = serde_json::json!({"device_id":trust.device_id,"device_hash":"recovery-candidate",
            "agent_version":env!("CARGO_PKG_VERSION"),"device_kind":"wincommander",
            "capabilities":[fleet_agent_core::HMAC_BODY_V2_CAPABILITY],
            "outcome_pubkey_b64":candidate.public_key,"enrollment_intent":"explicit_connect"});
        let obj = body.as_object_mut().ok_or(INVALID)?;
        obj.insert("enrollment_proof_version".into(), 1.into());
        obj.insert("enrollment_server_key_b64".into(), trust.signer_key.clone().into());
        obj.insert("enrollment_ts".into(), now.into());
        let mut nonce = [0u8;16]; SysRng.try_fill_bytes(&mut nonce).map_err(|_| INVALID)?;
        nonce[6] = (nonce[6] & 15) | 64; nonce[8] = (nonce[8] & 63) | 128;
        obj.insert("enrollment_nonce".into(), uuid::Uuid::from_bytes(nonce).to_string().into());
        let mut bytes = b"WinCommander Fleet enrollment proof v1\nPOST\n/v1/agents/enroll\n".to_vec();
        bytes.extend(serde_json::to_vec(&body).map_err(|_| INVALID)?);
        body["enrollment_signature_b64"] = B64.encode(key.sign(&bytes).to_bytes()).into();
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, path::Path, sync::Mutex};
    #[derive(Default)]
    struct Fs(Mutex<HashMap<PathBuf, Vec<u8>>>);
    impl PolicyFs for Fs {
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            self.0.lock().unwrap().get(path).cloned().ok_or(std::io::ErrorKind::NotFound.into())
        }
        fn atomic_write(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
            self.0.lock().unwrap().insert(path.to_owned(), data.to_vec()); Ok(())
        }
        fn ensure_dir_secure(&self, _: &Path) -> Result<(), crate::policy_store::PolicyStoreError> { Ok(()) }
    }
    struct TestProtector;
    impl Protector for TestProtector {
        fn protect(&self, plain: &[u8], entropy: &[u8]) -> Result<Vec<u8>, &'static str> {
            let mut out = entropy.to_vec(); out.extend(plain.iter().zip(entropy).map(|(a,b)| a ^ b)); Ok(out)
        }
        fn unprotect(&self, data: &[u8], entropy: &[u8]) -> Result<Zeroizing<Vec<u8>>, &'static str> {
            if data.len() != 64 || data[..32] != *entropy { return Err(INVALID); }
            Ok(Zeroizing::new(data[32..].iter().zip(entropy).map(|(a,b)| a ^ b).collect()))
        }
    }
    fn fixture() -> (FleetTrust, Vec<u8>) {
        let trust = FleetTrust { org_id:"org-a".into(), device_id:"0480b2ca-f4c3-4599-81fc-4876f73e70fa".into(),
            enrollment_generation:"22222222-2222-4222-8222-222222222222".into(),
            signer_key:"Zr5+Myx6RTMyvZ0Kf32wVfXF7xoGraZtmLOftoEMRzo=".into() };
        let r = serde_json::json!({"org_id":trust.org_id,"old_device_id":trust.device_id,
            "old_generation":trust.enrollment_generation,"retirement_id":"11111111-1111-4111-8111-111111111111",
            "issued_at":1000,"signer_key":trust.signer_key,
            "signature":"7YCy0JLIrYZGFnCrA/rAHrB+L1I97TwBUdjtpMA/9rnErwQKWRxXxHcUQjcrZE+HSjbBS2H6Ws0dzWc27jWmBg=="});
        (trust, serde_json::to_vec(&r).unwrap())
    }
    #[test]
    fn candidate_survives_retry_and_signs_bound_enrollment() {
        let fs=Fs::default(); let protector=TestProtector; let (trust, receipt)=fixture();
        let store=CandidateStore::open(&fs, "protected".into(), &protector).unwrap();
        let key=store.stage(&trust, &receipt, 1001).unwrap();
        assert_eq!(store.stage(&trust, &receipt, 1002).unwrap(), key);
        let proof=store.sign_handoff_request(&trust,"11111111-1111-4111-8111-111111111111",1002).unwrap();
        let bytes=serde_json::to_vec(&("wincommander:fleet:identity-handoff-request:v1", "POST",
            "/v1/agents/identity-handoff", &trust.org_id, &trust.device_id,
            "11111111-1111-4111-8111-111111111111", &trust.signer_key, 1002i64,
            proof["nonce"].as_str().unwrap())).unwrap();
        let public=VerifyingKey::from_bytes(&decode::<32>(&key).unwrap()).unwrap();
        assert!(public.verify_strict(&bytes, &Signature::from_bytes(&decode::<64>(proof["signature"].as_str().unwrap()).unwrap())).is_ok());
        assert!(store.sign_handoff_request(&trust,"33333333-3333-4333-8333-333333333333",1002).is_err());
        let fingerprint:[u8;32]=Sha256::digest(public.as_bytes()).into();
        let confirmation=store.sign_confirmation(&trust,"11111111-1111-4111-8111-111111111111",
            "33333333-3333-4333-8333-333333333333",&"ab".repeat(32),&"cd".repeat(32),&fingerprint,1002).unwrap();
        let bytes=serde_json::to_vec(&("wincommander:fleet:identity-handoff-confirmation:v1","POST",
            "/v1/agents/identity-handoff/confirm",&trust.org_id,&trust.device_id,"11111111-1111-4111-8111-111111111111",
            "33333333-3333-4333-8333-333333333333","ab".repeat(32),"cd".repeat(32),&trust.signer_key,1002i64,
            confirmation["nonce"].as_str().unwrap())).unwrap();
        assert!(public.verify_strict(&bytes,&Signature::from_bytes(&decode::<64>(confirmation["signature"].as_str().unwrap()).unwrap())).is_ok());
        let signed=store.sign_enrollment(&trust,"11111111-1111-4111-8111-111111111111",1002).unwrap();
        assert_eq!(signed["device_hash"],"recovery-candidate");
        assert_eq!(signed["outcome_pubkey_b64"],key);
        let mut unsigned=signed.clone(); let signature=unsigned.as_object_mut().unwrap().remove("enrollment_signature_b64").unwrap();
        let mut bytes=b"WinCommander Fleet enrollment proof v1\nPOST\n/v1/agents/enroll\n".to_vec(); bytes.extend(serde_json::to_vec(&unsigned).unwrap());
        let verifier=VerifyingKey::from_bytes(&decode::<32>(&key).unwrap()).unwrap();
        assert!(verifier.verify_strict(&bytes,&Signature::from_bytes(&decode::<64>(signature.as_str().unwrap()).unwrap())).is_ok());
        assert!(store.sign_enrollment(&trust,"33333333-3333-4333-8333-333333333333",1002).is_err());
    }
    #[test]
    fn activated_candidate_archive_is_idempotent_and_supports_historical_lookup() {
        let fs=Fs::default(); let protector=TestProtector; let (trust,receipt)=fixture();
        let store=CandidateStore::open(&fs,"protected".into(),&protector).unwrap();
        let id="11111111-1111-4111-8111-111111111111";
        let key=store.stage(&trust,&receipt,1001).unwrap();
        let fingerprint:[u8;32]=Sha256::digest(decode::<32>(&key).unwrap()).into();
        assert!(store.archive_activated(&trust,id,&[0;32]).is_err());
        store.archive_activated(&trust,id,&fingerprint).unwrap();
        store.archive_activated(&trust,id,&fingerprint).unwrap();
        let mut current:Candidate=serde_json::from_slice(&fs.read(&store.path).unwrap()).unwrap();
        current.retirement_id="44444444-4444-4444-8444-444444444444".into();
        fs.atomic_write(&store.path,&serde_json::to_vec(&current).unwrap()).unwrap();
        let reopened=CandidateStore::open(&fs,"protected".into(),&protector).unwrap();
        assert_eq!(B64.encode(reopened.public_key(&trust,id).unwrap()),key);
        let path=reopened.archive_path(id).unwrap();
        fs.atomic_write(&path,b"conflicting evidence").unwrap();
        assert!(reopened.public_key(&trust,id).is_err());
        assert_eq!(fs.read(&path).unwrap(),b"conflicting evidence");
    }
    #[test]
    fn later_signed_retirement_stages_distinct_key_without_replacing_prior_key() {
        let fs=Fs::default();let protector=TestProtector;let (old,receipt)=fixture();
        let store=CandidateStore::open(&fs,"protected".into(),&protector).unwrap();
        let old_id="11111111-1111-4111-8111-111111111111";
        let old_key=store.stage(&old,&receipt,1001).unwrap();let original=fs.read(&store.path).unwrap();
        let mut current=old.clone();current.enrollment_generation="33333333-3333-4333-8333-333333333333".into();
        let new_id="44444444-4444-4444-8444-444444444444";
        let bytes=serde_json::to_vec(&("fleet.windows.admin-retirement.v1",&current.org_id,&current.device_id,
            &current.enrollment_generation,new_id,1000i64,&current.signer_key)).unwrap();
        let receipt=serde_json::json!({"org_id":current.org_id,"old_device_id":current.device_id,"old_generation":current.enrollment_generation,
            "retirement_id":new_id,"issued_at":1000,"signer_key":current.signer_key,
            "signature":B64.encode(SigningKey::from_bytes(&[11;32]).sign(&bytes).to_bytes())});
        let receipt=serde_json::to_vec(&receipt).unwrap();
        let new_key=store.stage(&current,&receipt,1001).unwrap();assert_ne!(old_key,new_key);
        assert_eq!(store.stage(&current,&receipt,1002).unwrap(),new_key);
        assert_eq!(fs.read(&store.path).unwrap(),original);
        assert_eq!(B64.encode(store.public_key(&old,old_id).unwrap()),old_key);
        assert_eq!(B64.encode(store.public_key(&current,new_id).unwrap()),new_key);
        assert!(store.sign_handoff_request(&current,new_id,1002).is_ok());
    }
    #[test]
    fn corrupt_candidate_is_retained_and_never_replaced() {
        let fs=Fs::default(); let protector=TestProtector; let (trust, receipt)=fixture();
        let store=CandidateStore::open(&fs, "protected".into(), &protector).unwrap();
        fs.atomic_write(&store.path, b"corrupt").unwrap();
        assert!(store.stage(&trust, &receipt, 1001).is_err());
        assert_eq!(fs.read(&store.path).unwrap(), b"corrupt");
    }
    #[cfg(windows)]
    #[test]
    fn machine_dpapi_round_trips_candidate_across_store_instances() {
        let fs=Fs::default(); let protector=crate::fleet_credential::MachineDpapi; let (trust, receipt)=fixture();
        let store=CandidateStore::open(&fs, "protected".into(), &protector).unwrap();
        let key=store.stage(&trust, &receipt, 1001).unwrap();
        drop(store);
        let reopened=CandidateStore::open(&fs, "protected".into(), &protector).unwrap();
        assert_eq!(reopened.stage(&trust, &receipt, 1002).unwrap(), key);
    }
    #[test]
    fn forged_stale_and_changed_binding_cannot_replace_candidate() {
        let fs=Fs::default(); let protector=TestProtector; let (trust, receipt)=fixture();
        let store=CandidateStore::open(&fs, "protected".into(), &protector).unwrap();
        assert!(store.stage(&trust,&receipt,1301).is_err());
        let mut forged:serde_json::Value=serde_json::from_slice(&receipt).unwrap(); forged["retirement_id"]="33333333-3333-4333-8333-333333333333".into();
        assert!(store.stage(&trust,&serde_json::to_vec(&forged).unwrap(),1001).is_err());
        let original=store.stage(&trust,&receipt,1001).unwrap();
        let mut other=trust.clone(); other.org_id="other".into();
        assert!(store.stage(&other,&receipt,1001).is_err());
        assert_eq!(store.stage(&trust,&receipt,1001).unwrap(),original);
    }
}
