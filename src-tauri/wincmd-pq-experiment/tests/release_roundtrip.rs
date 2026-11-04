// SPDX-License-Identifier: AGPL-3.0-or-later
#![cfg(all(
    feature = "experimental-offline-verifier",
    feature = "experimental-signer"
))]
//! A disposable local release, through both shipped rehearsal commands.
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Command};

fn invoke(program: &str, args: &[&Path], trailing: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .args(trailing)
        .output()
        .unwrap()
        .status
        .success()
}

#[test]
fn staged_manifest_to_signed_release_to_offline_check() {
    let dir = std::env::temp_dir().join(format!(
        "wincmd-release-roundtrip-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let file = |name: &str| dir.join(name);
    let artifact = b"disposable local release";
    let ed = SigningKey::from_bytes(&[42; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let manifest = serde_json::json!({"version":"3.7.0", "url":"https://winupdates.servalabs.com/pro/v3.7.0/wincommander-pro.exe", "sha256":hex::encode(Sha256::digest(artifact)), "size":artifact.len(), "pub_date":"2026-11-03T00:00:00Z", "notes":"fixture"});
    let trust = serde_json::json!({"ed_key_id":"ed", "ed_public":URL_SAFE_NO_PAD.encode(ed.verifying_key().to_bytes()), "pq_key_id":"pq", "pq_public":URL_SAFE_NO_PAD.encode(pq.public_key().as_ref()), "minimum_epoch":7});
    std::fs::write(file("latest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
    std::fs::write(file("trust.json"), serde_json::to_vec(&trust).unwrap()).unwrap();
    std::fs::write(file("pro.exe"), artifact).unwrap();
    let prep = env!("CARGO_BIN_EXE_wincmd-pq-release-prepare");
    let check = env!("CARGO_BIN_EXE_wincmd-pq-release-check");
    let run = |args: &[&str]| {
        Command::new(prep)
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    };
    let p = |name: &str| file(name).to_string_lossy().into_owned();
    assert!(run(&[
        "stage",
        &p("latest.json"),
        &p("pro.exe"),
        "3.7.0",
        &p("payload.json")
    ]));
    assert!(run(&[
        "prepare",
        &p("payload.json"),
        &p("pro.exe"),
        &p("trust.json"),
        "3.7.0",
        "7",
        &p("ed.msg"),
        &p("pq.msg")
    ]));
    let ed_sig = ed.sign(&std::fs::read(file("ed.msg")).unwrap()).to_bytes();
    let mut pq_sig = vec![0; 3309];
    pq.sign(&std::fs::read(file("pq.msg")).unwrap(), &mut pq_sig)
        .unwrap();
    std::fs::write(file("ed.sig"), ed_sig).unwrap();
    std::fs::write(file("pq.sig"), pq_sig).unwrap();
    assert!(run(&[
        "finish",
        &p("payload.json"),
        &p("pro.exe"),
        &p("trust.json"),
        "3.7.0",
        "7",
        &p("ed.sig"),
        &p("pq.sig"),
        &p("hybrid.json")
    ]));
    assert!(invoke(
        check,
        &[&file("hybrid.json"), &file("pro.exe"), &file("trust.json")],
        &["3.7.0"]
    ));
    std::fs::write(file("pro.exe"), b"changed release").unwrap();
    assert!(!invoke(
        check,
        &[&file("hybrid.json"), &file("pro.exe"), &file("trust.json")],
        &["3.7.0"]
    ));
    for name in [
        "latest.json",
        "trust.json",
        "pro.exe",
        "payload.json",
        "ed.msg",
        "pq.msg",
        "ed.sig",
        "pq.sig",
        "hybrid.json",
    ] {
        std::fs::remove_file(file(name)).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();
}
