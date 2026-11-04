// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental offline release verification; no embedded production trust.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use std::{fs::File, io::Read, process::ExitCode};
use wincmd_pq_experiment::{jws, offline_release, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustFile {
    ed_key_id: String,
    ed_public: String,
    pq_key_id: String,
    pq_public: String,
    minimum_epoch: u64,
}

fn read_bounded(path: &str, limit: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {path}: {e}"))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {path}: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("file exceeds size limit: {path}"));
    }
    Ok(bytes)
}
fn decode_key<const N: usize>(text: &str) -> Result<[u8; N], String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| "invalid public key encoding")?;
    if URL_SAFE_NO_PAD.encode(&bytes) != text {
        return Err("invalid public key encoding".into());
    }
    bytes
        .try_into()
        .map_err(|_| "invalid public key length".into())
}
fn run(args: &[String]) -> Result<(), String> {
    if args.len() != 5 {
        return Err("usage: wincmd-pq-release-check <signed-record.json> <pro.exe> <independently-trusted-keys.json> <expected-version>".into());
    }
    let envelope = read_bounded(&args[1], 32768)?;
    let config: TrustFile = serde_json::from_slice(&read_bounded(&args[3], 8192)?)
        .map_err(|_| "invalid trust configuration")?;
    let ed: [u8; 32] = decode_key(&config.ed_public)?;
    let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = decode_key(&config.pq_public)?;
    let trust = jws::Trust {
        purpose: Purpose::Release,
        minimum_epoch: config.minimum_epoch,
        ed_key_id: &config.ed_key_id,
        ed25519_public: &ed,
        pq_key_id: &config.pq_key_id,
        ml_dsa_65_public: &pq,
    };
    let artifact = File::open(&args[2]).map_err(|e| format!("cannot open release file: {e}"))?;
    offline_release::verify_artifact(&envelope, artifact, &args[4], &trust).map_err(str::to_owned)
}
fn main() -> ExitCode {
    eprintln!("EXPERIMENTAL: use independently trusted public keys; this is not production release verification.");
    match run(&std::env::args().collect::<Vec<_>>()) {
        Ok(()) => {
            println!("Experimental hybrid signatures, version, file size and hash verified.");
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
    use sha2::{Digest, Sha256};
    use wincmd_pq_experiment::{jws::sign::assemble, VerifyError};

    #[test]
    fn command_checks_signed_bytes_with_independent_trust() {
        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let ed_public = ed.verifying_key().to_bytes();
        let pq_public: [u8; ML_DSA_65_PUBLIC_KEY_LEN] =
            pq.public_key().as_ref().try_into().unwrap();
        let trust = jws::Trust {
            purpose: Purpose::Release,
            minimum_epoch: 7,
            ed_key_id: "ed",
            ed25519_public: &ed_public,
            pq_key_id: "pq",
            ml_dsa_65_public: &pq_public,
        };
        let artifact = b"offline release fixture";
        let payload = serde_json::to_vec(&serde_json::json!({"format":"wincommander-pro-release-v1", "version":"3.7.0", "url":"https://winupdates.servalabs.com/pro/v3.7.0/wincommander-pro.exe", "size":artifact.len(), "sha256":hex::encode(Sha256::digest(artifact))})).unwrap();
        let signed = assemble(
            &payload,
            7,
            &trust,
            |m| Ok(ed.sign(m).to_bytes().to_vec()),
            |m| {
                let mut s = vec![0; 3309];
                pq.sign(m, &mut s)
                    .map_err(|_| VerifyError::InvalidSignature)?;
                Ok(s)
            },
        )
        .unwrap();
        let config = serde_json::to_vec(&serde_json::json!({"ed_key_id":"ed", "ed_public": URL_SAFE_NO_PAD.encode(ed_public), "pq_key_id":"pq", "pq_public":URL_SAFE_NO_PAD.encode(pq_public), "minimum_epoch":7})).unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "wincmd-pq-release-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let record = dir.join("record.json");
        let file = dir.join("pro.exe");
        let keys = dir.join("trusted.json");
        std::fs::write(&record, &signed).unwrap();
        std::fs::write(&file, artifact).unwrap();
        std::fs::write(&keys, &config).unwrap();
        let args = vec![
            "check".into(),
            record.to_string_lossy().into_owned(),
            file.to_string_lossy().into_owned(),
            keys.to_string_lossy().into_owned(),
            "3.7.0".into(),
        ];
        assert!(run(&args).is_ok());
        let mut wrong_version = args.clone();
        wrong_version[4] = "3.7.1".into();
        assert!(run(&wrong_version).is_err());
        let mut wrong_trust: serde_json::Value = serde_json::from_slice(&config).unwrap();
        wrong_trust["minimum_epoch"] = 8.into();
        std::fs::write(&keys, serde_json::to_vec(&wrong_trust).unwrap()).unwrap();
        assert!(run(&args).is_err());
        std::fs::write(&keys, &config).unwrap();
        std::fs::write(&file, b"altered release").unwrap();
        assert!(run(&args).is_err());
        std::fs::remove_file(record).unwrap();
        std::fs::remove_file(file).unwrap();
        std::fs::remove_file(keys).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
