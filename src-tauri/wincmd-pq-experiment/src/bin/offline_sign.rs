// SPDX-License-Identifier: AGPL-3.0-or-later
//! Offline adapter for the exact messages produced by the release preparation tools.
//! Key custody, artifact approval, and publication remain operator responsibilities.
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use serde::Deserialize;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    process::ExitCode,
};
use wincmd_pq_experiment::{free_release, jws, release, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    kid: String,
    typ: String,
    crit: Vec<String>,
    profile: String,
    purpose: String,
    epoch: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustFile {
    ed_key_id: String,
    ed_public: String,
    pq_key_id: String,
    pq_public: String,
    minimum_epoch: u64,
}

fn read(path: &str, limit: u64) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    let mut result = Zeroizing::new(Vec::new());
    File::open(path)
        .map_err(|_| "input unavailable")?
        .take(limit + 1)
        .read_to_end(&mut result)
        .map_err(|_| "input unavailable")?;
    if result.len() as u64 > limit {
        return Err("input too large");
    }
    Ok(result)
}

fn read_key(path: &str) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if path != "-" { return read(path, 16 * 1024); }
    let mut result = Zeroizing::new(Vec::new());
    std::io::stdin().lock().take(16 * 1024 + 1).read_to_end(&mut result)
        .map_err(|_| "key input unavailable")?;
    if result.len() > 16 * 1024 { return Err("key input too large"); }
    Ok(result)
}

fn check_message(bytes: &[u8], algorithm: &str) -> Result<(Header, Vec<u8>), &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "invalid message")?;
    let (header, payload) = text.split_once('.').ok_or("invalid message")?;
    let decode = |s: &str| -> Result<Vec<u8>, &'static str> {
        let value = URL_SAFE_NO_PAD.decode(s).map_err(|_| "invalid message")?;
        if URL_SAFE_NO_PAD.encode(&value) != s {
            return Err("invalid message");
        }
        Ok(value)
    };
    let header: Header = serde_json::from_slice(&decode(header)?).map_err(|_| "invalid message")?;
    if header.alg != algorithm
        || header.kid.is_empty()
        || header.kid.len() > 128
        || header.typ != "application/wincmd-experimental+jws"
        || header.crit != ["profile", "purpose", "epoch"]
        || header.profile != "experimental-ed25519-mldsa65-v1"
        || header.purpose != "release"
        || header.epoch == 0
    {
        return Err("invalid release signing context");
    }
    Ok((header, decode(payload)?))
}

fn decode_key<const N: usize>(value: &str) -> Result<[u8; N], &'static str> {
    let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| "invalid trust")?;
    if URL_SAFE_NO_PAD.encode(&bytes) != value {
        return Err("invalid trust");
    }
    bytes.try_into().map_err(|_| "invalid trust")
}

fn run(args: &[String]) -> Result<(), &'static str> {
    if args.len() != 10 {
        return Err(
            "usage: <ed25519|mldsa65> <private-key|-> <message> <artifact> <trust> <version> <stable|beta|pro> <epoch> <signature>",
        );
    }
    let algorithm = match args[1].as_str() {
        "ed25519" => "Ed25519",
        "mldsa65" => "ML-DSA-65",
        _ => return Err("unknown algorithm"),
    };
    let message = read(&args[3], 64 * 1024)?;
    let (header, payload) = check_message(&message, algorithm)?;
    let config: TrustFile =
        serde_json::from_slice(&read(&args[5], 8192)?).map_err(|_| "invalid trust")?;
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
    let epoch: u64 = args[8].parse().map_err(|_| "invalid epoch")?;
    if epoch != header.epoch {
        return Err("invalid epoch");
    }
    let artifact = File::open(&args[4]).map_err(|_| "artifact unavailable")?;
    let inputs = if args[7] == "pro" {
        release::prepare_release(&payload, artifact, &args[6], epoch, &trust)?
    } else {
        let url = format!(
            "https://winupdates.servalabs.com/free/v{0}/WinCommander_{0}_x64-setup.exe",
            args[6]
        );
        free_release::prepare_release_from_reader(
            &payload, artifact, &args[6], &args[7], &url, epoch, &trust,
        )?
    };
    if message.as_slice()
        != if algorithm == "Ed25519" {
            inputs.ed.as_slice()
        } else {
            inputs.pq.as_slice()
        }
    {
        return Err("prepared message does not match approved trust");
    }
    let key = read_key(&args[2])?;
    let signature = if algorithm == "Ed25519" {
        let seed: &[u8; 32] = key
            .as_slice()
            .try_into()
            .map_err(|_| "Ed25519 key must be a 32-byte seed")?;
        let pair = SigningKey::from_bytes(seed);
        if pair.verifying_key().to_bytes() != ed {
            return Err("private key does not match approved trust");
        }
        pair.sign(&message).to_bytes().to_vec()
    } else {
        let pair = PqdsaKeyPair::from_pkcs8(&ML_DSA_65_SIGNING, &key)
            .map_err(|_| "invalid ML-DSA-65 PKCS#8 key")?;
        if pair.public_key().as_ref() != pq {
            return Err("private key does not match approved trust");
        }
        let mut signature = vec![0; 3309];
        let length = pair
            .sign(&message, &mut signature)
            .map_err(|_| "signing failed")?;
        if length != 3309 {
            return Err("signing failed");
        }
        signature
    };
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[9])
        .map_err(|_| "cannot create signature")?;
    if output
        .write_all(&signature)
        .and_then(|_| output.sync_all())
        .is_err()
    {
        drop(output);
        let _ = std::fs::remove_file(&args[9]);
        return Err("cannot write signature");
    }
    Ok(())
}

fn main() -> ExitCode {
    eprintln!("EXPERIMENTAL offline signer: independently review release inputs and protect private keys.");
    match run(&std::env::args().collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("signing failed: {error}");
            ExitCode::FAILURE
        }
    }
}
