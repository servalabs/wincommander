// SPDX-License-Identifier: AGPL-3.0-or-later
//! Offline Free signing rehearsal. Private keys are never loaded by this tool.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    process::ExitCode,
};
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
fn write_new(path: &str, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("cannot create {path}: {e}"))?;
    if let Err(e) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(format!("cannot write {path}: {e}"));
    }
    Ok(())
}
fn run(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("expected stage, prepare or finish".into());
    }
    if args[1] == "stage" {
        if args.len() != 7 {
            return Err(
                "usage: stage <latest.json> <installer> <version> <channel> <payload>".into(),
            );
        }
        let manifest = read(&args[2], 32768)?;
        let file = File::open(&args[3]).map_err(|e| format!("cannot open installer: {e}"))?;
        let payload = free_release::stage_payload(&manifest, file, &args[4], &args[5])
            .map_err(str::to_owned)?;
        return write_new(&args[6], &payload);
    }
    let count = match args[1].as_str() {
        "prepare" => 10,
        "finish" => 11,
        _ => return Err("expected stage, prepare or finish".into()),
    };
    if args.len() != count {
        return Err("usage: prepare <payload> <installer> <trust> <version> <channel> <epoch> <ed-message> <pq-message>; finish <payload> <installer> <trust> <version> <channel> <epoch> <ed-signature> <pq-signature> <signed-record>".into());
    }
    let payload = read(&args[2], 32768)?;
    let config: TrustFile = serde_json::from_slice(&read(&args[4], 8192)?)
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
    let epoch: u64 = args[7].parse().map_err(|_| "invalid epoch")?;
    let url = format!(
        "https://winupdates.servalabs.com/free/v{}/WinCommander_{}_x64-setup.exe",
        args[5], args[5]
    );
    let file = File::open(&args[3]).map_err(|e| format!("cannot open installer: {e}"))?;
    if args[1] == "prepare" {
        let inputs = free_release::prepare_release_from_reader(
            &payload, file, &args[5], &args[6], &url, epoch, &trust,
        )
        .map_err(str::to_owned)?;
        write_new(&args[8], &inputs.ed)?;
        if let Err(e) = write_new(&args[9], &inputs.pq) {
            let _ = std::fs::remove_file(&args[8]);
            return Err(e);
        }
    } else {
        let ed_signature = read(&args[8], 64)?;
        let pq_signature = read(&args[9], 3309)?;
        let record = free_release::finish_release(
            &payload,
            file,
            &args[5],
            &args[6],
            epoch,
            &trust,
            (ed_signature, pq_signature),
        )
        .map_err(str::to_owned)?;
        write_new(&args[10], &record)?;
    }
    Ok(())
}
fn main() -> ExitCode {
    eprintln!(
        "EXPERIMENTAL: no production signing keys, publishing or rollback protection configured."
    );
    match run(&std::env::args().collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Free signing step failed: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn staged_file_is_signed_by_two_keys_and_rechecked_at_every_step() {
        let dir = std::env::temp_dir().join(format!(
            "wincmd-free-sign-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = |s: &str| dir.join(s).to_string_lossy().into_owned();
        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let url = "https://winupdates.servalabs.com/free/v3.7.0/WinCommander_3.7.0_x64-setup.exe";
        let manifest = serde_json::json!({"version":"3.7.0", "notes":"test", "platforms":{"windows-x86_64":{"url":url,"signature":"placeholder"}}});
        let config = serde_json::json!({"ed_key_id":"ed", "ed_public":URL_SAFE_NO_PAD.encode(ed.verifying_key().to_bytes()),
            "pq_key_id":"pq", "pq_public":URL_SAFE_NO_PAD.encode(pq.public_key().as_ref()), "minimum_epoch":7});
        std::fs::write(path("manifest"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        std::fs::write(path("installer"), b"disposable installer").unwrap();
        std::fs::write(path("trust"), serde_json::to_vec(&config).unwrap()).unwrap();
        let stage = vec![
            "tool".into(),
            "stage".into(),
            path("manifest"),
            path("installer"),
            "3.7.0".into(),
            "stable".into(),
            path("payload"),
        ];
        let mut wrong = stage.clone();
        wrong[4] = "3.7.1".into();
        assert!(run(&wrong).is_err());
        assert!(run(&stage).is_ok());
        assert!(run(&stage).is_err());
        let prepare = vec![
            "tool".into(),
            "prepare".into(),
            path("payload"),
            path("installer"),
            path("trust"),
            "3.7.0".into(),
            "stable".into(),
            "7".into(),
            path("ed-msg"),
            path("pq-msg"),
        ];
        let mut wrong_channel = prepare.clone();
        wrong_channel[6] = "beta".into();
        assert!(run(&wrong_channel).is_err());
        std::fs::write(path("installer"), b"changed installer").unwrap();
        assert!(run(&prepare).is_err());
        std::fs::write(path("installer"), b"disposable installer").unwrap();
        assert!(run(&prepare).is_ok());
        let ed_sig = ed.sign(&std::fs::read(path("ed-msg")).unwrap()).to_bytes();
        let mut pq_sig = vec![0; 3309];
        pq.sign(&std::fs::read(path("pq-msg")).unwrap(), &mut pq_sig)
            .unwrap();
        std::fs::write(path("ed-sig"), ed_sig).unwrap();
        std::fs::write(path("pq-sig"), &pq_sig).unwrap();
        let finish = vec![
            "tool".into(),
            "finish".into(),
            path("payload"),
            path("installer"),
            path("trust"),
            "3.7.0".into(),
            "stable".into(),
            "7".into(),
            path("ed-sig"),
            path("pq-sig"),
            path("signed"),
        ];
        std::fs::write(path("installer"), b"changed installer").unwrap();
        assert!(run(&finish).is_err());
        std::fs::write(path("installer"), b"disposable installer").unwrap();
        pq_sig[0] ^= 1;
        std::fs::write(path("pq-sig"), &pq_sig).unwrap();
        assert!(run(&finish).is_err());
        pq_sig[0] ^= 1;
        std::fs::write(path("pq-sig"), &pq_sig).unwrap();
        let mut bad_ed = ed_sig;
        bad_ed[0] ^= 1;
        std::fs::write(path("ed-sig"), bad_ed).unwrap();
        assert!(run(&finish).is_err());
        std::fs::write(path("ed-sig"), ed_sig).unwrap();
        assert!(run(&finish).is_ok());
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
        let signed = std::fs::read(path("signed")).unwrap();
        let (release, _) =
            free_release::verify_metadata(&signed, "3.7.0", "stable", url, &trust).unwrap();
        assert!(free_release::verify_artifact(&release, b"disposable installer").is_ok());
        assert!(free_release::verify_metadata(&signed, "3.7.0", "beta", url, &trust).is_err());
        for name in [
            "manifest",
            "installer",
            "trust",
            "payload",
            "ed-msg",
            "pq-msg",
            "ed-sig",
            "pq-sig",
            "signed",
        ] {
            std::fs::remove_file(path(name)).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
