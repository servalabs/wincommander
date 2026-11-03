// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::sync::Mutex;

fn binding() -> CredentialBinding<'static> {
    CredentialBinding {
        org_id: "org-a",
        device_id: "device-a",
        enrollment_generation: "generation-a",
        origin: "https://fleet.example",
    }
}

struct Entry {
    entropy: Vec<u8>,
    secret: Zeroizing<Vec<u8>>,
}
#[derive(Default)]
struct MockProtector(Mutex<Vec<Entry>>);
impl Protector for MockProtector {
    fn protect(&self, plain: &[u8], entropy: &[u8]) -> Result<Vec<u8>, &'static str> {
        let mut entries = self.0.lock().unwrap();
        entries.push(Entry {
            entropy: entropy.to_vec(),
            secret: Zeroizing::new(plain.to_vec()),
        });
        Ok((entries.len() as u64).to_be_bytes().to_vec())
    }
    fn unprotect(&self, sealed: &[u8], entropy: &[u8]) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        let id = u64::from_be_bytes(sealed.try_into().map_err(|_| UNAVAILABLE)?);
        let entries = self.0.lock().unwrap();
        let entry = entries
            .get(id.checked_sub(1).ok_or(UNAVAILABLE)? as usize)
            .ok_or(UNAVAILABLE)?;
        if entry.entropy != entropy {
            return Err(UNAVAILABLE);
        }
        Ok(Zeroizing::new(entry.secret.to_vec()))
    }
}

#[test]
fn sealed_credentials_bind_every_identity_field_and_exact_origin() {
    let protector = MockProtector::default();
    let secret = [3; 32];
    let sealed = seal(&protector, &binding(), &secret).unwrap();
    assert_eq!(
        unseal(&protector, &binding(), &sealed).unwrap().as_slice(),
        secret
    );
    for field in 0..4 {
        let mut changed = binding();
        match field {
            0 => changed.org_id = "org-b",
            1 => changed.device_id = "device-b",
            2 => changed.enrollment_generation = "generation-b",
            _ => changed.origin = "https://fleet.example/",
        }
        assert!(unseal(&protector, &changed, &sealed).is_err());
    }
    let mut malformed = sealed.clone();
    malformed[0] ^= 1;
    assert!(unseal(&protector, &binding(), &malformed).is_err());
    assert!(seal(&protector, &binding(), &[1; 31]).is_err());
    assert!(unseal(&protector, &binding(), &[]).is_err());
    assert!(unseal(&protector, &binding(), &vec![0; MAX_SEALED + 1]).is_err());
    let wrong_length = protector
        .protect(&[1; 31], &binding().entropy().unwrap())
        .unwrap();
    assert!(unseal(&protector, &binding(), &wrong_length).is_err());
    let mut empty = binding();
    empty.origin = "";
    assert!(seal(&protector, &empty, &secret).is_err());
    let entropy = binding().entropy().unwrap();
    assert_ne!(entropy, Sha256::digest(b"https://fleet.example").as_slice());
}

#[test]
fn protection_failures_cannot_return_a_credential() {
    struct Failed;
    impl Protector for Failed {
        fn protect(&self, _: &[u8], _: &[u8]) -> Result<Vec<u8>, &'static str> {
            Err(UNAVAILABLE)
        }
        fn unprotect(&self, _: &[u8], _: &[u8]) -> Result<Zeroizing<Vec<u8>>, &'static str> {
            Err(UNAVAILABLE)
        }
    }
    assert!(seal(&Failed, &binding(), &[1; 32]).is_err());
    assert!(unseal(&Failed, &binding(), b"sealed").is_err());
}

#[cfg(windows)]
#[test]
fn windows_machine_dpapi_roundtrip_rejects_wrong_origin_and_corruption() {
    let secret = Zeroizing::new(vec![0x5a; 32]);
    let sealed = seal(&MachineDpapi, &binding(), &secret).unwrap();
    assert_ne!(sealed, secret.as_slice());
    assert_eq!(
        unseal(&MachineDpapi, &binding(), &sealed)
            .unwrap()
            .as_slice(),
        secret.as_slice()
    );
    let mut changed = binding();
    changed.origin = "https://other.example";
    assert!(unseal(&MachineDpapi, &changed, &sealed).is_err());
    let mut corrupted = sealed;
    let index = corrupted.len() / 2;
    corrupted[index] ^= 1;
    assert!(unseal(&MachineDpapi, &binding(), &corrupted).is_err());
}
