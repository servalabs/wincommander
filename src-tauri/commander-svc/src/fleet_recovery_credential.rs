// SPDX-License-Identifier: AGPL-3.0-or-later
//! Inactive verification of a recovery credential returned by the pinned server.
#![allow(dead_code)]
use base64::{engine::general_purpose::STANDARD,Engine};
use ed25519_dalek::{Signature,VerifyingKey};
use serde::Deserialize;
use sha2::{Digest,Sha256};
use zeroize::Zeroizing;
use crate::{fleet_identity_handoff::VerifiedHandoff,fleet_machine_policy::FleetTrust};
const INVALID:&str="invalid recovery credential";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedCredential {
    org_id:String, device_id:String, recovery_id:String, new_generation:String,
    decision_digest:String, archive_sha256:String, credential_sha256:String,
    server_key:String, request_nonce:String, issued_at:i64, checkin_secret_b64:String, signature:String,
}
impl Drop for PreparedCredential { fn drop(&mut self) { use zeroize::Zeroize; self.checkin_secret_b64.zeroize(); } }
pub(crate) fn verify_response(bytes:&[u8], trust:&FleetTrust, handoff:&VerifiedHandoff, archive:&str,
    request_nonce:&str, now:i64)->Result<VerifiedCredential,&'static str> {
    if bytes.len()>16384 { return Err(INVALID); }
    let response:PreparedCredential=serde_json::from_slice(bytes).map_err(|_|INVALID)?;
    response.verify(trust,handoff,archive,request_nonce,now)
}
pub(crate) struct VerifiedCredential { secret:Zeroizing<Vec<u8>>, digest:String, recovery_id:String, new_generation:String, decision_digest:String, archive_sha256:String, issued_at:i64 }
impl VerifiedCredential {
    pub(crate) fn secret(&self)->&[u8] { &self.secret }
    pub(crate) fn digest(&self)->&str { &self.digest }
    pub(crate) fn recovery_id(&self)->&str { &self.recovery_id }
    pub(crate) fn new_generation(&self)->&str { &self.new_generation }
    pub(crate) fn decision_digest(&self)->&str { &self.decision_digest }
    pub(crate) fn archive_sha256(&self)->&str { &self.archive_sha256 }
    pub(crate) fn issued_at(&self)->i64 { self.issued_at }
}
fn hex(bytes:&[u8])->String { bytes.iter().map(|b|format!("{b:02x}")).collect() }
fn uuid(value:&str)->bool { uuid::Uuid::parse_str(value).ok().is_some_and(|id| !id.is_nil() && id.to_string()==value) }
fn decode<const N:usize>(value:&str)->Result<[u8;N],&'static str> {
    let bytes=STANDARD.decode(value).map_err(|_|INVALID)?;
    if STANDARD.encode(&bytes)!=value { return Err(INVALID); }
    bytes.try_into().map_err(|_|INVALID)
}
impl PreparedCredential {
    fn preimage(&self)->Result<Vec<u8>,&'static str> {
        serde_json::to_vec(&("wincommander:fleet:identity-handoff-prepare-response:v1","POST",
            "/v1/agents/identity-handoff/confirm",&self.org_id,&self.device_id,&self.recovery_id,
            &self.new_generation,&self.decision_digest,&self.archive_sha256,&self.credential_sha256,
            &self.server_key,&self.request_nonce,self.issued_at)).map_err(|_|INVALID)
    }
    fn verify(&self, trust:&FleetTrust, handoff:&VerifiedHandoff, archive:&str, request_nonce:&str,
        now:i64)->Result<VerifiedCredential,&'static str> {
        if self.org_id!=trust.org_id || self.device_id!=trust.device_id || self.recovery_id!=handoff.retirement_id()
            || self.new_generation!=handoff.new_generation() || self.decision_digest!=hex(handoff.decision_digest())
            || self.archive_sha256!=archive || !valid_hash(archive) || self.server_key!=trust.signer_key
            || self.request_nonce!=request_nonce || !uuid(request_nonce) || !crate::fleet_identity_handoff::recovery_timestamp_valid(self.issued_at,now) { return Err(INVALID); }
        let secret=Zeroizing::new(STANDARD.decode(&self.checkin_secret_b64).map_err(|_|INVALID)?);
        if secret.len()!=32 || STANDARD.encode(secret.as_slice())!=self.checkin_secret_b64 { return Err(INVALID); }
        if self.credential_sha256!=hex(&Sha256::digest(secret.as_slice())) { return Err(INVALID); }
        let key=VerifyingKey::from_bytes(&decode::<32>(&trust.signer_key)?).map_err(|_|INVALID)?;
        if key.is_weak() { return Err(INVALID); }
        key.verify_strict(&self.preimage()?,&Signature::from_bytes(&decode::<64>(&self.signature)?)).map_err(|_|INVALID)?;
        Ok(VerifiedCredential { secret:Zeroizing::new(secret.to_vec()),digest:self.credential_sha256.clone(), recovery_id:self.recovery_id.clone(), new_generation:self.new_generation.clone(), decision_digest:self.decision_digest.clone(), archive_sha256:self.archive_sha256.clone(), issued_at:self.issued_at })
    }
}
fn valid_hash(value:&str)->bool { value.len()==64 && value.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer,SigningKey};
    #[test]
    fn accepts_only_fresh_pinned_response_bound_to_secret_request_and_decision() {
        let trust=FleetTrust { org_id:"org-a".into(),device_id:"0480b2ca-f4c3-4599-81fc-4876f73e70fa".into(),
            enrollment_generation:"22222222-2222-4222-8222-222222222222".into(),signer_key:"Zr5+Myx6RTMyvZ0Kf32wVfXF7xoGraZtmLOftoEMRzo=".into() };
        let handoff:crate::fleet_identity_handoff::Handoff=serde_json::from_value(serde_json::json!({"recovery_id":"11111111-1111-4111-8111-111111111111",
            "org_id":trust.org_id,"device_id":trust.device_id,"old_generation":trust.enrollment_generation,
            "new_generation":"33333333-3333-4333-8333-333333333333","new_key":"kaKKC3Q4FZOk2UaVeSCJJq/IrYLIg5t2RDWbnrqaSzo=",
            "next_sequence":7,"previous_hash":"ab".repeat(32),"issued_at":1000,"server_key":trust.signer_key,
            "signature":"h97rXEpV9lskSg1jI2698m0urZAufIhNyq0qhcDFCNc2lVnOBaCWXIWxHgi6GDWP3rRyvbqdv/zhsv/TvjWZDQ=="})).unwrap();
        let local=decode::<32>("kaKKC3Q4FZOk2UaVeSCJJq/IrYLIg5t2RDWbnrqaSzo=").unwrap();
        let verified=handoff.verify(&trust,"11111111-1111-4111-8111-111111111111","33333333-3333-4333-8333-333333333333",&local,1001).unwrap();
        let nonce="44444444-4444-4444-8444-444444444444"; let archive="cd".repeat(32);
        let mut response=PreparedCredential { org_id:trust.org_id.clone(),device_id:trust.device_id.clone(),recovery_id:verified.retirement_id().into(),
            new_generation:verified.new_generation().into(),decision_digest:hex(verified.decision_digest()),archive_sha256:archive.clone(),
            credential_sha256:hex(&Sha256::digest([8u8;32])),server_key:trust.signer_key.clone(),request_nonce:nonce.into(),issued_at:1000,
            checkin_secret_b64:STANDARD.encode([8u8;32]),signature:String::new() };
        response.signature=STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&response.preimage().unwrap()).to_bytes());
        assert_eq!(response.verify(&trust,&verified,&archive,nonce,1001).unwrap().secret(), &[8u8;32]);
        assert!(response.verify(&trust,&verified,&archive,"55555555-5555-4555-8555-555555555555",1001).is_err());
        assert!(response.verify(&trust,&verified,&archive,nonce,1301).is_err());
        response.checkin_secret_b64=STANDARD.encode([9u8;32]);
        assert!(response.verify(&trust,&verified,&archive,nonce,1001).is_err());
    }
}

#[cfg(windows)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitResponse {
    org_id:String, device_id:String, recovery_id:String, new_generation:String, decision_digest:String,
    archive_sha256:String, local_commit_digest:String, credential_sha256:String, server_key:String,
    request_nonce:String, issued_at:i64, status:String, signature:String,
}
#[cfg(windows)]
pub(crate) struct VerifiedActivation { recovery_id:String, local_commit_digest:String, status:String, issued_at:i64 }
#[cfg(windows)]
impl VerifiedActivation {
    pub(crate) fn recovery_id(&self)->&str { &self.recovery_id }
    pub(crate) fn local_commit_digest(&self)->&str { &self.local_commit_digest }
    pub(crate) fn status(&self)->&str { &self.status }
    pub(crate) fn issued_at(&self)->i64 { self.issued_at }
}
#[cfg(windows)]
pub(crate) fn verify_commit_response(bytes:&[u8], expected:&crate::fleet_machine_policy::RecoveryExpected,
    request_nonce:&str, now:i64)->Result<VerifiedActivation,&'static str> {
    if bytes.len()>16384 || !uuid(request_nonce) { return Err(INVALID); }
    let response:CommitResponse=serde_json::from_slice(bytes).map_err(|_|INVALID)?;
    let trust=expected.old_trust();
    if response.org_id!=trust.org_id || response.device_id!=trust.device_id || response.server_key!=trust.signer_key
        || response.recovery_id!=expected.recovery_id() || response.new_generation!=expected.new_generation()
        || response.decision_digest!=expected.decision_digest() || response.archive_sha256!=expected.archive_sha256()
        || response.local_commit_digest!=expected.local_commit_digest() || response.credential_sha256!=expected.credential_sha256()
        || response.request_nonce!=request_nonce || !crate::fleet_identity_handoff::recovery_timestamp_valid(response.issued_at,now)
        || !matches!(response.status.as_str(),"credential_committed_pending_approval"|"credential_committed_active") { return Err(INVALID); }
    let key=VerifyingKey::from_bytes(&decode::<32>(&trust.signer_key)?).map_err(|_|INVALID)?;
    if key.is_weak() { return Err(INVALID); }
    let bytes=serde_json::to_vec(&("wincommander:fleet:identity-handoff-commit-response:v1","POST",
        "/v1/agents/identity-handoff/commit",&response.org_id,&response.device_id,&response.recovery_id,
        &response.new_generation,&response.decision_digest,&response.archive_sha256,&response.local_commit_digest,
        &response.credential_sha256,&response.server_key,&response.request_nonce,response.issued_at,&response.status)).map_err(|_|INVALID)?;
    key.verify_strict(&bytes,&Signature::from_bytes(&decode::<64>(&response.signature)?)).map_err(|_|INVALID)?;
    Ok(VerifiedActivation { recovery_id:response.recovery_id,local_commit_digest:response.local_commit_digest,
        status:response.status,issued_at:response.issued_at })
}
