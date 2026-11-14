// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disabled native confirmation building block; no IPC route calls it.
#![allow(dead_code)]
use super::{peer_has_active_interactive_session, service_peer_identity, service_peer_privileges, AuthenticatedPipePeer};
use crate::fleet_identity_handoff::{Handoff, VerifiedHandoff};
use rand::{rngs::SysRng, TryRng};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Mutex, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use windows_sys::Win32::{Foundation::CloseHandle,
    System::RemoteDesktop::{WTSQueryUserToken, WTSSendMessageW, WTS_CURRENT_SERVER_HANDLE},
    UI::WindowsAndMessaging::{IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO}};

const READY: bool = cfg!(feature="fleet-recovery-lab");

pub(super) struct NativeApproval {
    sid: String,
    session: u32,
    logon: (u32, i32),
    digest: [u8;32],
    fingerprint: [u8;32],
}

struct Grant { approval: NativeApproval, expires: Instant }
pub(super) struct ConsumedApproval { approval: NativeApproval, expires: Instant }
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StopOwner { sid:String,session:u32,logon:(u32,i32),pid:u32,process_created:u64,token_sha256:String }
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StopRequest { org_id:String,device_id:String,old_generation:String,recovery_id:String,decision_digest:String,state:String,owner:Option<StopOwner> }
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWorkLedger { org_id:String,device_id:String,generation:String,#[serde(default)] baseline_reviewed:bool,entries:Vec<LegacyWorkEntry> }
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWorkEntry { kind:String,work_id:String,owner:StopOwner,state:String }
const LEGACY_ACTIVE_LIMIT:usize=4096;
const LEGACY_COMPLETED_RETRY_LIMIT:usize=256;
static STOP_LOCK:Mutex<()>=Mutex::new(());
// Enable only after every old-generation server mutation is fenced and the
// installed-agent upgrade boundary and point-in-time archive are verified.
const UNKNOWN_STOP_REVIEW_READY:bool=cfg!(feature="fleet-recovery-lab");
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UnknownStopReview { org_id:String,device_id:String,generation:String,recovery_id:String,decision_digest:String,
    candidate_fingerprint:String,actor_sid:String,reviewed_at:i64,outcome:String,#[serde(default)] observed_stop_state:String }
const UPGRADE_RECOVERY_PROTOCOL:u32=1;
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UpgradeBootRecord { org_id:String,device_id:String,generation:String,recovery_id:String,decision_digest:String,
    service_build:String,pro_protocol:u32,prior_boot_sha256:String,actor_sid:String,prior_effects:String }
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UpgradeBootProof { record_sha256:String,new_boot_sha256:String,service_build:String,pro_protocol:u32,
    pro_image_sha256:String,pro_pid:u32,pro_process_created:u64,prior_effects:String }
struct UpgradeChallenge { device:String,recovery:String,generation:String,boot_sha256:String,owner:AuthenticatedExecutionPeer,expires:Instant }
static UPGRADE_CHALLENGES:std::sync::OnceLock<Mutex<HashMap<[u8;32],UpgradeChallenge>>>=std::sync::OnceLock::new();
fn valid_upgrade_challenge(pending:&UpgradeChallenge,owner:&AuthenticatedExecutionPeer,trust:&crate::fleet_machine_policy::FleetTrust,
    device:&str,recovery:&str,boot_sha256:&str)->bool {
    pending.expires>Instant::now() && pending.device==device && pending.device==trust.device_id && pending.recovery==recovery
        && pending.generation==trust.enrollment_generation && pending.boot_sha256==boot_sha256
        && pending.owner.sid==owner.sid && pending.owner.session==owner.session && pending.owner.logon==owner.logon
        && pending.owner.pid==owner.pid && pending.owner.process_created==owner.process_created
}

fn verified_upgrade_record(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust,
    recovery:&str,boot:&[u8;32])->Result<(UpgradeBootRecord,Vec<u8>,String),&'static str> {
    let id=uuid::Uuid::parse_str(recovery).map_err(|_| "upgrade witness unavailable")?;
    if id.is_nil() || id.to_string()!=recovery || boot.iter().all(|b|*b==0) { return Err("upgrade witness unavailable"); }
    let bytes=fs.read(&dir.join(format!("upgrade-boot-{id}.json"))).map_err(|_| "upgrade witness unavailable")?;
    if bytes.len()>16384 { return Err("upgrade witness unavailable"); }
    let record:UpgradeBootRecord=serde_json::from_slice(&bytes).map_err(|_| "upgrade witness unavailable")?;
    let current=lower_hex(&Sha256::digest(boot));
    if record.org_id!=trust.org_id || record.device_id!=trust.device_id || record.generation!=trust.enrollment_generation
        || record.recovery_id!=recovery || !valid_hash(&record.decision_digest) || !valid_hash(&record.prior_boot_sha256)
        || record.prior_boot_sha256==current || record.service_build!=env!("CARGO_PKG_VERSION")
        || record.pro_protocol!=UPGRADE_RECOVERY_PROTOCOL || record.prior_effects!="unknown" || !record.actor_sid.starts_with("S-1-") {
        return Err("restart required");
    }
    Ok((record,bytes,current))
}
pub(super) fn issue_upgrade_challenge(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,peer:&AuthenticatedPipePeer,device:&str,recovery:&str)->Result<String,&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    let owner=execution_peer(peer)?;let boot=crate::managed_deployment::registry::recovery_boot_token()?;
    store.with_handoff_trust(|trust| {
        if device!=trust.device_id { return Err("upgrade witness unavailable"); }
        let (_,_,boot_sha256)=verified_upgrade_record(fs,dir,trust,recovery,&boot)?;
        let mut token=zeroize::Zeroizing::new([0u8;32]);SysRng.try_fill_bytes(token.as_mut()).map_err(|_| "randomness unavailable")?;
        let digest:[u8;32]=Sha256::digest(token.as_slice()).into();
        let mut pending=UPGRADE_CHALLENGES.get_or_init(||Mutex::new(HashMap::new())).lock().map_err(|_| "upgrade witness unavailable")?;
        pending.retain(|_,value|value.expires>Instant::now());if pending.len()>=64 || pending.contains_key(&digest) { return Err("upgrade witness unavailable"); }
        pending.insert(digest,UpgradeChallenge { device:device.into(),recovery:recovery.into(),generation:trust.enrollment_generation.clone(),boot_sha256,
            owner,expires:Instant::now()+Duration::from_secs(60) });
        Ok(STANDARD.encode(token.as_slice()))
    })
}
pub(super) fn confirm_upgrade_challenge(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,peer:&AuthenticatedPipePeer,device:&str,recovery:&str,encoded:&str,protocol:u32)->Result<(),&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    use std::{io::Read,os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    if protocol!=UPGRADE_RECOVERY_PROTOCOL { return Err("upgrade witness unavailable"); }
    let raw=zeroize::Zeroizing::new(STANDARD.decode(encoded).map_err(|_| "upgrade witness unavailable")?);
    if raw.len()!=32 || STANDARD.encode(raw.as_slice())!=encoded { return Err("upgrade witness unavailable"); }
    let digest:[u8;32]=Sha256::digest(raw.as_slice()).into();let owner=execution_peer(peer)?;
    let boot=crate::managed_deployment::registry::recovery_boot_token()?;
    store.with_handoff_trust(|trust| {
        let pending=UPGRADE_CHALLENGES.get_or_init(||Mutex::new(HashMap::new())).lock().map_err(|_| "upgrade witness unavailable")?
            .remove(&digest).ok_or("upgrade witness unavailable")?;
        let (record,bytes,boot_sha256)=verified_upgrade_record(fs,dir,trust,recovery,&boot)?;
        if !valid_upgrade_challenge(&pending,&owner,trust,device,recovery,&boot_sha256) { return Err("upgrade witness unavailable"); }
        let path=crate::pro_broker::fixed_pro_path();crate::peer_auth_deployment_acl::verify_path(&path).map_err(|_| "upgrade witness unavailable")?;
        let mut file=std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(path).map_err(|_| "upgrade witness unavailable")?;
        let mut hash=Sha256::new();let mut buf=[0u8;65536];loop { let n=file.read(&mut buf).map_err(|_| "upgrade witness unavailable")?;if n==0 {break;}hash.update(&buf[..n]); }
        let proof=UpgradeBootProof { record_sha256:lower_hex(&Sha256::digest(&bytes)),new_boot_sha256:boot_sha256,
            service_build:record.service_build,pro_protocol:record.pro_protocol,pro_image_sha256:lower_hex(&hash.finalize()),
            pro_pid:owner.pid,pro_process_created:owner.process_created,prior_effects:"unknown".into() };
        let path=dir.join(format!("upgrade-proof-{}-{}.json",recovery,proof.new_boot_sha256));let encoded=serde_json::to_vec(&proof).map_err(|_| "upgrade witness unavailable")?;
        match fs.read(&path) {
            Ok(bytes) if bytes.len()<=16384=>{
                let previous:UpgradeBootProof=serde_json::from_slice(&bytes).map_err(|_| "upgrade witness unavailable")?;
                if previous.record_sha256!=proof.record_sha256 || previous.new_boot_sha256!=proof.new_boot_sha256
                    || previous.service_build!=proof.service_build || previous.pro_protocol!=proof.pro_protocol || previous.pro_image_sha256!=proof.pro_image_sha256
                    || previous.prior_effects!="unknown" || previous.pro_pid==0 || previous.pro_process_created==0 { return Err("upgrade witness changed"); }
            },
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
                fs.atomic_write(&path,&encoded).map_err(|_| "upgrade witness unavailable")?;
                if fs.read(&path).map_err(|_| "upgrade witness unavailable")?!=encoded { return Err("upgrade witness unavailable"); }
            },
            _=>return Err("upgrade witness unavailable"),
        } Ok(())
    })
}

fn prepare_upgrade_boundary(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff,approval:&ConsumedApproval,authorized:bool,boot:&[u8;32])->Result<(),&'static str> {
    if !authorized || approval.expires<=Instant::now() || approval.approval.digest!=*verified.digest()
        || approval.approval.fingerprint!=*verified.fingerprint() || boot.iter().all(|b|*b==0) { return Err("upgrade authorization unavailable"); }
    fs.ensure_dir_secure(dir).map_err(|_| "upgrade witness unavailable")?;
    let record=UpgradeBootRecord { org_id:trust.org_id.clone(),device_id:trust.device_id.clone(),generation:trust.enrollment_generation.clone(),
        recovery_id:verified.retirement_id().into(),decision_digest:lower_hex(verified.decision_digest()),service_build:env!("CARGO_PKG_VERSION").into(),
        pro_protocol:UPGRADE_RECOVERY_PROTOCOL,prior_boot_sha256:lower_hex(&Sha256::digest(boot)),actor_sid:approval.approval.sid.clone(),prior_effects:"unknown".into() };
    let path=dir.join(format!("upgrade-boot-{}.json",verified.retirement_id()));let bytes=serde_json::to_vec(&record).map_err(|_| "upgrade witness unavailable")?;
    match fs.read(&path) {
        Ok(existing) if existing.len()<=16384=>{
            let previous:UpgradeBootRecord=serde_json::from_slice(&existing).map_err(|_| "upgrade witness unavailable")?;
            if previous.org_id!=record.org_id || previous.device_id!=record.device_id || previous.generation!=record.generation
                || previous.recovery_id!=record.recovery_id || previous.decision_digest!=record.decision_digest || previous.service_build!=record.service_build
                || previous.pro_protocol!=record.pro_protocol || previous.prior_boot_sha256!=record.prior_boot_sha256
                || previous.prior_effects!="unknown" || !previous.actor_sid.starts_with("S-1-") { return Err("upgrade witness changed"); }
            Ok(())
        },
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
            fs.atomic_write(&path,&bytes).map_err(|_| "upgrade witness unavailable")?;
            if fs.read(&path).map_err(|_| "upgrade witness unavailable")?!=bytes { return Err("upgrade witness unavailable"); } Ok(())
        },
        _=>Err("upgrade witness changed"),
    }
}

/// The callback must perform the fresh service challenge against the exact
/// protected installed Pro process and return its image hash and process identity.
fn complete_upgrade_boundary(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff,approval:&ConsumedApproval,authorized:bool,boot:&[u8;32],
    challenge:impl FnOnce(u32)->Result<(String,u32,u64),&'static str>)->Result<(),&'static str> {
    if !authorized || approval.expires<=Instant::now() || approval.approval.digest!=*verified.digest()
        || approval.approval.fingerprint!=*verified.fingerprint() || boot.iter().all(|b|*b==0) { return Err("upgrade authorization unavailable"); }
    let path=dir.join(format!("upgrade-boot-{}.json",verified.retirement_id()));
    let bytes=fs.read(&path).map_err(|_| "upgrade witness unavailable")?;
    if bytes.len()>16384 { return Err("upgrade witness unavailable"); }
    let record:UpgradeBootRecord=serde_json::from_slice(&bytes).map_err(|_| "upgrade witness unavailable")?;
    let current=lower_hex(&Sha256::digest(boot));
    if record.org_id!=trust.org_id || record.device_id!=trust.device_id || record.generation!=trust.enrollment_generation
        || record.recovery_id!=verified.retirement_id() || record.decision_digest!=lower_hex(verified.decision_digest())
        || record.service_build!=env!("CARGO_PKG_VERSION") || record.pro_protocol!=UPGRADE_RECOVERY_PROTOCOL
        || !valid_hash(&record.prior_boot_sha256) || record.prior_boot_sha256==current || record.prior_effects!="unknown"
        || !record.actor_sid.starts_with("S-1-") { return Err("restart required"); }
    let (image,pid,created)=challenge(record.pro_protocol)?;
    if !valid_hash(&image) || pid==0 || created==0 { return Err("upgrade witness unavailable"); }
    let proof=UpgradeBootProof { record_sha256:lower_hex(&Sha256::digest(&bytes)),new_boot_sha256:current,
        service_build:record.service_build,pro_protocol:record.pro_protocol,pro_image_sha256:image,pro_pid:pid,pro_process_created:created,prior_effects:"unknown".into() };
    let proof_path=dir.join(format!("upgrade-proof-{}-{}.json",verified.retirement_id(),proof.new_boot_sha256));let encoded=serde_json::to_vec(&proof).map_err(|_| "upgrade witness unavailable")?;
    match fs.read(&proof_path) {
        Ok(existing) if existing.len()<=16384=>{
            let old:UpgradeBootProof=serde_json::from_slice(&existing).map_err(|_| "upgrade witness unavailable")?;
            if old.record_sha256!=proof.record_sha256 || old.new_boot_sha256!=proof.new_boot_sha256 || old.service_build!=proof.service_build
                || old.pro_protocol!=proof.pro_protocol || old.pro_image_sha256!=proof.pro_image_sha256 || old.pro_pid==0
                || old.pro_process_created==0 || old.prior_effects!="unknown" { return Err("upgrade witness changed"); }
            Ok(())
        },
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{ fs.atomic_write(&proof_path,&encoded).map_err(|_| "upgrade witness unavailable")?;
            if fs.read(&proof_path).map_err(|_| "upgrade witness unavailable")?!=encoded { return Err("upgrade witness unavailable"); } Ok(()) },
        _=>Err("upgrade witness changed"),
    }
}

/// Unmounted wrappers: prepare before Restart, then require a freshly
/// authenticated challenge to the protected installed Pro after Restart.
fn prepare_upgrade_before_restart(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,handoff:&Handoff,verified:&VerifiedHandoff,key:&[u8;32],approval:&ConsumedApproval,witness:&SessionWitness)->Result<(),&'static str> {
    let boot=crate::managed_deployment::registry::recovery_boot_token()?;
    store.with_handoff_trust(|trust| {
        let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
        let fresh=handoff.verify(trust,verified.retirement_id(),verified.new_generation(),key,now)?;
        if fresh.digest()!=verified.digest() || fresh.fingerprint()!=verified.fingerprint() { return Err("upgrade authorization changed"); }
        let authorized=witness.still_authorized() && approval.approval.sid==witness.sid && approval.approval.session==witness.session && approval.approval.logon==witness.logon;
        prepare_upgrade_boundary(fs,dir,trust,&fresh,approval,authorized,&boot)
    })
}
fn verify_upgrade_after_restart(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,handoff:&Handoff,verified:&VerifiedHandoff,key:&[u8;32],approval:&ConsumedApproval,witness:&SessionWitness,
    challenge:impl FnOnce(u32)->Result<(String,u32,u64),&'static str>)->Result<(),&'static str> {
    let boot=crate::managed_deployment::registry::recovery_boot_token()?;
    store.with_handoff_trust(|trust| {
        let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
        let fresh=handoff.verify(trust,verified.retirement_id(),verified.new_generation(),key,now)?;
        if fresh.digest()!=verified.digest() || fresh.fingerprint()!=verified.fingerprint() { return Err("upgrade authorization changed"); }
        let authorized=witness.still_authorized() && approval.approval.sid==witness.sid && approval.approval.session==witness.session && approval.approval.logon==witness.logon;
        complete_upgrade_boundary(fs,dir,trust,&fresh,approval,authorized,&boot,challenge)
    })
}

pub(super) fn upgrade_restart_required(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,handoff:&Handoff,verified:&VerifiedHandoff,key:&[u8;32],approval:&ConsumedApproval,witness:&SessionWitness)->Result<bool,&'static str> {
    let boot=crate::managed_deployment::registry::recovery_boot_token()?;
    store.with_handoff_trust(|trust| {
        let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
        let fresh=handoff.verify(trust,verified.retirement_id(),verified.new_generation(),key,now)?;
        if fresh.digest()!=verified.digest() || fresh.fingerprint()!=verified.fingerprint() { return Err("upgrade authorization changed"); }
        let authorized=witness.still_authorized() && approval.approval.sid==witness.sid && approval.approval.session==witness.session && approval.approval.logon==witness.logon;
        if !authorized || approval.expires<=Instant::now() { return Err("upgrade authorization unavailable"); }
        let path=dir.join(format!("upgrade-boot-{}.json",verified.retirement_id()));
        match fs.read(&path) {
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{ prepare_upgrade_boundary(fs,dir,trust,&fresh,approval,true,&boot)?;Ok(true) },
            Ok(bytes) if bytes.len()<=16384=>{
                let record:UpgradeBootRecord=serde_json::from_slice(&bytes).map_err(|_| "upgrade witness unavailable")?;
                if record.org_id!=trust.org_id || record.device_id!=trust.device_id || record.generation!=trust.enrollment_generation
                    || record.recovery_id!=fresh.retirement_id() || record.decision_digest!=lower_hex(fresh.decision_digest())
                    || record.service_build!=env!("CARGO_PKG_VERSION") || record.pro_protocol!=UPGRADE_RECOVERY_PROTOCOL
                    || record.prior_effects!="unknown" || !valid_hash(&record.prior_boot_sha256) || !record.actor_sid.starts_with("S-1-") {
                    return Err("upgrade witness changed");
                }
                Ok(record.prior_boot_sha256==lower_hex(&Sha256::digest(boot)))
            },
            _=>Err("upgrade witness unavailable"),
        }
    })
}

pub(super) async fn probe_upgrade_after_restart(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,handoff:&Handoff,verified:&VerifiedHandoff,key:&[u8;32],approval:&ConsumedApproval,witness:&SessionWitness)->Result<(),&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    use wincmd_shared::vault_access::VaultPresentation;
    if !witness.still_authorized() || approval.expires<=Instant::now() { return Err("upgrade authorization unavailable"); }
    let mut token=zeroize::Zeroizing::new([0u8;32]);SysRng.try_fill_bytes(token.as_mut()).map_err(|_| "randomness unavailable")?;
    let encoded=zeroize::Zeroizing::new(STANDARD.encode(token.as_slice()));let mut random=[0u8;8];SysRng.try_fill_bytes(&mut random).map_err(|_| "randomness unavailable")?;
    let result=crate::pro_broker::vault_call(crate::pro_broker::VaultCall { request_id:u64::from_le_bytes(random),target_session_id:0,
        caller_sid:"S-1-5-18",caller_token:None,caller_authentication_id:None,presentation:VaultPresentation::Machine,
        feature_id:"fleet.recovery.upgrade_probe",args:serde_json::json!({"challenge":encoded.as_str(),"protocol_version":UPGRADE_RECOVERY_PROTOCOL}) })
        .await.map_err(|_| "upgrade witness unavailable")?;
    let (image,pid,created)=verify_upgrade_probe_reply(result,encoded.as_str())?;
    if !witness.still_authorized() { return Err("upgrade witness unavailable"); }
    verify_upgrade_after_restart(store,fs,dir,handoff,verified,key,approval,witness,|protocol| {
        if protocol!=UPGRADE_RECOVERY_PROTOCOL { return Err("upgrade witness unavailable"); }
        Ok((image,pid,created))
    })
}
fn verify_upgrade_probe_reply(value:serde_json::Value,expected:&str)->Result<(String,u32,u64),&'static str> {
    #[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct BrokerResult { response:ProbeResponse,image_sha256:String,pid:u32,process_created:u64 }
    #[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct ProbeResponse { challenge:String,protocol_version:u32 }
    let mut result:BrokerResult=serde_json::from_value(value).map_err(|_| "upgrade witness unavailable")?;
    use subtle::ConstantTimeEq;
    let valid=result.response.protocol_version==UPGRADE_RECOVERY_PROTOCOL && result.response.challenge.len()==expected.len()
        && bool::from(result.response.challenge.as_bytes().ct_eq(expected.as_bytes()));
    use zeroize::Zeroize;result.response.challenge.zeroize();
    if !valid || !valid_hash(&result.image_sha256) || result.pid==0 || result.process_created==0 { return Err("upgrade witness unavailable"); }
    Ok((result.image_sha256,result.pid,result.process_created))
}
#[derive(Default)]
pub(super) struct GrantStore { grants: Mutex<HashMap<[u8;32], Grant>> }

impl GrantStore {
    /// The broker key must be freshly authenticated independently of IPC
    /// input. Trust is held while the approval is checked and grant is minted.
    fn issue(&self, store: &crate::fleet_machine_policy::FleetMachinePolicy, handoff: &Handoff,
        initial: &VerifiedHandoff, local_key: &[u8;32], peer: &AuthenticatedPipePeer,
        approval: NativeApproval) -> Result<[u8;32], &'static str> {
        store.with_handoff_trust(|trust| {
            let wall = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs();
            let now = i64::try_from(wall).map_err(|_| "clock unavailable")?;
            let verified = handoff.verify(trust, initial.retirement_id(), initial.new_generation(), local_key, now)?;
            if !session_matches(peer) || approval.sid != peer.caller_sid() || approval.session != peer.session_id()
                || approval.logon != peer.authentication_id() || approval.digest != *verified.digest()
                || approval.fingerprint != *verified.fingerprint() || initial.digest() != verified.digest()
                || initial.fingerprint() != verified.fingerprint() { return Err("handoff confirmation changed"); }
            let mut grants = self.grants.lock().map_err(|_| "handoff authorization unavailable")?;
            let now = Instant::now();
            grants.retain(|_, grant| grant.expires > now);
            if grants.len() >= 64 { return Err("handoff authorization unavailable"); }
            let mut token=[0;32]; SysRng.try_fill_bytes(&mut token).map_err(|_| "randomness unavailable")?;
            let hash: [u8;32] = Sha256::digest(token).into();
            if grants.contains_key(&hash) { return Err("handoff authorization unavailable"); }
            grants.insert(hash, Grant { approval, expires: now + Duration::from_secs(120) });
            Ok(token)
        })
    }

    fn consume(&self, store: &crate::fleet_machine_policy::FleetMachinePolicy, handoff: &Handoff,
        initial: &VerifiedHandoff, local_key: &[u8;32], peer: &AuthenticatedPipePeer,
        token: &[u8;32]) -> Result<ConsumedApproval, &'static str> {
        store.with_handoff_trust(|trust| {
            let wall = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs();
            let now = i64::try_from(wall).map_err(|_| "clock unavailable")?;
            let verified = handoff.verify(trust, initial.retirement_id(), initial.new_generation(), local_key, now)?;
            let mut grants = self.grants.lock().map_err(|_| "handoff authorization unavailable")?;
            let hash: [u8;32] = Sha256::digest(token).into();
            let grant = grants.remove(&hash).ok_or("handoff authorization unavailable")?;
            if grant.expires <= Instant::now() || !session_matches(peer)
                || grant.approval.sid != peer.caller_sid() || grant.approval.session != peer.session_id()
                || grant.approval.logon != peer.authentication_id() || grant.approval.digest != *verified.digest()
                || grant.approval.fingerprint != *verified.fingerprint() || initial.digest() != verified.digest()
                || initial.fingerprint() != verified.fingerprint() { return Err("handoff authorization changed"); }
            Ok(ConsumedApproval { approval:grant.approval, expires:grant.expires })
        })
    }
}

pub(super) fn authorize_recovery(store:&crate::fleet_machine_policy::FleetMachinePolicy, handoff:&Handoff,
    verified:&VerifiedHandoff, local_key:&[u8;32], peer:&AuthenticatedPipePeer)
    -> Result<(ConsumedApproval,SessionWitness),&'static str> {
    static GRANTS:std::sync::OnceLock<GrantStore>=std::sync::OnceLock::new();
    let witness=SessionWitness::capture(peer).ok_or("interactive administrator required")?;
    let approval=confirm(peer,verified)?;
    let grants=GRANTS.get_or_init(GrantStore::default);
    let token=grants.issue(store,handoff,verified,local_key,peer,approval)?;
    let consumed=grants.consume(store,handoff,verified,local_key,peer,&token)?;
    Ok((consumed,witness))
}

fn begin_stop_request(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff,approval:&ConsumedApproval,previous_commit:Option<(&str,&crate::fleet_machine_policy::FleetTrust,&str,&str)>)->Result<(),&'static str> {
    if approval.expires<=Instant::now() || approval.approval.digest!=*verified.digest()
        || approval.approval.fingerprint!=*verified.fingerprint() { return Err("stop authorization unavailable"); }
    fs.ensure_dir_secure(dir).map_err(|_| "stop request unavailable")?;
    let _guard=STOP_LOCK.lock().map_err(|_| "stop request unavailable")?; let path=dir.join("recovery-stop-request.json");
    let wanted=StopRequest { org_id:trust.org_id.clone(),device_id:trust.device_id.clone(),old_generation:trust.enrollment_generation.clone(),
        recovery_id:verified.retirement_id().into(),decision_digest:lower_hex(verified.decision_digest()),state:"requested".into(),owner:None };
    match fs.read(&path) {
        Ok(bytes)=> { let previous:StopRequest=serde_json::from_slice(&bytes).map_err(|_| "stop request unavailable")?;
            if previous.org_id==wanted.org_id && previous.device_id==wanted.device_id && previous.old_generation==wanted.old_generation
                && previous.recovery_id==wanted.recovery_id && previous.decision_digest==wanted.decision_digest
                && matches!(previous.state.as_str(),"requested"|"unknown"|"joined") { return Ok(()); }
            archive_previous_stop(fs,dir,trust,&previous,&bytes,previous_commit)?;
            fs.atomic_write(&path,&serde_json::to_vec(&wanted).map_err(|_| "stop request unavailable")?)
                .map_err(|_| "stop request unavailable")?;
        },
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>fs.atomic_write(&path,&serde_json::to_vec(&wanted).map_err(|_| "stop request unavailable")?)
            .map_err(|_| "stop request unavailable")?,
        Err(_)=>return Err("stop request unavailable"),
    }
    Ok(())
}
fn archive_previous_stop(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust,
    previous:&StopRequest,bytes:&[u8],commit:Option<(&str,&crate::fleet_machine_policy::FleetTrust,&str,&str)>)->Result<(),&'static str> {
    let (recovery,old,decision,fingerprint)=commit.ok_or("stop request changed")?;
    if bytes.len()>16384 || !matches!(previous.state.as_str(),"joined"|"unknown"|"requested")
        || (matches!(previous.state.as_str(),"joined"|"unknown") && previous.owner.is_none())
        || previous.recovery_id!=recovery || previous.decision_digest!=decision
        || previous.org_id!=trust.org_id || previous.device_id!=trust.device_id
        || previous.org_id!=old.org_id || previous.device_id!=old.device_id || previous.old_generation!=old.enrollment_generation
        || uuid::Uuid::parse_str(recovery).ok().is_none_or(|id|id.is_nil() || id.to_string()!=recovery)
        || !valid_hash(decision) || !valid_hash(fingerprint) { return Err("stop request changed"); }
    if previous.state!="joined" {
        let review_bytes=fs.read(&dir.join(format!("unknown-stop-review-{recovery}.json"))).map_err(|_| "stop review unavailable")?;
        if review_bytes.len()>16384 { return Err("stop review unavailable"); }
        let review:UnknownStopReview=serde_json::from_slice(&review_bytes).map_err(|_| "stop review unavailable")?;
        if review.org_id!=old.org_id || review.device_id!=old.device_id || review.generation!=old.enrollment_generation
            || review.recovery_id!=recovery || review.decision_digest!=decision || review.candidate_fingerprint!=fingerprint
            || review.outcome!="unknown_acknowledged" || !review.actor_sid.starts_with("S-1-") || review.reviewed_at<=0
            || !matches!(review.observed_stop_state.as_str(),""|"requested"|"unknown") { return Err("stop review changed"); }
    }
    let archive=dir.join(format!("recovery-stop-archive-{recovery}.json"));
    match fs.read(&archive) {
        Ok(existing) if existing==bytes=>Ok(()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
            fs.atomic_write(&archive,bytes).map_err(|_| "stop request unavailable")?;
            if fs.read(&archive).map_err(|_| "stop request unavailable")? != bytes { return Err("stop request unavailable"); }
            Ok(())
        },
        _=>Err("stop request changed"),
    }
}
fn read_stop(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust)
    ->Result<Option<StopRequest>,&'static str> {
    let path=dir.join("recovery-stop-request.json");
    match fs.read(&path) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),
        Ok(bytes) if bytes.len()<=16384=> { let request:StopRequest=serde_json::from_slice(&bytes).map_err(|_| "stop request unavailable")?;
            if request.org_id!=trust.org_id || request.device_id!=trust.device_id || request.old_generation!=trust.enrollment_generation
                || uuid::Uuid::parse_str(&request.recovery_id).ok().is_none_or(|id|id.is_nil() || id.to_string()!=request.recovery_id)
                || !valid_hash(&request.decision_digest) || !matches!(request.state.as_str(),"requested"|"unknown"|"joined")
                || ((request.state=="joined" || request.state=="unknown") && request.owner.is_none()) { return Err("stop request unavailable"); }
            Ok(Some(request)) },
        _=>Err("stop request unavailable"),
    }
}
fn same_stop_owner(owner:&StopOwner,peer:&AuthenticatedExecutionPeer)->bool {
    owner.sid==peer.sid && owner.session==peer.session && owner.logon==peer.logon && owner.pid==peer.pid && owner.process_created==peer.process_created
}
fn legacy_work_path(dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust)->Result<std::path::PathBuf,&'static str> {
    let id=uuid::Uuid::parse_str(&trust.enrollment_generation).map_err(|_| "legacy work unavailable")?;
    if id.is_nil() || id.to_string()!=trust.enrollment_generation { return Err("legacy work unavailable"); }
    Ok(dir.join(format!("legacy-work-{id}.json")))
}
fn read_legacy_work(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust)->Result<LegacyWorkLedger,&'static str> {
    let path=legacy_work_path(dir,trust)?;
    let ledger=match fs.read(&path) {
        Ok(bytes) if bytes.len()<=2*1024*1024=>serde_json::from_slice::<LegacyWorkLedger>(&bytes).map_err(|_| "legacy work unavailable")?,
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>LegacyWorkLedger { org_id:trust.org_id.clone(),device_id:trust.device_id.clone(),generation:trust.enrollment_generation.clone(),baseline_reviewed:false,entries:Vec::new() },
        _=>return Err("legacy work unavailable"),
    };
    let mut ids=std::collections::HashSet::new();
    if ledger.org_id!=trust.org_id || ledger.device_id!=trust.device_id || ledger.generation!=trust.enrollment_generation
        || ledger.entries.len()>LEGACY_ACTIVE_LIMIT+LEGACY_COMPLETED_RETRY_LIMIT
        || ledger.entries.iter().filter(|entry|entry.state=="active").count()>LEGACY_ACTIVE_LIMIT
        || ledger.entries.iter().any(|entry|!matches!(entry.kind.as_str(),"command"|"search") || entry.work_id.is_empty() || entry.work_id.len()>256
            || !matches!(entry.state.as_str(),"active"|"complete") || !valid_hash(&entry.owner.token_sha256)
            || !ids.insert((&entry.kind,&entry.work_id))) { return Err("legacy work unavailable"); }
    Ok(ledger)
}
fn start_legacy_work(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust,
    device:&str,kind:&str,work_id:&str,peer:&AuthenticatedExecutionPeer)->Result<String,&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    if device!=trust.device_id || !matches!(kind,"command"|"search") || work_id.is_empty() || work_id.len()>256 { return Err("legacy work unavailable"); }
    fs.ensure_dir_secure(dir).map_err(|_| "legacy work unavailable")?;
    let _guard=STOP_LOCK.lock().map_err(|_| "legacy work unavailable")?;
    if read_stop(fs,dir,trust)?.is_some() { return Err("recovery stop requested"); }
    let mut ledger=read_legacy_work(fs,dir,trust)?;
    if ledger.entries.iter().filter(|entry|entry.state=="active").count()>=LEGACY_ACTIVE_LIMIT
        || ledger.entries.iter().any(|entry|entry.state=="active" && entry.kind==kind && entry.work_id==work_id) { return Err("legacy work unavailable"); }
    // This ledger proves quiescence, not long-term execution deduplication.
    // A new delivery uses existing legacy semantics; unfinished work remains.
    ledger.entries.retain(|entry|entry.state!="complete" || entry.kind!=kind || entry.work_id!=work_id);
    while ledger.entries.iter().filter(|entry|entry.state=="complete").count()>LEGACY_COMPLETED_RETRY_LIMIT {
        let index=ledger.entries.iter().position(|entry|entry.state=="complete").ok_or("legacy work unavailable")?;
        ledger.entries.remove(index);
    }
    let mut token=zeroize::Zeroizing::new([0u8;32]);SysRng.try_fill_bytes(token.as_mut()).map_err(|_| "randomness unavailable")?;
    ledger.entries.push(LegacyWorkEntry { kind:kind.into(),work_id:work_id.into(),state:"active".into(),owner:StopOwner { sid:peer.sid.clone(),session:peer.session,
        logon:peer.logon,pid:peer.pid,process_created:peer.process_created,token_sha256:lower_hex(&Sha256::digest(token.as_slice())) } });
    fs.atomic_write(&legacy_work_path(dir,trust)?,&serde_json::to_vec(&ledger).map_err(|_| "legacy work unavailable")?).map_err(|_| "legacy work unavailable")?;
    Ok(STANDARD.encode(token.as_slice()))
}
fn complete_legacy_work(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust,
    device:&str,kind:&str,work_id:&str,token:&[u8;32],peer:&AuthenticatedExecutionPeer)->Result<(),&'static str> {
    use subtle::ConstantTimeEq;
    if device!=trust.device_id { return Err("legacy work unavailable"); }
    let _guard=STOP_LOCK.lock().map_err(|_| "legacy work unavailable")?;
    let mut ledger=read_legacy_work(fs,dir,trust)?;
    let entry=ledger.entries.iter_mut().find(|entry|entry.kind==kind && entry.work_id==work_id).ok_or("legacy work unavailable")?;
    let hash=lower_hex(&Sha256::digest(token));
    if !same_stop_owner(&entry.owner,peer) || !bool::from(entry.owner.token_sha256.as_bytes().ct_eq(hash.as_bytes())) { return Err("legacy work unavailable"); }
    if entry.state=="complete" { return Ok(()); }
    entry.state="complete".into();
    while ledger.entries.iter().filter(|entry|entry.state=="complete").count()>LEGACY_COMPLETED_RETRY_LIMIT {
        let index=ledger.entries.iter().position(|entry|entry.state=="complete").ok_or("legacy work unavailable")?;
        ledger.entries.remove(index);
    }
    fs.atomic_write(&legacy_work_path(dir,trust)?,&serde_json::to_vec(&ledger).map_err(|_| "legacy work unavailable")?).map_err(|_| "legacy work unavailable")
}
fn poll_stop_request(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust,
    device:&str,peer:&AuthenticatedExecutionPeer)->Result<serde_json::Value,&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    if device!=trust.device_id { return Err("stop request unavailable"); }
    let _guard=STOP_LOCK.lock().map_err(|_| "stop request unavailable")?;
    let Some(mut request)=read_stop(fs,dir,trust)? else { return Ok(serde_json::json!({"pending":false})); };
    if request.state=="joined" { return Ok(serde_json::json!({"pending":false})); }
    if request.owner.as_ref().is_some_and(|owner|!same_stop_owner(owner,peer)) { return Err("stop owner changed"); }
    let mut token=zeroize::Zeroizing::new([0u8;32]);SysRng.try_fill_bytes(token.as_mut()).map_err(|_| "randomness unavailable")?;
    request.owner=Some(StopOwner { sid:peer.sid.clone(),session:peer.session,logon:peer.logon,pid:peer.pid,
        process_created:peer.process_created,token_sha256:lower_hex(&Sha256::digest(token.as_slice())) });
    fs.atomic_write(&dir.join("recovery-stop-request.json"),&serde_json::to_vec(&request).map_err(|_| "stop request unavailable")?)
        .map_err(|_| "stop request unavailable")?;
    Ok(serde_json::json!({"pending":true,"device_id":request.device_id,"recovery_id":request.recovery_id,"token":STANDARD.encode(token.as_slice())}))
}
fn complete_stop_request(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust,
    device:&str,recovery:&str,token:&[u8;32],peer:&AuthenticatedExecutionPeer,state:&str)->Result<(),&'static str> {
    use subtle::ConstantTimeEq;
    if device!=trust.device_id || !matches!(state,"joined"|"unknown") { return Err("stop request unavailable"); }
    let _guard=STOP_LOCK.lock().map_err(|_| "stop request unavailable")?;
    let mut request=read_stop(fs,dir,trust)?.ok_or("stop request unavailable")?;
    let owner=request.owner.as_ref().ok_or("stop request unavailable")?;let hash=lower_hex(&Sha256::digest(token));
    if request.recovery_id!=recovery || !same_stop_owner(owner,peer) || owner.token_sha256.len()!=hash.len()
        || !bool::from(owner.token_sha256.as_bytes().ct_eq(hash.as_bytes())) || (request.state=="joined" && state!="joined") { return Err("stop request unavailable"); }
    if state=="joined" {
        let path=legacy_work_path(dir,trust)?;
        fs.read(&path).map_err(|_| "legacy work baseline unavailable")?;
        let ledger=read_legacy_work(fs,dir,trust)?;
        if !ledger.baseline_reviewed || ledger.entries.iter().any(|entry|entry.state=="active") { return Err("legacy work outcome unknown"); }
    }
    request.state=state.into();
    fs.atomic_write(&dir.join("recovery-stop-request.json"),&serde_json::to_vec(&request).map_err(|_| "stop request unavailable")?)
        .map_err(|_| "stop request unavailable")
}

pub(super) fn request_cooperative_stop(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,handoff:&Handoff,verified:&VerifiedHandoff,key:&[u8;32],approval:&ConsumedApproval)->Result<(),&'static str> {
    store.with_handoff_history(|trust,previous_commit| {
        let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
        let fresh=handoff.verify(trust,verified.retirement_id(),verified.new_generation(),key,now)?;
        if fresh.digest()!=verified.digest() || fresh.fingerprint()!=verified.fingerprint() { return Err("stop authorization changed"); }
        begin_stop_request(fs,dir,trust,&fresh,approval,previous_commit)
    })
}
fn initial_enrollment_stop_state(dir:&std::path::Path)->Result<serde_json::Value,&'static str> {
    use std::os::windows::fs::MetadataExt;
    // Recovery journals may have dynamically generated names. Only the
    // pending policy file may exist during an initial enrollment.
    let entries=std::fs::read_dir(dir).map_err(|_| "stop request unavailable")?;
    let mut saw_policy=false;
    for entry in entries {
        let entry=entry.map_err(|_| "stop request unavailable")?;
        if entry.file_name() != "fleet-machine-policy.json" { return Err("stop request unavailable"); }
        let metadata=std::fs::symlink_metadata(entry.path()).map_err(|_| "stop request unavailable")?;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 { return Err("stop request unavailable"); }
        std::fs::File::open(entry.path()).map_err(|_| "stop request unavailable")?;
        saw_policy=true;
    }
    if !saw_policy { return Err("stop request unavailable"); }
    Ok(serde_json::json!({"pending":false}))
}
pub(super) fn stop_poll_for_pro(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,peer:&AuthenticatedPipePeer,device:&str)->Result<serde_json::Value,&'static str> {
    let owner=execution_peer(peer)?;
    // No recovery authority may be silently ignored while provisioning.
    store.with_handoff_or_pending(device, |trust|poll_stop_request(fs,dir,trust,device,&owner), || initial_enrollment_stop_state(dir))
}
pub(super) fn legacy_work_start_for_pro(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,peer:&AuthenticatedPipePeer,device:&str,kind:&str,work_id:&str)->Result<String,&'static str> {
    let owner=execution_peer(peer)?;
    store.with_handoff_trust(|trust|start_legacy_work(fs,dir,trust,device,kind,work_id,&owner))
}
pub(super) fn legacy_work_complete_for_pro(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,peer:&AuthenticatedPipePeer,device:&str,kind:&str,work_id:&str,token:&str)->Result<(),&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    let owner=execution_peer(peer)?;let raw=zeroize::Zeroizing::new(STANDARD.decode(token).map_err(|_| "legacy work unavailable")?);
    if STANDARD.encode(raw.as_slice())!=token { return Err("legacy work unavailable"); }
    let token:zeroize::Zeroizing<[u8;32]>=zeroize::Zeroizing::new(raw.as_slice().try_into().map_err(|_| "legacy work unavailable")?);
    store.with_handoff_trust(|trust|complete_legacy_work(fs,dir,trust,device,kind,work_id,&token,&owner))
}
pub(super) fn stop_complete_for_pro(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,peer:&AuthenticatedPipePeer,device:&str,recovery:&str,token:&str,state:&str)->Result<(),&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    let owner=execution_peer(peer)?;
    let raw=zeroize::Zeroizing::new(STANDARD.decode(token).map_err(|_| "stop request unavailable")?);
    if STANDARD.encode(raw.as_slice())!=token { return Err("stop request unavailable"); }
    let token:zeroize::Zeroizing<[u8;32]>=zeroize::Zeroizing::new(raw.as_slice().try_into().map_err(|_| "stop request unavailable")?);
    store.with_handoff_trust(|trust|complete_stop_request(fs,dir,trust,device,recovery,&token,&owner,state))
}
pub(super) async fn await_cooperative_stop(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,verified:&VerifiedHandoff,approval:&ConsumedApproval,witness:&SessionWitness)->Result<(),&'static str> {
    let wait=async {
        loop {
            if !witness.still_authorized() || approval.expires<=Instant::now() { return Err("stop authorization changed"); }
            let state=store.with_handoff_trust(|trust| {
                let _guard=STOP_LOCK.lock().map_err(|_| "stop request unavailable")?;
                let request=read_stop(fs,dir,trust)?.ok_or("stop request unavailable")?;
                if request.recovery_id!=verified.retirement_id() || request.decision_digest!=lower_hex(verified.decision_digest()) { return Err("stop request changed"); }
                Ok(request.state)
            })?;
            if state=="joined" { return Ok(()); }
            if state=="unknown" {
                if !UNKNOWN_STOP_REVIEW_READY { return Err("stop outcome unknown"); }
                record_unknown_stop_review(store,fs,dir,verified,approval,witness)?;
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };
    match tokio::time::timeout(Duration::from_secs(45),wait).await {
        Ok(result)=>result,
        Err(_) if UNKNOWN_STOP_REVIEW_READY=>record_unknown_stop_review(store,fs,dir,verified,approval,witness),
        Err(_)=>Err("stop outcome unknown"),
    }
}

fn write_unknown_stop_review(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,trust:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff,approval:&ConsumedApproval,authorized:bool,now:i64)->Result<(),&'static str> {
    if !authorized || approval.expires<=Instant::now() || approval.approval.digest!=*verified.digest()
        || approval.approval.fingerprint!=*verified.fingerprint() { return Err("stop authorization changed"); }
    let _guard=STOP_LOCK.lock().map_err(|_| "stop request unavailable")?;
    let request=read_stop(fs,dir,trust)?.ok_or("stop request unavailable")?;
    if !matches!(request.state.as_str(),"requested"|"unknown") || (request.state=="unknown" && request.owner.is_none()) || request.recovery_id!=verified.retirement_id()
        || request.decision_digest!=lower_hex(verified.decision_digest()) { return Err("stop outcome changed"); }
    let wanted=UnknownStopReview { org_id:trust.org_id.clone(),device_id:trust.device_id.clone(),generation:trust.enrollment_generation.clone(),
        recovery_id:verified.retirement_id().into(),decision_digest:lower_hex(verified.decision_digest()),candidate_fingerprint:lower_hex(verified.fingerprint()),
        actor_sid:approval.approval.sid.clone(),reviewed_at:now,outcome:"unknown_acknowledged".into(),observed_stop_state:request.state.clone() };
    let path=dir.join(format!("unknown-stop-review-{}.json",verified.retirement_id()));
    match fs.read(&path) {
        Ok(bytes) if bytes.len()<=16384=>{
            let old:UnknownStopReview=serde_json::from_slice(&bytes).map_err(|_| "stop review unavailable")?;
            if old.org_id!=wanted.org_id || old.device_id!=wanted.device_id || old.generation!=wanted.generation
                || old.recovery_id!=wanted.recovery_id || old.decision_digest!=wanted.decision_digest
                || old.candidate_fingerprint!=wanted.candidate_fingerprint || old.outcome!="unknown_acknowledged"
                || !old.actor_sid.starts_with("S-1-") || old.reviewed_at<=0
                || !matches!(old.observed_stop_state.as_str(),""|"requested"|"unknown") { return Err("stop review changed"); }
        },
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
            let bytes=serde_json::to_vec(&wanted).map_err(|_| "stop review unavailable")?;
            fs.atomic_write(&path,&bytes).map_err(|_| "stop review unavailable")?;
            if fs.read(&path).map_err(|_| "stop review unavailable")?!=bytes { return Err("stop review unavailable"); }
        },
        _=>return Err("stop review unavailable"),
    }
    Ok(())
}
fn record_unknown_stop_review(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,verified:&VerifiedHandoff,approval:&ConsumedApproval,witness:&SessionWitness)->Result<(),&'static str> {
    store.with_handoff_trust(|trust| {
        let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
        let authorized=witness.still_authorized() && approval.approval.sid==witness.sid && approval.approval.session==witness.session && approval.approval.logon==witness.logon;
        write_unknown_stop_review(fs,dir,trust,verified,approval,authorized,now)
    })
}

pub(super) fn session_matches(peer: &AuthenticatedPipePeer) -> bool {
    if peer.session_id() == 0 || !peer_has_active_interactive_session(peer)
        || !service_peer_privileges::token_is_local_administrator(peer.token()).unwrap_or(false) { return false; }
    let mut token = std::ptr::null_mut();
    if unsafe { WTSQueryUserToken(peer.session_id(), &mut token) } == 0 || token.is_null() { return false; }
    let valid = service_peer_identity::token_sid(token).as_deref() == Some(peer.caller_sid())
        && service_peer_identity::token_session_id(token) == Some(peer.session_id())
        && service_peer_identity::token_authentication_id(token) == Some(peer.authentication_id());
    unsafe { CloseHandle(token); }
    valid
}

pub(crate) struct SessionWitness { sid:String, session:u32, logon:(u32,i32) }
impl SessionWitness {
    pub(crate) fn readmission_binding(&self)->Result<[u8;32],&'static str> {
        use sha2::{Digest,Sha256};
        let bytes=serde_json::to_vec(&("wincommander:readmission-session:v1",&self.sid,self.session,self.logon)).map_err(|_|"session unavailable")?;
        Ok(Sha256::digest(bytes).into())
    }
    pub(super) fn capture(peer:&AuthenticatedPipePeer)->Option<Self> {
        session_matches(peer).then(||Self { sid:peer.caller_sid().into(), session:peer.session_id(), logon:peer.authentication_id() })
    }
    pub(crate) fn still_authorized(&self)->bool {
        if !super::session_is_active(self.session,super::wts_connect_state(self.session)) { return false; }
        let mut token=std::ptr::null_mut();
        if unsafe { WTSQueryUserToken(self.session,&mut token) }==0 || token.is_null() { return false; }
        let valid=service_peer_identity::token_sid(token).as_deref()==Some(self.sid.as_str())
            && service_peer_identity::token_session_id(token)==Some(self.session)
            && service_peer_identity::token_authentication_id(token)==Some(self.logon)
            && service_peer_privileges::token_is_local_administrator(token).unwrap_or(false);
        unsafe { CloseHandle(token); }
        valid
    }
}

fn message(id: &str, fingerprint: &[u8;32]) -> String {
    let fingerprint: String = fingerprint.iter().map(|b| format!("{b:02x}")).collect();
    format!("Approve this Fleet device identity handoff?\n\nRetirement ID: {id}\nReplacement key SHA-256: {fingerprint}\n\nCompare this fingerprint with the Fleet administrator. Previous work may still be running or have an unknown result. Its history will be preserved, but this action does not stop old processes or confirm their work finished. Continue only if you accept this uncertainty.")
}

/// Call only on a blocking worker after verifying protected trust and reading
/// the local public key through an authenticated broker. Never accept renderer
/// confirmation. Revalidate trust and broker result after this dialog.
pub(super) fn confirm(peer: &AuthenticatedPipePeer, verified: &VerifiedHandoff) -> Result<NativeApproval, &'static str> {
    if !READY { return Err("native handoff confirmation is unavailable"); }
    if !session_matches(peer) { return Err("interactive administrator required"); }
    let title: Vec<u16> = "WinCommander Fleet identity handoff".encode_utf16().chain(std::iter::once(0)).collect();
    let body: Vec<u16> = message(verified.retirement_id(), verified.fingerprint()).encode_utf16().chain(std::iter::once(0)).collect();
    let mut response = 0;
    let success = unsafe { WTSSendMessageW(WTS_CURRENT_SERVER_HANDLE, peer.session_id(),
        title.as_ptr(), ((title.len()-1)*2) as u32, body.as_ptr(), ((body.len()-1)*2) as u32,
        MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2, 60, &mut response, 1) } != 0;
    if !success || response != IDYES || !session_matches(peer) { return Err("handoff confirmation denied or unavailable"); }
    Ok(NativeApproval { sid:peer.caller_sid().to_owned(), session:peer.session_id(), logon:peer.authentication_id(),
        digest:*verified.digest(), fingerprint:*verified.fingerprint() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dialog_identifies_exact_retirement_and_fingerprint() {
        let text=message("11111111-1111-4111-8111-111111111111", &[7;32]);
        assert!(text.contains("11111111-1111-4111-8111-111111111111"));
        assert!(text.contains(&"07".repeat(32)));
        assert!(text.contains("does not stop old processes"));
        assert!(text.contains("unknown result"));
    }
    #[test]
    fn readmission_witness_binds_sid_session_and_logon() {
        let witness=SessionWitness { sid:"S-1-5-21-1".into(),session:1,logon:(2,3) };
        let binding=witness.readmission_binding().unwrap();
        for changed in [
            SessionWitness { sid:"S-1-5-21-2".into(),session:1,logon:(2,3) },
            SessionWitness { sid:"S-1-5-21-1".into(),session:2,logon:(2,3) },
            SessionWitness { sid:"S-1-5-21-1".into(),session:1,logon:(2,4) },
        ] { assert_ne!(binding,changed.readmission_binding().unwrap()); }
        let noninteractive=SessionWitness { sid:"S-1-5-21-1".into(),session:0,logon:(2,3) };
        assert!(!noninteractive.still_authorized());
    }
}

#[derive(serde::Serialize,serde::Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
struct ArchiveReceipt {
    status: String, recovery_id: String, archive_sha256: String, archived_actor_sid: String,
    journal_present: bool, journal_sha256: Option<String>,
}
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ReviewRecord {
    org_id: String, device_id: String, old_generation: String, recovery_id: String,
    new_generation: String, handoff_digest: String, decision_digest: String, candidate_fingerprint: String,
    archive_sha256: String, journal_present: bool, journal_sha256: Option<String>,
    archived_actor_sid: String, reviewing_actor_sid: String, outcome: String,
}
fn lower_hex(bytes:&[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }
fn valid_hash(value:&str) -> bool { value.len()==64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TransferredArchive {
    device_id:String, recovery_id:String, candidate_key:String, actor_sid:String,
    journal:Option<String>, ledger:Option<String>, unknown:bool,
}
impl Drop for TransferredArchive {
    fn drop(&mut self) { use zeroize::Zeroize; self.journal.zeroize(); self.ledger.zeroize(); }
}

/// Store exact archived bytes in service-owned, machine-protected storage.
/// The caller must authenticate the broker and retain its journal lease until
/// this function succeeds and the broker receives the matching acknowledgement.
fn persist_transferred_archive(fs:&dyn crate::policy_store::PolicyFs, dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector, trust:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff, receipt:&ArchiveReceipt, bytes:zeroize::Zeroizing<Vec<u8>>) -> Result<(), &'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    const LIMIT:usize=12*1024*1024;
    if bytes.len()>LIMIT || receipt.status!="archived_unknown" || receipt.recovery_id!=verified.retirement_id()
        || !receipt.archived_actor_sid.starts_with("S-1-") || receipt.archived_actor_sid.len()>184
        || receipt.archived_actor_sid.chars().any(char::is_control) || !valid_hash(&receipt.archive_sha256)
        || lower_hex(&Sha256::digest(bytes.as_slice()))!=receipt.archive_sha256 { return Err("invalid archive"); }
    let archive:TransferredArchive=serde_json::from_slice(&bytes).map_err(|_| "invalid archive")?;
    if archive.device_id!=trust.device_id || archive.recovery_id!=verified.retirement_id()
        || archive.candidate_key!=STANDARD.encode(verified.public_key()) || archive.actor_sid!=receipt.archived_actor_sid
        || !archive.unknown || archive.journal.is_some()!=receipt.journal_present { return Err("invalid archive"); }
    if let Some(journal)=&archive.journal {
        let decoded=zeroize::Zeroizing::new(STANDARD.decode(journal).map_err(|_| "invalid archive")?);
        if zeroize::Zeroizing::new(STANDARD.encode(decoded.as_slice())).as_str()!=journal || decoded.len()>8*1024*1024
            || receipt.journal_sha256.as_deref()!=Some(lower_hex(&Sha256::digest(decoded.as_slice())).as_str()) { return Err("invalid archive"); }
    } else if receipt.journal_sha256.is_some() { return Err("invalid archive"); }
    if let Some(ledger)=&archive.ledger {
        let decoded=zeroize::Zeroizing::new(STANDARD.decode(ledger).map_err(|_| "invalid archive")?);
        if decoded.len()>65536 || zeroize::Zeroizing::new(STANDARD.encode(decoded.as_slice())).as_str()!=ledger { return Err("invalid archive"); }
    }
    let entropy: [u8;32]=Sha256::digest(serde_json::to_vec(&("fleet.recovery.protected-archive.v1",&trust.org_id,
        &trust.device_id,verified.retirement_id(),verified.decision_digest(),&receipt.archive_sha256)).map_err(|_| "invalid archive")?).into();
    fs.ensure_dir_secure(dir).map_err(|_| "archive storage unavailable")?;
    let path=dir.join(format!("archive-{}.dpapi",verified.retirement_id()));
    static ARCHIVE_LOCK:Mutex<()>=Mutex::new(());
    let _guard=ARCHIVE_LOCK.lock().map_err(|_| "archive storage unavailable")?;
    match fs.read(&path) {
        Ok(sealed) => {
            let existing=protector.unprotect(&sealed,&entropy)?;
            if existing.as_slice()!=bytes.as_slice() { return Err("archive changed"); }
        },
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => {
            let sealed=protector.protect(bytes.as_slice(),&entropy)?;
            if sealed.is_empty() || sealed.len()>LIMIT+65536 { return Err("archive storage unavailable"); }
            fs.atomic_write(&path,&sealed).map_err(|_| "archive storage unavailable")?;
            let stored=fs.read(&path).map_err(|_| "archive storage unavailable")?;
            let verified_copy=protector.unprotect(&stored,&entropy)?;
            if verified_copy.as_slice()!=bytes.as_slice() { return Err("archive storage unavailable"); }
        },
        Err(_) => return Err("archive storage unavailable"),
    }
    Ok(())
}

struct SensitivePayload(serde_json::Value);
impl Drop for SensitivePayload { fn drop(&mut self) { wincmd_shared::zeroize_json(&mut self.0); } }

/// Process one authenticated broker notification; return an acknowledgement
/// only after exact bytes have been sealed and read back successfully.
fn receive_transferred_archive(payload:serde_json::Value, operation:u64, fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path, protector:&dyn crate::fleet_credential::Protector,
    trust:&crate::fleet_machine_policy::FleetTrust, verified:&VerifiedHandoff,
    authorized:impl Fn()->bool) -> Result<(ArchiveReceipt,serde_json::Value),&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    let mut payload=SensitivePayload(payload);
    let object=payload.0.as_object_mut().ok_or("invalid archive")?;
    if object.len()!=3 || object.remove("operation_id").and_then(|v|v.as_u64())!=Some(operation) { return Err("invalid archive"); }
    let receipt:ArchiveReceipt=serde_json::from_value(object.remove("receipt").ok_or("invalid archive")?).map_err(|_| "invalid archive")?;
    let encoded=match object.remove("archive_b64") { Some(serde_json::Value::String(value))=>zeroize::Zeroizing::new(value), _=>return Err("invalid archive") };
    if encoded.len()>16*1024*1024 || !authorized() { return Err("invalid archive"); }
    let bytes=zeroize::Zeroizing::new(STANDARD.decode(encoded.as_bytes()).map_err(|_| "invalid archive")?);
    if zeroize::Zeroizing::new(STANDARD.encode(bytes.as_slice())).as_str()!=encoded.as_str() { return Err("invalid archive"); }
    persist_transferred_archive(fs,dir,protector,trust,verified,&receipt,bytes)?;
    if !authorized() { return Err("administrator authorization changed"); }
    let ack=serde_json::json!({"phase":"fleet_archive","archive_sha256":receipt.archive_sha256,"stored":true});
    Ok((receipt,ack))
}

/// Called only after revalidating protected trust and consuming a native grant.
/// The consumed native approval records acceptance of unknown legacy work;
/// it does not assert that old processes have stopped.
fn persist_review(fs:&dyn crate::policy_store::PolicyFs, dir:&std::path::Path,
    trust:&crate::fleet_machine_policy::FleetTrust, verified:&VerifiedHandoff, consumed:ConsumedApproval,
    receipt:ArchiveReceipt) -> Result<(), &'static str> {
    let approval=consumed.approval;
    if consumed.expires<=Instant::now() || approval.digest!=*verified.digest()
        || approval.fingerprint!=*verified.fingerprint() || receipt.status!="archived_unknown"
        || receipt.recovery_id!=verified.retirement_id() || !valid_hash(&receipt.archive_sha256)
        || receipt.journal_present!=receipt.journal_sha256.is_some()
        || receipt.journal_sha256.as_ref().is_some_and(|hash| !valid_hash(hash))
        || !receipt.archived_actor_sid.starts_with("S-1-") || receipt.archived_actor_sid.len()>184
        || receipt.archived_actor_sid.chars().any(char::is_control)
        || !approval.sid.starts_with("S-1-") || approval.sid.len()>184 { return Err("invalid recovery review"); }
    fs.ensure_dir_secure(dir).map_err(|_| "recovery review unavailable")?;
    let path=dir.join(format!("review-{}.json",verified.retirement_id()));
    let record=ReviewRecord { org_id:trust.org_id.clone(), device_id:trust.device_id.clone(), old_generation:trust.enrollment_generation.clone(),
        recovery_id:verified.retirement_id().into(), new_generation:verified.new_generation().into(),
        handoff_digest:lower_hex(verified.digest()), decision_digest:lower_hex(verified.decision_digest()), candidate_fingerprint:lower_hex(verified.fingerprint()),
        archive_sha256:receipt.archive_sha256, journal_present:receipt.journal_present,
        journal_sha256:receipt.journal_sha256, archived_actor_sid:receipt.archived_actor_sid,
        reviewing_actor_sid:approval.sid, outcome:"unknown_acknowledged".into() };
    static REVIEW_LOCK: Mutex<()> = Mutex::new(());
    let _guard=REVIEW_LOCK.lock().map_err(|_| "recovery review unavailable")?;
    match fs.read(&path) {
        Ok(bytes) => {
            let mut prior:ReviewRecord=serde_json::from_slice(&bytes).map_err(|_| "recovery review unavailable")?;
            // A fresh proof and newly authorized reviewer may resume the same decision.
            prior.handoff_digest=record.handoff_digest.clone();
            prior.reviewing_actor_sid=record.reviewing_actor_sid.clone();
            if prior!=record { return Err("recovery review changed"); }
        },
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => {
            let bytes=serde_json::to_vec(&record).map_err(|_| "recovery review unavailable")?;
            fs.atomic_write(&path,&bytes).map_err(|_| "recovery review unavailable")?;
        },
        Err(_) => return Err("recovery review unavailable"),
    }
    // Each authorized retry has its own immutable event; a failed event write
    // can be retried without changing the original review record.
    let event=serde_json::to_vec(&("fleet.recovery.review-attempt.v1",&record.recovery_id,
        &record.reviewing_actor_sid,&record.handoff_digest,&record.decision_digest,&record.archive_sha256))
        .map_err(|_| "recovery review unavailable")?;
    let event_id=lower_hex(&Sha256::digest(&event));
    let event_path=dir.join(format!("review-attempt-{}-{event_id}.json",record.recovery_id));
    match fs.read(&event_path) {
        Ok(existing) if existing==event => Ok(()),
        Ok(_) => Err("recovery review changed"),
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => fs.atomic_write(&event_path,&event).map_err(|_| "recovery review unavailable"),
        Err(_) => Err("recovery review unavailable"),
    }
}

fn record_review(store:&crate::fleet_machine_policy::FleetMachinePolicy, fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path, handoff:&Handoff, initial:&VerifiedHandoff, local_key:&[u8;32],
    consumed:ConsumedApproval, receipt:ArchiveReceipt) -> Result<(), &'static str> {
    store.with_handoff_trust(|trust| {
        let now=SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs();
        let verified=handoff.verify(trust,initial.retirement_id(),initial.new_generation(),local_key,
            i64::try_from(now).map_err(|_| "clock unavailable")?)?;
        if verified.digest()!=initial.digest() || verified.fingerprint()!=initial.fingerprint() { return Err("handoff confirmation changed"); }
        persist_historical_handoff(fs,dir,trust,&verified,handoff,local_key)?;
        persist_review(fs,dir,trust,&verified,consumed,receipt)
    })
}

fn persist_historical_handoff(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    trust:&crate::fleet_machine_policy::FleetTrust,verified:&VerifiedHandoff,handoff:&Handoff,key:&[u8;32]) -> Result<(),&'static str> {
    fs.ensure_dir_secure(dir).map_err(|_| "historical handoff unavailable")?;
    let expected=lower_hex(verified.decision_digest());
    handoff.verify_historical(trust,verified.retirement_id(),verified.new_generation(),key,&expected)?;
    let path=dir.join(format!("historical-handoff-{}.json",verified.retirement_id()));
    match fs.read(&path) {
        Ok(bytes) if bytes.len()<=16384 => {
            let existing:Handoff=serde_json::from_slice(&bytes).map_err(|_| "historical handoff unavailable")?;
            existing.verify_historical(trust,verified.retirement_id(),verified.new_generation(),key,&expected)?;
        },
        Ok(_)=>return Err("historical handoff unavailable"),
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => {
            fs.atomic_write(&path,&serde_json::to_vec(handoff).map_err(|_| "historical handoff unavailable")?)
                .map_err(|_| "historical handoff unavailable")?;
        },
        Err(_)=>return Err("historical handoff unavailable"),
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn test_seed_review(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    trust:&crate::fleet_machine_policy::FleetTrust,handoff:&Handoff,retirement:&str,key:&[u8;32],now:i64,archive:&str)
    ->Result<(),&'static str> {
    let verified=handoff.verify(trust,retirement,handoff.new_generation(),key,now)?;
    let approval=ConsumedApproval { approval:NativeApproval { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),
        digest:*verified.digest(),fingerprint:*verified.fingerprint() },expires:Instant::now()+Duration::from_secs(120) };
    let receipt=ArchiveReceipt { status:"archived_unknown".into(),recovery_id:retirement.into(),archive_sha256:archive.into(),
        archived_actor_sid:"S-1-5-21-1".into(),journal_present:false,journal_sha256:None };
    persist_historical_handoff(fs,dir,trust,&verified,handoff,key)?;
    persist_review(fs,dir,trust,&verified,approval,receipt)?;
    initialize_replacement(fs,dir,trust,&verified,archive)
}

#[cfg(test)]
pub(crate) async fn test_recovery_contact(store:&crate::fleet_machine_policy::FleetMachinePolicy,decoy:bool,policy_only:bool,
    protector:&dyn crate::fleet_credential::Protector)->Result<bool,&'static str> {
    recovery_contact_with(store,decoy,policy_only,protector).await
}

#[cfg(test)]
pub(crate) fn test_seed_late_evidence(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,
    command:fleet_proto::SignedCommand)->Result<serde_json::Value,&'static str> {
    let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,verified,archive,operator,epoch| {
        let id=command.command_id.clone(); let digest=command_digest(&command)?;
        persist_unified_batch(fs,dir,protector,old,verified,archive,AuthenticatedCommandBatch {
            trust:current.clone(),operator_key:operator.into(),response_sha256:"ab".repeat(32),commands:vec![command.clone()] })?;
        let peer=AuthenticatedExecutionPeer { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:1 };
        let (_,token)=issue_unified_grant(fs,dir,protector,current,old,verified,archive,operator,&peer,&id,epoch,now)?;
        start_unified_grant(fs,dir,protector,current,old,verified,archive,operator,&peer,&id,&token,epoch,now)?;
        let approval=UnknownApproval { actor_sid:peer.sid.clone(),session:peer.session,logon:peer.logon,command_digest:digest,expires:Instant::now()+Duration::from_secs(120) };
        acknowledge_unknown(fs,dir,protector,candidate,current,old,verified,archive,operator,&id,approval,now,|_|true)?;
        let journal:ReplacementJournal=serde_json::from_slice(&fs.read(&dir.join(format!("replacement-{}.json",verified.retirement_id()))).map_err(|_| "journal unavailable")?)
            .map_err(|_| "journal unavailable")?;
        let unknown=journal.pending.last().ok_or("journal unavailable")?.clone();
        let unknown_ack=journal.pending_acks.last().ok_or("journal unavailable")?.clone();
        let result=serde_json::json!({"state":"observed"});
        let ack=fleet_proto::CheckinAck { command_id:id.clone(),success:true,detail:None,result:Some(result.clone()),report_id:Some(uuid::Uuid::new_v4().to_string()),phase:None,terminal_reason:None };
        let outcome=fleet_proto::ActionOutcome { version:1,receipt_id:uuid::Uuid::new_v4().to_string(),device_id:fleet_proto::DeviceId(current.device_id.clone()),
            sequence:0,previous_hash:None,command_id:Some(id.clone()),catalog_id:command.catalog_id.clone(),action_class:command.action_class,
            outcome:fleet_proto::ActionOutcomeState::Pass,failure_stage:None,observed_at:utc_timestamp(now)?,
            result_digest:serde_json::json!({"typed_result_sha256":fleet_proto::canonical_json_sha256(&result)}),record_hash:String::new(),signature:String::new() };
        finish_unified_grant(fs,dir,protector,candidate,current,old,verified,archive,operator,&peer,&id,&token,epoch,now,AuthenticatedExecution { outcome,ack })?;
        Ok(serde_json::json!({"outcome":unknown,"ack":unknown_ack}))
    })
}

#[cfg(test)]
pub(crate) async fn test_upload_late(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,
    command_id:&str)->Result<(),&'static str> {
    upload_late_evidence(store,fs,dir,protector,candidate,command_id).await
}

#[cfg(test)]
pub(crate) async fn test_recovery_results(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>)
    ->Result<(),&'static str> { recovery_result_contact(store,fs,dir,protector,candidate).await }

#[cfg(test)]
pub(crate) async fn test_vault_work(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,id:&str)
    ->Result<(),&'static str> {
    recovery_work_contact(store,fs,dir,protector,candidate).await?;
    let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_|"clock unavailable")?.as_secs()).map_err(|_|"clock unavailable")?;
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,verified,archive,operator,epoch| {
        let peer=AuthenticatedExecutionPeer {sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:1};
        let (command,token)=issue_unified_grant(fs,dir,protector,current,old,verified,archive,operator,&peer,id,epoch,now)?;
        if command.catalog_id!="encryption.status" { return Err("unexpected command"); }
        start_unified_grant(fs,dir,protector,current,old,verified,archive,operator,&peer,id,&token,epoch,now)?;
        let result=serde_json::json!({"installed":true,"scope":"agent_authorized","mounted_count":2});
        let ack=fleet_proto::CheckinAck {command_id:id.into(),success:true,detail:None,result:Some(result.clone()),report_id:Some(uuid::Uuid::new_v4().to_string()),phase:None,terminal_reason:None};
        let outcome=fleet_proto::ActionOutcome {version:1,receipt_id:uuid::Uuid::new_v4().to_string(),device_id:command.device_id.clone(),sequence:0,previous_hash:None,command_id:Some(id.into()),catalog_id:command.catalog_id.clone(),action_class:command.action_class,outcome:fleet_proto::ActionOutcomeState::Pass,failure_stage:None,observed_at:utc_timestamp(now)?,result_digest:serde_json::json!({"typed_result_sha256":fleet_proto::canonical_json_sha256(&result)}),record_hash:String::new(),signature:String::new()};
        finish_unified_grant(fs,dir,protector,candidate,current,old,verified,archive,operator,&peer,id,&token,epoch,now,AuthenticatedExecution {outcome,ack})
    })?;
    recovery_result_contact(store,fs,dir,protector,candidate).await
}

#[cfg(test)]
pub(crate) fn test_pending_result_count(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>)->Result<usize,&'static str> {
    with_historical_recovery(store,fs,dir,candidate,None,|_,old,verified,archive,_,_| {
        Ok(load_replacement_batch(fs,dir,old,verified,archive)?.outcomes.len())
    })
}

/// Resolve historical signing context only while the protected active record
/// is locked; its stable digest and candidate fingerprint are authoritative.
fn with_historical_recovery<T>(store:&crate::fleet_machine_policy::FleetMachinePolicy,
    fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,
    expected:Option<&crate::fleet_machine_policy::CredentialSnapshot>,
    f:impl FnOnce(&crate::fleet_machine_policy::FleetTrust,&crate::fleet_machine_policy::FleetTrust,&VerifiedHandoff,&str,&str,i64)->Result<T,&'static str>) -> Result<T,&'static str> {
    store.with_active_recovery(expected,|current,old,recovery,decision,archive,fingerprint,operator,epoch,_policy| {
        let id=uuid::Uuid::parse_str(recovery).map_err(|_| "historical handoff unavailable")?;
        if id.is_nil() || id.to_string()!=recovery { return Err("historical handoff unavailable"); }
        let key=candidate.public_key(old,recovery)?;
        if lower_hex(&Sha256::digest(key))!=fingerprint { return Err("historical handoff unavailable"); }
        let bytes=fs.read(&dir.join(format!("historical-handoff-{id}.json"))).map_err(|_| "historical handoff unavailable")?;
        if bytes.len()>16384 { return Err("historical handoff unavailable"); }
        let handoff:Handoff=serde_json::from_slice(&bytes).map_err(|_| "historical handoff unavailable")?;
        let verified=handoff.verify_historical(old,recovery,&current.enrollment_generation,&key,decision)?;
        f(current,old,&verified,archive,operator,epoch)
    })
}

/// This coordinator is deliberately unmounted. Its caller must first obtain
/// native consent and consume a grant bound to the verified handoff.
pub(super) async fn archive_review_with_broker(store:&crate::fleet_machine_policy::FleetMachinePolicy,
    fs:&dyn crate::policy_store::PolicyFs, dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector, trust:&crate::fleet_machine_policy::FleetTrust,
    handoff:&Handoff, verified:&VerifiedHandoff, local_key:&[u8;32],
    consumed:ConsumedApproval, witness:&SessionWitness) -> Result<String,&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    use wincmd_shared::vault_access::{VaultMountReason,VaultPresentation};
    if consumed.expires<=Instant::now() || consumed.approval.digest!=*verified.digest()
        || consumed.approval.fingerprint!=*verified.fingerprint() || consumed.approval.sid!=witness.sid
        || consumed.approval.session!=witness.session || consumed.approval.logon!=witness.logon
        || !witness.still_authorized() { return Err("handoff authorization changed"); }
    let mut random=[0u8;8]; SysRng.try_fill_bytes(&mut random).map_err(|_| "randomness unavailable")?;
    let operation=u64::from_le_bytes(random);
    let mut stored_receipt=None;
    let mut receive=|payload| {
        let (receipt,ack)=receive_transferred_archive(payload,operation,fs,dir,protector,trust,verified,
            ||witness.still_authorized() && consumed.expires>Instant::now())
            .map_err(|_|VaultMountReason::BrokerReplyRejected)?;
        stored_receipt=Some(receipt);
        Ok(ack)
    };
    let result=crate::pro_broker::vault_archive_call(crate::pro_broker::VaultCall {
        request_id:operation,target_session_id:0,caller_sid:"S-1-5-18",caller_token:None,
        caller_authentication_id:None,presentation:VaultPresentation::Machine,
        feature_id:"fleet.journal.archive_transfer",
        args:serde_json::json!({"device_id":trust.device_id,"recovery_id":verified.retirement_id(),
            "candidate_key":STANDARD.encode(local_key),"actor_sid":witness.sid,"acknowledge_unknown":true}),
    },&mut receive).await.map_err(|_| "archive broker unavailable")?;
    drop(receive);
    let receipt=stored_receipt.ok_or("archive broker unavailable")?;
    let final_receipt:ArchiveReceipt=serde_json::from_value(result).map_err(|_| "archive broker unavailable")?;
    if final_receipt!=receipt || !witness.still_authorized() || consumed.expires<=Instant::now() { return Err("handoff authorization changed"); }
    let hash=receipt.archive_sha256.clone();
    record_review(store,fs,dir,handoff,verified,local_key,consumed,receipt)?;
    store.with_handoff_trust(|current| {
        if current!=trust { return Err("service trust changed"); }
        let wall=SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs();
        let fresh=handoff.verify(current,verified.retirement_id(),verified.new_generation(),local_key,
            i64::try_from(wall).map_err(|_| "clock unavailable")?)?;
        initialize_replacement(fs,dir,current,&fresh,&hash)
    })?;
    Ok(hash)
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::policy_store::PolicyFs;
    use std::{collections::HashMap,path::{Path,PathBuf}};
    #[derive(Default)] struct Fs(Mutex<HashMap<PathBuf,Vec<u8>>>);
    impl crate::policy_store::PolicyFs for Fs {
        fn read(&self,path:&Path)->std::io::Result<Vec<u8>> { self.0.lock().unwrap().get(path).cloned().ok_or(std::io::ErrorKind::NotFound.into()) }
        fn atomic_write(&self,path:&Path,bytes:&[u8])->std::io::Result<()> { self.0.lock().unwrap().insert(path.into(),bytes.into()); Ok(()) }
        fn ensure_dir_secure(&self,_:&Path)->Result<(),crate::policy_store::PolicyStoreError> { Ok(()) }
    }
    #[test]
    fn initial_enrollment_rejects_any_existing_recovery_artifact() {
        let dir=std::env::temp_dir().join(format!("fleet-initial-enrollment-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        assert!(initial_enrollment_stop_state(&dir).is_err());
        let policy=dir.join("fleet-machine-policy.json");
        let recovery=dir.join("recovery-candidate-11111111-1111-4111-8111-111111111111.json");
        std::fs::write(&policy, b"{}").unwrap();
        assert_eq!(initial_enrollment_stop_state(&dir).unwrap(), serde_json::json!({"pending":false}));
        std::fs::write(&recovery, b"{}").unwrap();
        assert!(initial_enrollment_stop_state(&dir).is_err());
        std::fs::remove_file(&recovery).unwrap();
        std::fs::write(dir.join("unexpected.txt"), b"unrelated").unwrap();
        assert!(initial_enrollment_stop_state(&dir).is_err());
        std::fs::remove_file(dir.join("unexpected.txt")).unwrap();
        assert!(initial_enrollment_stop_state(&dir.join("missing")).is_err());
        std::fs::remove_file(policy).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    fn fixture() -> (VerifiedHandoff,crate::fleet_machine_policy::FleetTrust) {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let h:Handoff=serde_json::from_value(serde_json::json!({"recovery_id":"11111111-1111-4111-8111-111111111111",
            "org_id":"org-a","device_id":"0480b2ca-f4c3-4599-81fc-4876f73e70fa",
            "old_generation":"22222222-2222-4222-8222-222222222222","new_generation":"33333333-3333-4333-8333-333333333333",
            "new_key":"kaKKC3Q4FZOk2UaVeSCJJq/IrYLIg5t2RDWbnrqaSzo=","next_sequence":7,"previous_hash":"ab".repeat(32),"issued_at":1000,
            "server_key":"Zr5+Myx6RTMyvZ0Kf32wVfXF7xoGraZtmLOftoEMRzo=","signature":"h97rXEpV9lskSg1jI2698m0urZAufIhNyq0qhcDFCNc2lVnOBaCWXIWxHgi6GDWP3rRyvbqdv/zhsv/TvjWZDQ=="})).unwrap();
        let trust=crate::fleet_machine_policy::FleetTrust { org_id:"org-a".into(),device_id:"0480b2ca-f4c3-4599-81fc-4876f73e70fa".into(),
            enrollment_generation:"22222222-2222-4222-8222-222222222222".into(),signer_key:"Zr5+Myx6RTMyvZ0Kf32wVfXF7xoGraZtmLOftoEMRzo=".into() };
        let key:[u8;32]=STANDARD.decode("kaKKC3Q4FZOk2UaVeSCJJq/IrYLIg5t2RDWbnrqaSzo=").unwrap().try_into().unwrap();
        let verified=h.verify(&trust,"11111111-1111-4111-8111-111111111111","33333333-3333-4333-8333-333333333333",&key,1001).unwrap();
        (verified,trust)
    }
    fn approval(v:&VerifiedHandoff)->ConsumedApproval { ConsumedApproval { approval:NativeApproval { sid:"S-1-5-21-1".into(), session:1,logon:(1,0),digest:*v.digest(),fingerprint:*v.fingerprint() }, expires:Instant::now()+Duration::from_secs(30) } }
    fn receipt()->ArchiveReceipt { ArchiveReceipt { status:"archived_unknown".into(), recovery_id:"11111111-1111-4111-8111-111111111111".into(),archive_sha256:"ab".repeat(32),archived_actor_sid:"S-1-5-21-1".into(),journal_present:true,journal_sha256:Some("cd".repeat(32)) } }
    #[test]
    fn cooperative_stop_receipt_is_bound_to_exact_owner_and_preserves_unknown() {
        use base64::{engine::general_purpose::STANDARD,Engine};
        let fs=Fs::default();let (verified,trust)=fixture();let dir=Path::new("protected");
        begin_stop_request(&fs,dir,&trust,&verified,&approval(&verified),None).unwrap();
        let owner=AuthenticatedExecutionPeer { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:10 };
        let response=poll_stop_request(&fs,dir,&trust,&trust.device_id,&owner).unwrap();
        let first:[u8;32]=STANDARD.decode(response["token"].as_str().unwrap()).unwrap().try_into().unwrap();
        // A lost poll reply is retried by the same owner; earlier tokens are invalidated.
        let response=poll_stop_request(&fs,dir,&trust,&trust.device_id,&owner).unwrap();
        let token:[u8;32]=STANDARD.decode(response["token"].as_str().unwrap()).unwrap().try_into().unwrap();
        assert!(complete_stop_request(&fs,dir,&trust,&trust.device_id,verified.retirement_id(),&first,&owner,"joined").is_err());
        let replaced=AuthenticatedExecutionPeer { sid:owner.sid.clone(),session:owner.session,logon:owner.logon,pid:owner.pid,process_created:11 };
        assert!(poll_stop_request(&fs,dir,&trust,&trust.device_id,&replaced).is_err());
        assert!(complete_stop_request(&fs,dir,&trust,&trust.device_id,verified.retirement_id(),&token,&replaced,"joined").is_err());
        assert!(complete_stop_request(&fs,dir,&trust,&trust.device_id,verified.retirement_id(),&[0u8;32],&owner,"joined").is_err());
        complete_stop_request(&fs,dir,&trust,&trust.device_id,verified.retirement_id(),&token,&owner,"unknown").unwrap();
        assert_eq!(read_stop(&fs,dir,&trust).unwrap().unwrap().state,"unknown");
        assert!(write_unknown_stop_review(&fs,dir,&trust,&verified,&approval(&verified),false,1001).is_err());
        write_unknown_stop_review(&fs,dir,&trust,&verified,&approval(&verified),true,1001).unwrap();
        let review_path=dir.join(format!("unknown-stop-review-{}.json",verified.retirement_id()));let review=fs.read(&review_path).unwrap();
        write_unknown_stop_review(&fs,dir,&trust,&verified,&approval(&verified),true,1002).unwrap();
        assert_eq!(fs.read(&review_path).unwrap(),review);
        assert_eq!(read_stop(&fs,dir,&trust).unwrap().unwrap().state,"unknown");
        begin_stop_request(&fs,dir,&trust,&verified,&approval(&verified),None).unwrap();
        // Waiting or restarting the service never changes Unknown to Joined.
        assert_eq!(read_stop(&fs,dir,&trust).unwrap().unwrap().state,"unknown");
        assert!(complete_stop_request(&fs,dir,&trust,&trust.device_id,verified.retirement_id(),&token,&owner,"joined").is_err());
        let ledger=LegacyWorkLedger { org_id:trust.org_id.clone(),device_id:trust.device_id.clone(),generation:trust.enrollment_generation.clone(),baseline_reviewed:true,entries:Vec::new() };
        fs.atomic_write(&legacy_work_path(dir,&trust).unwrap(),&serde_json::to_vec(&ledger).unwrap()).unwrap();
        complete_stop_request(&fs,dir,&trust,&trust.device_id,verified.retirement_id(),&token,&owner,"joined").unwrap();
        assert_eq!(read_stop(&fs,dir,&trust).unwrap().unwrap().state,"joined");
        assert!(complete_stop_request(&fs,dir,&trust,&trust.device_id,verified.retirement_id(),&token,&owner,"unknown").is_err());
    }
    #[test]
    fn work_start_and_stop_share_barrier_and_preserve_inflight_across_restart() {
        use base64::{engine::general_purpose::STANDARD,Engine};
        let fs=Fs::default();let (verified,trust)=fixture();let dir=Path::new("protected");
        let owner=AuthenticatedExecutionPeer { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:10 };
        let encoded=start_legacy_work(&fs,dir,&trust,&trust.device_id,"command","command-1",&owner).unwrap();
        let work_token:[u8;32]=STANDARD.decode(encoded).unwrap().try_into().unwrap();
        begin_stop_request(&fs,dir,&trust,&verified,&approval(&verified),None).unwrap();
        assert!(start_legacy_work(&fs,dir,&trust,&trust.device_id,"command","command-2",&owner).is_err());
        let poll=poll_stop_request(&fs,dir,&trust,&trust.device_id,&owner).unwrap();
        let stop_token:[u8;32]=STANDARD.decode(poll["token"].as_str().unwrap()).unwrap().try_into().unwrap();
        assert!(complete_stop_request(&fs,dir,&trust,&trust.device_id,verified.retirement_id(),&stop_token,&owner,"joined").is_err());
        assert_eq!(read_legacy_work(&fs,dir,&trust).unwrap().entries[0].state,"active");
        complete_legacy_work(&fs,dir,&trust,&trust.device_id,"command","command-1",&work_token,&owner).unwrap();
        complete_legacy_work(&fs,dir,&trust,&trust.device_id,"command","command-1",&work_token,&owner).unwrap();
        assert!(complete_stop_request(&fs,dir,&trust,&trust.device_id,verified.retirement_id(),&stop_token,&owner,"joined").is_err());
        let mut ledger=read_legacy_work(&fs,dir,&trust).unwrap();ledger.baseline_reviewed=true;
        fs.atomic_write(&legacy_work_path(dir,&trust).unwrap(),&serde_json::to_vec(&ledger).unwrap()).unwrap();
        complete_stop_request(&fs,dir,&trust,&trust.device_id,verified.retirement_id(),&stop_token,&owner,"joined").unwrap();
        assert!(start_legacy_work(&fs,dir,&trust,&trust.device_id,"command","command-1",&owner).is_err());
    }
    #[test]
    fn reviewed_unanswered_stop_remains_unknown_and_freezes_new_admission() {
        let fs=Fs::default();let (verified,trust)=fixture();let dir=Path::new("protected");
        let owner=AuthenticatedExecutionPeer { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:10 };
        begin_stop_request(&fs,dir,&trust,&verified,&approval(&verified),None).unwrap();
        assert!(write_unknown_stop_review(&fs,dir,&trust,&verified,&approval(&verified),false,1001).is_err());
        write_unknown_stop_review(&fs,dir,&trust,&verified,&approval(&verified),true,1001).unwrap();
        let path=dir.join(format!("unknown-stop-review-{}.json",verified.retirement_id()));
        let review:UnknownStopReview=serde_json::from_slice(&fs.read(&path).unwrap()).unwrap();
        assert_eq!(review.outcome,"unknown_acknowledged");assert_eq!(review.observed_stop_state,"requested");
        assert_eq!(read_stop(&fs,dir,&trust).unwrap().unwrap().state,"requested");
        assert!(start_legacy_work(&fs,dir,&trust,&trust.device_id,"command","later",&owner).is_err());
    }
    #[test]
    fn upgrade_boundary_requires_new_boot_and_fresh_pro_protocol_challenge() {
        let fs=Fs::default();let (verified,trust)=fixture();let dir=Path::new("protected");let before=[1u8;32];let after=[2u8;32];
        assert!(prepare_upgrade_boundary(&fs,dir,&trust,&verified,&approval(&verified),false,&before).is_err());
        prepare_upgrade_boundary(&fs,dir,&trust,&verified,&approval(&verified),true,&before).unwrap();
        prepare_upgrade_boundary(&fs,dir,&trust,&verified,&approval(&verified),true,&before).unwrap();
        assert!(complete_upgrade_boundary(&fs,dir,&trust,&verified,&approval(&verified),true,&before,|_|panic!("same boot must fail" )).is_err());
        assert!(complete_upgrade_boundary(&fs,dir,&trust,&verified,&approval(&verified),true,&after,|_|Err("old protocol")).is_err());
        complete_upgrade_boundary(&fs,dir,&trust,&verified,&approval(&verified),true,&after,|version| {
            assert_eq!(version,UPGRADE_RECOVERY_PROTOCOL);Ok(("ab".repeat(32),42,99))
        }).unwrap();
        // A restarted matching Pro must answer a new challenge; the original audit remains intact.
        complete_upgrade_boundary(&fs,dir,&trust,&verified,&approval(&verified),true,&after,|_|Ok(("ab".repeat(32),43,100))).unwrap();
        assert!(complete_upgrade_boundary(&fs,dir,&trust,&verified,&approval(&verified),true,&after,|_|Ok(("cd".repeat(32),44,101))).is_err());
        let path=dir.join(format!("upgrade-proof-{}-{}.json",verified.retirement_id(),lower_hex(&Sha256::digest(after))));
        let proof:UpgradeBootProof=serde_json::from_slice(&fs.read(&path).unwrap()).unwrap();assert_eq!(proof.prior_effects,"unknown");assert_eq!(proof.pro_pid,42);
        // A fresh server signature after reboot preserves the stable decision.
        use base64::{engine::general_purpose::STANDARD,Engine};use ed25519_dalek::{Signer,SigningKey};
        let key=STANDARD.encode(verified.public_key());let hash=Some("ab".repeat(32));
        let bytes=serde_json::to_vec(&("wincommander:fleet:identity-outcome-handoff:v1",verified.retirement_id(),&trust.org_id,&trust.device_id,
            &trust.enrollment_generation,verified.new_generation(),&key,7u64,&hash,1200i64,&trust.signer_key)).unwrap();
        let refreshed:Handoff=serde_json::from_value(serde_json::json!({"recovery_id":verified.retirement_id(),"org_id":trust.org_id,"device_id":trust.device_id,
            "old_generation":trust.enrollment_generation,"new_generation":verified.new_generation(),"new_key":key,"next_sequence":7,"previous_hash":hash,
            "issued_at":1200,"server_key":trust.signer_key,"signature":STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&bytes).to_bytes())})).unwrap();
        let fresh=refreshed.verify(&trust,verified.retirement_id(),verified.new_generation(),verified.public_key(),1201).unwrap();
        assert_ne!(fresh.digest(),verified.digest());assert_eq!(fresh.decision_digest(),verified.decision_digest());
        complete_upgrade_boundary(&fs,dir,&trust,&fresh,&approval(&fresh),true,&after,|_|Ok(("ab".repeat(32),44,101))).unwrap();
        let owner=AuthenticatedExecutionPeer { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:99 };
        let pending=UpgradeChallenge { device:trust.device_id.clone(),recovery:verified.retirement_id().into(),generation:trust.enrollment_generation.clone(),
            boot_sha256:"ab".repeat(32),owner:AuthenticatedExecutionPeer { sid:owner.sid.clone(),session:owner.session,logon:owner.logon,pid:owner.pid,process_created:owner.process_created },
            expires:Instant::now()+Duration::from_secs(30) };
        assert!(valid_upgrade_challenge(&pending,&owner,&trust,&trust.device_id,verified.retirement_id(),&"ab".repeat(32)));
        assert!(!valid_upgrade_challenge(&pending,&owner,&trust,&trust.device_id,verified.retirement_id(),&"cd".repeat(32)));
        let replaced=AuthenticatedExecutionPeer { sid:owner.sid.clone(),session:owner.session,logon:owner.logon,pid:owner.pid,process_created:100 };
        assert!(!valid_upgrade_challenge(&pending,&replaced,&trust,&trust.device_id,verified.retirement_id(),&"ab".repeat(32)));
        let reply=serde_json::json!({"response":{"challenge":"challenge","protocol_version":1},"image_sha256":"ab".repeat(32),"pid":42,"process_created":99});
        assert!(verify_upgrade_probe_reply(reply.clone(),"challenge").is_ok());
        assert!(verify_upgrade_probe_reply(reply.clone(),"different").is_err());
        let mut old=reply.clone();old["response"]["protocol_version"]=0.into();assert!(verify_upgrade_probe_reply(old,"challenge").is_err());
        let mut forged=reply;forged["response"]["pid"]=777.into();assert!(verify_upgrade_probe_reply(forged,"challenge").is_err());
    }
    #[test]
    fn concurrent_stop_and_start_never_admit_untracked_work() {
        let fs=Fs::default();let (verified,trust)=fixture();let dir=Path::new("race-protected");
        let owner=AuthenticatedExecutionPeer { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:10 };
        let barrier=std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let start=scope.spawn(|| { barrier.wait();start_legacy_work(&fs,dir,&trust,&trust.device_id,"command","race",&owner) });
            let stop=scope.spawn(|| { barrier.wait();begin_stop_request(&fs,dir,&trust,&verified,&approval(&verified),None) });
            let admitted=start.join().unwrap().is_ok();stop.join().unwrap().unwrap();
            let ledger=read_legacy_work(&fs,dir,&trust).unwrap();
            assert_eq!(ledger.entries.iter().any(|entry|entry.work_id=="race" && entry.state=="active"),admitted);
            assert!(start_legacy_work(&fs,dir,&trust,&trust.device_id,"command","after-stop",&owner).is_err());
        });
    }
    #[test]
    fn work_barrier_bounds_completed_retries_without_forgetting_active_work() {
        use base64::{engine::general_purpose::STANDARD,Engine};
        let fs=Fs::default();let (_,trust)=fixture();let dir=Path::new("bounded-protected");
        let owner=AuthenticatedExecutionPeer { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:10 };
        let pending=start_legacy_work(&fs,dir,&trust,&trust.device_id,"search","still-running",&owner).unwrap();
        for index in 0..(LEGACY_COMPLETED_RETRY_LIMIT+16) {
            let id=format!("command-{index}");let encoded=start_legacy_work(&fs,dir,&trust,&trust.device_id,"command",&id,&owner).unwrap();
            let token:[u8;32]=STANDARD.decode(encoded).unwrap().try_into().unwrap();
            complete_legacy_work(&fs,dir,&trust,&trust.device_id,"command",&id,&token,&owner).unwrap();
        }
        let ledger=read_legacy_work(&fs,dir,&trust).unwrap();
        assert_eq!(ledger.entries.iter().filter(|entry|entry.state=="complete").count(),LEGACY_COMPLETED_RETRY_LIMIT);
        assert!(ledger.entries.iter().any(|entry|entry.work_id=="still-running" && entry.state=="active"));
        assert!(start_legacy_work(&fs,dir,&trust,&trust.device_id,"search","still-running",&owner).is_err());
        let token:[u8;32]=STANDARD.decode(pending).unwrap().try_into().unwrap();
        complete_legacy_work(&fs,dir,&trust,&trust.device_id,"search","still-running",&token,&owner).unwrap();
        // Reoffers retain the pre-existing legacy execution semantics.
        assert!(start_legacy_work(&fs,dir,&trust,&trust.device_id,"command","command-271",&owner).is_ok());
    }
    #[test]
    fn previous_stop_archive_requires_committed_binding_and_preserves_exact_bytes() {
        let fs=Fs::default(); let (verified,old)=fixture();let dir=Path::new("protected");
        let mut current=old.clone();current.enrollment_generation=verified.new_generation().into();
        let previous=StopRequest { org_id:old.org_id.clone(),device_id:old.device_id.clone(),old_generation:old.enrollment_generation.clone(),
            recovery_id:verified.retirement_id().into(),decision_digest:lower_hex(verified.decision_digest()),state:"joined".into(),
            owner:Some(StopOwner { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:10,token_sha256:"ab".repeat(32) }) };
        let bytes=serde_json::to_vec(&previous).unwrap();let decision=previous.decision_digest.as_str();
        assert!(archive_previous_stop(&fs,dir,&current,&previous,&bytes,None).is_err());
        let fingerprint=lower_hex(verified.fingerprint());let binding=Some((previous.recovery_id.as_str(),&old,decision,fingerprint.as_str()));
        archive_previous_stop(&fs,dir,&current,&previous,&bytes,binding).unwrap();
        // Retry after interruption preserves the original archive, rather than rewriting it.
        archive_previous_stop(&fs,dir,&current,&previous,&bytes,binding).unwrap();
        let path=dir.join(format!("recovery-stop-archive-{}.json",previous.recovery_id));
        assert_eq!(fs.read(&path).unwrap(),bytes);
        fs.atomic_write(&path,b"different evidence").unwrap();
        assert!(archive_previous_stop(&fs,dir,&current,&previous,&bytes,binding).is_err());
        assert_eq!(fs.read(&path).unwrap(),b"different evidence");
        let mut unknown:StopRequest=serde_json::from_slice(&bytes).unwrap();unknown.state="unknown".into();
        let unknown_bytes=serde_json::to_vec(&unknown).unwrap();let separate=Fs::default();
        assert!(archive_previous_stop(&separate,dir,&current,&unknown,&unknown_bytes,binding).is_err());
        let review=UnknownStopReview { org_id:old.org_id.clone(),device_id:old.device_id.clone(),generation:old.enrollment_generation.clone(),
            recovery_id:previous.recovery_id.clone(),decision_digest:decision.into(),candidate_fingerprint:fingerprint.clone(),actor_sid:"S-1-5-21-1".into(),
            reviewed_at:1001,outcome:"unknown_acknowledged".into(),observed_stop_state:"unknown".into() };
        separate.atomic_write(&dir.join(format!("unknown-stop-review-{}.json",previous.recovery_id)),&serde_json::to_vec(&review).unwrap()).unwrap();
        archive_previous_stop(&separate,dir,&current,&unknown,&unknown_bytes,binding).unwrap();
        assert_eq!(separate.read(&path).unwrap(),unknown_bytes);
    }
    #[test]
    fn repeat_recovery_requires_drained_journal_and_matching_head() {
        let journal=ReplacementJournal { recovery_id:String::new(),org_id:String::new(),device_id:String::new(),generation:String::new(),
            decision_digest:String::new(),candidate_fingerprint:String::new(),archive_sha256:String::new(),
            origin_sequence:7,origin_hash:Some("ab".repeat(32)),base_sequence:7,base_hash:Some("ab".repeat(32)),
            next_sequence:7,previous_hash:Some("ab".repeat(32)),pending:Vec::new(),pending_acks:Vec::new(),history:Vec::new(),inbox_sealed:None,drain_freeze:None,cancelled_freezes:Vec::new() };
        let inbox=CommandInbox { org_id:String::new(),device_id:String::new(),generation:String::new(),operator_key:String::new(),entries:Vec::new() };
        assert!(ensure_repeat_drained(&journal,&inbox,7,Some(&"ab".repeat(32))).is_ok());
        assert!(ensure_repeat_drained(&journal,&inbox,8,Some(&"ab".repeat(32))).is_err());
        assert!(ensure_repeat_drained(&journal,&inbox,7,Some(&"cd".repeat(32))).is_err());
        let mut pending=journal;pending.base_sequence=6;
        assert!(ensure_repeat_drained(&pending,&inbox,7,Some(&"ab".repeat(32))).is_err());
    }
    #[test]
    fn historical_handoff_survives_restart_but_rejects_tampering() {
        use base64::{engine::general_purpose::STANDARD,Engine};
        let fs=Fs::default(); let (verified,trust)=fixture(); let key=*verified.public_key();
        let h:Handoff=serde_json::from_value(serde_json::json!({"recovery_id":"11111111-1111-4111-8111-111111111111",
            "org_id":"org-a","device_id":"0480b2ca-f4c3-4599-81fc-4876f73e70fa",
            "old_generation":"22222222-2222-4222-8222-222222222222","new_generation":"33333333-3333-4333-8333-333333333333",
            "new_key":STANDARD.encode(key),"next_sequence":7,"previous_hash":"ab".repeat(32),"issued_at":1000,
            "server_key":trust.signer_key,"signature":"h97rXEpV9lskSg1jI2698m0urZAufIhNyq0qhcDFCNc2lVnOBaCWXIWxHgi6GDWP3rRyvbqdv/zhsv/TvjWZDQ=="})).unwrap();
        let dir=Path::new("protected");
        persist_historical_handoff(&fs,dir,&trust,&verified,&h,&key).unwrap();
        let path=dir.join(format!("historical-handoff-{}.json",verified.retirement_id()));
        let bytes=fs.read(&path).unwrap(); let reopened:Handoff=serde_json::from_slice(&bytes).unwrap();
        assert!(reopened.verify(&trust,verified.retirement_id(),verified.new_generation(),&key,5000).is_err());
        assert!(reopened.verify_historical(&trust,verified.retirement_id(),verified.new_generation(),&key,&lower_hex(verified.decision_digest())).is_ok());
        assert!(reopened.verify_historical(&trust,verified.retirement_id(),verified.new_generation(),&key,&"00".repeat(32)).is_err());
        let mut altered:serde_json::Value=serde_json::from_slice(&bytes).unwrap(); altered["next_sequence"]=serde_json::json!(8);
        fs.0.lock().unwrap().insert(path,serde_json::to_vec(&altered).unwrap());
        assert!(persist_historical_handoff(&fs,dir,&trust,&verified,&h,&key).is_err());
    }
    #[test]
    fn unified_journal_migrates_old_schema_and_failed_write_preserves_evidence() {
        use base64::{engine::general_purpose::STANDARD,Engine};
        use ed25519_dalek::{Signer,SigningKey};
        use std::sync::atomic::{AtomicBool,Ordering};
        struct FailFs { files:Fs, fail:AtomicBool }
        impl PolicyFs for FailFs {
            fn read(&self,p:&Path)->std::io::Result<Vec<u8>> { self.files.read(p) }
            fn atomic_write(&self,p:&Path,b:&[u8])->std::io::Result<()> {
                if self.fail.swap(false,Ordering::SeqCst) { return Err(std::io::ErrorKind::Other.into()); }
                self.files.atomic_write(p,b)
            }
            fn ensure_dir_secure(&self,p:&Path)->Result<(),crate::policy_store::PolicyStoreError> { self.files.ensure_dir_secure(p) }
        }
        struct Protector;
        impl crate::fleet_credential::Protector for Protector {
            fn protect(&self,p:&[u8],e:&[u8])->Result<Vec<u8>,&'static str> { let mut v=e.to_vec();v.extend_from_slice(p);Ok(v) }
            fn unprotect(&self,p:&[u8],e:&[u8])->Result<zeroize::Zeroizing<Vec<u8>>,&'static str> {
                if !p.starts_with(e) { return Err("invalid binding"); } Ok(zeroize::Zeroizing::new(p[e.len()..].to_vec()))
            }
        }
        let fs=FailFs { files:Fs::default(),fail:AtomicBool::new(false) }; let (verified,old)=fixture();
        let mut current=old.clone();current.enrollment_generation=verified.new_generation().into();
        let operator=STANDARD.encode(SigningKey::from_bytes(&[33;32]).verifying_key().as_bytes());
        let dir=Path::new("protected"); let archive="ab".repeat(32);
        persist_review(&fs,dir,&old,&verified,approval(&verified),receipt()).unwrap();
        initialize_replacement(&fs,dir,&old,&verified,&archive).unwrap();
        let path=dir.join(format!("replacement-{}.json",verified.retirement_id()));
        let mut old_schema:serde_json::Value=serde_json::from_slice(&fs.read(&path).unwrap()).unwrap();
        old_schema.as_object_mut().unwrap().remove("inbox_sealed");
        fs.atomic_write(&path,&serde_json::to_vec(&old_schema).unwrap()).unwrap(); let original=fs.read(&path).unwrap();
        fs.fail.store(true,Ordering::SeqCst);
        assert!(mutate_unified_journal(&fs,dir,&Protector,&current,&old,&verified,&archive,&operator,|_,_|Ok(())).is_err());
        assert_eq!(fs.read(&path).unwrap(),original);
        mutate_unified_journal(&fs,dir,&Protector,&current,&old,&verified,&archive,&operator,|journal,inbox| {
            assert!(inbox.entries.is_empty()); assert_eq!(journal.next_sequence,7); Ok(())
        }).unwrap();
        let reopened:ReplacementJournal=serde_json::from_slice(&fs.read(&path).unwrap()).unwrap();
        assert!(reopened.inbox_sealed.is_some()); assert_eq!(reopened.next_sequence,7);
        let mut command=fleet_proto::SignedCommand { command_id:"command-1".into(),idempotency_key:"id-1".into(),
            device_id:fleet_proto::DeviceId(current.device_id.clone()),catalog_id:"device.checkin_now".into(),
            action_class:fleet_proto::ActionClass::Safe,payload:serde_json::json!({}),epoch_version:7,
            signature:String::new(),signer_key:current.signer_key.clone() };
        command.signature=STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&fleet_proto::canonical_command_bytes(
            &command.idempotency_key,&command.device_id.0,&command.catalog_id,command.action_class.as_wire_str(),&command.payload,command.epoch_version)).to_bytes());
        persist_unified_batch(&fs,dir,&Protector,&old,&verified,&archive,AuthenticatedCommandBatch {
            trust:current.clone(),operator_key:operator.clone(),response_sha256:"ab".repeat(32),commands:vec![command] }).unwrap();
        let ready=mutate_unified_journal(&fs,dir,&Protector,&current,&old,&verified,&archive,&operator,|_,inbox|Ok(ready_command_descriptors(inbox,7))).unwrap();
        assert_eq!(ready["commands"].as_array().unwrap().len(),1);
        let wrong_epoch=mutate_unified_journal(&fs,dir,&Protector,&current,&old,&verified,&archive,&operator,|_,inbox|Ok(ready_command_descriptors(inbox,8))).unwrap();
        assert!(wrong_epoch["commands"].as_array().unwrap().is_empty());
        let peer=AuthenticatedExecutionPeer { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:1 };
        let (command,_)=issue_unified_grant(&fs,dir,&Protector,&current,&old,&verified,&archive,&operator,&peer,"command-1",7,1000).unwrap();
        assert_eq!(command.command_id,"command-1");
        let no_retry=mutate_unified_journal(&fs,dir,&Protector,&current,&old,&verified,&archive,&operator,|_,inbox|Ok(ready_command_descriptors(inbox,7))).unwrap();
        assert!(no_retry["commands"].as_array().unwrap().is_empty());
        assert!(issue_unified_grant(&fs,dir,&Protector,&current,&old,&verified,&archive,&operator,&peer,"command-1",7,2000).is_err());
    }
    #[test]
    fn command_inbox_persists_retries_and_rejects_changed_or_forged_commands() {
        use base64::{engine::general_purpose::STANDARD,Engine};
        use ed25519_dalek::{Signer,SigningKey};
        struct Protector;
        impl crate::fleet_credential::Protector for Protector {
            fn protect(&self,p:&[u8],entropy:&[u8])->Result<Vec<u8>,&'static str> { let mut v=entropy.to_vec();v.extend_from_slice(p);Ok(v) }
            fn unprotect(&self,p:&[u8],entropy:&[u8])->Result<zeroize::Zeroizing<Vec<u8>>,&'static str> {
                if !p.starts_with(entropy) { return Err("invalid binding"); } Ok(zeroize::Zeroizing::new(p[entropy.len()..].to_vec()))
            }
        }
        let fs=Fs::default(); let (_,trust)=fixture();
        let mut command=fleet_proto::SignedCommand { command_id:"command-1".into(),idempotency_key:"idempotency-1".into(),
            device_id:fleet_proto::DeviceId(trust.device_id.clone()),catalog_id:"device.checkin_now".into(),
            action_class:fleet_proto::ActionClass::Safe,payload:serde_json::json!({}),epoch_version:7,
            signature:String::new(),signer_key:trust.signer_key.clone() };
        let sign=|c:&fleet_proto::SignedCommand|STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(
            &fleet_proto::canonical_command_bytes(&c.idempotency_key,&c.device_id.0,&c.catalog_id,c.action_class.as_wire_str(),&c.payload,c.epoch_version)).to_bytes());
        command.signature=sign(&command);
        let operator=STANDARD.encode(SigningKey::from_bytes(&[33;32]).verifying_key().as_bytes());
        let batch=|command|AuthenticatedCommandBatch { trust:trust.clone(),operator_key:operator.clone(),response_sha256:"ab".repeat(32),commands:vec![command] };
        let dir=Path::new("protected");
        persist_command_batch(&fs,dir,&Protector,batch(command.clone())).unwrap();
        let path=command_inbox_path(dir,&trust).unwrap(); let original=fs.0.lock().unwrap()[&path].clone();
        persist_command_batch(&fs,dir,&Protector,batch(command.clone())).unwrap();
        assert_eq!(fs.0.lock().unwrap()[&path],original);
        let mut forged=command.clone(); forged.payload=serde_json::json!({"changed":true});
        assert!(persist_command_batch(&fs,dir,&Protector,batch(forged.clone())).is_err());
        forged.signature=sign(&forged);
        assert!(persist_command_batch(&fs,dir,&Protector,batch(forged)).is_err());
        assert_eq!(fs.0.lock().unwrap()[&path],original);
        let mut duress=command.clone(); duress.command_id="command-2".into();
        duress.catalog_id=fleet_proto::DURESS_CATALOG_IDS[0].into(); duress.signature=sign(&duress);
        assert!(persist_command_batch(&fs,dir,&Protector,batch(duress.clone())).is_err());
        duress.signer_key=operator.clone();
        duress.signature=STANDARD.encode(SigningKey::from_bytes(&[33;32]).sign(
            &fleet_proto::canonical_command_bytes(&duress.idempotency_key,&duress.device_id.0,&duress.catalog_id,
                duress.action_class.as_wire_str(),&duress.payload,duress.epoch_version)).to_bytes());
        persist_command_batch(&fs,dir,&Protector,batch(duress)).unwrap();
        let peer=AuthenticatedExecutionPeer { sid:"S-1-5-21-1".into(),session:1,logon:(7,0),pid:123,process_created:1 };
        assert!(issue_execution_grant(&fs,dir,&Protector,&trust,&operator,&peer,"command-1",8,1000).is_err());
        let (granted,token)=issue_execution_grant(&fs,dir,&Protector,&trust,&operator,&peer,"command-1",7,1000).unwrap();
        assert_eq!(granted.command_id,"command-1"); assert_eq!(token.len(),32);
        // A lost grant response or restart must not execute the command again.
        assert!(issue_execution_grant(&fs,dir,&Protector,&trust,&operator,&peer,"command-1",7,2000).is_err());
    }
    #[test]
    fn archive_copy_is_bound_verified_and_retryable() {
        use base64::{engine::general_purpose::STANDARD,Engine};
        struct Protector;
        impl crate::fleet_credential::Protector for Protector {
            fn protect(&self,p:&[u8],entropy:&[u8])->Result<Vec<u8>,&'static str> {
                let mut result=entropy.to_vec(); result.extend_from_slice(p); Ok(result)
            }
            fn unprotect(&self,p:&[u8],entropy:&[u8])->Result<zeroize::Zeroizing<Vec<u8>>,&'static str> {
                if !p.starts_with(entropy) { return Err("wrong binding"); }
                Ok(zeroize::Zeroizing::new(p[entropy.len()..].to_vec()))
            }
        }
        let fs=Fs::default(); let (v,trust)=fixture(); let journal=b"saved evidence";
        let data=serde_json::to_vec(&serde_json::json!({"device_id":trust.device_id,"recovery_id":v.retirement_id(),
            "candidate_key":STANDARD.encode(v.public_key()),"actor_sid":"S-1-5-21-1",
            "journal":STANDARD.encode(journal),"ledger":null,"unknown":true})).unwrap();
        let mut r=receipt(); r.archive_sha256=lower_hex(&Sha256::digest(&data)); r.journal_sha256=Some(lower_hex(&Sha256::digest(journal)));
        let notification=||serde_json::json!({"operation_id":7,"receipt":{"status":r.status,"recovery_id":r.recovery_id,
            "archive_sha256":r.archive_sha256,"archived_actor_sid":r.archived_actor_sid,"journal_present":true,
            "journal_sha256":r.journal_sha256},"archive_b64":STANDARD.encode(&data)});
        assert!(receive_transferred_archive(notification(),7,&fs,Path::new("protected"),&Protector,&trust,&v,||false).is_err());
        assert!(fs.0.lock().unwrap().is_empty());
        let checks=std::cell::Cell::new(0);
        assert!(receive_transferred_archive(notification(),7,&fs,Path::new("protected"),&Protector,&trust,&v,|| {
            checks.set(checks.get()+1); checks.get()==1
        }).is_err()); // Logoff after persistence must not send an acknowledgement.
        assert_eq!(checks.get(),2);
        let (_,ack)=receive_transferred_archive(notification(),7,&fs,Path::new("protected"),&Protector,&trust,&v,||true).unwrap();
        assert_eq!(ack,serde_json::json!({"phase":"fleet_archive","archive_sha256":r.archive_sha256,"stored":true}));
        assert!(receive_transferred_archive(notification(),8,&fs,Path::new("protected"),&Protector,&trust,&v,||true).is_err());
        let store=|bytes:Vec<u8>|persist_transferred_archive(&fs,Path::new("protected"),&Protector,&trust,&v,&r,zeroize::Zeroizing::new(bytes));
        store(data.clone()).unwrap();
        store(data.clone()).unwrap(); // Retry after a lost acknowledgement.
        let mut altered=data.clone(); altered.push(b' ');
        assert!(store(altered).is_err());
        let path=Path::new("protected").join(format!("archive-{}.dpapi",v.retirement_id()));
        fs.0.lock().unwrap().insert(path,vec![1,2,3]);
        assert!(store(data).is_err()); // A damaged persisted copy never acknowledges.
    }
    #[test]
    fn fresh_proof_and_second_admin_resume_same_decision_with_separate_audit() {
        use base64::{engine::general_purpose::STANDARD,Engine};
        use ed25519_dalek::{Signer,SigningKey};
        let fs=Fs::default(); let (v,trust)=fixture(); let dir=Path::new("protected");
        persist_review(&fs,dir,&trust,&v,approval(&v),receipt()).unwrap();
        let original=fs.0.lock().unwrap().get(&dir.join(format!("review-{}.json",v.retirement_id()))).unwrap().clone();
        let key="kaKKC3Q4FZOk2UaVeSCJJq/IrYLIg5t2RDWbnrqaSzo=";
        let body=serde_json::to_vec(&("wincommander:fleet:identity-outcome-handoff:v1", "11111111-1111-4111-8111-111111111111",
            &trust.org_id,&trust.device_id,&trust.enrollment_generation,"33333333-3333-4333-8333-333333333333",key,7u64,Some("ab".repeat(32)),1100i64,&trust.signer_key)).unwrap();
        let sig=STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&body).to_bytes());
        let h:Handoff=serde_json::from_value(serde_json::json!({"recovery_id":"11111111-1111-4111-8111-111111111111",
            "org_id":trust.org_id,"device_id":trust.device_id,"old_generation":trust.enrollment_generation,
            "new_generation":"33333333-3333-4333-8333-333333333333","new_key":key,"next_sequence":7,
            "previous_hash":"ab".repeat(32),"issued_at":1100,"server_key":trust.signer_key,"signature":sig})).unwrap();
        let public:[u8;32]=STANDARD.decode(key).unwrap().try_into().unwrap();
        let fresh=h.verify(&trust,v.retirement_id(),v.new_generation(),&public,1101).unwrap();
        assert_eq!(fresh.decision_digest(),v.decision_digest()); assert_ne!(fresh.digest(),v.digest());
        let mut new_approval=approval(&fresh); new_approval.approval.sid="S-1-5-21-2".into();
        persist_review(&fs,dir,&trust,&fresh,new_approval,receipt()).unwrap();
        let records=fs.0.lock().unwrap();
        assert_eq!(records.get(&dir.join(format!("review-{}.json",v.retirement_id()))).unwrap(),&original);
        assert_eq!(records.len(),3);
    }
    #[test]
    fn replacement_journal_requires_review_and_preserves_signed_head_on_retry() {
        let fs=Fs::default(); let (v,trust)=fixture(); let dir=Path::new("protected");
        let archive="ab".repeat(32);
        assert!(initialize_replacement(&fs,dir,&trust,&v,&archive).is_err());
        persist_review(&fs,dir,&trust,&v,approval(&v),receipt()).unwrap();
        let before_changed_trust=fs.0.lock().unwrap().clone();
        let mut changed_signer=trust.clone();
        changed_signer.signer_key=base64::Engine::encode(&base64::engine::general_purpose::STANDARD,
            ed25519_dalek::SigningKey::from_bytes(&[12;32]).verifying_key().as_bytes());
        assert!(initialize_replacement(&fs,dir,&changed_signer,&v,&archive).is_err());
        assert_eq!(*fs.0.lock().unwrap(),before_changed_trust);
        initialize_replacement(&fs,dir,&trust,&v,&archive).unwrap();
        let before=fs.0.lock().unwrap().clone();
        initialize_replacement(&fs,dir,&trust,&v,&archive).unwrap();
        assert_eq!(*fs.0.lock().unwrap(),before);
        assert!(initialize_replacement(&fs,dir,&trust,&v,&"ef".repeat(32)).is_err());
        assert_eq!(*fs.0.lock().unwrap(),before);
        let path=dir.join(format!("replacement-{}.json",v.retirement_id()));
        let journal:serde_json::Value=serde_json::from_slice(before.get(&path).unwrap()).unwrap();
        assert_eq!(journal["next_sequence"],7); assert_eq!(journal["previous_hash"],"ab".repeat(32));
        assert_eq!(journal["pending"],serde_json::json!([]));
    }
    #[test]
    fn signed_outcome_and_ack_persist_together_and_retry_idempotently() {
        use base64::{engine::general_purpose::STANDARD,Engine};
        use ed25519_dalek::{Signer,SigningKey};
        struct Protector;
        impl crate::fleet_credential::Protector for Protector {
            fn protect(&self,p:&[u8],e:&[u8])->Result<Vec<u8>,&'static str> { let mut v=e.to_vec(); v.extend(p.iter().zip(e).map(|(a,b)|a^b)); Ok(v) }
            fn unprotect(&self,p:&[u8],e:&[u8])->Result<zeroize::Zeroizing<Vec<u8>>,&'static str> {
                if p.len()!=64 || p[..32]!=*e { return Err("invalid"); }
                Ok(zeroize::Zeroizing::new(p[32..].iter().zip(e).map(|(a,b)|a^b).collect()))
            }
        }
        let fs=Fs::default(); let (_v,trust)=fixture(); let protector=Protector;
        let candidate=crate::fleet_recovery_candidate::CandidateStore::open(&fs,"candidate".into(),&protector).unwrap();
        let retirement=serde_json::json!({"org_id":trust.org_id,"old_device_id":trust.device_id,"old_generation":trust.enrollment_generation,
            "retirement_id":"11111111-1111-4111-8111-111111111111","issued_at":1000,"signer_key":trust.signer_key,
            "signature":"7YCy0JLIrYZGFnCrA/rAHrB+L1I97TwBUdjtpMA/9rnErwQKWRxXxHcUQjcrZE+HSjbBS2H6Ws0dzWc27jWmBg=="});
        let key=candidate.stage(&trust,&serde_json::to_vec(&retirement).unwrap(),1001).unwrap();
        let body=serde_json::to_vec(&("wincommander:fleet:identity-outcome-handoff:v1","11111111-1111-4111-8111-111111111111",
            &trust.org_id,&trust.device_id,&trust.enrollment_generation,"33333333-3333-4333-8333-333333333333",&key,7u64,Some("ab".repeat(32)),1000i64,&trust.signer_key)).unwrap();
        let signature=STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&body).to_bytes());
        let handoff:Handoff=serde_json::from_value(serde_json::json!({"recovery_id":"11111111-1111-4111-8111-111111111111", "org_id":trust.org_id,"device_id":trust.device_id,
            "old_generation":trust.enrollment_generation,"new_generation":"33333333-3333-4333-8333-333333333333","new_key":key,"next_sequence":7,
            "previous_hash":"ab".repeat(32),"issued_at":1000,"server_key":trust.signer_key,"signature":signature})).unwrap();
        let public:[u8;32]=STANDARD.decode(&key).unwrap().try_into().unwrap();
        let verified=handoff.verify(&trust,"11111111-1111-4111-8111-111111111111","33333333-3333-4333-8333-333333333333",&public,1001).unwrap();
        let dir=Path::new("protected"); persist_review(&fs,dir,&trust,&verified,approval(&verified),receipt()).unwrap();
        let value=serde_json::json!({"answer":42});
        let ack=fleet_proto::CheckinAck { command_id:"command-1".into(),success:true,detail:None,result:Some(value.clone()),report_id:Some("report-1".into()),phase:None,terminal_reason:None };
        let outcome=fleet_proto::ActionOutcome { version:1,receipt_id:"receipt-1".into(),device_id:fleet_proto::DeviceId(trust.device_id.clone()),sequence:0,previous_hash:None,
            command_id:Some("command-1".into()),catalog_id:"device.checkin_now".into(),action_class:fleet_proto::ActionClass::Safe,outcome:fleet_proto::ActionOutcomeState::Pass,
            failure_stage:None,observed_at:"2026-01-01T00:00:00Z".into(),result_digest:serde_json::json!({"typed_result_sha256":fleet_proto::canonical_json_sha256(&value)}),record_hash:String::new(),signature:String::new() };
        let mut command=fleet_proto::SignedCommand { command_id:"command-1".into(), idempotency_key:"idempotency-1".into(), device_id:fleet_proto::DeviceId(trust.device_id.clone()), catalog_id:"device.checkin_now".into(), action_class:fleet_proto::ActionClass::Safe, payload:serde_json::json!({}), epoch_version:7, signature:String::new(), signer_key:trust.signer_key.clone() };
        let bytes=fleet_proto::canonical_command_bytes(&command.idempotency_key,&command.device_id.0,&command.catalog_id,command.action_class.as_wire_str(),&command.payload,command.epoch_version);
        command.signature=STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&bytes).to_bytes());
        let delivery=|| AuthenticatedDelivery { command:command.clone() };
        let execution=|| AuthenticatedExecution { outcome:outcome.clone(),ack:ack.clone() };
        let operator=STANDARD.encode(SigningKey::from_bytes(&[33;32]).verifying_key().as_bytes());
        let mut vault_command=command.clone();vault_command.catalog_id="encryption.status".into();
        let bytes=fleet_proto::canonical_command_bytes(&vault_command.idempotency_key,&vault_command.device_id.0,&vault_command.catalog_id,vault_command.action_class.as_wire_str(),&vault_command.payload,vault_command.epoch_version);
        vault_command.signature=STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&bytes).to_bytes());
        let vault_delivery=||AuthenticatedDelivery{command:vault_command.clone()};
        let make_vault_execution=|value:serde_json::Value| {
            let mut next=execution();next.outcome.catalog_id="encryption.status".into();
            next.outcome.result_digest=serde_json::json!({"typed_result_sha256":fleet_proto::canonical_json_sha256(&value)});next.ack.result=Some(value);next
        };
        assert!(authorize_result(vault_delivery(),make_vault_execution(serde_json::json!({"installed":true,"scope":"agent_authorized","mounted_count":2})),&trust,&operator,7).is_ok());
        assert!(authorize_result(vault_delivery(),make_vault_execution(serde_json::json!({"installed":true,"scope":"agent_authorized","mounted_count":2,"path":"secret"})),&trust,&operator,7).is_err());
        assert!(authorize_result(delivery(),execution(),&trust,&operator,8).is_err());
        let mut forged=delivery(); forged.command.signature.clear();
        assert!(authorize_result(forged,execution(),&trust,&operator,7).is_err());
        let authorized=authorize_result(delivery(),execution(),&trust,&operator,7).unwrap(); let archive="ab".repeat(32);
        let first=append_replacement(&fs,dir,&candidate,&trust,&verified,&archive,authorized.clone()).unwrap();
        assert_eq!(first.sequence,7); assert!(fleet_proto::verify_action_outcome(&first,&key));
        let batch=VerifiedJournalBatch { device_id:trust.device_id.clone(),generation:verified.new_generation().into(),
            outcomes:vec![first.clone();17],acks:vec![authorized.ack.clone();17] };
        let limited=limit_recovery_results(batch).unwrap();assert_eq!(limited.outcomes.len(),16);assert_eq!(limited.acks.len(),16);
        let mismatched=VerifiedJournalBatch { device_id:trust.device_id.clone(),generation:verified.new_generation().into(),
            outcomes:vec![first.clone()],acks:Vec::new() };
        assert!(limit_recovery_results(mismatched).is_err());
        let isolated=Fs(Mutex::new(fs.0.lock().unwrap().clone()));
        let confirmation=|hash:String| AuthenticatedBoundConfirmation { head:fleet_proto::ActionOutcomeHead { sequence:first.sequence,record_hash:first.record_hash.clone() },
            receipts:vec![RecoveryResultReceipt { command_id:authorized.ack.command_id.clone(),report_id:authorized.ack.report_id.clone(),ack_sha256:hash,
                outcome_receipt_id:first.receipt_id.clone(),sequence:first.sequence,record_hash:first.record_hash.clone(),disposition:"duplicate_accepted".into() }] };
        let unchanged=isolated.0.lock().unwrap().clone();
        assert!(confirm_replacement_bound(&isolated,dir,&trust,&verified,&archive,confirmation("00".repeat(32))).is_err());
        assert_eq!(*isolated.0.lock().unwrap(),unchanged);
        let hash=fleet_proto::canonical_json_sha256(&serde_json::to_value(&authorized.ack).unwrap());
        confirm_replacement_bound(&isolated,dir,&trust,&verified,&archive,confirmation(hash)).unwrap();
        let drained:ReplacementJournal=serde_json::from_slice(&isolated.read(&dir.join(format!("replacement-{}.json",verified.retirement_id()))).unwrap()).unwrap();
        assert!(drained.pending.is_empty() && drained.pending_acks.is_empty());
        // A server may have committed only the first pair before a lost reply.
        // A retry must retain both locally until every signed receipt matches.
        let retry_fs=Fs(Mutex::new(fs.0.lock().unwrap().clone()));
        let mut second_result=authorized.clone();second_result.outcome.command_id=Some("command-2".into());
        second_result.outcome.receipt_id="receipt-2".into();second_result.ack.command_id="command-2".into();second_result.ack.report_id=Some("report-2".into());
        let second_outcome=append_replacement(&retry_fs,dir,&candidate,&trust,&verified,&archive,second_result.clone()).unwrap();
        let make_receipt=|outcome:&fleet_proto::ActionOutcome,ack:&fleet_proto::CheckinAck,disposition:&str| RecoveryResultReceipt {
            command_id:ack.command_id.clone(),report_id:ack.report_id.clone(),ack_sha256:fleet_proto::canonical_json_sha256(&serde_json::to_value(ack).unwrap()),
            outcome_receipt_id:outcome.receipt_id.clone(),sequence:outcome.sequence,record_hash:outcome.record_hash.clone(),disposition:disposition.into() };
        let head=||fleet_proto::ActionOutcomeHead { sequence:second_outcome.sequence,record_hash:second_outcome.record_hash.clone() };
        let before_retry=retry_fs.0.lock().unwrap().clone();
        assert!(confirm_replacement_bound(&retry_fs,dir,&trust,&verified,&archive,AuthenticatedBoundConfirmation { head:head(),
            receipts:vec![make_receipt(&first,&authorized.ack,"duplicate_accepted")] }).is_err());
        assert_eq!(*retry_fs.0.lock().unwrap(),before_retry);
        confirm_replacement_bound(&retry_fs,dir,&trust,&verified,&archive,AuthenticatedBoundConfirmation { head:head(),receipts:vec![
            make_receipt(&first,&authorized.ack,"duplicate_accepted"),make_receipt(&second_outcome,&second_result.ack,"accepted")] }).unwrap();
        let before=fs.0.lock().unwrap().clone();
        assert_eq!(append_replacement(&fs,dir,&candidate,&trust,&verified,&archive,authorized.clone()).unwrap(),first);
        initialize_replacement(&fs,dir,&trust,&verified,&archive).unwrap();
        assert_eq!(*fs.0.lock().unwrap(),before);
        let mut changed=authorized.clone(); changed.ack.result=Some(serde_json::json!({"answer":43}));
        assert!(append_replacement(&fs,dir,&candidate,&trust,&verified,&archive,changed).is_err());
        assert_eq!(*fs.0.lock().unwrap(),before);
        let confirmation=AuthenticatedCheckinConfirmation {
            head:fleet_proto::ActionOutcomeHead { sequence:first.sequence, record_hash:first.record_hash.clone() },
            outcomes:vec![fleet_proto::ActionOutcomeReceipt { receipt_id:first.receipt_id.clone(), sequence:first.sequence, disposition:"accepted".into() }],
            acks:vec![fleet_proto::CheckinAckReceipt { command_id:"command-1".into(), report_id:Some("report-1".into()), disposition:"accepted".into(), resulting_status:None }],
        };
        assert!(confirm_replacement(&fs,dir,&trust,&verified,&archive,confirmation).is_err());
        initialize_replacement(&fs,dir,&trust,&verified,&archive).unwrap();
        let after_drain=fs.0.lock().unwrap().clone();
        assert_eq!(append_replacement(&fs,dir,&candidate,&trust,&verified,&archive,authorized).unwrap(),first);
        assert_eq!(*fs.0.lock().unwrap(),after_drain);

        // A grant and its ACK/outcome must commit as one journal replacement.
        struct InboxProtector;
        impl crate::fleet_credential::Protector for InboxProtector {
            fn protect(&self,p:&[u8],e:&[u8])->Result<Vec<u8>,&'static str> { let mut v=e.to_vec();v.extend_from_slice(p);Ok(v) }
            fn unprotect(&self,p:&[u8],e:&[u8])->Result<zeroize::Zeroizing<Vec<u8>>,&'static str> {
                if !p.starts_with(e) { return Err("invalid binding"); } Ok(zeroize::Zeroizing::new(p[e.len()..].to_vec()))
            }
        }
        use std::sync::atomic::{AtomicBool,Ordering};
        struct FailWrite<'a> { inner:&'a Fs,fail:AtomicBool }
        impl PolicyFs for FailWrite<'_> {
            fn read(&self,p:&Path)->std::io::Result<Vec<u8>> { self.inner.read(p) }
            fn atomic_write(&self,p:&Path,b:&[u8])->std::io::Result<()> {
                if self.fail.swap(false,Ordering::SeqCst) { return Err(std::io::ErrorKind::Other.into()); } self.inner.atomic_write(p,b)
            }
            fn ensure_dir_secure(&self,p:&Path)->Result<(),crate::policy_store::PolicyStoreError> { self.inner.ensure_dir_secure(p) }
        }
        let mut current=trust.clone(); current.enrollment_generation=verified.new_generation().into();
        let mut second=command.clone(); second.command_id="command-2".into(); second.idempotency_key="idempotency-2".into();
        second.signature=STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&fleet_proto::canonical_command_bytes(
            &second.idempotency_key,&second.device_id.0,&second.catalog_id,second.action_class.as_wire_str(),&second.payload,second.epoch_version)).to_bytes());
        persist_unified_batch(&fs,dir,&InboxProtector,&trust,&verified,&archive,AuthenticatedCommandBatch {
            trust:current.clone(),operator_key:operator.clone(),response_sha256:"ab".repeat(32),commands:vec![second] }).unwrap();
        mutate_unified_journal(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,|journal,inbox| {
            assert!(ensure_repeat_drained(journal,inbox,journal.next_sequence,journal.previous_hash.as_deref()).is_err()); Ok(())
        }).unwrap();
        let peer=AuthenticatedExecutionPeer { sid:"S-1-5-21-1".into(),session:1,logon:(1,0),pid:42,process_created:1 };
        let (_,token)=issue_unified_grant(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,&peer,"command-2",7,1000).unwrap();
        assert!(start_unified_grant(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,&peer,"command-2",&token,8,1001).is_err());
        start_unified_grant(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,&peer,"command-2",&token,7,1001).unwrap();
        assert!(start_unified_grant(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,&peer,"command-2",&token,7,1001).is_err());
        let exec=|| { let mut outcome=outcome.clone();outcome.command_id=Some("command-2".into());outcome.receipt_id="receipt-2".into();
            let mut ack=ack.clone();ack.command_id="command-2".into();ack.report_id=Some("report-2".into());AuthenticatedExecution { outcome,ack } };
        let replaced=AuthenticatedExecutionPeer { sid:peer.sid.clone(),session:peer.session,logon:peer.logon,pid:peer.pid,process_created:2 };
        assert!(finish_unified_grant(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,&replaced,"command-2",&token,7,1001,exec()).is_err());
        let failure=FailWrite { inner:&fs,fail:AtomicBool::new(true) }; let before_failure=fs.0.lock().unwrap().clone();
        assert!(finish_unified_grant(&failure,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,&peer,"command-2",&token,7,1001,exec()).is_err());
        assert_eq!(*fs.0.lock().unwrap(),before_failure);
        finish_unified_grant(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,&peer,"command-2",&token,8,2000,exec()).unwrap();
        let complete=fs.0.lock().unwrap().clone();
        finish_unified_grant(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,&peer,"command-2",&token,8,2000,exec()).unwrap();
        assert_eq!(*fs.0.lock().unwrap(),complete);
        let mut third=command.clone();third.command_id="command-3".into();third.idempotency_key="idempotency-3".into();
        third.signature=STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&fleet_proto::canonical_command_bytes(
            &third.idempotency_key,&third.device_id.0,&third.catalog_id,third.action_class.as_wire_str(),&third.payload,third.epoch_version)).to_bytes());
        let digest=command_digest(&third).unwrap();
        persist_unified_batch(&fs,dir,&InboxProtector,&trust,&verified,&archive,AuthenticatedCommandBatch {
            trust:current.clone(),operator_key:operator.clone(),response_sha256:"ab".repeat(32),commands:vec![third] }).unwrap();
        issue_unified_grant(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,&peer,"command-3",7,1000).unwrap();
        let approval=||UnknownApproval { actor_sid:"S-1-5-21-1".into(),session:1,logon:(1,0),command_digest:digest,expires:Instant::now()+Duration::from_secs(30) };
        let before=fs.0.lock().unwrap().clone();
        assert!(acknowledge_unknown(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,"command-3",approval(),2000,|_|false).is_err());
        assert_eq!(*fs.0.lock().unwrap(),before);
        acknowledge_unknown(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,"command-3",approval(),2000,|_|true).unwrap();
        let saved=fs.0.lock().unwrap().clone();
        assert!(issue_unified_grant(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,&peer,"command-3",7,2001).is_err());
        acknowledge_unknown(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,"command-3",approval(),2001,|_|true).unwrap();
        assert_eq!(*fs.0.lock().unwrap(),saved);
        let journal:ReplacementJournal=serde_json::from_slice(&fs.read(&dir.join(format!("replacement-{}.json",verified.retirement_id()))).unwrap()).unwrap();
        assert_eq!(journal.pending.last().unwrap().outcome,fleet_proto::ActionOutcomeState::Unknown);
        let mut fourth=command.clone();fourth.command_id="command-4".into();fourth.idempotency_key="idempotency-4".into();
        fourth.signature=STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&fleet_proto::canonical_command_bytes(
            &fourth.idempotency_key,&fourth.device_id.0,&fourth.catalog_id,fourth.action_class.as_wire_str(),&fourth.payload,fourth.epoch_version)).to_bytes());
        let digest=command_digest(&fourth).unwrap();
        persist_unified_batch(&fs,dir,&InboxProtector,&trust,&verified,&archive,AuthenticatedCommandBatch {
            trust:current.clone(),operator_key:operator.clone(),response_sha256:"ab".repeat(32),commands:vec![fourth] }).unwrap();
        let (_,token)=issue_unified_grant(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,&peer,"command-4",7,1000).unwrap();
        start_unified_grant(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,&peer,"command-4",&token,7,1001).unwrap();
        let approval=UnknownApproval { actor_sid:"S-1-5-21-1".into(),session:1,logon:(1,0),command_digest:digest,expires:Instant::now()+Duration::from_secs(30) };
        acknowledge_unknown(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,"command-4",approval,2000,|_|true).unwrap();
        let before_late:ReplacementJournal=serde_json::from_slice(&fs.read(&dir.join(format!("replacement-{}.json",verified.retirement_id()))).unwrap()).unwrap();
        let late=|| { let mut outcome=outcome.clone();outcome.command_id=Some("command-4".into());outcome.receipt_id="late-receipt".into();
            let mut ack=ack.clone();ack.command_id="command-4".into();ack.report_id=Some("late-report".into());AuthenticatedExecution { outcome,ack } };
        finish_unified_grant(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,&peer,"command-4",&token,8,3000,late()).unwrap();
        let after_late:ReplacementJournal=serde_json::from_slice(&fs.read(&dir.join(format!("replacement-{}.json",verified.retirement_id()))).unwrap()).unwrap();
        assert_eq!(before_late.pending,after_late.pending); assert_eq!(before_late.history,after_late.history);
        let saved=fs.0.lock().unwrap().clone();
        finish_unified_grant(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,&peer,"command-4",&token,8,3001,late()).unwrap();
        assert_eq!(*fs.0.lock().unwrap(),saved);
        let mut changed=late(); changed.ack.detail=Some("changed".into());
        assert!(finish_unified_grant(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,&peer,"command-4",&token,8,3001,changed).is_err());
        let path=dir.join(format!("replacement-{}.json",verified.retirement_id()));
        let mut journal:ReplacementJournal=serde_json::from_slice(&fs.read(&path).unwrap()).unwrap();
        let entropy:[u8;32]=Sha256::digest(serde_json::to_vec(&("fleet.recovery.command-inbox.v1",&current.org_id,&current.device_id,
            &current.enrollment_generation,&current.signer_key,&operator)).unwrap()).into();
        let sealed=STANDARD.decode(journal.inbox_sealed.as_deref().unwrap()).unwrap();
        let plain=crate::fleet_credential::Protector::unprotect(&InboxProtector,&sealed,&entropy).unwrap();
        let mut inbox:CommandInbox=serde_json::from_slice(&plain).unwrap();
        let entry=inbox.entries.iter().find(|entry|entry.command.command_id=="command-4").unwrap();
        let review=entry.unknown_review.as_ref().unwrap();let evidence=entry.late_evidence.as_ref().unwrap();
        let wire=serde_json::to_value(evidence).unwrap(); let keys=wire.as_object().unwrap();
        assert_eq!(keys.len(),5); assert!(!keys.contains_key("receipt"));
        for name in ["evidence_id","result_sha256","signature","outcome","ack"] { assert!(keys.contains_key(name)); }
        let nonce="ci-0123456789abcdef0123456789abcdef";
        let mut receipt=LateEvidenceReceipt { org_id:current.org_id.clone(),device_id:current.device_id.clone(),
            recovery_id:verified.retirement_id().into(),command_id:"command-4".into(),evidence_id:evidence.evidence_id.clone(),
            unknown_receipt_id:review.receipt_id.clone(),unknown_record_hash:review.record_hash.clone(),result_sha256:evidence.result_sha256.clone(),
            server_key:current.signer_key.clone(),request_nonce:nonce.into(),issued_at:3000,signature:String::new() };
        let bytes=serde_json::to_vec(&("wincommander:fleet:late-execution-evidence-receipt:v1",&receipt.org_id,&receipt.device_id,
            &receipt.recovery_id,&receipt.command_id,&receipt.evidence_id,&receipt.unknown_receipt_id,&receipt.unknown_record_hash,
            &receipt.result_sha256,&receipt.server_key,&receipt.request_nonce,receipt.issued_at)).unwrap();
        receipt.signature=STANDARD.encode(SigningKey::from_bytes(&[11;32]).sign(&bytes).to_bytes());
        let before_receipt=fs.read(&path).unwrap();
        assert!(record_late_receipt(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,"command-4","ci-ffffffffffffffffffffffffffffffff",3001,receipt.clone()).is_err());
        assert!(record_late_receipt(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,"command-4",nonce,4000,receipt.clone()).is_err());
        let mut forged=receipt.clone();forged.result_sha256="00".repeat(32);
        assert!(record_late_receipt(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,"command-4",nonce,3001,forged).is_err());
        let mut forged=receipt.clone();forged.signature=STANDARD.encode([0u8;64]);
        assert!(record_late_receipt(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,"command-4",nonce,3001,forged).is_err());
        assert_eq!(fs.read(&path).unwrap(),before_receipt);
        record_late_receipt(&fs,dir,&InboxProtector,&current,&trust,&verified,&archive,&operator,"command-4",nonce,3001,receipt).unwrap();
        // Reload the receipt-bearing journal before deliberately tampering it.
        journal=serde_json::from_slice(&fs.read(&path).unwrap()).unwrap();
        let sealed=STANDARD.decode(journal.inbox_sealed.as_deref().unwrap()).unwrap();
        let plain=crate::fleet_credential::Protector::unprotect(&InboxProtector,&sealed,&entropy).unwrap();
        inbox=serde_json::from_slice(&plain).unwrap();
        inbox.entries.iter_mut().find(|entry|entry.command.command_id=="command-4").unwrap().late_evidence.as_mut().unwrap().signature="forged".into();
        let plain=zeroize::Zeroizing::new(serde_json::to_vec(&inbox).unwrap());
        journal.inbox_sealed=Some(STANDARD.encode(crate::fleet_credential::Protector::protect(&InboxProtector,&plain,&entropy).unwrap()));
        fs.atomic_write(&path,&serde_json::to_vec(&journal).unwrap()).unwrap();
        assert!(finish_unified_grant(&fs,dir,&InboxProtector,&candidate,&current,&trust,&verified,&archive,&operator,&peer,"command-4",&token,8,3002,late()).is_err());
    }
    #[test]
    fn records_unknown_and_retries_without_rewriting_or_accepting_changed_archive() {
        let fs=Fs::default(); let (v,trust)=fixture(); let dir=Path::new("protected");
        persist_review(&fs,dir,&trust,&v,approval(&v),receipt()).unwrap();
        let before=fs.0.lock().unwrap().clone();
        persist_review(&fs,dir,&trust,&v,approval(&v),receipt()).unwrap();
        assert_eq!(*fs.0.lock().unwrap(),before);
        let mut changed=receipt(); changed.archive_sha256="ef".repeat(32);
        assert!(persist_review(&fs,dir,&trust,&v,approval(&v),changed).is_err());
        assert_eq!(*fs.0.lock().unwrap(),before);
        let bytes=before.get(&dir.join(format!("review-{}.json",v.retirement_id()))).unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(bytes).unwrap()["outcome"],"unknown_acknowledged");
    }
}

#[derive(serde::Serialize,serde::Deserialize,PartialEq)]
#[serde(deny_unknown_fields)]
struct ReplacementJournal {
    recovery_id:String, org_id:String, device_id:String, generation:String,
    decision_digest:String, candidate_fingerprint:String, archive_sha256:String,
    origin_sequence:u64, origin_hash:Option<String>, base_sequence:u64, base_hash:Option<String>, next_sequence:u64, previous_hash:Option<String>,
    pending:Vec<fleet_proto::ActionOutcome>, pending_acks:Vec<fleet_proto::CheckinAck>,
    history:Vec<RecordedCommand>,
    #[serde(default)] inbox_sealed:Option<String>,
    #[serde(default)] drain_freeze:Option<DrainFreeze>,
    #[serde(default)] cancelled_freezes:Vec<DrainFreeze>,
}
#[derive(Clone,serde::Serialize,serde::Deserialize,PartialEq)]
#[serde(deny_unknown_fields)]
struct DrainFreeze { next_recovery_id:String, next_generation:String, decision_digest:String, candidate_fingerprint:String, snapshot_sha256:String }
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DrainCancellation { org_id:String,device_id:String,current_generation:String,previous_recovery_id:String,next_recovery_id:String,
    next_generation:String,decision_digest:String,server_key:String,request_nonce:String,issued_at:i64,status:String,signature:String }
struct CancelApproval { sid:String,session:u32,logon:(u32,i32),device_id:String,recovery_id:String,decision_digest:String,expires:Instant }
fn confirm_cancellation(peer:&AuthenticatedPipePeer,device:&str,recovery:&str,decision:&str)->Result<(CancelApproval,SessionWitness),&'static str> {
    if !READY || !session_matches(peer) { return Err("cancellation confirmation unavailable"); }
    if uuid::Uuid::parse_str(device).ok().is_none_or(|id|id.is_nil() || id.to_string()!=device)
        || uuid::Uuid::parse_str(recovery).ok().is_none_or(|id|id.is_nil() || id.to_string()!=recovery)
        || !valid_hash(decision) { return Err("cancellation confirmation unavailable"); }
    let witness=SessionWitness::capture(peer).ok_or("interactive administrator required")?;
    let title:Vec<u16>="Cancel Fleet reconnection".encode_utf16().chain(std::iter::once(0)).collect();
    let body:Vec<u16>=format!("Cancel the pending Fleet reconnection for device {device}?\n\nRecovery ID: {recovery}\n\nThe previous device identity and evidence will be preserved locally. The Fleet record remains retired; this does not reconnect the device.")
        .encode_utf16().chain(std::iter::once(0)).collect();
    let mut response=0;
    let ok=unsafe { WTSSendMessageW(WTS_CURRENT_SERVER_HANDLE,peer.session_id(),title.as_ptr(),((title.len()-1)*2) as u32,
        body.as_ptr(),((body.len()-1)*2) as u32,MB_YESNO|MB_ICONWARNING|MB_DEFBUTTON2,60,&mut response,1) }!=0;
    if !ok || response!=IDYES || !witness.still_authorized() { return Err("cancellation confirmation denied"); }
    Ok((CancelApproval { sid:witness.sid.clone(),session:witness.session,logon:witness.logon,device_id:device.into(),
        recovery_id:recovery.into(),decision_digest:decision.into(),expires:Instant::now()+Duration::from_secs(120) },witness))
}
fn verify_drain_cancellation(receipt:&DrainCancellation,current:&crate::fleet_machine_policy::FleetTrust,previous:&str,
    freeze:&DrainFreeze,nonce:&str,now:i64)->Result<(),&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    if receipt.org_id!=current.org_id || receipt.device_id!=current.device_id || receipt.current_generation!=current.enrollment_generation
        || receipt.previous_recovery_id!=previous || receipt.next_recovery_id!=freeze.next_recovery_id || receipt.next_generation!=freeze.next_generation
        || receipt.decision_digest!=freeze.decision_digest || receipt.server_key!=current.signer_key || receipt.request_nonce!=nonce
        || receipt.status!="cancelled" || !crate::fleet_identity_handoff::recovery_timestamp_valid(receipt.issued_at,now)
        || uuid::Uuid::parse_str(nonce).ok().is_none_or(|id|id.is_nil() || id.to_string()!=nonce) { return Err("cancellation unavailable"); }
    let key=STANDARD.decode(&receipt.server_key).map_err(|_| "cancellation unavailable")?;
    let signature=STANDARD.decode(&receipt.signature).map_err(|_| "cancellation unavailable")?;
    if STANDARD.encode(&key)!=receipt.server_key || STANDARD.encode(&signature)!=receipt.signature { return Err("cancellation unavailable"); }
    let key=ed25519_dalek::VerifyingKey::from_bytes(&key.try_into().map_err(|_| "cancellation unavailable")?).map_err(|_| "cancellation unavailable")?;
    if key.is_weak() { return Err("cancellation unavailable"); }
    let signature=ed25519_dalek::Signature::from_bytes(&signature.try_into().map_err(|_| "cancellation unavailable")?);
    let bytes=serde_json::to_vec(&("wincommander:fleet:recovery-cancel-response:v1","POST","/v1/agents/identity-handoff/cancel",
        &receipt.org_id,&receipt.device_id,&receipt.current_generation,&receipt.previous_recovery_id,&receipt.next_recovery_id,
        &receipt.next_generation,&receipt.decision_digest,&receipt.server_key,&receipt.request_nonce,receipt.issued_at,&receipt.status))
        .map_err(|_| "cancellation unavailable")?;
    key.verify_strict(&bytes,&signature).map_err(|_| "cancellation unavailable")
}
fn clear_cancelled_freeze(journal:&mut ReplacementJournal,inbox:&CommandInbox,receipt:&DrainCancellation,current:&crate::fleet_machine_policy::FleetTrust,
    previous:&str,nonce:&str,now:i64,authorized:impl FnOnce()->bool)->Result<(),&'static str> {
    let freeze=journal.drain_freeze.as_ref().or_else(||journal.cancelled_freezes.iter().find(|item|
        item.next_recovery_id==receipt.next_recovery_id && item.decision_digest==receipt.decision_digest)).ok_or("cancellation unavailable")?;
    verify_drain_cancellation(receipt,current,previous,freeze,nonce,now)?;
    if journal.drain_freeze.is_some() && drain_snapshot(journal,inbox)?!=freeze.snapshot_sha256 { return Err("cancellation unavailable"); }
    if !authorized() { return Err("interactive administrator required"); }
    if let Some(freeze)=journal.drain_freeze.take() {
        if journal.cancelled_freezes.len()>=64 { journal.drain_freeze=Some(freeze);return Err("cancellation history full"); }
        journal.cancelled_freezes.push(freeze);
    }
    Ok(())
}
/// Unmounted; the caller must obtain the signed server cancellation using a
/// fresh request and capture the interactive administrator before networking.
fn unfreeze_cancelled_recovery(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,
    response:&[u8],nonce:&str,witness:&SessionWitness,approval:&CancelApproval)->Result<(),&'static str> {
    if response.len()>16384 || !witness.still_authorized() || approval.expires<=Instant::now()
        || approval.sid!=witness.sid || approval.session!=witness.session || approval.logon!=witness.logon { return Err("cancellation unavailable"); }
    let receipt:DrainCancellation=serde_json::from_slice(response).map_err(|_| "cancellation unavailable")?;
    if approval.device_id!=receipt.device_id || approval.recovery_id!=receipt.next_recovery_id || approval.decision_digest!=receipt.decision_digest
        { return Err("cancellation unavailable"); }
    let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
    if store.pending_recovery()?.is_some() {
        store.rollback_cancelled_prepare(|old,previous,next,generation,decision| {
            let binding=DrainFreeze { next_recovery_id:next.into(),next_generation:generation.into(),decision_digest:decision.into(),
                candidate_fingerprint:String::new(),snapshot_sha256:String::new() };
            verify_drain_cancellation(&receipt,old,previous,&binding,nonce,now)?;
            if approval.expires<=Instant::now() || !witness.still_authorized() { return Err("interactive administrator required"); }
            Ok(())
        })?;
    }
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,previous,archive,operator,_| {
        mutate_unified_journal_inner(fs,dir,protector,current,old,previous,archive,operator,true,|journal,inbox| {
            clear_cancelled_freeze(journal,inbox,&receipt,current,previous.retirement_id(),nonce,now,
                ||approval.expires>Instant::now() && witness.still_authorized())
        })
    })
}
#[derive(Debug,serde::Serialize,serde::Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
struct RecordedCommand { command_id:String, receipt_id:String, sequence:u64, record_hash:String }
static REPLACEMENT_LOCK:Mutex<()> = Mutex::new(());
/// Creates a distinct service-owned journal only after a protected review exists.
/// It never opens, deletes, or modifies the old Pro journal or its archive.
pub(crate) fn initialize_replacement(fs:&dyn crate::policy_store::PolicyFs, dir:&std::path::Path,
    trust:&crate::fleet_machine_policy::FleetTrust, verified:&VerifiedHandoff, archive_hash:&str)
    -> Result<(), &'static str> {
    if !verified.matches_trust(trust) { return Err("replacement journal binding changed"); }
    fs.ensure_dir_secure(dir).map_err(|_| "replacement journal unavailable")?;
    let review_path=dir.join(format!("review-{}.json",verified.retirement_id()));
    let review:ReviewRecord=serde_json::from_slice(&fs.read(&review_path).map_err(|_| "replacement journal unavailable")?)
        .map_err(|_| "replacement journal unavailable")?;
    if review.outcome!="unknown_acknowledged" || review.org_id!=trust.org_id || review.device_id!=trust.device_id
        || review.old_generation!=trust.enrollment_generation || review.recovery_id!=verified.retirement_id()
        || review.new_generation!=verified.new_generation() || review.decision_digest!=lower_hex(verified.decision_digest())
        || review.candidate_fingerprint!=lower_hex(verified.fingerprint()) || review.archive_sha256!=archive_hash {
        return Err("replacement journal binding changed");
    }
    let next=ReplacementJournal { recovery_id:verified.retirement_id().into(), org_id:trust.org_id.clone(),
        device_id:trust.device_id.clone(), generation:verified.new_generation().into(),
        decision_digest:lower_hex(verified.decision_digest()), candidate_fingerprint:lower_hex(verified.fingerprint()),
        archive_sha256:archive_hash.into(), origin_sequence:verified.next_sequence(), origin_hash:verified.previous_hash().map(str::to_owned),
        base_sequence:verified.next_sequence(), base_hash:verified.previous_hash().map(str::to_owned),
        next_sequence:verified.next_sequence(), previous_hash:verified.previous_hash().map(str::to_owned), pending:Vec::new(), pending_acks:Vec::new(), history:Vec::new(), inbox_sealed:None,drain_freeze:None,cancelled_freezes:Vec::new() };
    let _guard=REPLACEMENT_LOCK.lock().map_err(|_| "replacement journal unavailable")?;
    let path=dir.join(format!("replacement-{}.json",verified.retirement_id()));
    match fs.read(&path) {
        Ok(bytes) => {
            let existing:ReplacementJournal=serde_json::from_slice(&bytes).map_err(|_| "replacement journal unavailable")?;
            if !valid_replacement(&existing, &next, verified) { return Err("replacement journal binding changed"); }
            Ok(())
        }
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => {
            fs.atomic_write(&path,&serde_json::to_vec(&next).map_err(|_| "replacement journal unavailable")?)
                .map_err(|_| "replacement journal unavailable")
        }
        Err(_) => Err("replacement journal unavailable"),
    }
}

fn outcome_matches_ack(outcome:&fleet_proto::ActionOutcome, ack:&fleet_proto::CheckinAck) -> bool {
    let expected=fleet_proto::canonical_json_sha256(ack.result.as_ref().unwrap_or(&serde_json::Value::Null));
    outcome.command_id.as_deref()==Some(ack.command_id.as_str())
        && outcome.result_digest.get("typed_result_sha256").and_then(serde_json::Value::as_str)==Some(expected.as_str())
        && match outcome.outcome {
            fleet_proto::ActionOutcomeState::Pass => ack.success && outcome.failure_stage.is_none(),
            fleet_proto::ActionOutcomeState::Fail | fleet_proto::ActionOutcomeState::Unknown => !ack.success && outcome.failure_stage.is_some(),
        }
}
fn valid_replacement(journal:&ReplacementJournal, initial:&ReplacementJournal, verified:&VerifiedHandoff) -> bool {
    use base64::{engine::general_purpose::STANDARD,Engine};
    if journal.recovery_id!=initial.recovery_id || journal.org_id!=initial.org_id || journal.device_id!=initial.device_id
        || journal.generation!=initial.generation || journal.decision_digest!=initial.decision_digest
        || journal.candidate_fingerprint!=initial.candidate_fingerprint || journal.archive_sha256!=initial.archive_sha256
        || journal.origin_sequence!=initial.origin_sequence || journal.origin_hash!=initial.origin_hash
        || journal.base_sequence<journal.origin_sequence
        || journal.pending.len()!=journal.pending_acks.len() || journal.pending.len()>32 || journal.history.len()>4096
        || journal.inbox_sealed.as_ref().is_some_and(|value|value.len()>6*1024*1024) || journal.cancelled_freezes.len()>64 { return false; }
    let mut commands=std::collections::HashSet::new(); let mut receipts=std::collections::HashSet::new(); let mut sequences=std::collections::HashSet::new();
    if journal.history.iter().any(|entry| entry.command_id.is_empty() || entry.receipt_id.is_empty() || !valid_hash(&entry.record_hash)
        || entry.sequence<journal.origin_sequence || entry.sequence>=journal.next_sequence || !commands.insert(&entry.command_id) || !receipts.insert(&entry.receipt_id) || !sequences.insert(entry.sequence)) { return false; }
    if journal.next_sequence.checked_sub(journal.origin_sequence)!=Some(journal.history.len() as u64)
        || journal.history.iter().enumerate().any(|(index,entry)| journal.origin_sequence.checked_add(index as u64)!=Some(entry.sequence)) { return false; }
    if journal.base_sequence==journal.origin_sequence {
        if journal.base_hash!=journal.origin_hash { return false; }
    } else if !journal.history.iter().any(|entry| entry.sequence.checked_add(1)==Some(journal.base_sequence) && Some(&entry.record_hash)==journal.base_hash.as_ref()) { return false; }
    let key=STANDARD.encode(verified.public_key());
    let mut sequence=journal.base_sequence; let mut previous=journal.base_hash.clone();
    for (outcome,ack) in journal.pending.iter().zip(&journal.pending_acks) {
        if outcome.device_id.0!=journal.device_id || outcome.sequence!=sequence || outcome.previous_hash!=previous
            || !outcome_matches_ack(outcome,ack) || !fleet_proto::verify_action_outcome(outcome,&key)
            || !journal.history.iter().any(|entry| entry.command_id==ack.command_id && entry.receipt_id==outcome.receipt_id && entry.sequence==outcome.sequence && entry.record_hash==outcome.record_hash) { return false; }
        let Some(next)=sequence.checked_add(1) else { return false }; sequence=next;
        previous=Some(outcome.record_hash.clone());
    }
    journal.next_sequence==sequence && journal.previous_hash==previous
}

/// Only a future authenticated broker that verifies a signed command and its
/// result may construct this witness; no IPC route exposes this operation.
#[derive(Clone)]
struct AuthorizedResult { outcome:fleet_proto::ActionOutcome, ack:fleet_proto::CheckinAck }
fn append_replacement(fs:&dyn crate::policy_store::PolicyFs, dir:&std::path::Path,
    candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>, trust:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff, archive_hash:&str, authorized:AuthorizedResult)
    -> Result<fleet_proto::ActionOutcome, &'static str> {
    initialize_replacement(fs,dir,trust,verified,archive_hash)?;
    let _guard=REPLACEMENT_LOCK.lock().map_err(|_| "replacement journal unavailable")?;
    let path=dir.join(format!("replacement-{}.json",verified.retirement_id()));
    let mut journal:ReplacementJournal=serde_json::from_slice(&fs.read(&path).map_err(|_| "replacement journal unavailable")?)
        .map_err(|_| "replacement journal unavailable")?;
    if journal.drain_freeze.is_some() { return Err("recovery blocked: previous work admission is frozen"); }
    let outcome=append_to_journal(&mut journal,candidate,trust,verified,authorized)?;
    fs.atomic_write(&path,&serde_json::to_vec(&journal).map_err(|_| "replacement journal unavailable")?)
        .map_err(|_| "replacement journal unavailable")?;
    Ok(outcome)
}
fn append_to_journal(journal:&mut ReplacementJournal,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,
    trust:&crate::fleet_machine_policy::FleetTrust,verified:&VerifiedHandoff,mut authorized:AuthorizedResult)
    ->Result<fleet_proto::ActionOutcome,&'static str> {
    if !authorized.outcome.signature.is_empty() || !authorized.outcome.record_hash.is_empty()
        || authorized.outcome.device_id.0!=trust.device_id || !outcome_matches_ack(&authorized.outcome,&authorized.ack) {
        return Err("invalid command result");
    }
    if let Some((existing,ack))=journal.pending.iter().zip(&journal.pending_acks)
        .find(|(outcome,_)| outcome.receipt_id==authorized.outcome.receipt_id) {
        authorized.outcome.sequence=existing.sequence; authorized.outcome.previous_hash=existing.previous_hash.clone();
        if fleet_proto::action_outcome_hash(&authorized.outcome)==existing.record_hash && *ack==authorized.ack { return Ok(existing.clone()); }
        return Err("command result changed");
    }
    if journal.pending.len()>=32 || journal.history.len()>=4096
        || journal.history.iter().any(|entry| entry.command_id==authorized.ack.command_id || entry.receipt_id==authorized.outcome.receipt_id) {
        return Err("replacement journal full or duplicate command");
    }
    authorized.outcome.sequence=journal.next_sequence; authorized.outcome.previous_hash=journal.previous_hash.clone();
    candidate.sign_outcome(trust,verified.retirement_id(),verified.fingerprint(),&mut authorized.outcome)?;
    journal.next_sequence=journal.next_sequence.checked_add(1).ok_or("replacement journal unavailable")?;
    journal.previous_hash=Some(authorized.outcome.record_hash.clone());
    journal.history.push(RecordedCommand { command_id:authorized.ack.command_id.clone(), receipt_id:authorized.outcome.receipt_id.clone(), sequence:authorized.outcome.sequence, record_hash:authorized.outcome.record_hash.clone() });
    journal.pending.push(authorized.outcome.clone()); journal.pending_acks.push(authorized.ack);
    Ok(authorized.outcome)
}

// A future service-owned, authenticated check-in reader is the only component
// allowed to construct delivery/response witnesses. No IPC decoder does so.
async fn check_recovered_leave(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>) -> Result<serde_json::Value,&'static str> {
    if let Some((trust,token))=store.confirmed_leave()? {
        return Ok(serde_json::json!({"approved":true,"status":"approved","serviceTrust":{"orgId":trust.org_id,"deviceId":trust.device_id,"enrollmentGeneration":trust.enrollment_generation,"signerKey":trust.signer_key},"serviceFenceToken":token}));
    }
    let snapshot=store.credential_snapshot(protector)?.ok_or("leave status unavailable")?;
    if !snapshot.is_recovered || !store.credential_is_current(&snapshot)
        || crate::fleet_runtime_env::fingerprint().as_deref()!=Some(snapshot.profile.environment_fingerprint.as_str()) { return Err("leave status unavailable") }
    let mut random=[0u8;32]; SysRng.try_fill_bytes(&mut random).map_err(|_|"randomness unavailable")?;
    let nonce=lower_hex(&random);
    let request=store.with_leave_identity(&snapshot,|current,old,recovery,fingerprint| {
        if fingerprint.len()!=64 || !fingerprint.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {return Err("invalid recovery identity")}
        let mut fp=[0u8;32]; for (i,byte) in fp.iter_mut().enumerate() {
            *byte=u8::from_str_radix(&fingerprint[i*2..i*2+2],16).map_err(|_|"invalid recovery identity")?;
        }
        candidate.sign_unenroll_status(old,recovery,current,&fp,&nonce)
    })?;
    let proof=crate::fleet_recovery_leave::terminal_status(store,&snapshot,request,&nonce).await?;
    if let Some(terminal)=proof.terminal {
        let (trust,_,clear,connection,revision)=store.contact_state()?.ok_or("leave status unavailable")?;
        if trust!=snapshot.trust || revision!=snapshot.revision || !store.clear_after_verified_leave(clear.as_deref().ok_or("leave status unavailable")?,&connection,&terminal)? {return Err("leave identity changed")}
        let (_,token)=store.confirmed_leave()?.ok_or("leave cleanup unavailable")?;
        return Ok(serde_json::json!({"approved":true,"status":"approved","serviceTrust":{"orgId":trust.org_id,"deviceId":trust.device_id,"enrollmentGeneration":trust.enrollment_generation,"signerKey":trust.signer_key},"serviceFenceToken":token}));
    }
    let _=(fs,dir);
    Ok(serde_json::json!({"approved":false,"status":proof.status,"approvals":proof.approvals,"required_approvals":proof.required}))
}
#[cfg(test)]
pub(crate) async fn test_simple_leave_status(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>)->Result<serde_json::Value,&'static str> {
    check_recovered_leave(store,fs,dir,protector,candidate).await
}
pub(super) async fn recovered_leave_status(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>)->Result<serde_json::Value,&'static str> {
    check_recovered_leave(store,fs,dir,protector,candidate).await
}

struct AuthenticatedDelivery { command:fleet_proto::SignedCommand }
struct AuthenticatedExecution { outcome:fleet_proto::ActionOutcome, ack:fleet_proto::CheckinAck }
fn execution_hash(outcome:&fleet_proto::ActionOutcome,ack:&fleet_proto::CheckinAck)->Result<String,&'static str> {
    let mut value=serde_json::to_value((outcome,ack)).map_err(|_| "invalid command result")?;
    let bytes=zeroize::Zeroizing::new(fleet_proto::canonical_json_bytes(&value));
    wincmd_shared::zeroize_json(&mut value);
    if bytes.len()>2*1024*1024 { return Err("invalid command result"); }
    Ok(lower_hex(&Sha256::digest(bytes.as_slice())))
}
struct AuthenticatedCommandBatch { trust:crate::fleet_machine_policy::FleetTrust, operator_key:String, response_sha256:String, commands:Vec<fleet_proto::SignedCommand> }
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandInbox { org_id:String, device_id:String, generation:String, operator_key:String, entries:Vec<InboxCommand> }

/// A subsequent generation may not strand work or evidence which its server
/// credential cannot upload. Call only after validating the journal and inbox,
/// while holding protected trust and the replacement journal lock.
fn ensure_repeat_drained(journal:&ReplacementJournal,inbox:&CommandInbox,next_sequence:u64,previous_hash:Option<&str>) -> Result<(),&'static str> {
    if !journal.pending.is_empty() || !journal.pending_acks.is_empty() || journal.base_sequence!=journal.next_sequence
        || journal.base_hash!=journal.previous_hash || inbox.entries.iter().any(|entry|entry.state!="completed"
            || entry.late_evidence.as_ref().is_some_and(|late|late.receipt.is_none())) {
        return Err("recovery blocked: previous work or evidence is pending");
    }
    if journal.next_sequence!=next_sequence || journal.previous_hash.as_deref()!=previous_hash {
        return Err("recovery blocked: previous outcome head differs");
    }
    Ok(())
}
fn freeze_for_next(journal:&mut ReplacementJournal,inbox:&CommandInbox,next:&VerifiedHandoff)->Result<String,&'static str> {
    ensure_repeat_drained(journal,inbox,next.next_sequence(),next.previous_hash())?;
    if journal.cancelled_freezes.iter().any(|item|item.next_recovery_id==next.retirement_id()) { return Err("recovery was cancelled"); }
    let snapshot=drain_snapshot(journal,inbox)?;
    let wanted=DrainFreeze { next_recovery_id:next.retirement_id().into(),next_generation:next.new_generation().into(),
        decision_digest:lower_hex(next.decision_digest()),candidate_fingerprint:lower_hex(next.fingerprint()),snapshot_sha256:snapshot.clone() };
    if journal.drain_freeze.as_ref().is_some_and(|existing|existing!=&wanted) { return Err("recovery blocked: drain freeze changed"); }
    journal.drain_freeze=Some(wanted);Ok(snapshot)
}
fn drain_snapshot(journal:&ReplacementJournal,inbox:&CommandInbox)->Result<String,&'static str> {
    let mut journal_value=serde_json::to_value(&*journal).map_err(|_| "drain proof unavailable")?;
    let fields=journal_value.as_object_mut().ok_or("drain proof unavailable")?;
    fields.remove("inbox_sealed");fields.remove("drain_freeze");
    let mut value=serde_json::json!(["wincommander:fleet:recovery-drain-snapshot:v1",journal_value,inbox]);
    let bytes=zeroize::Zeroizing::new(serde_json::to_vec(&value).map_err(|_| "drain proof unavailable")?);
    let snapshot=lower_hex(&Sha256::digest(bytes.as_slice()));wincmd_shared::zeroize_json(&mut value);
    Ok(snapshot)
}

/// Unmounted proof generator. Trust and journal remain locked until the proof
/// is signed; the server must consume its nonce and recheck the head atomically.
fn issue_drain_statement(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,
    handoff:&Handoff,next_id:&str,next_generation:&str,now:i64)->Result<serde_json::Value,&'static str> {
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,previous,archive,operator,_| {
        let key=candidate.public_key(current,next_id)?;
        let next=handoff.verify(current,next_id,next_generation,&key,now)?;
        mutate_unified_journal_inner(fs,dir,protector,current,old,previous,archive,operator,true,|journal,inbox| {
            let snapshot=freeze_for_next(journal,inbox,&next)?;
            let mut random=[0u8;16];SysRng.try_fill_bytes(&mut random).map_err(|_| "randomness unavailable")?;
            random[6]=(random[6]&15)|64;random[8]=(random[8]&63)|128;let nonce=uuid::Uuid::from_bytes(random).to_string();
            let signature=candidate.sign_drain(old,current,previous.retirement_id(),&next,&snapshot,now,&nonce)?;
            Ok(serde_json::json!({"org_id":current.org_id,"device_id":current.device_id,"current_generation":current.enrollment_generation,
                "previous_recovery_id":previous.retirement_id(),"next_recovery_id":next.retirement_id(),"next_generation":next.new_generation(),
                "next_candidate_fingerprint":lower_hex(next.fingerprint()),"decision_digest":lower_hex(next.decision_digest()),
                "next_sequence":next.next_sequence(),"previous_hash":next.previous_hash(),"journal_snapshot_sha256":snapshot,
                "server_key":current.signer_key,"issued_at":now,"nonce":nonce,"signature":signature}))
        })
    })
}

/// Unmounted active-generation cancellation request. Prepared-state requests
/// require resolving the same prior identity from the protected rollback snapshot.
fn issue_cancel_request(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,
    witness:&SessionWitness,approval:&CancelApproval)->Result<serde_json::Value,&'static str> {
    if !witness.still_authorized() || approval.expires<=Instant::now() || approval.sid!=witness.sid
        || approval.session!=witness.session || approval.logon!=witness.logon { return Err("cancellation unavailable"); }
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,previous,archive,operator,_| {
        mutate_unified_journal_inner(fs,dir,protector,current,old,previous,archive,operator,true,|journal,inbox| {
            let freeze=journal.drain_freeze.as_ref().ok_or("cancellation unavailable")?;
            if approval.device_id!=current.device_id || approval.recovery_id!=freeze.next_recovery_id
                || approval.decision_digest!=freeze.decision_digest || drain_snapshot(journal,inbox)?!=freeze.snapshot_sha256
                || approval.expires<=Instant::now() || !witness.still_authorized() { return Err("cancellation unavailable"); }
            let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
            let mut random=[0u8;16];SysRng.try_fill_bytes(&mut random).map_err(|_| "randomness unavailable")?;
            random[6]=(random[6]&15)|64;random[8]=(random[8]&63)|128;let nonce=uuid::Uuid::from_bytes(random).to_string();
            let signature=candidate.sign_cancel(old,current,previous.retirement_id(),&freeze.next_recovery_id,&freeze.next_generation,&freeze.decision_digest,now,&nonce)?;
            Ok(serde_json::json!({"org_id":current.org_id,"device_id":current.device_id,"current_generation":current.enrollment_generation,
                "previous_recovery_id":previous.retirement_id(),"next_recovery_id":freeze.next_recovery_id,"next_generation":freeze.next_generation,
                "decision_digest":freeze.decision_digest,"server_key":current.signer_key,"issued_at":now,"nonce":nonce,"signature":signature}))
        })
    })
}

/// Caller holds the machine trust lock throughout this check and the ensuing
/// commit. Every normal writer acquires that lock before the journal lock.
pub(crate) fn check_previous_recovery_drained(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,current:&crate::fleet_machine_policy::FleetTrust,
    old:&crate::fleet_machine_policy::FleetTrust,recovery:&str,decision:&str,archive:&str,fingerprint:&str,
    operator:&str,next:&VerifiedHandoff)->Result<(),&'static str> {
    if !next.matches_trust(current) { return Err("recovery trust changed"); }
    let id=uuid::Uuid::parse_str(recovery).map_err(|_| "previous recovery unavailable")?;
    if id.is_nil() || id.to_string()!=recovery { return Err("previous recovery unavailable"); }
    let candidate=crate::fleet_recovery_candidate::CandidateStore::open(fs,dir.to_owned(),protector)?;
    let key=candidate.public_key(old,recovery)?;
    if lower_hex(&Sha256::digest(key))!=fingerprint { return Err("previous recovery unavailable"); }
    let bytes=fs.read(&dir.join(format!("historical-handoff-{id}.json"))).map_err(|_| "previous recovery unavailable")?;
    if bytes.len()>16384 { return Err("previous recovery unavailable"); }
    let handoff:Handoff=serde_json::from_slice(&bytes).map_err(|_| "previous recovery unavailable")?;
    let verified=handoff.verify_historical(old,recovery,&current.enrollment_generation,&key,decision)?;
    mutate_unified_journal_inner(fs,dir,protector,current,old,&verified,archive,operator,true,|journal,inbox| {
        if journal.drain_freeze.is_none() { return Err("recovery blocked: drain proof required"); }
        freeze_for_next(journal,inbox,next).map(|_|())
    })
}
#[cfg(test)]
pub(crate) fn test_freeze_drain(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,current:&crate::fleet_machine_policy::FleetTrust,
    old:&crate::fleet_machine_policy::FleetTrust,previous:&VerifiedHandoff,archive:&str,operator:&str,next:&VerifiedHandoff)->Result<(),&'static str> {
    mutate_unified_journal_inner(fs,dir,protector,current,old,previous,archive,operator,true,|journal,inbox|freeze_for_next(journal,inbox,next).map(|_|()))
}
#[cfg(test)]
pub(crate) fn test_frozen_admission(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,current:&crate::fleet_machine_policy::FleetTrust,
    old:&crate::fleet_machine_policy::FleetTrust,previous:&VerifiedHandoff,archive:&str,operator:&str)->Result<(),&'static str> {
    mutate_unified_journal(fs,dir,protector,current,old,previous,archive,operator,|_,_|Ok(()))
}
#[cfg(test)]
pub(crate) fn test_cancel_drain(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,current:&crate::fleet_machine_policy::FleetTrust,
    old:&crate::fleet_machine_policy::FleetTrust,previous:&VerifiedHandoff,archive:&str,operator:&str,
    bytes:&[u8],nonce:&str,now:i64,authorized:bool)->Result<(),&'static str> {
    let receipt:DrainCancellation=serde_json::from_slice(bytes).map_err(|_| "cancellation unavailable")?;
    mutate_unified_journal_inner(fs,dir,protector,current,old,previous,archive,operator,true,|journal,inbox|
        clear_cancelled_freeze(journal,inbox,&receipt,current,previous.retirement_id(),nonce,now,||authorized))
}
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InboxCommand { command:fleet_proto::SignedCommand, response_sha256:String, state:String, #[serde(default)] grant:Option<InboxGrant>, #[serde(default)] result_sha256:Option<String>, #[serde(default)] unknown_review:Option<UnknownReview>, #[serde(default)] late_evidence:Option<LateEvidence> }
#[derive(Clone,serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UnknownReview { actor_sid:String, reviewed_at:i64, receipt_id:String, record_hash:String }
#[derive(Clone,serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LateEvidence { evidence_id:String, result_sha256:String, signature:String, outcome:fleet_proto::ActionOutcome, ack:fleet_proto::CheckinAck, #[serde(default,skip_serializing_if="Option::is_none")] receipt:Option<LateEvidenceReceipt> }
#[derive(Clone,serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LateEvidenceReceipt { org_id:String,device_id:String,recovery_id:String,command_id:String,evidence_id:String,
    unknown_receipt_id:String,unknown_record_hash:String,result_sha256:String,server_key:String,request_nonce:String,issued_at:i64,signature:String }
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InboxGrant { token_sha256:String, sid:String, session:u32, logon:(u32,i32), pid:u32, #[serde(default)] process_created:u64, expires:i64, #[serde(default)] start_epoch:Option<i64>, #[serde(default)] started_at:Option<i64> }
struct AuthenticatedExecutionPeer { sid:String, session:u32, logon:(u32,i32), pid:u32, process_created:u64 }
fn process_creation_time(pid:u32)->Option<u64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetProcessTimes,OpenProcess,PROCESS_QUERY_LIMITED_INFORMATION};
    let process=unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid) };
    if process.is_null() { return None; }
    let mut create=FILETIME { dwLowDateTime:0,dwHighDateTime:0 };
    let mut exit=create; let mut kernel=create; let mut user=create;
    let ok=unsafe { GetProcessTimes(process,&mut create,&mut exit,&mut kernel,&mut user) }!=0;
    unsafe { CloseHandle(process); }
    ok.then_some((u64::from(create.dwHighDateTime)<<32)|u64::from(create.dwLowDateTime)).filter(|time|*time!=0)
}
fn execution_peer(peer:&AuthenticatedPipePeer)->Result<AuthenticatedExecutionPeer,&'static str> {
    if !peer_has_active_interactive_session(peer) || peer.session_id()==0 || peer.client_pid()==0 { return Err("execution authorization unavailable"); }
    Ok(AuthenticatedExecutionPeer { sid:peer.caller_sid().into(),session:peer.session_id(),logon:peer.authentication_id(),pid:peer.client_pid(),
        process_created:process_creation_time(peer.client_pid()).ok_or("execution authorization unavailable")? })
}
pub(super) fn ready_for_pro(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,
    candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,peer:&AuthenticatedPipePeer)->Result<serde_json::Value,&'static str> {
    let _principal=execution_peer(peer)?;
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,verified,archive,operator,epoch| {
        mutate_unified_journal(fs,dir,protector,current,old,verified,archive,operator,|_,inbox| {
            Ok(ready_command_descriptors(inbox,epoch))
        })
    })
}
fn ready_command_descriptors(inbox:&CommandInbox,epoch:i64)->serde_json::Value {
    let ready:Vec<_>=inbox.entries.iter().filter(|entry|entry.state=="ready" && entry.command.epoch_version==epoch).take(1024)
        .map(|entry|serde_json::json!({"command_id":entry.command.command_id,"catalog_id":entry.command.catalog_id,"action_class":entry.command.action_class})).collect();
    serde_json::json!({"commands":ready})
}

/// The pipe dispatcher must first apply the exact installed-Pro identity gate.
pub(super) fn grant_for_pro(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,
    candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,peer:&AuthenticatedPipePeer,command_id:&str)
    ->Result<serde_json::Value,&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    let principal=execution_peer(peer)?;
    let now=SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs();
    let now=i64::try_from(now).map_err(|_| "clock unavailable")?;
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,verified,archive,operator,epoch| {
        let (command,token)=issue_unified_grant(fs,dir,protector,current,old,verified,archive,operator,&principal,command_id,epoch,now)?;
        Ok(serde_json::json!({"command":command,"grant":STANDARD.encode(token.as_slice())}))
    })
}

/// A returned result is accepted only against the issuing Pro process and the
/// exact durable one-use grant; journal completion is one atomic write.
pub(super) fn result_from_pro(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,
    candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,peer:&AuthenticatedPipePeer,
    command_id:&str,grant:&str,outcome:fleet_proto::ActionOutcome,ack:fleet_proto::CheckinAck)->Result<(),&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    let principal=execution_peer(peer)?;
    let raw=zeroize::Zeroizing::new(STANDARD.decode(grant).map_err(|_| "execution authorization unavailable")?);
    if STANDARD.encode(raw.as_slice())!=grant { return Err("execution authorization unavailable"); }
    let token:zeroize::Zeroizing<[u8;32]>=zeroize::Zeroizing::new(raw.as_slice().try_into().map_err(|_| "execution authorization unavailable")?);
    let now=SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs();
    let now=i64::try_from(now).map_err(|_| "clock unavailable")?;
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,verified,archive,operator,epoch| {
        finish_unified_grant(fs,dir,protector,candidate,current,old,verified,archive,operator,&principal,command_id,&token,epoch,now,
            AuthenticatedExecution { outcome,ack })
    })
}

pub(super) fn start_for_pro(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,
    candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,peer:&AuthenticatedPipePeer,
    command_id:&str,grant:&str)->Result<(),&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    let principal=execution_peer(peer)?;
    let raw=zeroize::Zeroizing::new(STANDARD.decode(grant).map_err(|_| "execution authorization unavailable")?);
    if STANDARD.encode(raw.as_slice())!=grant { return Err("execution authorization unavailable"); }
    let token:zeroize::Zeroizing<[u8;32]>=zeroize::Zeroizing::new(raw.as_slice().try_into().map_err(|_| "execution authorization unavailable")?);
    let now=SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs();
    let now=i64::try_from(now).map_err(|_| "clock unavailable")?;
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,verified,archive,operator,epoch| {
        start_unified_grant(fs,dir,protector,current,old,verified,archive,operator,&principal,command_id,&token,epoch,now)
    })
}
impl Drop for LateEvidence { fn drop(&mut self) { wincmd_shared::zeroize_json(&mut self.outcome.result_digest); if let Some(result)=&mut self.ack.result { wincmd_shared::zeroize_json(result); } } }
impl Drop for InboxCommand { fn drop(&mut self) { wincmd_shared::zeroize_json(&mut self.command.payload); } }
#[cfg(test)] static INBOX_LOCK:Mutex<()>=Mutex::new(());
#[cfg(test)]
fn command_inbox_path(dir:&std::path::Path, trust:&crate::fleet_machine_policy::FleetTrust)->Result<std::path::PathBuf,&'static str> {
    let id=uuid::Uuid::parse_str(&trust.enrollment_generation).map_err(|_| "command inbox unavailable")?;
    if id.is_nil() || id.to_string()!=trust.enrollment_generation { return Err("command inbox unavailable"); }
    Ok(dir.join(format!("recovery-command-inbox-{id}.dpapi")))
}
fn valid_inbox_command(command:&fleet_proto::SignedCommand, trust:&crate::fleet_machine_policy::FleetTrust, operator:&str)->bool {
    if crate::fleet_machine_policy::validate_operator_key(trust,operator).is_err() { return false; }
    let expected_key=if fleet_proto::is_duress_catalog(&command.catalog_id) { operator } else { &trust.signer_key };
    if command.command_id.is_empty() || command.command_id.len()>256 || command.command_id.chars().any(char::is_control) || command.idempotency_key.is_empty()
        || command.idempotency_key.len()>256 || command.catalog_id.is_empty() || command.catalog_id.len()>256 || command.catalog_id.chars().any(char::is_control)
        || command.device_id.0!=trust.device_id || command.signer_key!=expected_key { return false; }
    let bytes=fleet_proto::canonical_command_bytes(&command.idempotency_key,&command.device_id.0,&command.catalog_id,
        command.action_class.as_wire_str(),&command.payload,command.epoch_version);
    fleet_proto::verify_signature_b64(expected_key,&bytes,&command.signature)
}
fn valid_late_receipt(receipt:&LateEvidenceReceipt,current:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff,command:&str,review:&UnknownReview,late:&LateEvidence,nonce:Option<&str>,now:Option<i64>)->bool {
    use base64::{engine::general_purpose::STANDARD,Engine};
    if receipt.org_id!=current.org_id || receipt.device_id!=current.device_id || receipt.recovery_id!=verified.retirement_id()
        || receipt.command_id!=command || receipt.evidence_id!=late.evidence_id || receipt.unknown_receipt_id!=review.receipt_id
        || receipt.unknown_record_hash!=review.record_hash || receipt.result_sha256!=late.result_sha256 || receipt.server_key!=current.signer_key
        || receipt.request_nonce.strip_prefix("ci-").is_none_or(|value|value.len()!=32 || !value.bytes().all(|byte|byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        || receipt.issued_at<=0 || nonce.is_some_and(|value|value!=receipt.request_nonce)
        || now.is_some_and(|value|!crate::fleet_identity_handoff::recovery_timestamp_valid(receipt.issued_at,value)) { return false; }
    let Ok(raw)=STANDARD.decode(&receipt.signature) else { return false; };
    if raw.len()!=64 || STANDARD.encode(&raw)!=receipt.signature { return false; }
    let Ok(bytes)=serde_json::to_vec(&("wincommander:fleet:late-execution-evidence-receipt:v1",&receipt.org_id,&receipt.device_id,
        &receipt.recovery_id,&receipt.command_id,&receipt.evidence_id,&receipt.unknown_receipt_id,&receipt.unknown_record_hash,
        &receipt.result_sha256,&receipt.server_key,&receipt.request_nonce,receipt.issued_at)) else { return false; };
    fleet_proto::verify_signature_b64(&current.signer_key,&bytes,&receipt.signature)
}
fn valid_late_evidence(entry:&InboxCommand,journal:&ReplacementJournal,current:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff,operator:&str)->bool {
    use base64::{engine::general_purpose::STANDARD,Engine};
    let Some(late)=entry.late_evidence.as_ref() else { return true; };
    let Some(review)=entry.unknown_review.as_ref() else { return false; };
    let Some(grant)=entry.grant.as_ref() else { return false; };
    if entry.state!="unknown" || grant.started_at.is_none() || !valid_hash(&late.result_sha256)
        || uuid::Uuid::parse_str(&late.evidence_id).ok().is_none_or(|id|id.is_nil() || id.to_string()!=late.evidence_id)
        || !journal.history.iter().any(|item|item.command_id==entry.command.command_id && item.receipt_id==review.receipt_id && item.record_hash==review.record_hash)
        || !late.outcome.signature.is_empty() || !late.outcome.record_hash.is_empty()
        || execution_hash(&late.outcome,&late.ack).ok().as_deref()!=Some(late.result_sha256.as_str())
        || authorize_result(AuthenticatedDelivery { command:entry.command.clone() },AuthenticatedExecution { outcome:late.outcome.clone(),ack:late.ack.clone() },current,operator,entry.command.epoch_version).is_err() { return false; }
    let Ok(bytes)=serde_json::to_vec(&("wincommander:fleet:late-execution-evidence:v1",&current.org_id,&current.device_id,
        verified.retirement_id(),&entry.command.command_id,&late.evidence_id,&review.receipt_id,&review.record_hash,&late.result_sha256)) else { return false; };
    fleet_proto::verify_signature_b64(&STANDARD.encode(verified.public_key()),&bytes,&late.signature)
        && late.receipt.as_ref().is_none_or(|receipt|valid_late_receipt(receipt,current,verified,&entry.command.command_id,review,late,None,None))
}

fn record_late_receipt(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,current:&crate::fleet_machine_policy::FleetTrust,
    old:&crate::fleet_machine_policy::FleetTrust,verified:&VerifiedHandoff,archive:&str,operator:&str,
    command_id:&str,nonce:&str,now:i64,receipt:LateEvidenceReceipt)->Result<(),&'static str> {
    mutate_unified_journal(fs,dir,protector,current,old,verified,archive,operator,|_,inbox| {
        let entry=inbox.entries.iter_mut().find(|entry|entry.command.command_id==command_id).ok_or("late evidence unavailable")?;
        let review=entry.unknown_review.as_ref().ok_or("late evidence unavailable")?;
        let late=entry.late_evidence.as_mut().ok_or("late evidence unavailable")?;
        if !valid_late_receipt(&receipt,current,verified,command_id,review,late,Some(nonce),Some(now)) { return Err("invalid late evidence receipt"); }
        // Keep the first independently verified receipt and all original evidence.
        if late.receipt.is_none() { late.receipt=Some(receipt); }
        Ok(())
    })
}

/// Gated uploader. A durable signed receipt is recorded without deleting or
/// changing the original Unknown outcome or the late evidence.
async fn upload_late_evidence(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,
    candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,command_id:&str)->Result<(),&'static str> {
    let snapshot=store.credential_snapshot(protector)?.ok_or("recovery credential unavailable")?;
    if !snapshot.is_recovered || !store.credential_is_current(&snapshot) { return Err("recovery credential unavailable"); }
    if crate::fleet_runtime_env::fingerprint().as_deref()!=Some(snapshot.profile.environment_fingerprint.as_str()) { return Err("network configuration differs"); }
    let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&snapshot.profile.spki_pins);
    if !snapshot.profile.spki_pins.trim().is_empty() && pins.is_none() { return Err("transport security invalid"); }
    let pending=with_historical_recovery(store,fs,dir,candidate,Some(&snapshot),|current,old,verified,archive,operator,_| {
        if *current!=snapshot.trust || snapshot.operator_key.as_deref()!=Some(operator) { return Err("recovery credential changed"); }
        mutate_unified_journal(fs,dir,protector,current,old,verified,archive,operator,|_,inbox| {
            let entry=inbox.entries.iter().find(|entry|entry.command.command_id==command_id).ok_or("late evidence unavailable")?;
            let review=entry.unknown_review.as_ref().ok_or("late evidence unavailable")?;
            let late=entry.late_evidence.as_ref().ok_or("late evidence unavailable")?;
            Ok((review.clone(),late.clone(),verified.retirement_id().to_owned()))
        })
    })?;
    let (review,late,recovery)=pending;
    if late.receipt.is_some() { return Ok(()); }
    if uuid::Uuid::parse_str(command_id).ok().is_none_or(|id|id.is_nil() || id.to_string()!=command_id)
        || matches!(late.outcome.outcome,fleet_proto::ActionOutcomeState::Unknown) { return Err("late evidence upload unavailable"); }
    let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
    let mut random=[0u8;16];SysRng.try_fill_bytes(&mut random).map_err(|_| "randomness unavailable")?;
    let nonce=format!("ci-{}",lower_hex(&random));
    let mut payload=SensitivePayload(serde_json::json!({"org_id":snapshot.trust.org_id,"device_id":snapshot.trust.device_id,
        "recovery_id":recovery,"command_id":command_id,"unknown_receipt_id":review.receipt_id,"unknown_record_hash":review.record_hash,
        "evidence":late,"ts":now,"nonce":nonce,"hmac_version":fleet_agent_core::HMAC_VERSION_V2,"hmac":""}));
    let hmac=fleet_agent_core::compute_request_hmac_v2(&snapshot.secret,"POST","/v1/agents/recovery-late-evidence",&payload.0)
        .map_err(|_| "invalid late evidence request")?;
    payload.0["hmac"]=serde_json::Value::String(hmac);
    let body=zeroize::Zeroizing::new(serde_json::to_vec(&payload.0).map_err(|_| "invalid late evidence request")?);
    // Match the endpoint's 2 MiB evidence bound plus fixed envelope allowance.
    if body.len()>2*1024*1024+16384 { return Err("late evidence too large for upload"); }
    if !store.credential_is_current(&snapshot) { return Err("recovery credential changed"); }
    let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_| "recovery transport unavailable")?;
    let operation=async {
        let mut response=client.post(format!("{}/v1/agents/recovery-late-evidence",snapshot.origin))
            .header(reqwest::header::CONTENT_TYPE,"application/json").body(body.to_vec()).send().await.map_err(|_| "recovery transport unavailable")?;
        if response.status()!=reqwest::StatusCode::OK { return Err("recovery transport rejected"); }
        let mut bytes=Vec::new();while let Some(chunk)=response.chunk().await.map_err(|_| "recovery transport unavailable")? {
            if bytes.len().checked_add(chunk.len()).is_none_or(|size|size>16384) { return Err("invalid late evidence receipt"); } bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<LateEvidenceReceipt>(&bytes).map_err(|_| "invalid late evidence receipt")
    };
    let receipt=tokio::time::timeout(Duration::from_secs(30),operation).await.map_err(|_| "recovery transport unavailable")??;
    if !store.credential_is_current(&snapshot) { return Err("recovery credential changed"); }
    let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
    with_historical_recovery(store,fs,dir,candidate,Some(&snapshot),|current,old,verified,archive,operator,_| {
        if *current!=snapshot.trust || snapshot.operator_key.as_deref()!=Some(operator) { return Err("recovery credential changed"); }
        record_late_receipt(fs,dir,protector,current,old,verified,archive,operator,command_id,&nonce,now,receipt)
    })
}

fn pending_late_ids(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,after:Option<&str>)
    ->Result<Vec<String>,&'static str> {
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,verified,archive,operator,_| {
        mutate_unified_journal(fs,dir,protector,current,old,verified,archive,operator,|_,inbox| {
            let eligible:Vec<String>=inbox.entries.iter().filter(|entry|entry.late_evidence.as_ref().is_some_and(|late|late.receipt.is_none()
                    && !matches!(late.outcome.outcome,fleet_proto::ActionOutcomeState::Unknown))
                    && uuid::Uuid::parse_str(&entry.command.command_id).ok().is_some_and(|id|!id.is_nil() && id.to_string()==entry.command.command_id))
                .map(|entry|entry.command.command_id.clone()).collect();
            Ok(next_late_retry(&eligible,after).into_iter().collect())
        })
    })
}
fn next_late_retry(eligible:&[String],after:Option<&str>)->Option<String> {
    if eligible.is_empty() { return None; }
    let next=after.and_then(|id|eligible.iter().position(|candidate|candidate==id)).map_or(0,|index|(index+1)%eligible.len());
    Some(eligible[next].clone())
}

/// The encrypted inbox and outcome chain share one atomic journal write.
/// A pre-inbox draft journal loads with an empty inbox; no prior evidence is removed.
fn mutate_unified_journal<T>(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,current:&crate::fleet_machine_policy::FleetTrust,
    old:&crate::fleet_machine_policy::FleetTrust,verified:&VerifiedHandoff,archive:&str,operator:&str,
    change:impl FnOnce(&mut ReplacementJournal,&mut CommandInbox)->Result<T,&'static str>) ->Result<T,&'static str> {
    mutate_unified_journal_inner(fs,dir,protector,current,old,verified,archive,operator,false,change)
}
fn mutate_unified_journal_inner<T>(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,current:&crate::fleet_machine_policy::FleetTrust,
    old:&crate::fleet_machine_policy::FleetTrust,verified:&VerifiedHandoff,archive:&str,operator:&str,allow_frozen:bool,
    change:impl FnOnce(&mut ReplacementJournal,&mut CommandInbox)->Result<T,&'static str>) ->Result<T,&'static str> {
    use base64::{engine::general_purpose::STANDARD,Engine};
    if current.org_id!=old.org_id || current.device_id!=old.device_id || current.signer_key!=old.signer_key
        || current.enrollment_generation!=verified.new_generation()
        || crate::fleet_machine_policy::validate_operator_key(current,operator).is_err() { return Err("command inbox unavailable"); }
    initialize_replacement(fs,dir,old,verified,archive)?;
    let entropy:[u8;32]=Sha256::digest(serde_json::to_vec(&("fleet.recovery.command-inbox.v1",&current.org_id,&current.device_id,
        &current.enrollment_generation,&current.signer_key,operator)).map_err(|_| "command inbox unavailable")?).into();
    let path=dir.join(format!("replacement-{}.json",verified.retirement_id()));
    let _guard=REPLACEMENT_LOCK.lock().map_err(|_| "command inbox unavailable")?;
    let mut journal:ReplacementJournal=serde_json::from_slice(&fs.read(&path).map_err(|_| "command inbox unavailable")?)
        .map_err(|_| "command inbox unavailable")?;
    if journal.drain_freeze.is_some() && !allow_frozen { return Err("recovery blocked: previous work admission is frozen"); }
    let mut inbox=match &journal.inbox_sealed {
        Some(encoded)=> {
            let sealed=STANDARD.decode(encoded).map_err(|_| "command inbox unavailable")?;
            if sealed.is_empty() || sealed.len()>4*1024*1024+65536 || STANDARD.encode(&sealed)!=*encoded { return Err("command inbox unavailable"); }
            let plain=protector.unprotect(&sealed,&entropy)?;
            serde_json::from_slice::<CommandInbox>(&plain).map_err(|_| "command inbox unavailable")?
        },
        None=>CommandInbox { org_id:current.org_id.clone(),device_id:current.device_id.clone(),generation:current.enrollment_generation.clone(),operator_key:operator.into(),entries:Vec::new() },
    };
    if inbox.org_id!=current.org_id || inbox.device_id!=current.device_id || inbox.generation!=current.enrollment_generation
        || inbox.operator_key!=operator || inbox.entries.len()>1024 { return Err("command inbox unavailable"); }
    let mut ids=std::collections::HashSet::new();
    if inbox.entries.iter().any(|entry|!valid_inbox_command(&entry.command,current,operator) || !valid_hash(&entry.response_sha256)
        || !matches!(entry.state.as_str(),"ready"|"in_flight"|"running"|"completed"|"unknown") || ((entry.state!="ready")!=entry.grant.is_some())
        || entry.grant.as_ref().is_some_and(|grant|grant.start_epoch!=Some(entry.command.epoch_version)
            || (entry.state!="unknown" && ((entry.state=="running" || entry.state=="completed")!=grant.started_at.is_some())))
        || (if matches!(entry.state.as_str(),"completed"|"unknown") { !entry.result_sha256.as_ref().is_some_and(|hash|valid_hash(hash)) } else { entry.result_sha256.is_some() })
        || ((entry.state=="unknown")!=entry.unknown_review.is_some())
        || entry.unknown_review.as_ref().is_some_and(|review|!review.actor_sid.starts_with("S-1-") || review.actor_sid.len()>184 || !valid_hash(&review.record_hash))
        || !valid_late_evidence(entry,&journal,current,verified,operator)
        || !ids.insert(entry.command.command_id.clone())) { return Err("command inbox unavailable"); }
    let result=change(&mut journal,&mut inbox)?;
    let plain=zeroize::Zeroizing::new(serde_json::to_vec(&inbox).map_err(|_| "command inbox unavailable")?);
    if plain.len()>4*1024*1024 { return Err("command inbox unavailable"); }
    let sealed=protector.protect(&plain,&entropy)?;
    if sealed.is_empty() || sealed.len()>4*1024*1024+65536 { return Err("command inbox unavailable"); }
    journal.inbox_sealed=Some(STANDARD.encode(sealed));
    let encoded=serde_json::to_vec(&journal).map_err(|_| "command inbox unavailable")?;
    fs.atomic_write(&path,&encoded).map_err(|_| "command inbox unavailable")?;
    if fs.read(&path).map_err(|_| "command inbox unavailable")?!=encoded { return Err("command inbox unavailable"); }
    Ok(result)
}

fn persist_unified_batch(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,old:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff,archive:&str,batch:AuthenticatedCommandBatch)->Result<(),&'static str> {
    if batch.commands.len()>128 || !valid_hash(&batch.response_sha256) { return Err("invalid command batch"); }
    mutate_unified_journal(fs,dir,protector,&batch.trust,old,verified,archive,&batch.operator_key,|_,inbox| {
        for command in batch.commands {
            if !valid_inbox_command(&command,&batch.trust,&batch.operator_key) { return Err("invalid command batch"); }
            if let Some(existing)=inbox.entries.iter().find(|entry|entry.command.command_id==command.command_id) {
                if existing.command!=command { return Err("command delivery changed"); }
            } else {
                if inbox.entries.len()>=1024 { return Err("command inbox full"); }
                inbox.entries.push(InboxCommand { command,response_sha256:batch.response_sha256.clone(),state:"ready".into(),grant:None,result_sha256:None,unknown_review:None,late_evidence:None });
            }
        }
        Ok(())
    })
}

fn issue_unified_grant(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,current:&crate::fleet_machine_policy::FleetTrust,
    old:&crate::fleet_machine_policy::FleetTrust,verified:&VerifiedHandoff,archive:&str,operator:&str,
    peer:&AuthenticatedExecutionPeer,command_id:&str,epoch:i64,now:i64)
    ->Result<(fleet_proto::SignedCommand,zeroize::Zeroizing<[u8;32]>),&'static str> {
    if !peer.sid.starts_with("S-1-") || peer.sid.len()>184 || peer.session==0 || peer.pid==0 || peer.process_created==0 { return Err("execution authorization unavailable"); }
    mutate_unified_journal(fs,dir,protector,current,old,verified,archive,operator,|_,inbox| {
        let entry=inbox.entries.iter_mut().find(|entry|entry.command.command_id==command_id).ok_or("command unavailable")?;
        if entry.state!="ready" || entry.command.epoch_version!=epoch { return Err("command unavailable"); }
        let mut token=zeroize::Zeroizing::new([0u8;32]); SysRng.try_fill_bytes(token.as_mut()).map_err(|_| "randomness unavailable")?;
        entry.state="in_flight".into(); entry.grant=Some(InboxGrant { token_sha256:lower_hex(&Sha256::digest(token.as_slice())),
            sid:peer.sid.clone(),session:peer.session,logon:peer.logon,pid:peer.pid,process_created:peer.process_created,expires:now.checked_add(120).ok_or("clock unavailable")?,start_epoch:Some(epoch),started_at:None });
        Ok((entry.command.clone(),token))
    })
}

/// Persist a single start authorization before the helper begins the effect.
/// A lost start response is unknown and cannot be retried as a new execution.
fn start_unified_grant(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,current:&crate::fleet_machine_policy::FleetTrust,
    old:&crate::fleet_machine_policy::FleetTrust,verified:&VerifiedHandoff,archive:&str,operator:&str,
    peer:&AuthenticatedExecutionPeer,command_id:&str,token:&[u8;32],epoch:i64,now:i64)->Result<(),&'static str> {
    use subtle::ConstantTimeEq;
    let token_hash=lower_hex(&Sha256::digest(token));
    mutate_unified_journal(fs,dir,protector,current,old,verified,archive,operator,|_,inbox| {
        let entry=inbox.entries.iter_mut().find(|entry|entry.command.command_id==command_id).ok_or("command unavailable")?;
        let grant=entry.grant.as_mut().ok_or("execution authorization unavailable")?;
        if entry.state!="in_flight" || now>grant.expires || grant.start_epoch!=Some(epoch)
            || grant.start_epoch!=Some(entry.command.epoch_version) || grant.sid!=peer.sid || grant.session!=peer.session
            || grant.logon!=peer.logon || grant.pid!=peer.pid || grant.process_created!=peer.process_created || grant.token_sha256.len()!=token_hash.len()
            || !bool::from(grant.token_sha256.as_bytes().ct_eq(token_hash.as_bytes())) { return Err("execution authorization unavailable"); }
        grant.started_at=Some(now); entry.state="running".into(); Ok(())
    })
}

/// Commit the verified result, ACK and grant completion in one journal write.
fn finish_unified_grant(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,
    current:&crate::fleet_machine_policy::FleetTrust,old:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff,archive:&str,operator:&str,peer:&AuthenticatedExecutionPeer,
    command_id:&str,token:&[u8;32],_epoch:i64,_now:i64,execution:AuthenticatedExecution)->Result<(),&'static str> {
    use subtle::ConstantTimeEq;
    let result_hash=execution_hash(&execution.outcome,&execution.ack)?;
    let token_hash=lower_hex(&Sha256::digest(token));
    mutate_unified_journal(fs,dir,protector,current,old,verified,archive,operator,|journal,inbox| {
        let entry=inbox.entries.iter_mut().find(|entry|entry.command.command_id==command_id).ok_or("command unavailable")?;
        let grant=entry.grant.as_ref().ok_or("execution authorization unavailable")?;
        if grant.sid!=peer.sid || grant.session!=peer.session || grant.logon!=peer.logon || grant.pid!=peer.pid || grant.process_created!=peer.process_created
            || grant.token_sha256.len()!=token_hash.len() || !bool::from(grant.token_sha256.as_bytes().ct_eq(token_hash.as_bytes())) {
            return Err("execution authorization unavailable");
        }
        if entry.state=="completed" {
            return if entry.result_sha256.as_deref()==Some(result_hash.as_str()) { Ok(()) } else { Err("command result changed") };
        }
        if entry.state=="unknown" {
            if grant.started_at.is_none() || grant.start_epoch!=Some(entry.command.epoch_version) { return Err("execution authorization unavailable"); }
            if let Some(existing)=&entry.late_evidence {
                return if existing.result_sha256==result_hash { Ok(()) } else { Err("late evidence changed") };
            }
            if !execution.outcome.signature.is_empty() || !execution.outcome.record_hash.is_empty() { return Err("invalid command result"); }
            authorize_result(AuthenticatedDelivery { command:entry.command.clone() },AuthenticatedExecution {
                outcome:execution.outcome.clone(),ack:execution.ack.clone() },current,operator,entry.command.epoch_version)?;
            let review=entry.unknown_review.as_ref().ok_or("unknown review unavailable")?;
            let mut raw=[0u8;16];SysRng.try_fill_bytes(&mut raw).map_err(|_| "randomness unavailable")?;
            raw[6]=(raw[6]&15)|64;raw[8]=(raw[8]&63)|128;let evidence_id=uuid::Uuid::from_bytes(raw).to_string();
            let signature=candidate.sign_late_evidence(old,verified.retirement_id(),verified.fingerprint(),command_id,
                &evidence_id,&review.receipt_id,&review.record_hash,&result_hash)?;
            entry.late_evidence=Some(LateEvidence { evidence_id,result_sha256:result_hash,signature,outcome:execution.outcome,ack:execution.ack,receipt:None });
            return Ok(());
        }
        if entry.state!="running" || grant.start_epoch!=Some(entry.command.epoch_version) { return Err("execution authorization unavailable"); }
        let start_epoch=grant.start_epoch.ok_or("execution authorization unavailable")?;
        let authorized=authorize_result(AuthenticatedDelivery { command:entry.command.clone() },execution,current,operator,start_epoch)?;
        append_to_journal(journal,candidate,old,verified,authorized)?;
        entry.state="completed".into(); entry.result_sha256=Some(result_hash);
        Ok(())
    })
}

/// Approval is minted only after the native warning in the authenticated
/// administrator's live session, and is bound to one exact command digest.
struct UnknownApproval { actor_sid:String, session:u32, logon:(u32,i32), command_digest:[u8;32], expires:Instant }
fn command_digest(command:&fleet_proto::SignedCommand)->Result<[u8;32],&'static str> {
    Ok(Sha256::digest(serde_json::to_vec(command).map_err(|_| "invalid command")?).into())
}
fn utc_timestamp(seconds:i64)->Result<String,&'static str> {
    use windows_sys::Win32::Foundation::{FILETIME,SYSTEMTIME};
    use windows_sys::Win32::System::Time::FileTimeToSystemTime;
    let ticks=u64::try_from(seconds).map_err(|_| "clock unavailable")?.checked_add(11_644_473_600)
        .and_then(|value|value.checked_mul(10_000_000)).ok_or("clock unavailable")?;
    let file=FILETIME { dwLowDateTime:ticks as u32,dwHighDateTime:(ticks>>32) as u32 };
    let mut system:SYSTEMTIME=unsafe { std::mem::zeroed() };
    if unsafe { FileTimeToSystemTime(&file,&mut system) }==0 { return Err("clock unavailable"); }
    Ok(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",system.wYear,system.wMonth,system.wDay,system.wHour,system.wMinute,system.wSecond))
}
fn confirm_unknown(peer:&AuthenticatedPipePeer, command:&fleet_proto::SignedCommand)->Result<UnknownApproval,&'static str> {
    if !session_matches(peer) { return Err("interactive administrator required"); }
    let digest=command_digest(command)?;
    let title:Vec<u16>="WinCommander Fleet unknown work".encode_utf16().chain(std::iter::once(0)).collect();
    let body:Vec<u16>=format!("A Fleet command has an unknown result.\n\nDevice: {}\nAction: {}\nCommand: {}\n\nThe work may still be running or may already have changed this device. This records an unknown result and does not retry the command. Continue?",command.device_id.0,command.catalog_id,command.command_id)
        .encode_utf16().chain(std::iter::once(0)).collect();
    let mut response=0;
    let ok=unsafe { WTSSendMessageW(WTS_CURRENT_SERVER_HANDLE,peer.session_id(),title.as_ptr(),((title.len()-1)*2) as u32,
        body.as_ptr(),((body.len()-1)*2) as u32,MB_YESNO|MB_ICONWARNING|MB_DEFBUTTON2,60,&mut response,1) }!=0;
    if !ok || response!=IDYES || !session_matches(peer) { return Err("unknown review denied"); }
    Ok(UnknownApproval { actor_sid:peer.caller_sid().into(),session:peer.session_id(),logon:peer.authentication_id(),command_digest:digest,expires:Instant::now()+Duration::from_secs(120) })
}
fn acknowledge_unknown(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,
    current:&crate::fleet_machine_policy::FleetTrust,old:&crate::fleet_machine_policy::FleetTrust,
    verified:&VerifiedHandoff,archive:&str,operator:&str,command_id:&str,approval:UnknownApproval,now:i64,
    recheck:impl FnOnce(&UnknownApproval)->bool)->Result<(),&'static str> {
    if approval.expires<=Instant::now() || !approval.actor_sid.starts_with("S-1-") || approval.actor_sid.len()>184
        || approval.session==0 || !recheck(&approval) { return Err("unknown review unavailable"); }
    mutate_unified_journal(fs,dir,protector,current,old,verified,archive,operator,|journal,inbox| {
        let entry=inbox.entries.iter_mut().find(|entry|entry.command.command_id==command_id).ok_or("command unavailable")?;
        if command_digest(&entry.command)?!=approval.command_digest { return Err("unknown review changed"); }
        if entry.state=="unknown" { return Ok(()); }
        if !matches!(entry.state.as_str(),"in_flight"|"running") { return Err("unknown review changed"); }
        let mut random=[0u8;32]; SysRng.try_fill_bytes(&mut random).map_err(|_| "randomness unavailable")?;
        let mut receipt=[0u8;16];receipt.copy_from_slice(&random[..16]);receipt[6]=(receipt[6]&15)|64;receipt[8]=(receipt[8]&63)|128;
        let mut report=[0u8;16];report.copy_from_slice(&random[16..]);report[6]=(report[6]&15)|64;report[8]=(report[8]&63)|128;
        let result=serde_json::json!({"state":"execution_unknown"});
        let ack=fleet_proto::CheckinAck { command_id:entry.command.command_id.clone(),success:false,detail:Some("execution result unknown".into()),
            result:Some(result.clone()),report_id:Some(uuid::Uuid::from_bytes(report).to_string()),phase:None,terminal_reason:Some("execution_unknown".into()) };
        let outcome=fleet_proto::ActionOutcome { version:1,receipt_id:uuid::Uuid::from_bytes(receipt).to_string(),device_id:fleet_proto::DeviceId(current.device_id.clone()),
            sequence:0,previous_hash:None,command_id:Some(entry.command.command_id.clone()),catalog_id:entry.command.catalog_id.clone(),action_class:entry.command.action_class,
            outcome:fleet_proto::ActionOutcomeState::Unknown,failure_stage:Some(fleet_proto::CommandFailureStage::Report),
            observed_at:utc_timestamp(now)?,
            result_digest:serde_json::json!({"typed_result_sha256":fleet_proto::canonical_json_sha256(&result)}),record_hash:String::new(),signature:String::new() };
        let digest=execution_hash(&outcome,&ack)?;
        let unknown=append_to_journal(journal,candidate,old,verified,AuthorizedResult { outcome,ack })?;
        entry.state="unknown".into();entry.result_sha256=Some(digest);
        entry.unknown_review=Some(UnknownReview { actor_sid:approval.actor_sid,reviewed_at:now,receipt_id:unknown.receipt_id,record_hash:unknown.record_hash });
        Ok(())
    })
}

pub(super) fn review_unknown_for_admin(store:&crate::fleet_machine_policy::FleetMachinePolicy,
    fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,
    candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>,peer:&AuthenticatedPipePeer,command_id:&str)->Result<(),&'static str> {
    if !session_matches(peer) || command_id.is_empty() || command_id.len()>256 { return Err("unknown review unavailable"); }
    let command=with_historical_recovery(store,fs,dir,candidate,None,|current,old,verified,archive,operator,_| {
        mutate_unified_journal(fs,dir,protector,current,old,verified,archive,operator,|_,inbox| {
            let entry=inbox.entries.iter().find(|entry|entry.command.command_id==command_id).ok_or("command unavailable")?;
            if !matches!(entry.state.as_str(),"in_flight"|"running"|"unknown") { return Err("unknown review unavailable"); }
            Ok(entry.command.clone())
        })
    })?;
    let approval=confirm_unknown(peer,&command)?;
    let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
    with_historical_recovery(store,fs,dir,candidate,None,|current,old,verified,archive,operator,_| {
        acknowledge_unknown(fs,dir,protector,candidate,current,old,verified,archive,operator,command_id,approval,now,|approval| {
            session_matches(peer) && approval.actor_sid==peer.caller_sid() && approval.session==peer.session_id() && approval.logon==peer.authentication_id()
        })
    })
}

/// Persists the whole authenticated delivery before any command may execute.
/// In-flight or completed entries are retained so restart never implies retry.
#[cfg(test)]
fn persist_command_batch(fs:&dyn crate::policy_store::PolicyFs, dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector, batch:AuthenticatedCommandBatch) -> Result<(),&'static str> {
    let trust=&batch.trust;
    if batch.commands.len()>128 || !valid_hash(&batch.response_sha256) { return Err("invalid command batch"); }
    fs.ensure_dir_secure(dir).map_err(|_| "command inbox unavailable")?;
    let entropy:[u8;32]=Sha256::digest(serde_json::to_vec(&("fleet.recovery.command-inbox.v1",&trust.org_id,&trust.device_id,
        &trust.enrollment_generation,&trust.signer_key,&batch.operator_key)).map_err(|_| "command inbox unavailable")?).into();
    let path=command_inbox_path(dir,trust)?;
    let _guard=INBOX_LOCK.lock().map_err(|_| "command inbox unavailable")?;
    let mut inbox=match fs.read(&path) {
        Ok(sealed)=> {
            let plain=protector.unprotect(&sealed,&entropy)?;
            serde_json::from_slice::<CommandInbox>(&plain).map_err(|_| "command inbox unavailable")?
        },
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>CommandInbox { org_id:trust.org_id.clone(),device_id:trust.device_id.clone(),generation:trust.enrollment_generation.clone(),operator_key:batch.operator_key.clone(),entries:Vec::new() },
        Err(_)=>return Err("command inbox unavailable"),
    };
    if inbox.org_id!=trust.org_id || inbox.device_id!=trust.device_id || inbox.generation!=trust.enrollment_generation || inbox.operator_key!=batch.operator_key
        || inbox.entries.len()>1024 { return Err("command inbox unavailable"); }
    let mut ids=std::collections::HashSet::new();
    if inbox.entries.iter().any(|entry| !valid_inbox_command(&entry.command,trust,&batch.operator_key) || !valid_hash(&entry.response_sha256)
        || !matches!(entry.state.as_str(),"ready"|"in_flight"|"completed")
        || ((entry.state=="in_flight")!=entry.grant.is_some()) || !ids.insert(entry.command.command_id.clone())) {
        return Err("command inbox unavailable");
    }
    for command in batch.commands {
        if !valid_inbox_command(&command,trust,&batch.operator_key) { return Err("invalid command batch"); }
        if let Some(existing)=inbox.entries.iter().find(|entry|entry.command.command_id==command.command_id) {
            if existing.command!=command { return Err("command delivery changed"); }
        } else {
            if inbox.entries.len()>=1024 { return Err("command inbox full"); }
            inbox.entries.push(InboxCommand { command,response_sha256:batch.response_sha256.clone(),state:"ready".into(),grant:None,result_sha256:None,unknown_review:None,late_evidence:None });
        }
    }
    let encoded=zeroize::Zeroizing::new(serde_json::to_vec(&inbox).map_err(|_| "command inbox unavailable")?);
    if encoded.len()>4*1024*1024 { return Err("command inbox full"); }
    let sealed=protector.protect(&encoded,&entropy)?;
    if sealed.is_empty() || sealed.len()>4*1024*1024+65536 { return Err("command inbox unavailable"); }
    fs.atomic_write(&path,&sealed).map_err(|_| "command inbox unavailable")?;
    let stored=fs.read(&path).map_err(|_| "command inbox unavailable")?;
    if protector.unprotect(&stored,&entropy)?.as_slice()!=encoded.as_slice() { return Err("command inbox unavailable"); }
    Ok(())
}

/// The principal must come from an authenticated pinned Pro pipe connection.
/// Once granted, the entry stays in flight even if the reply is lost or the
/// process crashes; only a verified result can ever complete it.
#[cfg(test)]
fn issue_execution_grant(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    protector:&dyn crate::fleet_credential::Protector, trust:&crate::fleet_machine_policy::FleetTrust,
    operator:&str,principal:&AuthenticatedExecutionPeer, command_id:&str,epoch:i64,now:i64)
    ->Result<(fleet_proto::SignedCommand,zeroize::Zeroizing<[u8;32]>),&'static str> {
    if !principal.sid.starts_with("S-1-") || principal.sid.len()>184 || principal.session==0 || principal.pid==0
        || command_id.is_empty() || command_id.len()>256 { return Err("execution authorization unavailable"); }
    let entropy:[u8;32]=Sha256::digest(serde_json::to_vec(&("fleet.recovery.command-inbox.v1",&trust.org_id,&trust.device_id,
        &trust.enrollment_generation,&trust.signer_key,operator)).map_err(|_| "command inbox unavailable")?).into();
    let _guard=INBOX_LOCK.lock().map_err(|_| "command inbox unavailable")?;
    let path=command_inbox_path(dir,trust)?;
    let sealed=fs.read(&path).map_err(|_| "command inbox unavailable")?;
    let plain=protector.unprotect(&sealed,&entropy)?;
    let mut inbox:CommandInbox=serde_json::from_slice(&plain).map_err(|_| "command inbox unavailable")?;
    if inbox.org_id!=trust.org_id || inbox.device_id!=trust.device_id || inbox.generation!=trust.enrollment_generation
        || inbox.operator_key!=operator || inbox.entries.len()>1024 { return Err("command inbox unavailable"); }
    let mut ids=std::collections::HashSet::new();
    if inbox.entries.iter().any(|entry|!valid_inbox_command(&entry.command,trust,operator) || !valid_hash(&entry.response_sha256)
        || !matches!(entry.state.as_str(),"ready"|"in_flight"|"completed") || ((entry.state=="in_flight")!=entry.grant.is_some())
        || !ids.insert(entry.command.command_id.clone())) { return Err("command inbox unavailable"); }
    let entry=inbox.entries.iter_mut().find(|entry|entry.command.command_id==command_id).ok_or("command unavailable")?;
    if entry.state!="ready" || entry.command.epoch_version!=epoch { return Err("command unavailable"); }
    let mut token=zeroize::Zeroizing::new([0u8;32]); SysRng.try_fill_bytes(token.as_mut()).map_err(|_| "randomness unavailable")?;
    entry.state="in_flight".into();
    entry.grant=Some(InboxGrant { token_sha256:lower_hex(&Sha256::digest(token.as_slice())),sid:principal.sid.clone(),
        session:principal.session,logon:principal.logon,pid:principal.pid,process_created:principal.process_created,expires:now.checked_add(120).ok_or("clock unavailable")?,start_epoch:Some(epoch),started_at:None });
    let command=entry.command.clone();
    let encoded=zeroize::Zeroizing::new(serde_json::to_vec(&inbox).map_err(|_| "command inbox unavailable")?);
    if encoded.len()>4*1024*1024 { return Err("command inbox unavailable"); }
    let sealed=protector.protect(&encoded,&entropy)?;
    fs.atomic_write(&path,&sealed).map_err(|_| "command inbox unavailable")?;
    let stored=fs.read(&path).map_err(|_| "command inbox unavailable")?;
    if protector.unprotect(&stored,&entropy)?.as_slice()!=encoded.as_slice() { return Err("command inbox unavailable"); }
    Ok((command,token))
}
fn authorize_result(delivery:AuthenticatedDelivery, execution:AuthenticatedExecution,
    trust:&crate::fleet_machine_policy::FleetTrust, operator:&str, expected_epoch:i64) -> Result<AuthorizedResult,&'static str> {
    let command=delivery.command;
    if !valid_inbox_command(&command,trust,operator) || command.epoch_version!=expected_epoch
        || execution.outcome.command_id.as_deref()!=Some(command.command_id.as_str())
        || execution.ack.command_id!=command.command_id || execution.outcome.catalog_id!=command.catalog_id
        || execution.outcome.action_class!=command.action_class || !outcome_matches_ack(&execution.outcome,&execution.ack) {
        return Err("invalid authenticated command result");
    }
    if command.catalog_id=="encryption.status" && (command.action_class!=fleet_proto::ActionClass::Safe
        || command.payload!=serde_json::json!({})
        || (execution.ack.success && !execution.ack.result.as_ref().is_some_and(fleet_proto::valid_recovery_vault_status))) {
        return Err("invalid authenticated command result");
    }
    Ok(AuthorizedResult { outcome:execution.outcome, ack:execution.ack })
}

// Constructed only by a future authenticated service-owned check-in reader.
struct AuthenticatedCheckinConfirmation {
    head:fleet_proto::ActionOutcomeHead,
    outcomes:Vec<fleet_proto::ActionOutcomeReceipt>,
    acks:Vec<fleet_proto::CheckinAckReceipt>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryResultReceipt { command_id:String,report_id:Option<String>,ack_sha256:String,outcome_receipt_id:String,
    sequence:u64,record_hash:String,disposition:String }
/// Constructed only after verifying the v2 signature over the exact check-in
/// response and revalidating the protected credential snapshot.
struct AuthenticatedBoundConfirmation { head:fleet_proto::ActionOutcomeHead, receipts:Vec<RecoveryResultReceipt> }
fn confirm_replacement_bound(fs:&dyn crate::policy_store::PolicyFs,dir:&std::path::Path,
    trust:&crate::fleet_machine_policy::FleetTrust,verified:&VerifiedHandoff,archive:&str,
    confirmation:AuthenticatedBoundConfirmation)->Result<(),&'static str> {
    initialize_replacement(fs,dir,trust,verified,archive)?;
    let _guard=REPLACEMENT_LOCK.lock().map_err(|_| "replacement journal unavailable")?;
    let path=dir.join(format!("replacement-{}.json",verified.retirement_id()));
    let mut journal:ReplacementJournal=serde_json::from_slice(&fs.read(&path).map_err(|_| "replacement journal unavailable")?)
        .map_err(|_| "replacement journal unavailable")?;
    if journal.drain_freeze.is_some() { return Err("recovery blocked: previous work admission is frozen"); }
    let count=journal.pending.iter().position(|outcome|outcome.sequence==confirmation.head.sequence && outcome.record_hash==confirmation.head.record_hash)
        .map(|index|index+1).ok_or("confirmation does not match pending head")?;
    if confirmation.receipts.len()!=count { return Err("confirmation does not prove accepted reports"); }
    let mut seen=std::collections::HashSet::new();
    for (outcome,ack) in journal.pending.iter().zip(&journal.pending_acks).take(count) {
        let digest=fleet_proto::canonical_json_sha256(&serde_json::to_value(ack).map_err(|_| "replacement journal unavailable")?);
        let receipt=confirmation.receipts.iter().find(|item|item.command_id==ack.command_id).ok_or("confirmation does not prove accepted reports")?;
        if !seen.insert(&receipt.command_id) || receipt.report_id!=ack.report_id || receipt.ack_sha256!=digest
            || receipt.outcome_receipt_id!=outcome.receipt_id || receipt.sequence!=outcome.sequence || receipt.record_hash!=outcome.record_hash
            || !matches!(receipt.disposition.as_str(),"accepted"|"duplicate_accepted") { return Err("confirmation does not prove accepted reports"); }
    }
    journal.base_sequence=confirmation.head.sequence.checked_add(1).ok_or("replacement journal unavailable")?;
    journal.base_hash=Some(confirmation.head.record_hash);journal.pending.drain(..count);journal.pending_acks.drain(..count);
    fs.atomic_write(&path,&serde_json::to_vec(&journal).map_err(|_| "replacement journal unavailable")?).map_err(|_| "replacement journal unavailable")
}
fn confirm_replacement(fs:&dyn crate::policy_store::PolicyFs, dir:&std::path::Path,
    trust:&crate::fleet_machine_policy::FleetTrust, verified:&VerifiedHandoff, archive_hash:&str,
    confirmation:AuthenticatedCheckinConfirmation) -> Result<(), &'static str> {
    // Current server ACK receipts identify reports but do not prove exact
    // payload acceptance. Keep every local outcome until that contract exists.
    const PAYLOAD_BOUND_ACK_READY:bool=false;
    if !PAYLOAD_BOUND_ACK_READY { return Err("payload-bound acknowledgement unavailable"); }
    initialize_replacement(fs,dir,trust,verified,archive_hash)?;
    let _guard=REPLACEMENT_LOCK.lock().map_err(|_| "replacement journal unavailable")?;
    let path=dir.join(format!("replacement-{}.json",verified.retirement_id()));
    let mut journal:ReplacementJournal=serde_json::from_slice(&fs.read(&path).map_err(|_| "replacement journal unavailable")?)
        .map_err(|_| "replacement journal unavailable")?;
    if journal.drain_freeze.is_some() { return Err("recovery blocked: previous work admission is frozen"); }
    let count=journal.pending.iter().position(|outcome| outcome.sequence==confirmation.head.sequence
        && outcome.record_hash==confirmation.head.record_hash).map(|index| index+1).ok_or("confirmation does not match pending head")?;
    for (outcome,ack) in journal.pending.iter().zip(&journal.pending_acks).take(count) {
        if !confirmation.outcomes.iter().any(|receipt| receipt.receipt_id==outcome.receipt_id
            && receipt.sequence==outcome.sequence && receipt.disposition=="accepted")
            || !confirmation.acks.iter().any(|receipt| receipt.command_id==ack.command_id
                && receipt.report_id==ack.report_id && matches!(receipt.disposition.as_str(),"accepted"|"duplicate_accepted")) {
            return Err("confirmation does not prove accepted reports");
        }
    }
    journal.base_sequence=confirmation.head.sequence.checked_add(1).ok_or("replacement journal unavailable")?;
    journal.base_hash=Some(confirmation.head.record_hash);
    journal.pending.drain(..count); journal.pending_acks.drain(..count);
    fs.atomic_write(&path,&serde_json::to_vec(&journal).map_err(|_| "replacement journal unavailable")?)
        .map_err(|_| "replacement journal unavailable")
}

struct VerifiedJournalBatch { device_id:String, generation:String, outcomes:Vec<fleet_proto::ActionOutcome>, acks:Vec<fleet_proto::CheckinAck> }
fn limit_recovery_results(mut batch:VerifiedJournalBatch)->Result<VerifiedJournalBatch,&'static str> {
    if batch.outcomes.len()!=batch.acks.len() || batch.outcomes.len()>32 { return Err("recovery results unavailable"); }
    batch.outcomes.truncate(16);batch.acks.truncate(16);Ok(batch)
}
fn load_replacement_batch(fs:&dyn crate::policy_store::PolicyFs, dir:&std::path::Path,
    old:&crate::fleet_machine_policy::FleetTrust, verified:&VerifiedHandoff, archive:&str) -> Result<VerifiedJournalBatch,&'static str> {
    initialize_replacement(fs,dir,old,verified,archive)?;
    let _guard=REPLACEMENT_LOCK.lock().map_err(|_| "replacement journal unavailable")?;
    let path=dir.join(format!("replacement-{}.json",verified.retirement_id()));
    let journal:ReplacementJournal=serde_json::from_slice(&fs.read(&path).map_err(|_| "replacement journal unavailable")?)
        .map_err(|_| "replacement journal unavailable")?;
    let initial=ReplacementJournal { recovery_id:verified.retirement_id().into(),org_id:old.org_id.clone(),device_id:old.device_id.clone(),
        generation:verified.new_generation().into(),decision_digest:lower_hex(verified.decision_digest()),
        candidate_fingerprint:lower_hex(verified.fingerprint()),archive_sha256:archive.into(),
        origin_sequence:verified.next_sequence(),origin_hash:verified.previous_hash().map(str::to_owned),
        base_sequence:verified.next_sequence(),base_hash:verified.previous_hash().map(str::to_owned),
        next_sequence:verified.next_sequence(),previous_hash:verified.previous_hash().map(str::to_owned),
        pending:Vec::new(),pending_acks:Vec::new(),history:Vec::new(),inbox_sealed:None,drain_freeze:None,cancelled_freezes:Vec::new() };
    if !valid_replacement(&journal,&initial,verified) { return Err("replacement journal binding changed"); }
    Ok(VerifiedJournalBatch { device_id:journal.device_id,generation:journal.generation,
        outcomes:journal.pending,acks:journal.pending_acks })
}
struct PreparedRecoveryCheckin { trust:crate::fleet_machine_policy::FleetTrust, operator_key:String, revision:u64, nonce:String, request:Vec<u8>, issued:Instant, used:bool, decoy:bool, observation:bool, policy_only:bool }
fn prepare_recovery_checkin(snapshot:&crate::fleet_machine_policy::CredentialSnapshot, batch:VerifiedJournalBatch,
    mut request:fleet_proto::CheckinRequest, now:i64) -> Result<PreparedRecoveryCheckin,&'static str> {
    // A helper-supplied envelope cannot claim a held-policy or command-only
    // mode. Only `prepare_recovery_commands` derives it from protected state.
    request.recovery_held_policy=None;
    request.capabilities.retain(|cap|cap!="windows_recovery_commands_only_v1");
    prepare_recovery_checkin_inner(snapshot,batch,request,now,false)
}
fn prepare_recovery_commands(store:&crate::fleet_machine_policy::FleetMachinePolicy,
    snapshot:&crate::fleet_machine_policy::CredentialSnapshot,batch:VerifiedJournalBatch,now:i64)->Result<PreparedRecoveryCheckin,&'static str> {
    let held=store.held_recovery_policy(snapshot)?;
    let request=fleet_proto::CheckinRequest { capabilities:vec!["windows_recovery_commands_only_v1".into()],
        recovery_held_policy:Some(held),..Default::default() };
    prepare_recovery_checkin_inner(snapshot,batch,request,now,false)
}
fn prepare_recovery_results(store:&crate::fleet_machine_policy::FleetMachinePolicy,
    snapshot:&crate::fleet_machine_policy::CredentialSnapshot,batch:VerifiedJournalBatch,now:i64)->Result<PreparedRecoveryCheckin,&'static str> {
    let batch=limit_recovery_results(batch)?;
    let held=store.held_recovery_policy(snapshot)?;
    let request=fleet_proto::CheckinRequest { capabilities:vec!["windows_recovery_results_v1".into()],recovery_held_policy:Some(held),..Default::default() };
    prepare_recovery_checkin_inner(snapshot,batch,request,now,false)
}
#[cfg(test)]
pub(crate) fn test_held_policy_request(store:&crate::fleet_machine_policy::FleetMachinePolicy,
    snapshot:&crate::fleet_machine_policy::CredentialSnapshot)->Result<fleet_proto::CheckinRequest,&'static str> {
    let batch=VerifiedJournalBatch { device_id:snapshot.trust.device_id.clone(),generation:snapshot.trust.enrollment_generation.clone(),outcomes:Vec::new(),acks:Vec::new() };
    let prepared=prepare_recovery_commands(store,snapshot,batch,1234)?;
    serde_json::from_slice(&prepared.request).map_err(|_| "invalid recovery request")
}
#[cfg(test)]
pub(crate) fn test_results_policy_request(store:&crate::fleet_machine_policy::FleetMachinePolicy,
    snapshot:&crate::fleet_machine_policy::CredentialSnapshot)->Result<fleet_proto::CheckinRequest,&'static str> {
    let batch=VerifiedJournalBatch { device_id:snapshot.trust.device_id.clone(),generation:snapshot.trust.enrollment_generation.clone(),outcomes:Vec::new(),acks:Vec::new() };
    let prepared=prepare_recovery_results(store,snapshot,batch,1234)?;
    serde_json::from_slice(&prepared.request).map_err(|_| "invalid recovery request")
}
/// Cover traffic is constructed entirely by the service and carries no
/// journal, telemetry or commands; its response must never change local state.
fn prepare_recovery_decoy(snapshot:&crate::fleet_machine_policy::CredentialSnapshot, now:i64) -> Result<PreparedRecoveryCheckin,&'static str> {
    if !snapshot.is_recovered { return Err("recovery credential unavailable"); }
    let batch=VerifiedJournalBatch { device_id:snapshot.trust.device_id.clone(),generation:snapshot.trust.enrollment_generation.clone(),outcomes:Vec::new(),acks:Vec::new() };
    prepare_recovery_checkin_inner(snapshot,batch,fleet_proto::CheckinRequest::default(),now,true)
}

/// Gated contact-only network path. It intentionally never claims commands or
/// acknowledges outcomes; normal recovered command delivery is separate.
pub(super) async fn recovery_contact(store:&crate::fleet_machine_policy::FleetMachinePolicy, decoy:bool, policy_only:bool) -> Result<bool,&'static str> {
    recovery_contact_with(store,decoy,policy_only,&crate::fleet_credential::MachineDpapi).await
}
async fn recovery_contact_with(store:&crate::fleet_machine_policy::FleetMachinePolicy,decoy:bool,policy_only:bool,
    protector:&dyn crate::fleet_credential::Protector) -> Result<bool,&'static str> {
    use std::time::{Duration,SystemTime,UNIX_EPOCH};
    static CONTACT_LIMIT:std::sync::OnceLock<tokio::sync::Semaphore>=std::sync::OnceLock::new();
    let _permit=CONTACT_LIMIT.get_or_init(||tokio::sync::Semaphore::new(1)).try_acquire().map_err(|_| "recovery contact busy")?;
    if decoy && policy_only { return Err("invalid recovery request"); }
    let snapshot=store.credential_snapshot(protector)?.ok_or("recovery credential unavailable")?;
    if !snapshot.is_recovered || !store.credential_is_current(&snapshot) { return Err("recovery credential unavailable"); }
    if crate::fleet_runtime_env::fingerprint().as_deref()!=Some(snapshot.profile.environment_fingerprint.as_str()) { return Err("network configuration differs"); }
    let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&snapshot.profile.spki_pins);
    if !snapshot.profile.spki_pins.trim().is_empty() && pins.is_none() { return Err("transport security invalid"); }
    let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_| "recovery transport unavailable")?;
    let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
    let mut request=if decoy { prepare_recovery_decoy(&snapshot,now)? } else if policy_only { prepare_recovery_policy(&snapshot,now)? } else { prepare_recovery_observation(&snapshot,now)? };
    if !store.credential_is_current(&snapshot) { return Err("recovery credential changed"); }
    let operation=async {
        let mut response=client.post(format!("{}/v1/agents/checkin",snapshot.origin))
            .header(reqwest::header::CONTENT_TYPE,"application/json").body(request.request.clone()).send().await
            .map_err(|_| "recovery transport unavailable")?;
        if response.status()!=reqwest::StatusCode::OK { return Err("recovery transport rejected"); }
        let receipt=response.headers().get("x-wc-windows-recovery-contact").and_then(|v|v.to_str().ok()).ok_or("invalid recovery response")?.to_owned();
        let mut bytes=Vec::new();
        while let Some(chunk)=response.chunk().await.map_err(|_| "recovery transport unavailable")? {
            if bytes.len().checked_add(chunk.len()).is_none_or(|size|size>2*1024*1024) { return Err("invalid recovery response"); }
            bytes.extend_from_slice(&chunk);
        }
        Ok((bytes,receipt))
    };
    let (bytes,receipt)=tokio::time::timeout(Duration::from_secs(30),operation).await.map_err(|_| "recovery transport unavailable")??;
    if !store.credential_is_current(&snapshot) { return Err("recovery credential changed"); }
    if decoy { request.verify_decoy_response(&bytes,&receipt)?; Ok(false) }
    else if policy_only { let policy=request.verify_policy_response(&bytes,&receipt)?; store.install_recovery_policy(policy,&snapshot)?; Ok(false) }
    else { request.verify_observation(&bytes,&receipt) }
}

const AUTOMATIC_RECOVERY_CONTACT_READY:bool=cfg!(feature="fleet-recovery-lab");
const AUTOMATIC_RECOVERY_COMMANDS_READY:bool=cfg!(feature="fleet-recovery-lab");
const AUTOMATIC_RECOVERY_RESULTS_READY:bool=cfg!(feature="fleet-recovery-lab");
/// Service-owned observation continues after restart without exposing the
/// credential to the desktop. Disabled until the full recovery flow is tested.
pub(crate) async fn run_automatic_recovery_contact() {
    // Automatic recovery remains unavailable until signed v2 admission is integrated.
    const SIGNED_V2_RECOVERY_ADMISSION_READY: bool = false;
    if !SIGNED_V2_RECOVERY_ADMISSION_READY || !AUTOMATIC_RECOVERY_CONTACT_READY { return; }
    let mut last_late_id:Option<String>=None;
    loop {
        if let Ok(store)=crate::fleet_machine_policy::system_store() {
            if matches!(recovery_contact(store,false,false).await,Ok(false)) { let _=recovery_contact(store,false,true).await; }
            if let Some(base)=std::env::var_os("ProgramData") {
                let dir=std::path::PathBuf::from(base).join("WinCommander").join("fleet-machine");
                let fs=crate::fleet_machine_policy::WindowsFleetPolicyFs;
                let protector=crate::fleet_credential::MachineDpapi;
                if let Ok(candidate)=crate::fleet_recovery_candidate::CandidateStore::open(&fs,dir.clone(),&protector) {
                    if AUTOMATIC_RECOVERY_COMMANDS_READY { let _=recovery_work_contact(store,&fs,&dir,&protector,&candidate).await; }
                    if AUTOMATIC_RECOVERY_RESULTS_READY { let _=recovery_result_contact(store,&fs,&dir,&protector,&candidate).await; }
                    let _=check_recovered_leave(store,&fs,&dir,&protector,&candidate).await;
                    if let Ok(ids)=pending_late_ids(store,&fs,&dir,&protector,&candidate,last_late_id.as_deref()) {
                        for id in ids { let _=upload_late_evidence(store,&fs,&dir,&protector,&candidate,&id).await; last_late_id=Some(id); }
                    }
                }
            }
            let mut random=[0u8;1];
            if SysRng.try_fill_bytes(&mut random).is_ok() {
                tokio::time::sleep(Duration::from_secs(15+u64::from(random[0])%31)).await;
                let _=recovery_contact(store,true,false).await;
            }
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
fn prepare_recovery_observation(snapshot:&crate::fleet_machine_policy::CredentialSnapshot, now:i64) -> Result<PreparedRecoveryCheckin,&'static str> {
    if !snapshot.is_recovered { return Err("recovery credential unavailable"); }
    let batch=VerifiedJournalBatch { device_id:snapshot.trust.device_id.clone(),generation:snapshot.trust.enrollment_generation.clone(),outcomes:Vec::new(),acks:Vec::new() };
    let request=fleet_proto::CheckinRequest { capabilities:vec!["windows_recovery_contact_only_v1".into()],..Default::default() };
    let mut prepared=prepare_recovery_checkin_inner(snapshot,batch,request,now,false)?;
    prepared.observation=true;
    Ok(prepared)
}
fn prepare_recovery_policy(snapshot:&crate::fleet_machine_policy::CredentialSnapshot, now:i64)->Result<PreparedRecoveryCheckin,&'static str> {
    if !snapshot.is_recovered { return Err("recovery credential unavailable"); }
    let batch=VerifiedJournalBatch { device_id:snapshot.trust.device_id.clone(),generation:snapshot.trust.enrollment_generation.clone(),outcomes:Vec::new(),acks:Vec::new() };
    let request=fleet_proto::CheckinRequest { capabilities:vec!["windows_recovery_policy_only_v1".into()],..Default::default() };
    let mut prepared=prepare_recovery_checkin_inner(snapshot,batch,request,now,false)?; prepared.policy_only=true; Ok(prepared)
}
fn prepare_recovery_checkin_inner(snapshot:&crate::fleet_machine_policy::CredentialSnapshot, batch:VerifiedJournalBatch,
    mut request:fleet_proto::CheckinRequest, now:i64, decoy:bool) -> Result<PreparedRecoveryCheckin,&'static str> {
    if snapshot.trust.device_id!=batch.device_id || snapshot.trust.enrollment_generation!=batch.generation
        || snapshot.secret.len()!=32 { return Err("invalid recovery check-in"); }
    let operator_key=snapshot.operator_key.as_deref().ok_or("operator_key_not_pinned")?;
    crate::fleet_machine_policy::validate_operator_key(&snapshot.trust,operator_key)?;
    let mut random=[0u8;16]; SysRng.try_fill_bytes(&mut random).map_err(|_| "randomness unavailable")?;
    let nonce=format!("ci-{}",lower_hex(&random));
    request.protocol_version=fleet_proto::CHECKIN_PROTOCOL_VERSION;
    request.device_id=snapshot.trust.device_id.clone(); request.ts=now; request.nonce=nonce.clone();
    request.hmac_version=fleet_agent_core::HMAC_VERSION_V2; request.hmac.clear(); request.decoy=decoy;
    request.acks=batch.acks; request.action_outcomes=batch.outcomes;
    if !request.capabilities.iter().any(|v|v=="windows_recovery_contact_v2") { request.capabilities.push("windows_recovery_contact_v2".into()); }
    request.hmac=fleet_agent_core::compute_request_hmac_v2(&snapshot.secret,"POST","/v1/agents/checkin",&request)
        .map_err(|_| "invalid recovery check-in")?;
    let bytes=serde_json::to_vec(&request).map_err(|_| "invalid recovery check-in")?;
    if bytes.len()>2*1024*1024 { return Err("invalid recovery check-in"); }
    Ok(PreparedRecoveryCheckin { trust:snapshot.trust.clone(), operator_key:operator_key.into(), revision:snapshot.revision,nonce,request:bytes,issued:Instant::now(),used:false,decoy,observation:false,policy_only:false })
}
impl PreparedRecoveryCheckin {
    fn verify_bound_results(&mut self,response:&[u8],receipt:&str)->Result<AuthenticatedBoundConfirmation,&'static str> {
        let decoded=self.verify_response(response,receipt)?;
        if decoded.pending_approval || decoded.all_clear || decoded.policy.is_some() || !decoded.commands.is_empty()
            || !decoded.pending_search_jobs.is_empty() || !decoded.cancelled_search_job_ids.is_empty()
            || !decoded.ack_receipts.is_empty() || !decoded.action_outcome_receipts.is_empty()
            || decoded.action_outcome_recovery_checkpoint.is_some() || decoded.productivity_detail_receipt.is_some()
            || decoded.inventory_receipt.is_some() || !decoded.capabilities.iter().any(|cap|cap=="windows_recovery_results_v1") {
            return Err("invalid recovery result response");
        }
        let value:serde_json::Value=serde_json::from_slice(response).map_err(|_| "invalid recovery result response")?;
        let receipts:Vec<RecoveryResultReceipt>=serde_json::from_value(value.get("recovery_result_receipts").cloned()
            .ok_or("invalid recovery result response")?).map_err(|_| "invalid recovery result response")?;
        if receipts.is_empty() || receipts.len()>32 { return Err("invalid recovery result response"); }
        Ok(AuthenticatedBoundConfirmation { head:decoded.action_outcome_head.ok_or("invalid recovery result response")?,receipts })
    }
    /// Only a verified contact response can create a command batch. The epoch
    /// must come from protected service policy, never from a helper request.
    fn verify_command_batch(&mut self,response:&[u8],receipt:&str,expected_epoch:i64)->Result<AuthenticatedCommandBatch,&'static str> {
        let decoded=self.verify_response(response,receipt)?;
        if decoded.pending_approval || !decoded.capabilities.iter().any(|cap|cap=="windows_recovery_commands_only_v1")
            || !decoded.pending_search_jobs.is_empty() || !decoded.cancelled_search_job_ids.is_empty()
            || !decoded.ack_receipts.is_empty() || !decoded.action_outcome_receipts.is_empty()
            || decoded.action_outcome_head.is_some() || decoded.action_outcome_recovery_checkpoint.is_some()
            || decoded.productivity_detail_receipt.is_some() || decoded.inventory_receipt.is_some()
            || decoded.commands.len()>128 || decoded.commands.iter().any(|command|command.epoch_version!=expected_epoch
                || !valid_inbox_command(command,&self.trust,&self.operator_key)) { return Err("invalid command delivery"); }
        Ok(AuthenticatedCommandBatch { trust:self.trust.clone(),operator_key:self.operator_key.clone(),response_sha256:lower_hex(&Sha256::digest(response)),commands:decoded.commands })
    }
    /// The service must recheck its protected credential revision before and after transport.
    fn verify_response(&mut self,response:&[u8],receipt:&str)->Result<fleet_proto::CheckinResponse,&'static str> {
        if self.decoy || self.observation || self.policy_only { return Err("invalid recovery response"); }
        self.verify_response_inner(response,receipt)
    }
    fn verify_decoy_response(&mut self,response:&[u8],receipt:&str)->Result<(),&'static str> {
        if !self.decoy { return Err("invalid recovery response"); }
        self.verify_response_inner(response,receipt).map(|_|())
    }
    fn verify_observation(&mut self,response:&[u8],receipt:&str)->Result<bool,&'static str> {
        if !self.observation || self.decoy { return Err("invalid recovery response"); }
        self.verify_response_inner(response,receipt).map(|value|value.pending_approval)
    }
    fn verify_policy_response(&mut self,response:&[u8],receipt:&str)->Result<fleet_proto::PolicyEnvelope,&'static str> {
        if !self.policy_only || self.decoy || self.observation { return Err("invalid recovery response"); }
        let mut value=self.verify_response_inner(response,receipt)?;
        if value.pending_approval || !value.capabilities.iter().any(|cap|cap=="windows_recovery_policy_only_v1") { return Err("invalid recovery response"); }
        serde_json::from_value(value.policy.take().ok_or("invalid recovery response")?).map_err(|_| "invalid recovery response")
    }
    fn verify_response_inner(&mut self,response:&[u8],receipt:&str)->Result<fleet_proto::CheckinResponse,&'static str> {
        use base64::{engine::general_purpose::STANDARD,Engine};
        use ed25519_dalek::{Signature,VerifyingKey};
        if self.used || self.issued.elapsed()>Duration::from_secs(30) || response.len()>16*1024*1024 || !receipt.starts_with("v2.") { return Err("invalid recovery response"); }
        let encoded=&receipt[3..];
        let signature=STANDARD.decode(encoded).map_err(|_| "invalid recovery response")?;
        if STANDARD.encode(&signature)!=encoded { return Err("invalid recovery response"); }
        let signature=Signature::from_slice(&signature).map_err(|_| "invalid recovery response")?;
        let raw:[u8;32]=STANDARD.decode(&self.trust.signer_key).map_err(|_| "invalid recovery response")?.try_into().map_err(|_| "invalid recovery response")?;
        let key=VerifyingKey::from_bytes(&raw).map_err(|_| "invalid recovery response")?;
        if key.is_weak() { return Err("invalid recovery response"); }
        let bytes=fleet_proto::canonical_json_bytes(&serde_json::json!(["fleet.windows.recovery-contact.v2",
            self.trust.org_id,self.trust.device_id,"wincommander",self.nonce,self.operator_key,
            STANDARD.encode(Sha256::digest(&self.request)),STANDARD.encode(Sha256::digest(response))]));
        key.verify_strict(&bytes,&signature).map_err(|_| "invalid recovery response")?;
        let decoded:fleet_proto::CheckinResponse=serde_json::from_slice(response).map_err(|_| "invalid recovery response")?;
        if decoded.protocol_version!=fleet_proto::CHECKIN_PROTOCOL_VERSION || ((decoded.pending_approval || self.decoy || self.observation || self.policy_only)
            && (!decoded.commands.is_empty() || !decoded.pending_search_jobs.is_empty()
                || !decoded.cancelled_search_job_ids.is_empty() || decoded.all_clear || ((!self.policy_only || decoded.pending_approval) && decoded.policy.is_some())
                || !decoded.ack_receipts.is_empty() || !decoded.action_outcome_receipts.is_empty()
                || decoded.action_outcome_head.is_some() || decoded.action_outcome_recovery_checkpoint.is_some()
                || decoded.productivity_detail_receipt.is_some() || decoded.inventory_receipt.is_some())) { return Err("invalid recovery response"); }
        self.used=true;
        Ok(decoded)
    }
}

/// Staged command-only contact. No ACKs or outcomes are sent or drained until
/// payload-bound server receipts are implemented. Server must reoffer work.
async fn recovery_work_contact(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>)
    ->Result<(),&'static str> {
    let snapshot=store.credential_snapshot(protector)?.ok_or("recovery credential unavailable")?;
    if !snapshot.is_recovered || !store.credential_is_current(&snapshot) { return Err("recovery credential unavailable"); }
    if crate::fleet_runtime_env::fingerprint().as_deref()!=Some(snapshot.profile.environment_fingerprint.as_str()) { return Err("network configuration differs"); }
    let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&snapshot.profile.spki_pins);
    if !snapshot.profile.spki_pins.trim().is_empty() && pins.is_none() { return Err("transport security invalid"); }
    let epoch=store.held_recovery_epoch(&snapshot)?;
    let empty=VerifiedJournalBatch { device_id:snapshot.trust.device_id.clone(),generation:snapshot.trust.enrollment_generation.clone(),outcomes:Vec::new(),acks:Vec::new() };
    let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
    let mut prepared=prepare_recovery_commands(store,&snapshot,empty,now)?;
    if !store.credential_is_current(&snapshot) { return Err("recovery credential changed"); }
    let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_| "recovery transport unavailable")?;
    let operation=async {
        let mut response=client.post(format!("{}/v1/agents/checkin",snapshot.origin))
            .header(reqwest::header::CONTENT_TYPE,"application/json").body(prepared.request.clone()).send().await.map_err(|_| "recovery transport unavailable")?;
        if response.status()!=reqwest::StatusCode::OK { return Err("recovery transport rejected"); }
        let receipt=response.headers().get("x-wc-windows-recovery-contact").and_then(|value|value.to_str().ok()).ok_or("invalid recovery response")?.to_owned();
        let mut bytes=Vec::new();while let Some(chunk)=response.chunk().await.map_err(|_| "recovery transport unavailable")? {
            if bytes.len().checked_add(chunk.len()).is_none_or(|size|size>2*1024*1024) { return Err("invalid recovery response"); } bytes.extend_from_slice(&chunk);
        }
        Ok((bytes,receipt))
    };
    let (bytes,receipt)=tokio::time::timeout(Duration::from_secs(30),operation).await.map_err(|_| "recovery transport unavailable")??;
    let batch=prepared.verify_command_batch(&bytes,&receipt,epoch)?;
    with_historical_recovery(store,fs,dir,candidate,Some(&snapshot),|current,old,verified,archive,operator,current_epoch| {
        if current_epoch!=epoch || *current!=batch.trust || operator!=batch.operator_key { return Err("recovery policy changed"); }
        persist_unified_batch(fs,dir,protector,old,verified,archive,batch)
    })
}

/// Staged results-only exchange; never exposes credentials or drains on a
/// network success without the signed, payload-bound server receipts.
async fn recovery_result_contact(store:&crate::fleet_machine_policy::FleetMachinePolicy,fs:&dyn crate::policy_store::PolicyFs,
    dir:&std::path::Path,protector:&dyn crate::fleet_credential::Protector,candidate:&crate::fleet_recovery_candidate::CandidateStore<'_>)
    ->Result<(),&'static str> {
    static LIMIT:std::sync::OnceLock<tokio::sync::Semaphore>=std::sync::OnceLock::new();
    let _permit=LIMIT.get_or_init(||tokio::sync::Semaphore::new(1)).try_acquire().map_err(|_| "recovery results already in progress")?;
    let snapshot=store.credential_snapshot(protector)?.ok_or("recovery credential unavailable")?;
    if !snapshot.is_recovered || !store.credential_is_current(&snapshot) { return Err("recovery credential unavailable"); }
    if crate::fleet_runtime_env::fingerprint().as_deref()!=Some(snapshot.profile.environment_fingerprint.as_str()) { return Err("network configuration differs"); }
    let pins=fleet_agent_core::SpkiPinSet::from_hex_list(&snapshot.profile.spki_pins);
    if !snapshot.profile.spki_pins.trim().is_empty() && pins.is_none() { return Err("transport security invalid"); }
    let batch=with_historical_recovery(store,fs,dir,candidate,Some(&snapshot),|_,old,verified,archive,_,_|load_replacement_batch(fs,dir,old,verified,archive))?;
    if batch.outcomes.is_empty() { return Ok(()); }
    let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock unavailable")?.as_secs()).map_err(|_| "clock unavailable")?;
    let mut prepared=prepare_recovery_results(store,&snapshot,batch,now)?;
    if !store.credential_is_current(&snapshot) { return Err("recovery credential changed"); }
    let client=fleet_agent_core::build_client_without_redirects(pins).map_err(|_| "recovery transport unavailable")?;
    let operation=async {
        let mut response=client.post(format!("{}/v1/agents/checkin",snapshot.origin)).header(reqwest::header::CONTENT_TYPE,"application/json")
            .body(prepared.request.clone()).send().await.map_err(|_| "recovery transport unavailable")?;
        if response.status()!=reqwest::StatusCode::OK { return Err("recovery transport rejected"); }
        let receipt=response.headers().get("x-wc-windows-recovery-contact").and_then(|v|v.to_str().ok()).ok_or("invalid recovery response")?.to_owned();
        let mut bytes=Vec::new();while let Some(chunk)=response.chunk().await.map_err(|_| "recovery transport unavailable")? {
            if bytes.len().checked_add(chunk.len()).is_none_or(|size|size>2*1024*1024) { return Err("invalid recovery response"); } bytes.extend_from_slice(&chunk);
        } Ok((bytes,receipt))
    };
    let (bytes,receipt)=tokio::time::timeout(Duration::from_secs(30),operation).await.map_err(|_| "recovery transport unavailable")??;
    let confirmation=prepared.verify_bound_results(&bytes,&receipt)?;
    with_historical_recovery(store,fs,dir,candidate,Some(&snapshot),|_,old,verified,archive,_,_| {
        confirm_replacement_bound(fs,dir,old,verified,archive,confirmation)
    })
}

/// Local administrator approval for exactly one protected operator-key binding.
pub(crate) struct OperatorApproval { binding:[u8;32], sid:String, session:u32, logon:(u32,i32), expires:Instant }
fn operator_binding(trust:&crate::fleet_machine_policy::FleetTrust, operator:&str)->Result<[u8;32],&'static str> {
    crate::fleet_machine_policy::validate_operator_key(trust,operator)?;
    let bytes=serde_json::to_vec(&("wincommander:fleet:operator-key-pin:v1",&trust.org_id,&trust.device_id,
        &trust.enrollment_generation,&trust.signer_key,operator)).map_err(|_| "invalid operator key")?;
    Ok(Sha256::digest(bytes).into())
}
impl OperatorApproval {
    pub(crate) fn validate(self, trust:&crate::fleet_machine_policy::FleetTrust, operator:&str,
        peer:&AuthenticatedPipePeer)->Result<String,&'static str> {
        if self.expires<=Instant::now() || !session_matches(peer) || self.sid!=peer.caller_sid()
            || self.session!=peer.session_id() || self.logon!=peer.authentication_id()
            || self.binding!=operator_binding(trust,operator)? { return Err("operator approval unavailable"); }
        Ok(self.sid)
    }
}
/// A future service route must show this dialog after independently obtaining
/// the offline operator key; no renderer-supplied confirmation is accepted.
pub(super) fn confirm_operator(peer:&AuthenticatedPipePeer, trust:&crate::fleet_machine_policy::FleetTrust,
    operator:&str)->Result<OperatorApproval,&'static str> {
    if !session_matches(peer) { return Err("operator approval unavailable"); }
    let binding=operator_binding(trust,operator)?;
    let raw=crate::fleet_machine_policy::validate_operator_key(trust,operator)?;
    let fingerprint=lower_hex(&Sha256::digest(raw));
    let title:Vec<u16>="WinCommander Fleet operator key".encode_utf16().chain(std::iter::once(0)).collect();
    let body:Vec<u16>=format!("Approve this offline operator key?\n\nSHA-256: {fingerprint}\n\nCompare this fingerprint with the independently held offline key.").encode_utf16().chain(std::iter::once(0)).collect();
    let mut response=0;
    let success=unsafe { WTSSendMessageW(WTS_CURRENT_SERVER_HANDLE,peer.session_id(),title.as_ptr(),((title.len()-1)*2) as u32,
        body.as_ptr(),((body.len()-1)*2) as u32,MB_YESNO|MB_ICONWARNING|MB_DEFBUTTON2,60,&mut response,1) }!=0;
    if !success || response!=IDYES || !session_matches(peer) { return Err("operator approval denied or unavailable"); }
    Ok(OperatorApproval { binding,sid:peer.caller_sid().into(),session:peer.session_id(),logon:peer.authentication_id(),expires:Instant::now()+Duration::from_secs(120) })
}

#[cfg(test)]
mod recovery_contact_tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn late_retry_rotates_after_failure_and_wraps() {
        let ids=vec!["first".to_owned(),"second".to_owned(),"third".to_owned()];
        assert_eq!(next_late_retry(&ids,None).as_deref(),Some("first"));
        assert_eq!(next_late_retry(&ids,Some("first")).as_deref(),Some("second"));
        assert_eq!(next_late_retry(&ids,Some("third")).as_deref(),Some("first"));
        assert!(next_late_retry(&[],Some("first")).is_none());
    }

    #[test]
    fn decoy_is_service_built_empty_and_request_authenticated() {
        let signer=SigningKey::from_bytes(&[11;32]);
        let snapshot=crate::fleet_machine_policy::CredentialSnapshot {
            trust:crate::fleet_machine_policy::FleetTrust { org_id:"org".into(),device_id:"device".into(),
                enrollment_generation:"generation".into(),signer_key:STANDARD.encode(signer.verifying_key().as_bytes()) },
            origins:vec![],origin:"https://fleet.example".into(),profile:crate::fleet_machine_policy::TransportProfile { spki_pins:String::new(),environment_fingerprint:String::new() },
            revision:1,connection_revision:1,sealed_credential_sha256:[0;32],recovery_commit:None,secret:zeroize::Zeroizing::new(vec![7;32]),
            operator_key:Some(STANDARD.encode(SigningKey::from_bytes(&[33;32]).verifying_key().as_bytes())),is_recovered:true,
        };
        let prepared=prepare_recovery_decoy(&snapshot,1234).unwrap();
        let request:fleet_proto::CheckinRequest=serde_json::from_slice(&prepared.request).unwrap();
        assert!(request.decoy && request.acks.is_empty() && request.action_outcomes.is_empty() && request.events.is_empty());
        assert!(request.posture.is_none() && request.telemetry.is_none() && request.decoy_tripwire.is_none());
        assert_eq!(request.hmac,fleet_agent_core::compute_request_hmac_v2(&snapshot.secret,"POST","/v1/agents/checkin",&request).unwrap());
        assert!(request.capabilities.iter().any(|cap|cap=="windows_recovery_contact_v2"));
        let observation=prepare_recovery_observation(&snapshot,1234).unwrap();
        let request:fleet_proto::CheckinRequest=serde_json::from_slice(&observation.request).unwrap();
        assert!(!request.decoy && request.acks.is_empty() && request.action_outcomes.is_empty());
        assert!(request.capabilities.iter().any(|cap|cap=="windows_recovery_contact_only_v1"));
    }
    #[test]
    fn recovery_lab_security_dependencies_are_enabled_together() {
        let enabled=cfg!(feature="fleet-recovery-lab");
        assert_eq!(READY,enabled);assert_eq!(UNKNOWN_STOP_REVIEW_READY,enabled);
        assert_eq!(AUTOMATIC_RECOVERY_CONTACT_READY,enabled);assert_eq!(AUTOMATIC_RECOVERY_COMMANDS_READY,enabled);
        assert_eq!(AUTOMATIC_RECOVERY_RESULTS_READY,enabled);
    }
    #[cfg(not(feature="fleet-recovery-lab"))]
    #[tokio::test]
    async fn automatic_contact_remains_disabled() {
        assert!(!AUTOMATIC_RECOVERY_CONTACT_READY);
        tokio::time::timeout(Duration::from_millis(100),run_automatic_recovery_contact()).await.unwrap();
    }

    #[test]
    fn signed_contact_binds_operator_request_and_safe_pending_response() {
        let signer = SigningKey::from_bytes(&[11; 32]);
        let trust = crate::fleet_machine_policy::FleetTrust {
            org_id: "org".into(), device_id: "device".into(),
            enrollment_generation: "generation".into(),
            signer_key: STANDARD.encode(signer.verifying_key().as_bytes()),
        };
        let operator = STANDARD.encode(SigningKey::from_bytes(&[33; 32]).verifying_key().as_bytes());
        let mut prepared = PreparedRecoveryCheckin { trust, operator_key: operator.clone(), revision: 1,
            nonce: "ci-test".into(), request: b"request".to_vec(), issued: Instant::now(), used: false, decoy:false, observation:false,policy_only:false };
        let sign = |prepared: &PreparedRecoveryCheckin, response: &[u8], key: &str| {
            let tuple = fleet_proto::canonical_json_bytes(&serde_json::json!(["fleet.windows.recovery-contact.v2",
                prepared.trust.org_id, prepared.trust.device_id, "wincommander", prepared.nonce, key,
                STANDARD.encode(Sha256::digest(&prepared.request)), STANDARD.encode(Sha256::digest(response))]));
            format!("v2.{}", STANDARD.encode(signer.sign(&tuple).to_bytes()))
        };
        let mut response = fleet_proto::CheckinResponse::<fleet_proto::SignedCommand>::default();
        response.pending_approval = true;
        let safe = serde_json::to_vec(&response).unwrap();
        let wrong = sign(&prepared, &safe, &STANDARD.encode([1u8; 32]));
        assert!(prepared.verify_response(&safe, &wrong).is_err());
        response.all_clear = true;
        let unsafe_response = serde_json::to_vec(&response).unwrap();
        let unsafe_receipt = sign(&prepared, &unsafe_response, &operator);
        assert!(prepared.verify_response(&unsafe_response, &unsafe_receipt).is_err());
        response.all_clear = false;
        response.policy = Some(serde_json::json!({"unexpected":true}));
        let policy_response = serde_json::to_vec(&response).unwrap();
        let policy_receipt = sign(&prepared, &policy_response, &operator);
        assert!(prepared.verify_response(&policy_response, &policy_receipt).is_err());
        let valid = sign(&prepared, &safe, &operator);
        assert!(prepared.verify_response(&safe, &valid).is_ok());
        assert!(prepared.verify_response(&safe, &valid).is_err());
        prepared.used=false; prepared.decoy=true;
        assert!(prepared.verify_response(&safe, &valid).is_err());
        assert!(prepared.verify_decoy_response(&policy_response, &policy_receipt).is_err());
        assert!(prepared.verify_decoy_response(&safe, &valid).is_ok());
        assert!(prepared.verify_decoy_response(&safe, &valid).is_err());
        prepared.used=false; prepared.decoy=false; prepared.observation=true;
        assert!(prepared.verify_response(&safe,&valid).is_err());
        assert!(prepared.verify_observation(&policy_response,&policy_receipt).is_err());
        assert!(prepared.verify_observation(&safe,&valid).unwrap());
        let mut command=fleet_proto::SignedCommand { command_id:"command-1".into(),idempotency_key:"key-1".into(),
            device_id:fleet_proto::DeviceId("device".into()),catalog_id:"device.checkin_now".into(),
            action_class:fleet_proto::ActionClass::Safe,payload:serde_json::json!({}),epoch_version:7,
            signature:String::new(),signer_key:prepared.trust.signer_key.clone() };
        command.signature=STANDARD.encode(signer.sign(&fleet_proto::canonical_command_bytes(&command.idempotency_key,
            &command.device_id.0,&command.catalog_id,command.action_class.as_wire_str(),&command.payload,command.epoch_version)).to_bytes());
        let mut delivery=fleet_proto::CheckinResponse::<fleet_proto::SignedCommand>::default();
        delivery.capabilities.push("windows_recovery_commands_only_v1".into()); delivery.commands.push(command);
        let bytes=serde_json::to_vec(&delivery).unwrap(); let receipt=sign(&prepared,&bytes,&operator);
        prepared.used=false; prepared.observation=false;
        assert!(prepared.verify_command_batch(&bytes,&receipt,8).is_err());
        prepared.used=false;
        assert_eq!(prepared.verify_command_batch(&bytes,&receipt,7).unwrap().commands.len(),1);
        // A response from a previous request cannot be replayed under a fresh nonce.
        prepared.used=false;let original_nonce=prepared.nonce.clone();prepared.nonce="ci-00000000000000000000000000000000".into();
        assert!(prepared.verify_command_batch(&bytes,&receipt,7).is_err());prepared.nonce=original_nonce;
        // Even a freshly signed envelope cannot authorize a modified command.
        let mut injected:serde_json::Value=serde_json::from_slice(&bytes).unwrap();
        injected["commands"][0]["payload"]=serde_json::json!({"injected":true});
        let injected_bytes=serde_json::to_vec(&injected).unwrap();let injected_receipt=sign(&prepared,&injected_bytes,&operator);
        prepared.used=false;assert!(prepared.verify_command_batch(&injected_bytes,&injected_receipt,7).is_err());
        let mut result=serde_json::to_value(fleet_proto::CheckinResponse::<fleet_proto::SignedCommand>::default()).unwrap();
        result["capabilities"]=serde_json::json!(["windows_recovery_results_v1"]);
        result["action_outcome_head"]=serde_json::json!({"sequence":7,"record_hash":"ab".repeat(32)});
        result["recovery_result_receipts"]=serde_json::json!([{"command_id":"command-1","report_id":null,"ack_sha256":"cd".repeat(32),
            "outcome_receipt_id":"receipt-1","sequence":7,"record_hash":"ab".repeat(32),"disposition":"accepted"}]);
        let result_bytes=serde_json::to_vec(&result).unwrap();let result_receipt=sign(&prepared,&result_bytes,&operator);
        prepared.used=false;
        assert_eq!(prepared.verify_bound_results(&result_bytes,&result_receipt).unwrap().receipts.len(),1);
        prepared.used=false;result["recovery_result_receipts"][0]["ack_sha256"]="ef".repeat(32).into();
        assert!(prepared.verify_bound_results(&serde_json::to_vec(&result).unwrap(),&result_receipt).is_err());
        let mut policy=fleet_proto::PolicyEnvelope { org_id:fleet_proto::OrgId("org".into()),device_id:fleet_proto::DeviceId("device".into()),
            enrollment_generation:Some("generation".into()),config_revision:Some(1),resolution_revision:Some(1),config:None,
            shield:None,shield_revision:0,clipboard_guard_enabled:false,signature:String::new(),signer_key:prepared.trust.signer_key.clone() };
        policy.signature=STANDARD.encode(signer.sign(&policy.preimage()).to_bytes());
        let mut response=fleet_proto::CheckinResponse::<fleet_proto::SignedCommand>::default();
        response.capabilities.push("windows_recovery_policy_only_v1".into()); response.policy=Some(serde_json::to_value(&policy).unwrap());
        let bytes=serde_json::to_vec(&response).unwrap();let receipt=sign(&prepared,&bytes,&operator);
        prepared.used=false;prepared.policy_only=true;
        assert_eq!(prepared.verify_policy_response(&bytes,&receipt).unwrap(),policy);
    }
}
