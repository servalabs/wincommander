// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental, unintegrated hybrid verification. This is not a release format.
//!
//! Callers must obtain trusted public keys and the minimum epoch independently.
//! An opt-in signing rehearsal exists; no production issuer, persistent rollback
//! protection, key rotation, expiry, or product format is implemented.

pub mod jws;
pub mod release;
#[cfg(feature = "experimental-offline-verifier")]
pub mod offline_release;

use aws_lc_rs::signature::{UnparsedPublicKey, ML_DSA_65};
use ed25519_dalek::{Signature, VerifyingKey};

const DOMAIN: &[u8] = b"WinCommander experimental hybrid signature v1\0";
const MAGIC: &[u8; 4] = b"WCX1";
const PROFILE: [u8; 3] = [1, 1, 2]; // Profile 1: Ed25519 AND ML-DSA-65.
const HEADER_LEN: usize = 52;
const ED_SIGNATURE_LEN: usize = 64;
const PQ_SIGNATURE_LEN: usize = 3309;
pub const MAX_PAYLOAD_LEN: usize = 1024 * 1024;
pub const ML_DSA_65_PUBLIC_KEY_LEN: usize = 1952;

/// A closed set of experimental purposes; it is not a production registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Purpose {
    Licence = 1,
    Release = 2,
}

/// Trust is local policy; none of these values may be selected by the sender.
pub struct Trust<'a> {
    pub purpose: Purpose,
    pub minimum_epoch: u64,
    pub key_id: &'a [u8; 32],
    pub ed25519_public: &'a [u8; 32],
    pub ml_dsa_65_public: &'a [u8; ML_DSA_65_PUBLIC_KEY_LEN],
}

#[derive(Debug, PartialEq, Eq)]
pub enum VerifyError {
    Malformed,
    UntrustedContext,
    InvalidSignature,
}

/// A borrowed payload is returned only after both required signatures verify.
pub struct Verified<'a> {
    pub payload: &'a [u8],
    pub epoch: u64,
}

/// Verify a bounded experimental object using caller-provided trust policy.
pub fn verify<'a>(encoded: &'a [u8], trust: &Trust<'_>) -> Result<Verified<'a>, VerifyError> {
    let fixed_len = HEADER_LEN + ED_SIGNATURE_LEN + PQ_SIGNATURE_LEN;
    if encoded.len() < fixed_len || encoded.len() > fixed_len + MAX_PAYLOAD_LEN {
        return Err(VerifyError::Malformed);
    }
    if &encoded[..4] != MAGIC || encoded[4..7] != PROFILE {
        return Err(VerifyError::Malformed);
    }
    if encoded[7] != trust.purpose as u8 || &encoded[16..48] != trust.key_id {
        return Err(VerifyError::UntrustedContext);
    }
    let epoch = u64::from_be_bytes(
        encoded[8..16]
            .try_into()
            .map_err(|_| VerifyError::Malformed)?,
    );
    if epoch < trust.minimum_epoch {
        return Err(VerifyError::UntrustedContext);
    }
    let payload_len = u32::from_be_bytes(
        encoded[48..52]
            .try_into()
            .map_err(|_| VerifyError::Malformed)?,
    ) as usize;
    if payload_len > MAX_PAYLOAD_LEN || encoded.len() != fixed_len + payload_len {
        return Err(VerifyError::Malformed);
    }
    let body_end = HEADER_LEN + payload_len;
    let ed_end = body_end + ED_SIGNATURE_LEN;
    let mut message = Vec::with_capacity(DOMAIN.len() + body_end);
    message.extend_from_slice(DOMAIN);
    message.extend_from_slice(&encoded[..body_end]);

    let key = VerifyingKey::from_bytes(trust.ed25519_public)
        .map_err(|_| VerifyError::InvalidSignature)?;
    let sig = Signature::from_slice(&encoded[body_end..ed_end])
        .map_err(|_| VerifyError::InvalidSignature)?;
    key.verify_strict(&message, &sig)
        .map_err(|_| VerifyError::InvalidSignature)?;
    UnparsedPublicKey::new(&ML_DSA_65, trust.ml_dsa_65_public)
        .verify(&message, &encoded[ed_end..])
        .map_err(|_| VerifyError::InvalidSignature)?;
    Ok(Verified {
        payload: &encoded[HEADER_LEN..body_end],
        epoch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn requires_both_signatures_and_enforces_local_policy() {
        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let pq_public: [u8; ML_DSA_65_PUBLIC_KEY_LEN] =
            pq.public_key().as_ref().try_into().unwrap();
        let ed_public = ed.verifying_key().to_bytes();
        let key_id = [7; 32];
        let trust = Trust {
            purpose: Purpose::Licence,
            minimum_epoch: 3,
            key_id: &key_id,
            ed25519_public: &ed_public,
            ml_dsa_65_public: &pq_public,
        };
        let payload = b"existing customer fixture";
        let mut body = Vec::new();
        body.extend_from_slice(MAGIC);
        body.extend_from_slice(&PROFILE);
        body.push(Purpose::Licence as u8);
        body.extend_from_slice(&3u64.to_be_bytes());
        body.extend_from_slice(&key_id);
        body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        body.extend_from_slice(payload);
        let mut message = DOMAIN.to_vec();
        message.extend_from_slice(&body);
        let ed_signature = ed.sign(&message);
        let mut pq_signature = vec![0; PQ_SIGNATURE_LEN];
        assert_eq!(
            pq.sign(&message, &mut pq_signature).unwrap(),
            PQ_SIGNATURE_LEN
        );
        let mut encoded = body;
        encoded.extend_from_slice(&ed_signature.to_bytes());
        encoded.extend_from_slice(&pq_signature);
        let verified = verify(&encoded, &trust).unwrap();
        assert_eq!(verified.payload, payload);
        assert_eq!(verified.epoch, 3);

        for offset in [
            HEADER_LEN,
            HEADER_LEN + payload.len(),
            HEADER_LEN + payload.len() + ED_SIGNATURE_LEN,
        ] {
            let mut changed = encoded.clone();
            changed[offset] ^= 1;
            assert_eq!(
                verify(&changed, &trust).err(),
                Some(VerifyError::InvalidSignature)
            );
        }
        for offset in [0, 4, 5, 6] {
            let mut changed = encoded.clone();
            changed[offset] ^= 1;
            assert_eq!(verify(&changed, &trust).err(), Some(VerifyError::Malformed));
        }
        for offset in [7, 16] {
            let mut changed = encoded.clone();
            changed[offset] ^= 1;
            assert_eq!(
                verify(&changed, &trust).err(),
                Some(VerifyError::UntrustedContext)
            );
        }
        let mut changed = encoded.clone();
        changed[15] = 2;
        assert_eq!(
            verify(&changed, &trust).err(),
            Some(VerifyError::UntrustedContext)
        );
        let mut changed = encoded.clone();
        changed.extend_from_slice(&[0]);
        assert_eq!(verify(&changed, &trust).err(), Some(VerifyError::Malformed));
        assert_eq!(
            verify(&encoded[..encoded.len() - 1], &trust).err(),
            Some(VerifyError::Malformed)
        );
        let mut changed = encoded.clone();
        changed[48..52].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(verify(&changed, &trust).err(), Some(VerifyError::Malformed));
        let release_trust = Trust {
            purpose: Purpose::Release,
            ..trust
        };
        assert_eq!(
            verify(&encoded, &release_trust).err(),
            Some(VerifyError::UntrustedContext)
        );
    }
}
