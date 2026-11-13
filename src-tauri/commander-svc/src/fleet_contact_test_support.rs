// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::policy_store::PolicyStoreError;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub(super) struct MemoryFs {
    pub bytes: Arc<Mutex<Option<Vec<u8>>>>,
    pub fail: Arc<AtomicBool>,
}
impl PolicyFs for MemoryFs {
    fn read(&self, _: &Path) -> io::Result<Vec<u8>> {
        self.bytes
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
    fn atomic_write(&self, _: &Path, bytes: &[u8]) -> io::Result<()> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        *self.bytes.lock().unwrap() = Some(bytes.to_vec());
        Ok(())
    }
    fn ensure_dir_secure(&self, _: &Path) -> Result<(), PolicyStoreError> {
        Ok(())
    }
}
pub(super) fn key() -> SigningKey {
    SigningKey::from_bytes(&[41; 32])
}
pub(super) fn trust() -> FleetTrust {
    FleetTrust {
        org_id: "contact-test-org".into(),
        device_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
        enrollment_generation: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
        signer_key: STANDARD.encode(key().verifying_key().as_bytes()),
    }
}
pub(super) fn proof(store: &FleetMachinePolicy, trust: &FleetTrust) -> String {
    let nonce = store
        .begin_enrollment(
            trust.device_id.clone(),
            trust.signer_key.clone(),
            Vec::new(),
            None,
        )
        .unwrap();
    let bytes = serde_json::to_vec(&[
        "wincommander:shield-progress:enrollment:v1",
        &trust.org_id,
        &trust.device_id,
        &trust.enrollment_generation,
        &nonce,
    ])
    .unwrap();
    STANDARD.encode(key().sign(&bytes).to_bytes())
}
pub(super) fn policy(binding: FleetTrust) -> PolicyEnvelope {
    let mut policy = PolicyEnvelope {
        org_id: fleet_proto::OrgId(binding.org_id),
        device_id: fleet_proto::DeviceId(binding.device_id),
        enrollment_generation: Some(binding.enrollment_generation),
        config_revision: Some(1),
        resolution_revision: Some(1),
        config: None,
        shield: None,
        shield_revision: 0,
        clipboard_guard_enabled: false,
        signature: String::new(),
        signer_key: binding.signer_key,
    };
    policy.signature = STANDARD.encode(key().sign(&policy.preimage()).to_bytes());
    policy
}
pub(super) fn request(nonce: &str) -> Value {
    json!({"protocol_version":1,"device_id":trust().device_id,"nonce":nonce,"ts":1000,
        "hmac_version":2,"hmac":STANDARD.encode([7;32]),"capabilities":["windows_contact_receipt_v1"],"decoy":false})
}
pub(super) fn response() -> Value {
    json!({"protocol_version":1,"policy":policy(trust()),"pending_approval":false})
}
pub(super) fn receipt(nonce: &str, request: &[u8], response: &[u8]) -> String {
    let preimage = fleet_proto::canonical_json_bytes(&json!([
        "fleet.windows.contact.v1",
        trust().org_id,
        trust().device_id,
        "wincommander",
        nonce,
        STANDARD.encode(Sha256::digest(request)),
        STANDARD.encode(Sha256::digest(response))
    ]));
    format!("v1.{}", STANDARD.encode(key().sign(&preimage).to_bytes()))
}
pub(super) struct Fixture {
    pub store: FleetMachinePolicy,
    pub fs: MemoryFs,
    pub issued: Instant,
}
impl Fixture {
    pub fn new() -> Self {
        let fs = MemoryFs::default();
        let store =
            FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("contact-test")).unwrap();
        let signature = proof(&store, &trust());
        store.provision(trust(), &signature, "S-1-5-18").unwrap();
        Self {
            store,
            fs,
            issued: Instant::now(),
        }
    }
    pub fn nonce(&self) -> String {
        self.store
            .begin_contact_with_clock(|| self.issued)
            .unwrap()
            .nonce
    }
    pub fn submit(
        &self,
        nonce: &str,
        request: &[u8],
        response: &[u8],
        age: Duration,
        wall: u64,
    ) -> Result<ContactObservation, &'static str> {
        self.store.record_contact_with_clock(
            nonce,
            request,
            response,
            &receipt(nonce, request, response),
            || (self.issued + age, Ok(wall)),
        )
    }
    pub fn packet(&self, nonce: &str) -> (Vec<u8>, Vec<u8>) {
        (
            serde_json::to_vec(&request(nonce)).unwrap(),
            serde_json::to_vec(&response()).unwrap(),
        )
    }
}
