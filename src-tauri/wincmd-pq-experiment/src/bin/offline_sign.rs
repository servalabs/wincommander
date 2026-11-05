// SPDX-License-Identifier: AGPL-3.0-or-later
//! Offline adapter for the exact messages produced by the release preparation tools.
//! Key custody, artifact approval, and publication remain operator responsibilities.
use aws_lc_rs::signature::{PqdsaKeyPair, ML_DSA_65_SIGNING};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use serde::Deserialize;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    process::ExitCode,
};
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

fn check_message(bytes: &[u8], algorithm: &str) -> Result<(), &'static str> {
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
    let payload: serde_json::Value =
        serde_json::from_slice(&decode(payload)?).map_err(|_| "invalid payload")?;
    if !matches!(
        payload.get("format").and_then(|v| v.as_str()),
        Some("wincommander-free-release-v1" | "wincommander-pro-release-v1")
    ) {
        return Err("invalid release payload");
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), &'static str> {
    if args.len() != 5 {
        return Err(
            "usage: <ed25519|mldsa65> <private-key-file> <prepared-message> <signature-output>",
        );
    }
    let algorithm = match args[1].as_str() {
        "ed25519" => "Ed25519",
        "mldsa65" => "ML-DSA-65",
        _ => return Err("unknown algorithm"),
    };
    let message = read(&args[3], 64 * 1024)?;
    check_message(&message, algorithm)?;
    let key = read(&args[2], 16 * 1024)?;
    let signature = if algorithm == "Ed25519" {
        let seed: &[u8; 32] = key
            .as_slice()
            .try_into()
            .map_err(|_| "Ed25519 key must be a 32-byte seed")?;
        SigningKey::from_bytes(seed)
            .sign(&message)
            .to_bytes()
            .to_vec()
    } else {
        let pair = PqdsaKeyPair::from_pkcs8(&ML_DSA_65_SIGNING, &key)
            .map_err(|_| "invalid ML-DSA-65 PKCS#8 key")?;
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
        .open(&args[4])
        .map_err(|_| "cannot create signature")?;
    if output
        .write_all(&signature)
        .and_then(|_| output.sync_all())
        .is_err()
    {
        drop(output);
        let _ = std::fs::remove_file(&args[4]);
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
