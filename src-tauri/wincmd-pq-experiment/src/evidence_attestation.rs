// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental detached re-attestation of exact archived bytes.
//! It does not verify the archive's original provenance or signing time.
use crate::{jws, Purpose};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;

const FORMAT: &str = "wincommander-evidence-reattestation-experimental-v1";
const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    format: String,
    sha256: String,
    size: u64,
}

fn digest<R: Read>(mut file: R) -> Result<Claims, &'static str> {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buffer).map_err(|_| "evidence read failed")?;
        if n == 0 {
            break;
        }
        size = size.checked_add(n as u64).ok_or("evidence too large")?;
        if size > MAX_BYTES {
            return Err("evidence too large");
        }
        hash.update(&buffer[..n]);
    }
    if size == 0 {
        return Err("evidence is empty");
    }
    Ok(Claims {
        format: FORMAT.into(),
        sha256: hex::encode(hash.finalize()),
        size,
    })
}

/// Verify both signatures and the exact streamed bytes using independently
/// supplied trust. The caller must prevent changes during reading. This does
/// not validate the original evidence or its author.
pub fn verify<R: Read>(
    envelope: &[u8],
    file: R,
    trust: &jws::Trust<'_>,
) -> Result<u64, &'static str> {
    if trust.purpose != Purpose::EvidenceAttestation
        || trust.minimum_epoch == 0
        || envelope.len() > 32768
    {
        return Err("invalid attestation policy");
    }
    let verified = jws::verify(envelope, trust).map_err(|_| "invalid attestation signatures")?;
    let claims: Claims =
        serde_json::from_slice(&verified.payload).map_err(|_| "invalid attestation claims")?;
    let actual = digest(file)?;
    if claims.format != FORMAT || claims.size != actual.size || claims.sha256 != actual.sha256 {
        return Err("evidence does not match attestation");
    }
    Ok(verified.epoch)
}

/// Prepare the two exact messages for separately controlled signers. The
/// caller must validate the original case and keep the archive stable while
/// reading it. Signatures must be checked again against the current archive.
#[cfg(feature = "experimental-signer")]
pub fn prepare<R: Read>(
    file: R,
    epoch: u64,
    trust: &jws::Trust<'_>,
) -> Result<jws::sign::SigningInputs, &'static str> {
    if trust.purpose != Purpose::EvidenceAttestation || trust.minimum_epoch == 0 {
        return Err("invalid attestation policy");
    }
    let payload = serde_json::to_vec(&digest(file)?).map_err(|_| "invalid attestation claims")?;
    jws::sign::signing_inputs(&payload, epoch, trust).map_err(|_| "attestation preparation failed")
}

/// Recompute the statement from the current archive and verify both detached
/// signatures before returning an envelope. Neither input signature is trusted.
#[cfg(feature = "experimental-signer")]
pub fn assemble<R: Read>(
    file: R,
    epoch: u64,
    trust: &jws::Trust<'_>,
    ed_signature: &[u8],
    pq_signature: &[u8],
) -> Result<Vec<u8>, &'static str> {
    if ed_signature.len() != 64 || pq_signature.len() != 3309 {
        return Err("invalid attestation signature length");
    }
    sign(
        file,
        epoch,
        trust,
        |_| Ok(ed_signature.to_vec()),
        |_| Ok(pq_signature.to_vec()),
    )
}

/// The caller owns both signing keys and must separately validate the original
/// archive and prevent changes during reading. No historical protection or
/// trusted timestamp is claimed.
#[cfg(feature = "experimental-signer")]
pub fn sign<R, E, P>(
    file: R,
    epoch: u64,
    trust: &jws::Trust<'_>,
    ed: E,
    pq: P,
) -> Result<Vec<u8>, &'static str>
where
    R: Read,
    E: FnOnce(&[u8]) -> Result<Vec<u8>, crate::VerifyError>,
    P: FnOnce(&[u8]) -> Result<Vec<u8>, crate::VerifyError>,
{
    if trust.purpose != Purpose::EvidenceAttestation || trust.minimum_epoch == 0 {
        return Err("invalid attestation policy");
    }
    let claims = digest(file)?;
    let payload = serde_json::to_vec(&claims).map_err(|_| "invalid attestation claims")?;
    jws::sign::assemble(&payload, epoch, trust, ed, pq).map_err(|_| "attestation signing failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn requires_both_signatures_exact_bytes_and_purpose() {
        let ed = SigningKey::from_bytes(&[41; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let ep = ed.verifying_key().to_bytes();
        let pp = pq.public_key().as_ref().try_into().unwrap();
        let trust = jws::Trust {
            purpose: Purpose::EvidenceAttestation,
            minimum_epoch: 3,
            ed_key_id: "evidence-ed",
            ed25519_public: &ep,
            pq_key_id: "evidence-pq",
            ml_dsa_65_public: &pp,
        };
        let original = b"original evidence archive";
        let claims = serde_json::to_vec(&digest(&original[..]).unwrap()).unwrap();
        let encoded = jws::sign::assemble(
            &claims,
            3,
            &trust,
            |m| Ok(ed.sign(m).to_bytes().to_vec()),
            |m| {
                let mut s = vec![0; 3309];
                pq.sign(m, &mut s).unwrap();
                Ok(s)
            },
        )
        .unwrap();
        assert_eq!(verify(&encoded, &original[..], &trust), Ok(3));
        #[cfg(feature = "experimental-signer")]
        {
            let signed = sign(
                &original[..],
                3,
                &trust,
                |m| Ok(ed.sign(m).to_bytes().to_vec()),
                |m| {
                    let mut s = vec![0; 3309];
                    pq.sign(m, &mut s).unwrap();
                    Ok(s)
                },
            )
            .unwrap();
            assert_eq!(verify(&signed, &original[..], &trust), Ok(3));
            let messages = prepare(&original[..], 3, &trust).unwrap();
            let ed_signature = ed.sign(&messages.ed).to_bytes();
            let mut pq_signature = vec![0; 3309];
            pq.sign(&messages.pq, &mut pq_signature).unwrap();
            let detached =
                assemble(&original[..], 3, &trust, &ed_signature, &pq_signature).unwrap();
            assert_eq!(verify(&detached, &original[..], &trust), Ok(3));
            assert!(assemble(
                &b"modified archive"[..],
                3,
                &trust,
                &ed_signature,
                &pq_signature
            )
            .is_err());
            assert!(assemble(&original[..], 4, &trust, &ed_signature, &pq_signature).is_err());
            assert!(
                assemble(&original[..], 3, &trust, &ed_signature[..63], &pq_signature).is_err()
            );

            assert!(sign(
                &b""[..],
                3,
                &trust,
                |_| panic!("empty archive signed"),
                |_| panic!("empty archive signed")
            )
            .is_err());
        }
        for index in 0..2 {
            let mut changed: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
            changed["signatures"][index]["signature"] = serde_json::Value::String("AA".into());
            assert!(verify(
                &serde_json::to_vec(&changed).unwrap(),
                &original[..],
                &trust
            )
            .is_err());
        }
        assert!(verify(&encoded, &b"altered evidence archive"[..], &trust).is_err());
        assert!(verify(&encoded, &b""[..], &trust).is_err());
        let wrong = jws::Trust {
            purpose: Purpose::Release,
            ..trust
        };
        assert!(verify(&encoded, &original[..], &wrong).is_err());
        let mut stripped: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        stripped["signatures"].as_array_mut().unwrap().pop();
        assert!(verify(
            &serde_json::to_vec(&stripped).unwrap(),
            &original[..],
            &trust
        )
        .is_err());
    }
}
