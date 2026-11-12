// SPDX-License-Identifier: AGPL-3.0-or-later
//! A recovered identity's leave intent is sent by the credential-owning service.
//! A response here never authorizes cleanup or establishes revocation.
#[cfg(windows)]
use crate::fleet_machine_policy::FleetMachinePolicy;
use base64::{engine::general_purpose::STANDARD,Engine};
use sha2::{Digest,Sha256};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusProof {protocol_version:u8,org_id:String,device_id:String,enrollment_generation:String,nonce:String,request_sha256:String,status:String,approved:bool,approvals:u64,required_approvals:u64,signature_b64:String}
pub(crate) struct VerifiedTerminal { trust:crate::fleet_machine_policy::FleetTrust, revision:u64, evidence:TerminalEvidence }
#[cfg(windows)]
pub(crate) struct VerifiedStatus {pub terminal:Option<VerifiedTerminal>,pub status:String,pub approvals:u64,pub required:u64}
#[derive(Clone,serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TerminalEvidence {request_b64:String,response_b64:String,nonce:String}
impl VerifiedTerminal {
    pub(crate) fn matches(&self,trust:&crate::fleet_machine_policy::FleetTrust,revision:u64)->bool {&self.trust==trust && self.revision==revision}
    pub(crate) fn evidence(&self)->TerminalEvidence {self.evidence.clone()}
}

fn verify_terminal(trust:&crate::fleet_machine_policy::FleetTrust,request:&[u8],nonce:&str,response:&[u8])->Result<StatusProof,&'static str> {
    let proof:StatusProof=serde_json::from_slice(response).map_err(|_|"invalid terminal proof")?;
    if proof.protocol_version!=1 || proof.org_id!=trust.org_id || proof.device_id!=trust.device_id
        || proof.enrollment_generation!=trust.enrollment_generation || proof.nonce!=nonce
        || proof.request_sha256!=STANDARD.encode(Sha256::digest(request)) || proof.approved!=(proof.status=="approved")
        || !matches!(proof.status.as_str(),"approved"|"pending"|"none") || (proof.status!="pending" && (proof.approvals!=0 || proof.required_approvals!=0))
        || (proof.status=="pending" && (!(1..=2).contains(&proof.required_approvals) || proof.approvals>2)) {return Err("invalid terminal proof")}
    let key=STANDARD.decode(&trust.signer_key).map_err(|_|"invalid terminal proof")?;
    if STANDARD.encode(&key)!=trust.signer_key {return Err("invalid terminal proof")}
    let key=ed25519_dalek::VerifyingKey::from_bytes(&key.try_into().map_err(|_|"invalid terminal proof")?).map_err(|_|"invalid terminal proof")?;
    let signature=STANDARD.decode(&proof.signature_b64).map_err(|_|"invalid terminal proof")?;
    if key.is_weak() || STANDARD.encode(&signature)!=proof.signature_b64 {return Err("invalid terminal proof")}
    let signature=ed25519_dalek::Signature::from_slice(&signature).map_err(|_|"invalid terminal proof")?;
    let bytes=serde_json::to_vec(&serde_json::json!(["fleet.windows.unenroll-status.response.v1","POST","/v1/windows/unenroll-status",proof.org_id,proof.device_id,proof.enrollment_generation,proof.nonce,proof.request_sha256,proof.status,proof.approved,proof.approvals,proof.required_approvals])).map_err(|_|"invalid terminal proof")?;
    key.verify_strict(&bytes,&signature).map_err(|_|"invalid terminal proof")?;
    Ok(proof)
}

pub(crate) fn evidence_valid(trust:&crate::fleet_machine_policy::FleetTrust,evidence:&TerminalEvidence)->bool {
    if evidence.request_b64.len()>5500 || evidence.response_b64.len()>22000 || evidence.nonce.len()!=64 {return false}
    let Ok(request)=STANDARD.decode(&evidence.request_b64) else{return false};let Ok(response)=STANDARD.decode(&evidence.response_b64) else{return false};
    request.len()<=4096 && response.len()<=16384 && STANDARD.encode(&request)==evidence.request_b64 && STANDARD.encode(&response)==evidence.response_b64
        && verify_terminal(trust,&request,&evidence.nonce,&response).is_ok_and(|proof|proof.approved)
}

#[cfg(windows)]
pub(crate) async fn terminal_status(store:&FleetMachinePolicy,snapshot:&crate::fleet_machine_policy::CredentialSnapshot,request:serde_json::Value,nonce:&str)->Result<VerifiedStatus,&'static str> {
    let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&snapshot.profile.spki_pins);
    if !snapshot.profile.spki_pins.trim().is_empty() && pins.is_none(){return Err("invalid transport")}
    let bytes=serde_json::to_vec(&request).map_err(|_|"invalid terminal proof")?;
    let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_|"terminal status unavailable")?;
    let op=async {
        let mut response=client.post(format!("{}/v1/windows/unenroll-status",snapshot.origin)).header(reqwest::header::CONTENT_TYPE,"application/json").body(bytes.clone()).send().await.map_err(|_|"terminal status unavailable")?;
        if response.status()!=reqwest::StatusCode::OK {return Err("terminal status unavailable")}
        let mut body=Vec::new();while let Some(chunk)=response.chunk().await.map_err(|_|"terminal status unavailable")? {if body.len().checked_add(chunk.len()).is_none_or(|n|n>16384){return Err("invalid terminal proof")}body.extend_from_slice(&chunk);}
        let proof=verify_terminal(&snapshot.trust,&bytes,nonce,&body)?;
        Ok((proof,body))
    };
    let (proof,response)=tokio::time::timeout(std::time::Duration::from_secs(30),op).await.map_err(|_|"terminal status unavailable")??;
    if !store.credential_is_current(snapshot) {return Err("credential changed")}
    Ok(VerifiedStatus{terminal:proof.approved.then(||VerifiedTerminal{trust:snapshot.trust.clone(),revision:snapshot.revision,evidence:TerminalEvidence{request_b64:STANDARD.encode(bytes),response_b64:STANDARD.encode(response),nonce:nonce.into()}}),status:proof.status,approvals:proof.approvals,required:proof.required_approvals})
}

#[cfg(windows)]
fn pending_response(value:serde_json::Value)->Result<serde_json::Value,&'static str> {
    let id=value.get("command_id").and_then(serde_json::Value::as_str).ok_or("leave request unavailable")?;
    if uuid::Uuid::parse_str(id).ok().is_none_or(|v|v.is_nil() || v.to_string()!=id) {return Err("leave request unavailable")}
    let approvals=value.get("approvals").and_then(serde_json::Value::as_u64).ok_or("leave request unavailable")?;
    let required=value.get("required_approvals").and_then(serde_json::Value::as_u64).ok_or("leave request unavailable")?;
    if !(1..=2).contains(&required) || approvals>2 {return Err("leave request unavailable")}
    // The endpoint is a request acknowledgement, not a terminal signed proof.
    Ok(serde_json::json!({"command_id":id,"status":"pending","approved":false,"approvals":approvals,"required_approvals":required}))
}

#[cfg(windows)]
pub(crate) async fn request(store:&FleetMachinePolicy,protector:&dyn crate::fleet_credential::Protector,expected_device:&str)->Result<serde_json::Value,&'static str> {
    const PATH:&str="/v1/agents/unenroll-request";
    let snapshot=store.credential_snapshot(protector)?.ok_or("leave request unavailable")?;
    if !snapshot.is_recovered || snapshot.trust.device_id != expected_device || !store.credential_is_current(&snapshot)
        || crate::fleet_runtime_env::fingerprint().as_deref()!=Some(snapshot.profile.environment_fingerprint.as_str()) {return Err("leave request unavailable")}
    let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&snapshot.profile.spki_pins);
    if !snapshot.profile.spki_pins.trim().is_empty() && pins.is_none() {return Err("leave request unavailable")}
    let now=i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"clock unavailable")?.as_secs()).map_err(|_|"clock unavailable")?;
    let mut body=serde_json::json!({"device_id":snapshot.trust.device_id,"ts":now,"nonce":uuid::Uuid::new_v4().to_string(),"hmac_version":fleet_agent_core::HMAC_VERSION_V2,"hmac":""});
    body["hmac"]=serde_json::json!(fleet_agent_core::compute_request_hmac_v2(&snapshot.secret,"POST",PATH,&body).map_err(|_|"leave request unavailable")?);
    if !store.credential_is_current(&snapshot) {return Err("leave request unavailable")}
    let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_|"leave request unavailable")?;
    let operation=async {
        let mut response=client.post(format!("{}{PATH}",snapshot.origin)).json(&body).send().await.map_err(|_|"leave request unavailable")?;
        if !response.status().is_success() {return Err("leave request unavailable")}
        let mut bytes=Vec::new();while let Some(chunk)=response.chunk().await.map_err(|_|"leave request unavailable")? {
            if bytes.len().checked_add(chunk.len()).is_none_or(|n|n>16384) {return Err("leave request unavailable")} bytes.extend_from_slice(&chunk);
        }
        pending_response(serde_json::from_slice(&bytes).map_err(|_|"leave request unavailable")?)
    };
    let result=tokio::time::timeout(std::time::Duration::from_secs(30),operation).await.map_err(|_|"leave request unavailable")??;
    if !store.credential_is_current(&snapshot) {return Err("leave request unavailable")}
    Ok(result)
}

#[cfg(all(test,windows))]
#[test]
fn request_ack_cannot_claim_terminal_or_return_arbitrary_fields() {
    let id=uuid::Uuid::new_v4().to_string();
    let result=pending_response(serde_json::json!({"command_id":id,"status":"approved","approved":true,"approvals":1,"required_approvals":1,"secret":"discard"})).unwrap();
    assert_eq!(result["status"],"pending");assert_eq!(result["approved"],false);assert!(result.get("secret").is_none());
    assert!(pending_response(serde_json::json!({"command_id":"bad","approvals":0,"required_approvals":1})).is_err());
}

#[cfg(all(test,windows))]
#[test]
fn terminal_proof_is_bound_to_server_generation_nonce_and_exact_request() {
    use ed25519_dalek::{Signer,SigningKey};
    let key=SigningKey::from_bytes(&[7;32]);
    let trust=crate::fleet_machine_policy::FleetTrust {org_id:"local".into(),device_id:uuid::Uuid::new_v4().to_string(),enrollment_generation:uuid::Uuid::new_v4().to_string(),signer_key:STANDARD.encode(key.verifying_key().as_bytes())};
    let nonce="a".repeat(64);let request=b"exact signed request";
    let hash=STANDARD.encode(Sha256::digest(request));
    let bytes=serde_json::to_vec(&serde_json::json!(["fleet.windows.unenroll-status.response.v1","POST","/v1/windows/unenroll-status",trust.org_id,trust.device_id,trust.enrollment_generation,nonce,hash,"approved",true,0,0])).unwrap();
    let mut response=serde_json::json!({"protocol_version":1,"org_id":trust.org_id,"device_id":trust.device_id,"enrollment_generation":trust.enrollment_generation,"nonce":nonce,"request_sha256":hash,"status":"approved","approved":true,"approvals":0,"required_approvals":0,"signature_b64":STANDARD.encode(key.sign(&bytes).to_bytes())});
    let encoded=serde_json::to_vec(&response).unwrap();assert!(verify_terminal(&trust,request,&nonce,&encoded).unwrap().approved);
    assert!(verify_terminal(&trust,b"different request",&nonce,&encoded).is_err());
    assert!(verify_terminal(&trust,request,&"b".repeat(64),&encoded).is_err());
    let mut wrong=trust.clone();wrong.enrollment_generation=uuid::Uuid::new_v4().to_string();assert!(verify_terminal(&wrong,request,&nonce,&encoded).is_err());
    response["signature_b64"]=serde_json::json!(STANDARD.encode([0u8;64]));
    assert!(verify_terminal(&trust,request,&nonce,&serde_json::to_vec(&response).unwrap()).is_err());
}
