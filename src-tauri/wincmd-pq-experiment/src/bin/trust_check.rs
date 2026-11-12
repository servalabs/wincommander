// SPDX-License-Identifier: AGPL-3.0-or-later
//! Verify independent public-key possession proofs before a release build.
use aws_lc_rs::signature::{UnparsedPublicKey, ML_DSA_65};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use std::process::ExitCode;
use wincmd_pq_experiment::ML_DSA_65_PUBLIC_KEY_LEN;

fn decode<const N: usize>(value: &str) -> Result<[u8; N], &'static str> {
    let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| "invalid encoding")?;
    if URL_SAFE_NO_PAD.encode(&bytes) != value { return Err("invalid encoding"); }
    bytes.try_into().map_err(|_| "invalid length")
}

fn check(purpose: &str, values: [&str; 7]) -> Result<(), &'static str> {
    let [ed_text, pq_text, ed_id, pq_id, epoch, ed_proof, pq_proof] = values;
    let ed = decode::<32>(ed_text)?;
    let pq = decode::<ML_DSA_65_PUBLIC_KEY_LEN>(pq_text)?;
    let ed_signature = decode::<64>(ed_proof)?;
    let pq_signature = decode::<3309>(pq_proof)?;
    let key = VerifyingKey::from_bytes(&ed).map_err(|_| "invalid Ed25519 key")?;
    if key.is_weak() { return Err("weak Ed25519 key"); }
    let message = format!("WinCommander release trust proof v1\npurpose={purpose}\ned_id={ed_id}\npq_id={pq_id}\nepoch={epoch}\ned_key={ed_text}\npq_key={pq_text}\n");
    key.verify_strict(message.as_bytes(), &Signature::from_bytes(&ed_signature))
        .map_err(|_| "invalid Ed25519 proof")?;
    UnparsedPublicKey::new(&ML_DSA_65, pq).verify(message.as_bytes(), &pq_signature)
        .map_err(|_| "invalid ML-DSA proof")?;
    Ok(())
}

fn run() -> Result<(), String> {
    for purpose in ["LIC", "FREE", "PRO"] {
        let prefix = format!("WINCMD_PQ_{purpose}_");
        let names = ["ED_KEY", "MLDSA_KEY", "ED_ID", "MLDSA_ID", "EPOCH", "ED_PROOF", "MLDSA_PROOF"];
        let values: Vec<String> = names.iter().map(|name| std::env::var(format!("{prefix}{name}"))
            .map_err(|_| format!("missing {purpose} trust proof"))).collect::<Result<_, _>>()?;
        check(purpose, [&values[0], &values[1], &values[2], &values[3], &values[4], &values[5], &values[6]])
            .map_err(|error| format!("{purpose} trust proof failed: {error}"))?;
        let old = format!("{prefix}OLD_ED_KEY");
        if let Ok(value) = std::env::var(old) {
            let bytes = decode::<32>(&value).map_err(|_| format!("invalid historical {purpose} key"))?;
            let key = VerifyingKey::from_bytes(&bytes).map_err(|_| format!("invalid historical {purpose} key"))?;
            if key.is_weak() { return Err(format!("weak historical {purpose} key")); }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => { println!("Release trust proofs verified."); ExitCode::SUCCESS }
        Err(error) => { eprintln!("Release trust verification failed: {error}"); ExitCode::FAILURE }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn requires_both_real_keys_and_exact_purpose_ids_and_epoch() {
        let ed = SigningKey::from_bytes(&[71; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let ed_public = URL_SAFE_NO_PAD.encode(ed.verifying_key().to_bytes());
        let pq_public = URL_SAFE_NO_PAD.encode(pq.public_key().as_ref());
        let message = format!("WinCommander release trust proof v1\npurpose=LIC\ned_id=ed\npq_id=pq\nepoch=7\ned_key={ed_public}\npq_key={pq_public}\n");
        let ed_signature = URL_SAFE_NO_PAD.encode(ed.sign(message.as_bytes()).to_bytes());
        let mut pq_signature = vec![0; 3309];
        pq.sign(message.as_bytes(), &mut pq_signature).unwrap();
        let pq_signature = URL_SAFE_NO_PAD.encode(pq_signature);
        let values = [&*ed_public, &*pq_public, "ed", "pq", "7", &*ed_signature, &*pq_signature];
        assert!(check("LIC", values).is_ok());
        assert!(check("FREE", values).is_err());
        assert!(check("LIC", [&*ed_public, &*pq_public, "ed", "pq", "8", &*ed_signature, &*pq_signature]).is_err());
        assert!(check("LIC", [&*ed_public, &*pq_public, "other", "pq", "7", &*ed_signature, &*pq_signature]).is_err());
        assert!(check("LIC", [&*ed_public, &*pq_public, "ed", "pq", "7", &*ed_signature, &URL_SAFE_NO_PAD.encode([0u8; 3309])]).is_err());
    }
}
