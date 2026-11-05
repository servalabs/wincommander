// SPDX-License-Identifier: AGPL-3.0-or-later
#![cfg(all(
    feature = "experimental-offline-verifier",
    feature = "experimental-signer"
))]
//! A disposable local release, through both shipped rehearsal commands.
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::SigningKey;
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
fn free_prepared_messages_interoperate_with_offline_signer() {
    let dir = std::env::temp_dir().join(format!(
        "wincmd-free-offline-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let p = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/free-tauri-disposable.json")).unwrap();
    let ed = SigningKey::from_bytes(&[42; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let url = "https://winupdates.servalabs.com/free/v99.0.0/WinCommander_99.0.0_x64-setup.exe";
    let manifest = serde_json::json!({"version":"99.0.0", "platforms":{"windows-x86_64":{"url":url,"signature":fixture["signature"]}}});
    let tauri = serde_json::json!({"plugins":{"updater":{"pubkey":fixture["publicKey"]}}});
    let trust = serde_json::json!({"ed_key_id":"ed", "ed_public":URL_SAFE_NO_PAD.encode(ed.verifying_key().to_bytes()), "pq_key_id":"pq", "pq_public":URL_SAFE_NO_PAD.encode(pq.public_key().as_ref()), "minimum_epoch":7});
    for (name, bytes) in [
        ("manifest", serde_json::to_vec(&manifest).unwrap()),
        ("tauri", serde_json::to_vec(&tauri).unwrap()),
        ("trust", serde_json::to_vec(&trust).unwrap()),
        ("installer", b"MZ".to_vec()),
        ("ed.key", ed.to_bytes().to_vec()),
        ("pq.key", pq.to_pkcs8v1().unwrap().as_ref().to_vec()),
    ] {
        std::fs::write(p(name), bytes).unwrap();
    }
    let run = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    };
    let prep = env!("CARGO_BIN_EXE_wincmd-pq-free-release-prepare");
    let signer = env!("CARGO_BIN_EXE_wincmd-pq-offline-sign");
    assert!(run(
        prep,
        &[
            "stage",
            &p("manifest"),
            &p("installer"),
            &p("tauri"),
            "99.0.0",
            "stable",
            &p("payload")
        ]
    ));
    assert!(run(
        prep,
        &[
            "prepare",
            &p("payload"),
            &p("installer"),
            &p("trust"),
            "99.0.0",
            "stable",
            "7",
            &p("ed.msg"),
            &p("pq.msg")
        ]
    ));
    assert!(run(
        signer,
        &["ed25519", &p("ed.key"), &p("ed.msg"), &p("ed.sig")]
    ));
    assert!(run(
        signer,
        &["mldsa65", &p("pq.key"), &p("pq.msg"), &p("pq.sig")]
    ));
    assert!(run(
        prep,
        &[
            "finish",
            &p("payload"),
            &p("installer"),
            &p("trust"),
            "99.0.0",
            "stable",
            "7",
            &p("ed.sig"),
            &p("pq.sig"),
            &p("signed")
        ]
    ));
    let check = env!("CARGO_BIN_EXE_wincmd-pq-free-release-check");
    assert!(run(
        check,
        &[
            &p("signed"),
            &p("installer"),
            &p("trust"),
            "99.0.0",
            "stable"
        ]
    ));
    assert!(!run(
        check,
        &[&p("signed"), &p("installer"), &p("trust"), "99.0.0", "beta"]
    ));
    let mut signature = std::fs::read(p("pq.sig")).unwrap();
    signature[0] ^= 1;
    std::fs::write(p("pq.bad"), signature).unwrap();
    assert!(!run(
        prep,
        &[
            "finish",
            &p("payload"),
            &p("installer"),
            &p("trust"),
            "99.0.0",
            "stable",
            "7",
            &p("ed.sig"),
            &p("pq.bad"),
            &p("bad")
        ]
    ));
    std::fs::remove_dir_all(dir).unwrap();
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
    std::fs::write(file("ed.key"), ed.to_bytes()).unwrap();
    std::fs::write(file("pq.key"), pq.to_pkcs8v1().unwrap().as_ref()).unwrap();
    let signer = env!("CARGO_BIN_EXE_wincmd-pq-offline-sign");
    let sign = |algorithm: &str, key: &str, message: &str, output: &str| {
        Command::new(signer)
            .args([algorithm, &p(key), &p(message), &p(output)])
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(!sign("ed25519", "ed.key", "pq.msg", "wrong.sig"));
    let message = std::fs::read_to_string(file("ed.msg")).unwrap();
    let (header, payload) = message.split_once('.').unwrap();
    let mut header: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
    header["purpose"] = "licence".into();
    std::fs::write(
        file("wrong.msg"),
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap()),
            payload
        ),
    )
    .unwrap();
    assert!(!sign("ed25519", "ed.key", "wrong.msg", "wrong.sig"));
    assert!(sign("ed25519", "ed.key", "ed.msg", "ed.sig"));
    assert!(sign("mldsa65", "pq.key", "pq.msg", "pq.sig"));
    assert!(!sign("ed25519", "ed.key", "ed.msg", "ed.sig"));
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
        "ed.key",
        "pq.key",
        "wrong.msg",
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
