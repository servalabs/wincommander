// SPDX-License-Identifier: AGPL-3.0-or-later
//! Offline Free release rehearsal. Trust must come from a separate source.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use std::{fs::File, io::Read, process::ExitCode};
use wincmd_pq_experiment::{free_release, jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};

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
        .map_err(|e| format!("cannot open {path}: {e}"))?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {path}: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("file too large: {path}"));
    }
    Ok(bytes)
}
fn decode<const N: usize>(value: &str) -> Result<[u8; N], String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| "invalid key encoding")?;
    if URL_SAFE_NO_PAD.encode(&bytes) != value {
        return Err("invalid key encoding".into());
    }
    bytes.try_into().map_err(|_| "invalid key length".into())
}
fn run(args: &[String]) -> Result<(), String> {
    if args.len() != 6 {
        return Err("usage: wincmd-pq-free-release-check <signed-record> <installer> <independent-trust> <version> <stable|beta>".into());
    }
    let envelope = read(&args[1], 32768)?;
    let config: TrustFile = serde_json::from_slice(&read(&args[3], 8192)?)
        .map_err(|_| "invalid trust configuration")?;
    let ed: [u8; 32] = decode(&config.ed_public)?;
    let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = decode(&config.pq_public)?;
    let trust = jws::Trust {
        purpose: Purpose::Release,
        minimum_epoch: config.minimum_epoch,
        ed_key_id: &config.ed_key_id,
        ed25519_public: &ed,
        pq_key_id: &config.pq_key_id,
        ml_dsa_65_public: &pq,
    };
    let file = File::open(&args[2]).map_err(|e| format!("cannot open installer: {e}"))?;
    free_release::verify_offline(&envelope, file, &args[4], &args[5], &trust).map_err(str::to_owned)
}
fn main() -> ExitCode {
    eprintln!("EXPERIMENTAL: independent trust required; no revocation or rollback verification.");
    match run(&std::env::args().collect::<Vec<_>>()) {
        Ok(()) => {
            println!("Free hybrid signatures and installer verified.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("verification failed: {e}");
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
    fn independently_checks_signed_free_file_and_rejects_wrong_context() {
        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let ep = ed.verifying_key().to_bytes();
        let pp = pq.public_key().as_ref().try_into().unwrap();
        let trust = jws::Trust {
            purpose: Purpose::Release,
            minimum_epoch: 7,
            ed_key_id: "ed",
            ed25519_public: &ep,
            pq_key_id: "pq",
            ml_dsa_65_public: &pp,
        };
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/free-tauri-disposable.json"
        ))
        .unwrap();
        let bytes = b"MZ";
        let url = "https://winupdates.servalabs.com/free/v99.0.0/WinCommander_99.0.0_x64-setup.exe";
        let manifest = serde_json::to_vec(&serde_json::json!({"version":"99.0.0", "platforms":{"windows-x86_64":{"url":url,"signature":fixture["signature"]}}})).unwrap();
        let payload = free_release::stage_payload(
            &manifest,
            &bytes[..],
            "99.0.0",
            "stable",
            fixture["publicKey"].as_str().unwrap(),
        )
        .unwrap();
        let messages = free_release::prepare_release_from_reader(
            &payload,
            &bytes[..],
            "99.0.0",
            "stable",
            url,
            7,
            &trust,
        )
        .unwrap();
        let ed_sig = ed.sign(&messages.ed).to_bytes().to_vec();
        let mut pq_sig = vec![0; 3309];
        pq.sign(&messages.pq, &mut pq_sig).unwrap();
        let envelope = free_release::finish_release(
            &payload,
            &bytes[..],
            "99.0.0",
            "stable",
            7,
            &trust,
            (ed_sig, pq_sig),
        )
        .unwrap();
        let config = serde_json::json!({"ed_key_id":"ed", "ed_public":URL_SAFE_NO_PAD.encode(ep), "pq_key_id":"pq", "pq_public":URL_SAFE_NO_PAD.encode(pp), "minimum_epoch":7});
        let dir = std::env::temp_dir().join(format!(
            "free-offline-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        std::fs::write(path("record"), &envelope).unwrap();
        std::fs::write(path("file"), bytes).unwrap();
        std::fs::write(path("trust"), serde_json::to_vec(&config).unwrap()).unwrap();
        let args = vec![
            "check".into(),
            path("record"),
            path("file"),
            path("trust"),
            "99.0.0".into(),
            "stable".into(),
        ];
        assert!(run(&args).is_ok());
        let mut wrong = args.clone();
        wrong[5] = "beta".into();
        assert!(run(&wrong).is_err());
        wrong[5] = "stable".into();
        wrong[4] = "99.0.1".into();
        assert!(run(&wrong).is_err());
        let mut stale = config.clone();
        stale["minimum_epoch"] = 8.into();
        std::fs::write(path("trust"), serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(run(&args).is_err());
        let mut wrong_key = config.clone();
        wrong_key["ed_public"] = URL_SAFE_NO_PAD
            .encode(SigningKey::from_bytes(&[43; 32]).verifying_key().to_bytes())
            .into();
        std::fs::write(path("trust"), serde_json::to_vec(&wrong_key).unwrap()).unwrap();
        assert!(run(&args).is_err());
        std::fs::write(path("trust"), serde_json::to_vec(&config).unwrap()).unwrap();
        std::fs::write(path("file"), b"changed installer").unwrap();
        assert!(run(&args).is_err());
        std::fs::write(path("file"), bytes).unwrap();
        for index in 0..2 {
            let mut bad: serde_json::Value = serde_json::from_slice(&envelope).unwrap();
            bad["signatures"][index]["signature"] = serde_json::json!("bad");
            std::fs::write(path("record"), serde_json::to_vec(&bad).unwrap()).unwrap();
            assert!(run(&args).is_err());
        }
        for name in ["record", "file", "trust"] {
            std::fs::remove_file(path(name)).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
