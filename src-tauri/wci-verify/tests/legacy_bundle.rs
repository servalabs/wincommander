use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn run(path: &Path, files: &[(String, Vec<u8>)], key: &str) -> bool {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    for (name, body) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body).unwrap();
    }
    zip.finish().unwrap();
    Command::new(env!("CARGO_BIN_EXE_wci-verify"))
        .arg(path)
        .arg("--trusted-key")
        .arg(key)
        .output()
        .unwrap()
        .status
        .success()
}

#[test]
fn signed_manifest_and_expected_key_are_required() {
    let signing = SigningKey::from_bytes(&[42; 32]);
    let public = hex::encode(signing.verifying_key().to_bytes());
    let mut entry = json!({"ts":1,"examiner_id":"examiner","case_no":"case",
        "op":"case-seal","prev_hash":"0".repeat(64),"sig":""});
    let signature = signing.sign(&serde_json::to_vec(&entry).unwrap());
    entry["sig"] = Value::String(hex::encode(signature.to_bytes()));
    let mut audit = serde_json::to_vec(&entry).unwrap();
    let head = digest(&audit);
    audit.push(b'\n');
    let manifest = json!({"case_no":"case","examiner_id":"examiner",
        "examiner_name":"Examiner","agency":"Agency","warrant_ref":"Ref",
        "started_at":1,"finalized_at":2,"audit_chain_head_hash":head,
        "session_key_public_hex":public,
        "file_hashes":[["audit-chain.jsonl",digest(&audit)],["evidence.txt",digest(b"evidence")]]});
    let manifest = serde_json::to_vec(&manifest).unwrap();
    let hashes = format!("{}  case-manifest.json\n", digest(&manifest)).into_bytes();
    let files: Vec<(String, Vec<u8>)> = vec![
        ("case-manifest.json".into(), manifest),
        (
            "signature.sig".into(),
            hex::encode(signing.sign(&hashes).to_bytes()).into_bytes(),
        ),
        ("hashes.txt".into(), hashes),
        ("verify-pubkey.hex".into(), public.as_bytes().to_vec()),
        ("audit-chain.jsonl".into(), audit),
        ("evidence.txt".into(), b"evidence".to_vec()),
    ];
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("wci-verify-{}-{nonce}.zip", std::process::id()));
    assert!(run(&path, &files, &public));
    let mut changed = files.clone();
    let mut altered_manifest: Value = serde_json::from_slice(&changed[0].1).unwrap();
    altered_manifest["agency"] = Value::String("forged agency".into());
    changed[0].1 = serde_json::to_vec(&altered_manifest).unwrap();
    assert!(!run(&path, &changed, &public));
    assert!(!run(&path, &files, &"11".repeat(32)));
    let missing: Vec<_> = files
        .iter()
        .filter(|(name, _)| name != "evidence.txt")
        .cloned()
        .collect();
    assert!(!run(&path, &missing, &public));
    let mut extra = files.clone();
    extra.push(("extra.txt".into(), b"extra".to_vec()));
    assert!(!run(&path, &extra, &public));
    let no_audit: Vec<_> = files
        .iter()
        .filter(|(name, _)| name != "audit-chain.jsonl")
        .cloned()
        .collect();
    assert!(!run(&path, &no_audit, &public));
    std::fs::remove_file(path).unwrap();
}
