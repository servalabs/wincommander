// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental signing assembly; does not provide key custody or publication.
use super::{verify, Trust, MAX_ENVELOPE_LEN, MAX_HEADER_LEN, PROFILE, TYPE};
use crate::{Purpose, VerifyError, MAX_PAYLOAD_LEN};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::json;

/// Exact JWS signing messages for independently held signing keys.
pub struct SigningInputs {
    pub ed: Vec<u8>,
    pub pq: Vec<u8>,
}

/// Prepare signing inputs without accessing or choosing private keys.
pub fn signing_inputs(
    payload: &[u8],
    epoch: u64,
    trust: &Trust<'_>,
) -> Result<SigningInputs, VerifyError> {
    if payload.len() > MAX_PAYLOAD_LEN
        || epoch == 0
        || epoch < trust.minimum_epoch
        || trust.ed_key_id.is_empty()
        || trust.pq_key_id.is_empty()
        || trust.ed_key_id.len() > 128
        || trust.pq_key_id.len() > 128
        || trust.ed_key_id == trust.pq_key_id
    {
        return Err(VerifyError::Malformed);
    }
    let purpose = match trust.purpose {
        Purpose::Licence => "licence",
        Purpose::Release => "release",
        Purpose::LicenceEnrolment => "licence-enrolment",
        Purpose::EvidenceAttestation => "evidence-attestation",
    };
    let header = |alg: &str, kid: &str| -> Result<String, VerifyError> {
        let bytes = serde_json::to_vec(&json!({"alg": alg, "kid": kid, "typ": TYPE,
            "crit": ["profile", "purpose", "epoch"], "profile": PROFILE, "purpose": purpose, "epoch": epoch
        })).map_err(|_| VerifyError::Malformed)?;
        if bytes.len() > MAX_HEADER_LEN {
            return Err(VerifyError::Malformed);
        }
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    };
    let ed_header = header("Ed25519", trust.ed_key_id)?;
    let pq_header = header("ML-DSA-65", trust.pq_key_id)?;
    let encoded_payload = URL_SAFE_NO_PAD.encode(payload);
    Ok(SigningInputs {
        ed: format!("{ed_header}.{encoded_payload}").into_bytes(),
        pq: format!("{pq_header}.{encoded_payload}").into_bytes(),
    })
}

/// Assemble and independently verify both signatures before returning any bytes.
/// Signer callbacks are responsible for private-key custody and access control.
/// This does not enforce claim semantics, persistent rollback, or key rotation.
pub fn assemble<E, P>(
    payload: &[u8],
    epoch: u64,
    trust: &Trust<'_>,
    sign_ed: E,
    sign_pq: P,
) -> Result<Vec<u8>, VerifyError>
where
    E: FnOnce(&[u8]) -> Result<Vec<u8>, VerifyError>,
    P: FnOnce(&[u8]) -> Result<Vec<u8>, VerifyError>,
{
    let inputs = signing_inputs(payload, epoch, trust)?;
    let ed_input = std::str::from_utf8(&inputs.ed).map_err(|_| VerifyError::Malformed)?;
    let pq_input = std::str::from_utf8(&inputs.pq).map_err(|_| VerifyError::Malformed)?;
    let (ed_header, encoded_payload) = ed_input.split_once('.').ok_or(VerifyError::Malformed)?;
    let (pq_header, _) = pq_input.split_once('.').ok_or(VerifyError::Malformed)?;
    let ed_signature = sign_ed(&inputs.ed)?;
    if ed_signature.len() != 64 {
        return Err(VerifyError::InvalidSignature);
    }
    let pq_signature = sign_pq(&inputs.pq)?;
    if pq_signature.len() != 3309 {
        return Err(VerifyError::InvalidSignature);
    }
    let envelope = serde_json::to_vec(&json!({"payload": encoded_payload, "signatures": [
        {"protected": ed_header, "signature": URL_SAFE_NO_PAD.encode(ed_signature)},
        {"protected": pq_header, "signature": URL_SAFE_NO_PAD.encode(pq_signature)}
    ]}))
    .map_err(|_| VerifyError::Malformed)?;
    if envelope.len() > MAX_ENVELOPE_LEN {
        return Err(VerifyError::Malformed);
    }
    let verified = verify(&envelope, trust)?;
    if verified.payload != payload || verified.epoch != epoch {
        return Err(VerifyError::UntrustedContext);
    }
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ML_DSA_65_PUBLIC_KEY_LEN;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn signs_both_and_refuses_untrusted_or_invalid_output() {
        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let ed_public = ed.verifying_key().to_bytes();
        let pq_public: [u8; ML_DSA_65_PUBLIC_KEY_LEN] =
            pq.public_key().as_ref().try_into().unwrap();
        let trust = Trust {
            purpose: Purpose::Release,
            minimum_epoch: 7,
            ed_key_id: "ed-key",
            ed25519_public: &ed_public,
            pq_key_id: "pq-key",
            ml_dsa_65_public: &pq_public,
        };
        let sign_ed = |message: &[u8]| Ok(ed.sign(message).to_bytes().to_vec());
        let sign_pq = |message: &[u8]| {
            let mut signature = vec![0; 3309];
            pq.sign(message, &mut signature)
                .map_err(|_| VerifyError::InvalidSignature)?;
            Ok(signature)
        };
        let payload = br#"{"format":"wincommander-pro-release-v1"}"#;
        let encoded = assemble(payload, 7, &trust, sign_ed, sign_pq).unwrap();
        assert_eq!(verify(&encoded, &trust).unwrap().payload, payload);
        let wrong = SigningKey::from_bytes(&[43; 32]);
        assert_eq!(
            assemble(
                payload,
                7,
                &trust,
                |msg| Ok(wrong.sign(msg).to_bytes().to_vec()),
                sign_pq
            )
            .err(),
            Some(VerifyError::InvalidSignature)
        );
        assert_eq!(
            assemble(payload, 7, &trust, sign_ed, |_| Ok(vec![0; 3309])).err(),
            Some(VerifyError::InvalidSignature)
        );
        assert_eq!(
            assemble(payload, 6, &trust, sign_ed, sign_pq).err(),
            Some(VerifyError::Malformed)
        );
        assert_eq!(
            assemble(&vec![0; MAX_PAYLOAD_LEN + 1], 7, &trust, sign_ed, sign_pq).err(),
            Some(VerifyError::Malformed)
        );
    }
}
