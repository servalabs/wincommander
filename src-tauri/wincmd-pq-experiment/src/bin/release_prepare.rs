// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental offline two-party signing preparation. No private keys are loaded.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    process::ExitCode,
};
use wincmd_pq_experiment::{jws, release, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};

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
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| format!("cannot open {path}: {e}"))?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {path}: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("file exceeds size limit: {path}"));
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
fn write_new(path: &str, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("cannot create {path}: {e}"))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(format!("cannot write {path}: {error}"));
    }
    Ok(())
}
fn run(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("expected stage, prepare or finish".into());
    }
    if args[1] == "stage" {
        if args.len() != 6 {
            return Err("usage: stage <latest.pro.json> <pro.exe> <version> <payload.json>".into());
        }
        let manifest = read_bounded(&args[2], 32768)?;
        let artifact = File::open(&args[3]).map_err(|e| format!("cannot open artifact: {e}"))?;
        let payload =
            release::stage_payload(&manifest, artifact, &args[4]).map_err(str::to_owned)?;
        return write_new(&args[5], &payload);
    }
    let expected = match args[1].as_str() {
        "prepare" => 9,
        "finish" => 10,
        _ => return Err("expected stage, prepare or finish".into()),
    };
    if args.len() != expected {
        return Err("usage: prepare <payload.json> <pro.exe> <trust.json> <version> <epoch> <ed-message> <pq-message>; finish <payload.json> <pro.exe> <trust.json> <version> <epoch> <ed-signature> <pq-signature> <signed-record>".into());
    }
    let payload = read_bounded(&args[2], 32768)?;
    let config: TrustFile = serde_json::from_slice(&read_bounded(&args[4], 8192)?)
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
    let epoch = args[6].parse::<u64>().map_err(|_| "invalid epoch")?;
    let artifact = File::open(&args[3]).map_err(|e| format!("cannot open artifact: {e}"))?;
    if args[1] == "prepare" {
        let messages = release::prepare_release(&payload, artifact, &args[5], epoch, &trust)
            .map_err(str::to_owned)?;
        write_new(&args[7], &messages.ed)?;
        if let Err(error) = write_new(&args[8], &messages.pq) {
            let _ = std::fs::remove_file(&args[7]);
            return Err(error);
        }
    } else {
        let ed_signature = read_bounded(&args[7], 64)?;
        let pq_signature = read_bounded(&args[8], 3309)?;
        let signed = release::sign_release(
            &payload,
            artifact,
            &args[5],
            epoch,
            &trust,
            |_| Ok(ed_signature),
            |_| Ok(pq_signature),
        )
        .map_err(str::to_owned)?;
        write_new(&args[9], &signed)?;
    }
    Ok(())
}
fn main() -> ExitCode {
    eprintln!("EXPERIMENTAL: no production release trust, key custody, rollback or publishing is configured.");
    match run(&std::env::args().collect::<Vec<_>>()) {
        Ok(()) => {
            println!("Experimental signing step completed.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("signing step failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};

    #[test]
    fn offline_two_key_signing_requires_the_same_valid_artifact() {
        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "wincmd-sign-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        let data = b"disposable release file";
        let payload = serde_json::json!({"format":"wincommander-pro-release-v1", "version":"3.7.0", "url":"https://winupdates.servalabs.com/pro/v3.7.0/wincommander-pro.exe", "size":data.len(), "sha256":hex::encode(Sha256::digest(data))});
        let config = serde_json::json!({"ed_key_id":"ed", "ed_public":URL_SAFE_NO_PAD.encode(ed.verifying_key().to_bytes()), "pq_key_id":"pq", "pq_public":URL_SAFE_NO_PAD.encode(pq.public_key().as_ref()), "minimum_epoch":7});
        let mut staged = payload.clone();
        staged.as_object_mut().unwrap().remove("format");
        staged["pub_date"] = "2026-11-03T00:00:00Z".into();
        staged["notes"] = "WinCommander Pro v3.7.0".into();
        std::fs::write(path("manifest"), serde_json::to_vec(&staged).unwrap()).unwrap();
        std::fs::write(path("binary"), b"wrong file").unwrap();
        std::fs::write(path("trust"), serde_json::to_vec(&config).unwrap()).unwrap();
        let stage = vec![
            "tool".into(),
            "stage".into(),
            path("manifest"),
            path("binary"),
            "3.7.0".into(),
            path("payload"),
        ];
        assert!(run(&stage).is_err());
        assert!(!std::path::Path::new(&path("payload")).exists());
        std::fs::write(path("binary"), data).unwrap();
        let mut wrong_version = stage.clone();
        wrong_version[4] = "3.7.1".into();
        assert!(run(&wrong_version).is_err());
        assert!(run(&stage).is_ok());
        assert!(run(&stage).is_err());
        let prepared_payload: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path("payload")).unwrap()).unwrap();
        assert_eq!(prepared_payload, payload);
        let prepare = vec![
            "tool".into(),
            "prepare".into(),
            path("payload"),
            path("binary"),
            path("trust"),
            "3.7.0".into(),
            "7".into(),
            path("ed-msg"),
            path("pq-msg"),
        ];
        assert!(run(&prepare).is_ok());
        assert!(run(&prepare).is_err()); // output paths must not be overwritten
        let ed_sig = ed.sign(&std::fs::read(path("ed-msg")).unwrap()).to_bytes();
        let mut pq_sig = vec![0; 3309];
        pq.sign(&std::fs::read(path("pq-msg")).unwrap(), &mut pq_sig)
            .unwrap();
        std::fs::write(path("ed-sig"), ed_sig).unwrap();
        std::fs::write(path("pq-sig"), pq_sig).unwrap();
        let finish = vec![
            "tool".into(),
            "finish".into(),
            path("payload"),
            path("binary"),
            path("trust"),
            "3.7.0".into(),
            "7".into(),
            path("ed-sig"),
            path("pq-sig"),
            path("signed"),
        ];
        std::fs::write(path("binary"), b"tampered").unwrap();
        assert!(run(&finish).is_err());
        assert!(!std::path::Path::new(&path("signed")).exists());
        std::fs::write(path("binary"), data).unwrap();
        assert!(run(&finish).is_ok());
        assert!(run(&finish).is_err());
        for name in [
            "manifest", "payload", "binary", "trust", "ed-msg", "pq-msg", "ed-sig", "pq-sig",
            "signed",
        ] {
            std::fs::remove_file(path(name)).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
