// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental restricted JWS general serialization; not a product format.
//!
//! RFC 7515 permits accepting one signature. This profile instead requires
//! both independently trusted signatures and rejects unsupported headers.
//! Callers must enforce persistent rollback, key lifecycle and claim semantics.

#[cfg(any(test, feature = "experimental-signer"))]
pub mod sign;

use aws_lc_rs::signature::{UnparsedPublicKey, ML_DSA_65};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;

use crate::{Purpose, VerifyError, MAX_PAYLOAD_LEN, ML_DSA_65_PUBLIC_KEY_LEN};

const MAX_HEADER_LEN: usize = 1024;
const MAX_ENVELOPE_LEN: usize = MAX_PAYLOAD_LEN.div_ceil(3) * 4 + 16384;
const TYPE: &str = "application/wincmd-experimental+jws";
const PROFILE: &str = "experimental-ed25519-mldsa65-v1";

/// All trust settings come from local policy, not the JWS being verified.
pub struct Trust<'a> {
    pub purpose: Purpose,
    pub minimum_epoch: u64,
    pub ed_key_id: &'a str,
    pub ed25519_public: &'a [u8; 32],
    pub pq_key_id: &'a str,
    pub ml_dsa_65_public: &'a [u8; ML_DSA_65_PUBLIC_KEY_LEN],
}

pub struct Verified {
    pub payload: Vec<u8>,
    pub epoch: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: String,
    signatures: Vec<SignatureEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignatureEntry {
    protected: String,
    signature: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Protected {
    alg: String,
    kid: String,
    typ: String,
    crit: Vec<String>,
    profile: String,
    purpose: String,
    epoch: u64,
}

fn decode_canonical(value: &str, limit: usize) -> Result<Vec<u8>, VerifyError> {
    if value.len() > limit.div_ceil(3) * 4 {
        return Err(VerifyError::Malformed);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| VerifyError::Malformed)?;
    if bytes.len() > limit || URL_SAFE_NO_PAD.encode(&bytes) != value {
        return Err(VerifyError::Malformed);
    }
    Ok(bytes)
}

/// Verify an experimental two-signature JWS with an independently supplied trust policy.
pub fn verify(input: &[u8], trust: &Trust<'_>) -> Result<Verified, VerifyError> {
    if input.len() > MAX_ENVELOPE_LEN
        || trust.ed_key_id.is_empty()
        || trust.pq_key_id.is_empty()
        || trust.ed_key_id.len() > 128
        || trust.pq_key_id.len() > 128
        || trust.ed_key_id == trust.pq_key_id
    {
        return Err(VerifyError::Malformed);
    }
    let envelope: Envelope = serde_json::from_slice(input).map_err(|_| VerifyError::Malformed)?;
    if envelope.signatures.len() != 2 {
        return Err(VerifyError::Malformed);
    }
    let payload = decode_canonical(&envelope.payload, MAX_PAYLOAD_LEN)?;
    let mut found_ed = false;
    let mut found_pq = false;
    let mut common_epoch = None;
    for entry in &envelope.signatures {
        let header_bytes = decode_canonical(&entry.protected, MAX_HEADER_LEN)?;
        let header: Protected =
            serde_json::from_slice(&header_bytes).map_err(|_| VerifyError::Malformed)?;
        let purpose = match trust.purpose {
            Purpose::Licence => "licence",
            Purpose::Release => "release",
            Purpose::LicenceEnrolment => "licence-enrolment",
        };
        if header.typ != TYPE
            || header.crit != ["profile", "purpose", "epoch"]
            || header.profile != PROFILE
            || header.purpose != purpose
            || header.epoch < trust.minimum_epoch
            || common_epoch.is_some_and(|epoch| epoch != header.epoch)
        {
            return Err(VerifyError::UntrustedContext);
        }
        common_epoch = Some(header.epoch);
        let input = format!("{}.{}", entry.protected, envelope.payload);
        match header.alg.as_str() {
            "Ed25519" if !found_ed && header.kid == trust.ed_key_id => {
                let signature = decode_canonical(&entry.signature, 64)?;
                let signature =
                    Signature::from_slice(&signature).map_err(|_| VerifyError::InvalidSignature)?;
                VerifyingKey::from_bytes(trust.ed25519_public)
                    .map_err(|_| VerifyError::InvalidSignature)?
                    .verify_strict(input.as_bytes(), &signature)
                    .map_err(|_| VerifyError::InvalidSignature)?;
                found_ed = true;
            }
            "ML-DSA-65" if !found_pq && header.kid == trust.pq_key_id => {
                let signature = decode_canonical(&entry.signature, 3309)?;
                UnparsedPublicKey::new(&ML_DSA_65, trust.ml_dsa_65_public)
                    .verify(input.as_bytes(), &signature)
                    .map_err(|_| VerifyError::InvalidSignature)?;
                found_pq = true;
            }
            _ => return Err(VerifyError::UntrustedContext),
        }
    }
    if !found_ed || !found_pq {
        return Err(VerifyError::UntrustedContext);
    }
    Ok(Verified {
        payload,
        epoch: common_epoch.ok_or(VerifyError::Malformed)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::{json, Value};

    fn sign_fixture(ed: &SigningKey, pq: &PqdsaKeyPair) -> Value {
        let payload = URL_SAFE_NO_PAD.encode(b"disposable licence fixture");
        let header = |alg: &str, kid: &str| {
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&json!({
                    "alg": alg, "kid": kid, "typ": TYPE, "crit": ["profile", "purpose", "epoch"], "profile": PROFILE,
                    "purpose": "licence", "epoch": 7
                }))
                .unwrap(),
            )
        };
        let ed_header = header("Ed25519", "ed-key");
        let pq_header = header("ML-DSA-65", "pq-key");
        let ed_sig = ed.sign(format!("{ed_header}.{payload}").as_bytes());
        let mut pq_sig = vec![0; 3309];
        assert_eq!(
            pq.sign(format!("{pq_header}.{payload}").as_bytes(), &mut pq_sig)
                .unwrap(),
            3309
        );
        json!({"payload": payload, "signatures": [
            {"protected": ed_header, "signature": URL_SAFE_NO_PAD.encode(ed_sig.to_bytes())},
            {"protected": pq_header, "signature": URL_SAFE_NO_PAD.encode(pq_sig)}
        ]})
    }

    #[test]
    fn requires_both_authorized_signatures_and_matching_policy() {
        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let pq_public: [u8; ML_DSA_65_PUBLIC_KEY_LEN] =
            pq.public_key().as_ref().try_into().unwrap();
        let ed_public = ed.verifying_key().to_bytes();
        let trust = Trust {
            purpose: Purpose::Licence,
            minimum_epoch: 7,
            ed_key_id: "ed-key",
            ed25519_public: &ed_public,
            pq_key_id: "pq-key",
            ml_dsa_65_public: &pq_public,
        };
        let fixture = sign_fixture(&ed, &pq);
        let encoded = serde_json::to_vec(&fixture).unwrap();
        let verified = verify(&encoded, &trust).unwrap();
        assert_eq!(verified.payload, b"disposable licence fixture");
        assert_eq!(verified.epoch, 7);
        let mut reordered = fixture.clone();
        reordered["signatures"].as_array_mut().unwrap().reverse();
        assert!(verify(&serde_json::to_vec(&reordered).unwrap(), &trust).is_ok());

        for index in 0..2 {
            let mut changed = fixture.clone();
            let signature = changed["signatures"][index]["signature"].as_str().unwrap();
            let mut bytes = URL_SAFE_NO_PAD.decode(signature).unwrap();
            bytes[0] ^= 1;
            changed["signatures"][index]["signature"] = URL_SAFE_NO_PAD.encode(bytes).into();
            assert_eq!(
                verify(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
                Some(VerifyError::InvalidSignature)
            );
        }
        let mut changed = fixture.clone();
        changed["payload"] = URL_SAFE_NO_PAD.encode(b"different licence").into();
        assert_eq!(
            verify(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
            Some(VerifyError::InvalidSignature)
        );
        let mut changed = fixture.clone();
        changed["signatures"].as_array_mut().unwrap().pop();
        assert_eq!(
            verify(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
            Some(VerifyError::Malformed)
        );
        let mut changed = fixture.clone();
        changed["signatures"][1] = changed["signatures"][0].clone();
        assert_eq!(
            verify(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
            Some(VerifyError::UntrustedContext)
        );
        let mut changed = fixture.clone();
        changed["signatures"][0]["header"] = json!({"alg":"none"});
        assert_eq!(
            verify(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
            Some(VerifyError::Malformed)
        );
        let mut changed = fixture.clone();
        let header = changed["signatures"][0]["protected"].as_str().unwrap();
        let mut header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
        header["crit"] = json!(["profile", "purpose"]);
        changed["signatures"][0]["protected"] = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&header).unwrap())
            .into();
        assert_eq!(
            verify(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
            Some(VerifyError::UntrustedContext)
        );
        let mut changed = fixture.clone();
        let header = changed["signatures"][0]["protected"].as_str().unwrap();
        let header = String::from_utf8(URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
        let duplicate = header.replacen("{", "{\"alg\":\"Ed25519\",", 1);
        changed["signatures"][0]["protected"] = URL_SAFE_NO_PAD.encode(duplicate).into();
        assert_eq!(
            verify(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
            Some(VerifyError::Malformed)
        );
        let mut changed = fixture.clone();
        let header = changed["signatures"][1]["protected"].as_str().unwrap();
        let mut header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
        header["epoch"] = 8.into();
        changed["signatures"][1]["protected"] = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&header).unwrap())
            .into();
        assert_eq!(
            verify(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
            Some(VerifyError::UntrustedContext)
        );
        let mut changed = fixture.clone();
        changed["payload"] = format!("{}=", fixture["payload"].as_str().unwrap()).into();
        assert_eq!(
            verify(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
            Some(VerifyError::Malformed)
        );
        let wrong = Trust {
            purpose: Purpose::Release,
            ..trust
        };
        assert_eq!(
            verify(&encoded, &wrong).err(),
            Some(VerifyError::UntrustedContext)
        );
        let stale = Trust {
            purpose: Purpose::Licence,
            minimum_epoch: 8,
            ..wrong
        };
        assert_eq!(
            verify(&encoded, &stale).err(),
            Some(VerifyError::UntrustedContext)
        );
        assert_eq!(
            verify(&vec![b' '; MAX_ENVELOPE_LEN + 1], &stale).err(),
            Some(VerifyError::Malformed)
        );
    }
}
