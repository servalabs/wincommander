// SPDX-License-Identifier: AGPL-3.0-or-later
//! Verification primitive for a future service-owned identity handoff.
//! No IPC route or trust mutation is enabled by this module.
#![allow(dead_code)]

use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use crate::fleet_machine_policy::FleetTrust;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Handoff {
    recovery_id: String,
    org_id: String,
    device_id: String,
    old_generation: String,
    new_generation: String,
    new_key: String,
    next_sequence: u64,
    previous_hash: Option<String>,
    issued_at: i64,
    server_key: String,
    signature: String,
}

pub(crate) struct VerifiedHandoff {
    retirement_id: String,
    new_generation: String,
    fingerprint: [u8; 32],
    digest: [u8; 32],
    decision_digest: [u8; 32],
    public_key: [u8;32],
    next_sequence: u64,
    previous_hash: Option<String>,
    issued_at: i64,
}
impl VerifiedHandoff {
    pub(crate) fn retirement_id(&self) -> &str { &self.retirement_id }
    pub(crate) fn new_generation(&self) -> &str { &self.new_generation }
    pub(crate) fn fingerprint(&self) -> &[u8;32] { &self.fingerprint }
    pub(crate) fn digest(&self) -> &[u8;32] { &self.digest }
    pub(crate) fn decision_digest(&self) -> &[u8;32] { &self.decision_digest }
    pub(crate) fn public_key(&self) -> &[u8;32] { &self.public_key }
    pub(crate) fn next_sequence(&self) -> u64 { self.next_sequence }
    pub(crate) fn previous_hash(&self) -> Option<&str> { self.previous_hash.as_deref() }
    pub(crate) fn issued_at(&self) -> i64 { self.issued_at }
}

fn canonical_uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).ok().is_some_and(|id| !id.is_nil() && id.to_string() == s)
}
fn decode<const N: usize>(s: &str) -> Result<[u8; N], &'static str> {
    let bytes = STANDARD.decode(s).map_err(|_| "invalid identity handoff")?;
    if STANDARD.encode(&bytes) != s { return Err("invalid identity handoff"); }
    bytes.try_into().map_err(|_| "invalid identity handoff")
}

impl Handoff {
    pub(crate) fn new_generation(&self) -> &str { &self.new_generation }
    /// Historical proof is evidence of an already committed decision, never
    /// fresh authority to start a new recovery. The expected digest must come
    /// from the protected active-recovery record.
    pub(crate) fn verify_historical(&self, trust:&FleetTrust, retirement_id:&str, new_generation:&str,
        local_key:&[u8;32], expected_decision:&str) -> Result<VerifiedHandoff,&'static str> {
        let verified=self.verify(trust,retirement_id,new_generation,local_key,self.issued_at)?;
        let digest:String=verified.decision_digest().iter().map(|byte|format!("{byte:02x}")).collect();
        if digest!=expected_decision { return Err("historical handoff changed"); }
        Ok(verified)
    }
    fn bytes(&self) -> Result<Vec<u8>, &'static str> {
        serde_json::to_vec(&("wincommander:fleet:identity-outcome-handoff:v1", &self.recovery_id,
            &self.org_id, &self.device_id, &self.old_generation, &self.new_generation, &self.new_key,
            self.next_sequence, &self.previous_hash, self.issued_at, &self.server_key))
            .map_err(|_| "invalid identity handoff")
    }

    /// `trust` is protected service state. `local_key` must be read through
    /// the authenticated broker; neither value may come from the renderer.
    pub(crate) fn verify(&self, trust: &FleetTrust, retirement_id: &str, new_generation: &str,
        local_key: &[u8; 32], now: i64) -> Result<VerifiedHandoff, &'static str> {
        let head = match (&self.previous_hash, self.next_sequence) {
            (None, 1) => true,
            (Some(hash), n) if n > 1 => hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            _ => false,
        };
        if self.org_id != trust.org_id || self.org_id.is_empty() || self.org_id.len() > 256
            || self.device_id != trust.device_id || self.old_generation != trust.enrollment_generation
            || self.server_key != trust.signer_key || self.recovery_id != retirement_id
            || self.new_generation != new_generation || self.old_generation == self.new_generation
            || !canonical_uuid(&self.device_id) || !canonical_uuid(&self.old_generation)
            || !canonical_uuid(&self.new_generation) || !canonical_uuid(&self.recovery_id)
            || self.new_key != STANDARD.encode(local_key) || !recovery_timestamp_valid(self.issued_at, now) || !head {
            return Err("invalid identity handoff");
        }
        let key = VerifyingKey::from_bytes(&decode::<32>(&trust.signer_key)?).map_err(|_| "invalid identity handoff")?;
        if key.is_weak() { return Err("invalid identity handoff"); }
        let local = VerifyingKey::from_bytes(local_key).map_err(|_| "invalid identity handoff")?;
        if local.is_weak() { return Err("invalid identity handoff"); }
        let bytes = self.bytes()?;
        key.verify_strict(&bytes, &Signature::from_bytes(&decode::<64>(&self.signature)?))
            .map_err(|_| "invalid identity handoff")?;
        let decision=serde_json::to_vec(&("wincommander:fleet:identity-handoff-decision:v1", &self.recovery_id,
            &self.org_id,&self.device_id,&self.old_generation,&self.new_generation,&self.new_key,
            self.next_sequence,&self.previous_hash,&self.server_key)).map_err(|_| "invalid identity handoff")?;
        Ok(VerifiedHandoff { retirement_id:self.recovery_id.clone(), new_generation:self.new_generation.clone(), fingerprint:Sha256::digest(local_key).into(), digest:Sha256::digest(&bytes).into(), decision_digest:Sha256::digest(&decision).into(), public_key:*local_key, next_sequence:self.next_sequence, previous_hash:self.previous_hash.clone(), issued_at:self.issued_at })
    }
}

// Timestamps are whole seconds and originate on different machines. Keep the
// existing five-minute expiry while tolerating a small forward clock skew.
pub(crate) fn recovery_timestamp_valid(issued_at:i64, now:i64)->bool {
    issued_at <= now.saturating_add(5) && now.saturating_sub(issued_at) <= 300
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn timestamp_skew_and_expiry_are_bounded() {
        assert!(recovery_timestamp_valid(1000, 995));
        assert!(!recovery_timestamp_valid(1000, 994));
        assert!(recovery_timestamp_valid(1000, 1300));
        assert!(!recovery_timestamp_valid(1000, 1301));
    }
    #[test]
    fn requires_protected_trust_exact_local_key_and_untampered_signature() {
        let signer = SigningKey::from_bytes(&[11;32]);
        let local_key = *SigningKey::from_bytes(&[22;32]).verifying_key().as_bytes();
        let mut h = Handoff { recovery_id:"11111111-1111-4111-8111-111111111111".into(),
            org_id:"org-a".into(), device_id:"0480b2ca-f4c3-4599-81fc-4876f73e70fa".into(),
            old_generation:"22222222-2222-4222-8222-222222222222".into(),
            new_generation:"33333333-3333-4333-8333-333333333333".into(),
            new_key:STANDARD.encode(local_key), next_sequence:1, previous_hash:None, issued_at:1000,
            server_key:STANDARD.encode(signer.verifying_key().as_bytes()), signature:String::new() };
        h.signature = STANDARD.encode(signer.sign(&h.bytes().unwrap()).to_bytes());
        let trust = FleetTrust { org_id:h.org_id.clone(), device_id:h.device_id.clone(),
            enrollment_generation:h.old_generation.clone(), signer_key:h.server_key.clone() };
        let verify = |v:&Handoff, key:&[u8;32], now| v.verify(&trust, &h.recovery_id, &h.new_generation, key, now);
        assert!(verify(&h, &local_key, 1001).is_ok());
        assert!(verify(&h, &local_key, 995).is_ok());
        assert!(verify(&h, &local_key, 994).is_err());
        assert!(verify(&h, &[23;32], 1001).is_err());
        assert!(verify(&h, &local_key, 1301).is_err());
        let mut changed=h.clone(); changed.next_sequence=2;
        assert!(verify(&changed, &local_key, 1001).is_err());
        changed=h.clone(); changed.old_generation=h.new_generation.clone();
        assert!(verify(&changed, &local_key, 1001).is_err());
    }

    #[test]
    fn accepts_server_wire_field_and_rejects_ambiguous_old_name() {
        let wire = serde_json::json!({
            "recovery_id": "11111111-1111-4111-8111-111111111111",
            "org_id": "org-a", "device_id": "0480b2ca-f4c3-4599-81fc-4876f73e70fa",
            "old_generation": "22222222-2222-4222-8222-222222222222",
            "new_generation": "33333333-3333-4333-8333-333333333333",
            "new_key": "kaKKC3Q4FZOk2UaVeSCJJq/IrYLIg5t2RDWbnrqaSzo=",
            "next_sequence": 7, "previous_hash": "ab".repeat(32), "issued_at": 1000,
            "server_key": "Zr5+Myx6RTMyvZ0Kf32wVfXF7xoGraZtmLOftoEMRzo=",
            "signature": "h97rXEpV9lskSg1jI2698m0urZAufIhNyq0qhcDFCNc2lVnOBaCWXIWxHgi6GDWP3rRyvbqdv/zhsv/TvjWZDQ=="
        });
        let handoff: Handoff = serde_json::from_value(wire.clone()).unwrap();
        let trust = FleetTrust { org_id: handoff.org_id.clone(), device_id: handoff.device_id.clone(),
            enrollment_generation: handoff.old_generation.clone(), signer_key: handoff.server_key.clone() };
        let key = decode::<32>(&handoff.new_key).unwrap();
        assert!(handoff.verify(&trust, &handoff.recovery_id, &handoff.new_generation, &key, 1001).is_ok());
        let mut ambiguous = wire;
        ambiguous["retirement_id"] = ambiguous["recovery_id"].clone();
        assert!(serde_json::from_value::<Handoff>(ambiguous).is_err());
    }

    #[test]
    fn independent_server_vector_verifies() {
        let h = Handoff { recovery_id:"11111111-1111-4111-8111-111111111111".into(),
            org_id:"org-a".into(), device_id:"0480b2ca-f4c3-4599-81fc-4876f73e70fa".into(),
            old_generation:"22222222-2222-4222-8222-222222222222".into(),
            new_generation:"33333333-3333-4333-8333-333333333333".into(),
            new_key:"kaKKC3Q4FZOk2UaVeSCJJq/IrYLIg5t2RDWbnrqaSzo=".into(),
            next_sequence:7, previous_hash:Some("ab".repeat(32)), issued_at:1000,
            server_key:"Zr5+Myx6RTMyvZ0Kf32wVfXF7xoGraZtmLOftoEMRzo=".into(),
            signature:"h97rXEpV9lskSg1jI2698m0urZAufIhNyq0qhcDFCNc2lVnOBaCWXIWxHgi6GDWP3rRyvbqdv/zhsv/TvjWZDQ==".into() };
        let trust=FleetTrust { org_id:h.org_id.clone(), device_id:h.device_id.clone(),
            enrollment_generation:h.old_generation.clone(), signer_key:h.server_key.clone() };
        let key=decode::<32>(&h.new_key).unwrap();
        assert!(h.verify(&trust, &h.recovery_id, &h.new_generation, &key, 1001).is_ok());
    }
}
