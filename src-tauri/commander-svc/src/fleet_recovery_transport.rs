// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gated service-owned resume of a previously prepared recovery.
#![allow(dead_code)]
#[cfg(windows)]
use crate::{fleet_machine_policy::FleetMachinePolicy,fleet_recovery_candidate::CandidateStore};
#[cfg(windows)]
use std::time::{Duration,SystemTime,UNIX_EPOCH};
#[cfg(windows)]
const PATH:&str="/v1/agents/identity-handoff/commit";
#[cfg(windows)]
fn activate_if_authorized<T>(authorized:impl FnOnce()->bool, activate:impl FnOnce()->Result<T,&'static str>) -> Result<T,&'static str> {
    if !authorized() { return Err("administrator authorization changed"); }
    activate()
}
#[cfg(windows)]
fn now()->Result<i64,&'static str> {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs())
        .map_err(|_| "clock unavailable")
}
#[cfg(windows)]
fn proposal_conflict(status:reqwest::StatusCode)->Result<bool,&'static str> {
    match status {
        reqwest::StatusCode::FORBIDDEN => Ok(false),
        reqwest::StatusCode::CONFLICT => Ok(true),
        _ => Err("candidate submission unavailable"),
    }
}
/// Submit the protected candidate's proof; the server deliberately returns
/// 403 both for an accepted proposal and for rejection. It is not approval.
#[cfg(windows)]
pub(crate) async fn submit_candidate(store:&FleetMachinePolicy, candidate:&CandidateStore<'_>,
    receipt:&[u8], authorized:impl Fn()->bool) -> Result<(String, bool),&'static str> {
    let (trust,_,origins,profile,enabled)=store.trust_with_origins()?.ok_or("not provisioned")?;
    if !enabled || !authorized() { return Err("administrator authorization unavailable"); }
    let profile=profile.ok_or("network configuration unavailable")?;
    let origin=origins.first().ok_or("network configuration unavailable")?;
    if crate::fleet_runtime_env::fingerprint().as_deref()!=Some(profile.environment_fingerprint.as_str()) { return Err("network configuration differs"); }
    let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&profile.spki_pins);
    if !profile.spki_pins.trim().is_empty() && pins.is_none() { return Err("transport security invalid"); }
    let value:serde_json::Value=serde_json::from_slice(receipt).map_err(|_| "invalid retirement receipt")?;
    let retirement=value.get("retirement_id").and_then(serde_json::Value::as_str).ok_or("invalid retirement receipt")?;
    let key=store.with_handoff_trust(|current| {
        if current!=&trust || !authorized() { return Err("service trust changed"); }
        candidate.stage(current,receipt,now()?)
    })?;
    // The server requires enrollment_ts to be later than the retirement
    // second. A verified receipt cannot predate retirement, so wait until its
    // signed issue second has elapsed before making this one attempt.
    let issued=value.get("issued_at").and_then(serde_json::Value::as_i64).ok_or("invalid retirement receipt")?;
    if now()?<=issued { tokio::time::sleep(Duration::from_millis(1100)).await; }
    let request=store.with_handoff_trust(|current| {
        if current!=&trust || !authorized() { return Err("service trust changed"); }
        candidate.sign_enrollment(current,retirement,now()?)
    })?;
    let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_| "recovery transport unavailable")?;
    let response=tokio::time::timeout(Duration::from_secs(30),client.post(format!("{origin}/v1/windows/enroll-v2")).json(&request).send())
        .await.map_err(|_| "recovery transport unavailable")?.map_err(|_| "recovery transport unavailable")?;
    // A conflict may mean an older identity is already pending, or a replayed
    // nonce. Expose the locally generated fingerprint for comparison, without
    // implying that the server accepted this proposal. Never consume a body or
    // credential, including on an unexpected successful ordinary enrollment.
    let conflict=proposal_conflict(response.status())?;
    store.with_handoff_trust(|current| {
        if current!=&trust || !authorized() { return Err("administrator authorization changed"); }
        Ok(())
    })?;
    Ok((key, conflict))
}
/// Fetch a candidate-bound decision over the saved transport. This is not an
/// approval and has no authority to change the service trust or journal.
#[cfg(windows)]
pub(crate) async fn request_handoff(store:&FleetMachinePolicy, candidate:&CandidateStore<'_>,
    receipt:&[u8], authorized:impl Fn()->bool) -> Result<crate::fleet_identity_handoff::Handoff,&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    let (trust,_,origins,profile,enabled)=store.trust_with_origins()?.ok_or("not provisioned")?;
    if !enabled { return Err("service trust unavailable"); }
    let profile=profile.ok_or("network configuration unavailable")?;
    let origin=origins.first().ok_or("network configuration unavailable")?;
    if crate::fleet_runtime_env::fingerprint().as_deref()!=Some(profile.environment_fingerprint.as_str()) { return Err("network configuration differs"); }
    let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&profile.spki_pins);
    if !profile.spki_pins.trim().is_empty() && pins.is_none() { return Err("transport security invalid"); }
    let value:serde_json::Value=serde_json::from_slice(receipt).map_err(|_| "invalid retirement receipt")?;
    let retirement=value.get("retirement_id").and_then(serde_json::Value::as_str).ok_or("invalid retirement receipt")?;
    let (key,request)=store.with_handoff_trust(|current| {
        if current!=&trust { return Err("service trust changed"); }
        let key=candidate.stage(current,receipt,now()?)?;
        let request=candidate.sign_handoff_request(current,retirement,now()?)?;
        Ok((key,request))
    })?;
    let local:[u8;32]=STANDARD.decode(&key).map_err(|_| "invalid candidate")?.try_into().map_err(|_| "invalid candidate")?;
    let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_| "recovery transport unavailable")?;
    let operation=async {
        let mut response=client.post(format!("{origin}/v1/agents/identity-handoff")).json(&request).send().await.map_err(|_| "recovery transport unavailable")?;
        if response.status()!=reqwest::StatusCode::OK { return Err("recovery transport rejected"); }
        let mut bytes=Vec::new();
        while let Some(chunk)=response.chunk().await.map_err(|_| "recovery transport unavailable")? {
            if bytes.len().checked_add(chunk.len()).is_none_or(|size|size>16384) { return Err("invalid recovery response"); }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    };
    let response=tokio::time::timeout(Duration::from_secs(30),operation).await.map_err(|_| "recovery transport unavailable")??;
    let handoff:crate::fleet_identity_handoff::Handoff=serde_json::from_slice(&response).map_err(|_| "invalid recovery response")?;
    if !authorized() { return Err("administrator authorization changed"); }
    store.with_handoff_trust(|current| {
        if current!=&trust { return Err("service trust changed"); }
        handoff.verify(current,retirement,handoff.new_generation(),&local,now()?)?;
        Ok(())
    })?;
    Ok(handoff)
}
/// Called only from a future authorized recovery coordinator, never a renderer URL.
#[cfg(windows)]
pub(crate) async fn resume_prepared(store:&FleetMachinePolicy, candidate:&CandidateStore<'_>, authorized:impl Fn()->bool) -> Result<String,&'static str> {
    resume_prepared_with(store,candidate,&crate::fleet_credential::MachineDpapi,authorized).await
}
#[cfg(windows)]
async fn resume_prepared_with(store:&FleetMachinePolicy, candidate:&CandidateStore<'_>, protector:&dyn crate::fleet_credential::Protector, authorized:impl Fn()->bool) -> Result<String,&'static str> {
    let expected=store.pending_recovery()?.ok_or("recovery not pending")?;
    store.validate_pending_recovery_credential(&expected,protector)?;
    if crate::fleet_runtime_env::fingerprint().as_deref()!=Some(expected.profile().environment_fingerprint.as_str()) {
        return Err("network configuration differs");
    }
    let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&expected.profile().spki_pins);
    if !expected.profile().spki_pins.trim().is_empty() && pins.is_none() { return Err("transport security invalid"); }
    let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_| "recovery transport unavailable")?;
    let request=candidate.sign_commit(&expected,now()?)?;
    let nonce=request.get("nonce").and_then(serde_json::Value::as_str).ok_or("invalid recovery request")?.to_owned();
    let operation=async {
        let mut response=client.post(format!("{}{}",expected.origin(),PATH)).json(&request).send().await.map_err(|_| "recovery transport unavailable")?;
        if response.status()!=reqwest::StatusCode::OK { return Err("recovery transport rejected"); }
        let mut bytes=Vec::new();
        while let Some(chunk)=response.chunk().await.map_err(|_| "recovery transport unavailable")? {
            if bytes.len().checked_add(chunk.len()).is_none_or(|size| size>16384) { return Err("invalid recovery response"); }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    };
    let response=tokio::time::timeout(Duration::from_secs(30),operation).await.map_err(|_| "recovery transport unavailable")??;
    let activation=crate::fleet_recovery_credential::verify_commit_response(&response,&expected,&nonce,now()?)?;
    let status=activation.status().to_owned();
    activate_if_authorized(authorized,||store.activate_recovery(&expected,&activation,now()?))?;
    Ok(status)
}

#[cfg(all(test,windows))]
mod tests {
    #[test]
    fn proposal_conflict_does_not_treat_unexpected_success_as_acceptance() {
        assert_eq!(super::proposal_conflict(reqwest::StatusCode::FORBIDDEN),Ok(false));
        assert_eq!(super::proposal_conflict(reqwest::StatusCode::CONFLICT),Ok(true));
        assert!(super::proposal_conflict(reqwest::StatusCode::OK).is_err());
        assert!(super::proposal_conflict(reqwest::StatusCode::INTERNAL_SERVER_ERROR).is_err());
    }
    #[test]
    fn activation_requires_authorization_at_commit_time() {
        use std::cell::Cell;
        let active=Cell::new(true);
        let administrator=Cell::new(true);
        let activated=Cell::new(false);
        // A session can log off or lose its administrator role while the network is in flight.
        for (session,role) in [(false,true),(true,false)] {
            active.set(session); administrator.set(role);
            assert!(super::activate_if_authorized(||active.get() && administrator.get(),|| { activated.set(true); Ok(()) }).is_err());
            assert!(!activated.get());
        }
        active.set(true); administrator.set(true);
        assert!(super::activate_if_authorized(||active.get() && administrator.get(),|| { activated.set(true); Ok(()) }).is_ok());
        assert!(activated.get());
    }
}

/// Begins only from a reviewed, protected old trust and signed retirement.
/// A future caller must first establish native approval and old-process quiescence.
#[cfg(windows)]
pub(crate) async fn prepare_and_resume(store:&FleetMachinePolicy, candidate:&CandidateStore<'_>,
    retirement_receipt:&[u8], handoff:&crate::fleet_identity_handoff::Handoff,
    recovery:&str, generation:&str, archive:&str, protector:&dyn crate::fleet_credential::Protector,
    authorized:impl Fn()->bool)
    -> Result<String,&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    let (trust,_audit,origins,profile,enabled)=store.trust_with_origins()?.ok_or("not provisioned")?;
    if !enabled { return Err("service trust unavailable"); }
    let profile=profile.ok_or("network configuration unavailable")?;
    let origin=origins.first().ok_or("network configuration unavailable")?.clone();
    if crate::fleet_runtime_env::fingerprint().as_deref()!=Some(profile.environment_fingerprint.as_str()) { return Err("network configuration differs"); }
    let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&profile.spki_pins);
    if !profile.spki_pins.trim().is_empty() && pins.is_none() { return Err("transport security invalid"); }
    let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_| "recovery transport unavailable")?;
    let key=candidate.stage(&trust,retirement_receipt,now()?)?;
    let public:[u8;32]=STANDARD.decode(&key).map_err(|_| "invalid candidate")?.try_into().map_err(|_| "invalid candidate")?;
    let verified=handoff.verify(&trust,recovery,generation,&public,now()?)?;
    store.validate_recovery_review(&verified,archive)?;
    if !authorized() { return Err("administrator authorization changed"); }
    let decision=verified.decision_digest().iter().map(|b|format!("{b:02x}")).collect::<String>();
    let request=candidate.sign_confirmation(&trust,recovery,generation,&decision,archive,verified.fingerprint(),now()?)?;
    let nonce=request.get("nonce").and_then(serde_json::Value::as_str).ok_or("invalid recovery request")?.to_owned();
    let operation=async {
        let mut response=client.post(format!("{origin}/v1/agents/identity-handoff/confirm"))
            .json(&request).send().await.map_err(|_| "recovery transport unavailable")?;
        if response.status()!=reqwest::StatusCode::OK { return Err("recovery transport rejected"); }
        let mut bytes=zeroize::Zeroizing::new(Vec::new());
        while let Some(chunk)=response.chunk().await.map_err(|_| "recovery transport unavailable")? {
            if bytes.len().checked_add(chunk.len()).is_none_or(|size|size>16384) { return Err("invalid recovery response"); }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    };
    let response=tokio::time::timeout(Duration::from_secs(30),operation).await.map_err(|_| "recovery transport unavailable")??;
    let credential=crate::fleet_recovery_credential::verify_response(&response,&trust,&verified,archive,&nonce,now()?)?;
    if !authorized() { return Err("administrator authorization changed"); }
    store.prepare_recovery_commit(&verified,&credential,archive,&origin,protector,now()?)?;
    resume_prepared_with(store,candidate,protector,authorized).await
}
