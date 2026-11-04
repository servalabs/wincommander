// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental detached re-attestation verification; no original provenance check.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use std::{fs::File, io::Read, process::ExitCode};
use wincmd_pq_experiment::{evidence_attestation, jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustFile {
    ed_key_id: String,
    ed_public: String,
    pq_key_id: String,
    pq_public: String,
    minimum_epoch: u64,
}
fn read(path: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open input")?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read input")?;
    if bytes.len() as u64 > limit {
        return Err("input too large".into());
    }
    Ok(bytes)
}
fn decode<const N: usize>(value: &str) -> Result<[u8; N], String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| "invalid public key")?;
    if URL_SAFE_NO_PAD.encode(&bytes) != value {
        return Err("invalid public key".into());
    }
    bytes
        .try_into()
        .map_err(|_| "invalid public key length".into())
}
fn run(args: &[String]) -> Result<u64, String> {
    if args.len() != 4 {
        return Err(
            "usage: wincmd-pq-evidence-check <attestation> <archive> <independent-trust>".into(),
        );
    }
    let envelope = read(&args[1], 32768)?;
    let config: TrustFile = serde_json::from_slice(&read(&args[3], 8192)?)
        .map_err(|_| "invalid trust configuration")?;
    let ed: [u8; 32] = decode(&config.ed_public)?;
    let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = decode(&config.pq_public)?;
    let trust = jws::Trust {
        purpose: Purpose::EvidenceAttestation,
        minimum_epoch: config.minimum_epoch,
        ed_key_id: &config.ed_key_id,
        ed25519_public: &ed,
        pq_key_id: &config.pq_key_id,
        ml_dsa_65_public: &pq,
    };
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0x0000_0001); // FILE_SHARE_READ
    }
    let file = options.open(&args[2]).map_err(|_| "cannot open archive")?;
    evidence_attestation::verify(&envelope, file, &trust).map_err(str::to_owned)
}
fn main() -> ExitCode {
    eprintln!("EXPERIMENTAL: this checks a new detached attestation only, not original evidence provenance, creation time, revocation or historical quantum protection.");
    match run(&std::env::args().collect::<Vec<_>>()) {
        Ok(epoch) => {
            println!("Detached Ed25519 and ML-DSA-65 signatures and exact archive bytes verified (epoch {epoch}).");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("verification failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(all(test, feature = "experimental-signer"))]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn offline_checker_requires_independent_keys_and_exact_bytes() {
        let ed = SigningKey::from_bytes(&[41; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let ep = ed.verifying_key().to_bytes();
        let pp = pq.public_key().as_ref().try_into().unwrap();
        let trust = jws::Trust {
            purpose: Purpose::EvidenceAttestation,
            minimum_epoch: 3,
            ed_key_id: "ed",
            ed25519_public: &ep,
            pq_key_id: "pq",
            ml_dsa_65_public: &pp,
        };
        let archive = b"disposable archive bytes";
        let signed = evidence_attestation::sign(
            &archive[..],
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
        let dir = std::env::temp_dir().join(format!(
            "wincmd-attestation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let attestation = dir.join("attestation.json");
        let data = dir.join("archive.zip");
        let config = dir.join("trust.json");
        std::fs::write(&attestation, signed).unwrap();
        std::fs::write(&data, archive).unwrap();
        std::fs::write(
            &config,
            serde_json::to_vec(&serde_json::json!({
                "ed_key_id":"ed", "ed_public":URL_SAFE_NO_PAD.encode(ep), "pq_key_id":"pq",
                "pq_public":URL_SAFE_NO_PAD.encode(pp), "minimum_epoch":3
            }))
            .unwrap(),
        )
        .unwrap();
        let args = vec![
            "check".into(),
            attestation.to_string_lossy().into_owned(),
            data.to_string_lossy().into_owned(),
            config.to_string_lossy().into_owned(),
        ];
        assert_eq!(run(&args), Ok(3));
        std::fs::write(&data, b"modified archive bytes").unwrap();
        assert!(run(&args).is_err());
        std::fs::remove_file(attestation).unwrap();
        std::fs::remove_file(data).unwrap();
        std::fs::remove_file(config).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
