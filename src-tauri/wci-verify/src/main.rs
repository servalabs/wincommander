// wci-verify — standalone case-bundle verifier.
//
// Walks a `.case.zip` produced by WinCommander Investigator and verifies:
//
//   1. case-manifest.json's per-file SHA-256 hashes match what's in the
//      bundle (every file in the bundle has either a manifest entry or
//      is one of the special manifest/signature files).
//   2. signature.sig is a valid Ed25519 signature over hashes.txt using
//      verify-pubkey.hex as the public key.
//   3. The audit chain (audit-chain.jsonl) is internally consistent —
//      prev_hash links match the previous line's SHA-256 and every
//      entry's `sig` validates against the verify pubkey.
//
// Exit codes:
//   0  bundle verified
//   1  manifest hash mismatch
//   2  signature did not verify
//   3  audit chain broken
//   10 usage / bundle missing / I/O error
//
// This binary is independent of the main Commander codebase so the
// verifier can be open-sourced + audited separately. It has zero
// network access, no telemetry.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Deserialize, Serialize)]
struct CaseManifest {
    case_no: String,
    examiner_id: String,
    examiner_name: String,
    agency: String,
    warrant_ref: String,
    started_at: u64,
    finalized_at: u64,
    audit_chain_head_hash: String,
    session_key_public_hex: String,
    file_hashes: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
struct AuditEntry {
    ts: u64,
    examiner_id: String,
    case_no: String,
    op: String,
    #[serde(default)]
    artifact: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    prev_hash: String,
    sig: String,
}

fn print_usage() {
    eprintln!("wci-verify — WinCommander Investigator case-bundle verifier");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    wci-verify <BUNDLE.case.zip> --trusted-key <ed25519-public-key-hex>");
    eprintln!("For current Investigator bundles, use the Investigator verifier with its root fingerprint.");
    eprintln!();
    eprintln!(
        "Exit codes: 0=ok, 1=hash mismatch, 2=bad signature, 3=broken chain, 10=usage/IO error."
    );
}

fn read_zip_entry(path: &PathBuf, name: &str) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("open zip: {}", e))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("read zip: {}", e))?;
    let mut e = z
        .by_name(name)
        .map_err(|_| format!("missing in bundle: {}", name))?;
    let mut buf = Vec::new();
    e.read_to_end(&mut buf)
        .map_err(|e| format!("read entry: {}", e))?;
    Ok(buf)
}

fn read_zip_text(path: &PathBuf, name: &str) -> Result<String, String> {
    let bytes = read_zip_entry(path, name)?;
    String::from_utf8(bytes).map_err(|e| format!("entry not UTF-8: {}", e))
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 || args[2] != "--trusted-key" {
        print_usage();
        return ExitCode::from(10);
    }
    let trusted_key = match hex::decode(&args[3]) {
        Ok(bytes) if bytes.len() == 32 => bytes,
        _ => {
            eprintln!("error: trusted public key must be 32 bytes in hexadecimal");
            return ExitCode::from(10);
        }
    };
    let bundle = PathBuf::from(&args[1]);
    if !bundle.exists() {
        eprintln!("error: bundle not found: {}", bundle.display());
        return ExitCode::from(10);
    }
    println!("─ wci-verify ─ {}", bundle.display());

    // ── 1. Load + parse manifest ────────────────────────────────────
    let manifest_text = match read_zip_text(&bundle, "case-manifest.json") {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {}", e);
            return ExitCode::from(10);
        }
    };
    let manifest: CaseManifest = match serde_json::from_str(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: parse manifest: {}", e);
            return ExitCode::from(10);
        }
    };
    println!("  case_no:        {}", manifest.case_no);
    println!("  examiner_id:    {}", manifest.examiner_id);
    println!("  examiner_name:  {}", manifest.examiner_name);
    println!("  agency:         {}", manifest.agency);
    println!("  warrant_ref:    {}", manifest.warrant_ref);

    // ── 2. Verify per-file hashes ──────────────────────────────────
    let f = match std::fs::File::open(&bundle) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: reopen zip: {}", e);
            return ExitCode::from(10);
        }
    };
    let mut z = match zip::ZipArchive::new(f) {
        Ok(z) => z,
        Err(e) => {
            eprintln!("error: read zip: {}", e);
            return ExitCode::from(10);
        }
    };
    if z.has_duplicate_names() {
        eprintln!("error: duplicate zip entry name");
        return ExitCode::from(10);
    }

    let expected: HashMap<String, String> = manifest.file_hashes.iter().cloned().collect();
    if expected.len() != manifest.file_hashes.len() {
        eprintln!("FAIL: duplicate manifest file names");
        return ExitCode::from(1);
    }
    let mut seen = HashSet::new();
    let mut mismatches = 0usize;
    let mut total_checked = 0usize;
    for i in 0..z.len() {
        let mut entry = match z.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("error: unreadable ZIP entry: {e}");
                return ExitCode::from(10);
            }
        };
        let name = match entry.name() {
            Ok(name) => name.into_owned(),
            Err(e) => {
                eprintln!("error: invalid zip entry name: {}", e);
                return ExitCode::from(10);
            }
        };
        // Skip the special files (the manifest itself is hashed separately
        // in hashes.txt; the signature artefacts aren't expected to be hashed).
        if matches!(
            name.as_str(),
            "case-manifest.json" | "hashes.txt" | "signature.sig" | "verify-pubkey.hex"
        ) {
            continue;
        }
        if entry.is_dir() {
            continue;
        }
        let mut buf = Vec::new();
        if entry.read_to_end(&mut buf).is_err() {
            eprintln!("error: unable to read ZIP entry: {name}");
            return ExitCode::from(10);
        }
        seen.insert(name.clone());
        let got = sha256_hex(&buf);
        match expected.get(&name) {
            Some(want) if want == &got => {
                total_checked += 1;
            }
            Some(want) => {
                eprintln!("  ✗ HASH MISMATCH: {}", name);
                eprintln!("    expected: {}", want);
                eprintln!("    got:      {}", got);
                mismatches += 1;
            }
            None => {
                eprintln!("  ✗ extra file (not in manifest): {}", name);
                mismatches += 1;
            }
        }
    }
    mismatches += expected.keys().filter(|name| !seen.contains(*name)).count();
    if mismatches > 0 {
        eprintln!(
            "FAIL: {} files mismatched or missing from the manifest",
            mismatches
        );
        return ExitCode::from(1);
    }
    println!("  ✓ {} files matched manifest hashes", total_checked);

    // ── 3. Ed25519 signature over hashes.txt ──────────────────────
    let hashes_txt = match read_zip_entry(&bundle, "hashes.txt") {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: {}", e);
            return ExitCode::from(10);
        }
    };
    let sig_hex = match read_zip_text(&bundle, "signature.sig") {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            eprintln!("error: {}", e);
            return ExitCode::from(10);
        }
    };
    let pk_hex = match read_zip_text(&bundle, "verify-pubkey.hex") {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            eprintln!("error: {}", e);
            return ExitCode::from(10);
        }
    };
    let pk_bytes = match hex::decode(&pk_hex) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: decode pubkey: {}", e);
            return ExitCode::from(10);
        }
    };
    if pk_bytes.len() != 32 {
        eprintln!("error: pubkey wrong length: {}", pk_bytes.len());
        return ExitCode::from(10);
    }
    if pk_bytes != trusted_key {
        eprintln!("FAIL: bundle public key does not match the independently supplied key");
        return ExitCode::from(2);
    }
    if !manifest
        .session_key_public_hex
        .eq_ignore_ascii_case(&pk_hex)
    {
        eprintln!("FAIL: manifest and bundle public keys differ");
        return ExitCode::from(2);
    }
    let mut pk_arr = [0u8; 32];
    pk_arr.copy_from_slice(&pk_bytes);
    let verifying = match VerifyingKey::from_bytes(&pk_arr) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("error: invalid pubkey curve point");
            return ExitCode::from(10);
        }
    };
    let sig_bytes = match hex::decode(&sig_hex) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: decode sig: {}", e);
            return ExitCode::from(10);
        }
    };
    if sig_bytes.len() != 64 {
        eprintln!("error: signature wrong length: {}", sig_bytes.len());
        return ExitCode::from(10);
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let signature = Signature::from_bytes(&sig_arr);
    if verifying.verify(&hashes_txt, &signature).is_err() {
        eprintln!("FAIL: signature over hashes.txt does NOT verify against bundle pubkey");
        return ExitCode::from(2);
    }
    if !signed_manifest_matches(&hashes_txt, manifest_text.as_bytes()) {
        eprintln!("FAIL: signed hash list does not uniquely bind case-manifest.json; unsupported legacy format");
        return ExitCode::from(2);
    }
    println!("  ✓ Ed25519 signature and signed manifest hash verify (classical only)");

    // ── 4. Audit chain ────────────────────────────────────────────
    let audit = match read_zip_text(&bundle, "audit-chain.jsonl") {
        Ok(t) => t,
        Err(_) => {
            eprintln!("FAIL: audit-chain.jsonl is missing or unreadable");
            return ExitCode::from(3);
        }
    };
    let mut prev = "0".repeat(64);
    let mut count = 0usize;
    for (i, line) in audit.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: AuditEntry = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("FAIL: audit chain line {}: bad JSON: {}", i + 1, e);
                return ExitCode::from(3);
            }
        };
        if entry.case_no != manifest.case_no || entry.examiner_id != manifest.examiner_id {
            eprintln!("FAIL: audit chain line {}: identity mismatch", i + 1);
            return ExitCode::from(3);
        }
        if entry.prev_hash != prev {
            eprintln!("FAIL: audit chain line {}: prev_hash mismatch", i + 1);
            return ExitCode::from(3);
        }
        // Re-serialise with sig empty to recover the signed bytes.
        let to_sign = AuditEntry {
            ts: entry.ts,
            examiner_id: entry.examiner_id.clone(),
            case_no: entry.case_no.clone(),
            op: entry.op.clone(),
            artifact: entry.artifact.clone(),
            sha256: entry.sha256.clone(),
            prev_hash: entry.prev_hash.clone(),
            sig: String::new(),
        };
        // serde_json uses the same field order as the struct, so the
        // signed-over JSON is regenerated identically.
        let unsigned_json = match serde_json::to_string(&serde_json::json!({
            "ts": to_sign.ts,
            "examiner_id": to_sign.examiner_id,
            "case_no": to_sign.case_no,
            "op": to_sign.op,
            "artifact": to_sign.artifact,
            "sha256": to_sign.sha256,
            "prev_hash": to_sign.prev_hash,
            "sig": "",
        })) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("FAIL: audit chain line {}: re-serialise error", i + 1);
                return ExitCode::from(3);
            }
        };
        // The original writer skipped Option::None fields. Rebuild via
        // serde_json::to_value with skip-null filter to match.
        let v: serde_json::Value = serde_json::from_str(&unsigned_json).unwrap();
        let v = compact_nulls(v);
        let unsigned_json = serde_json::to_string(&v).unwrap();

        let sb = match hex::decode(&entry.sig) {
            Ok(b) if b.len() == 64 => b,
            _ => {
                eprintln!("FAIL: audit chain line {}: bad sig hex/length", i + 1);
                return ExitCode::from(3);
            }
        };
        let mut sa = [0u8; 64];
        sa.copy_from_slice(&sb);
        let sig = Signature::from_bytes(&sa);
        if verifying.verify(unsigned_json.as_bytes(), &sig).is_err() {
            eprintln!(
                "FAIL: audit chain line {}: signature does not verify",
                i + 1
            );
            return ExitCode::from(3);
        }
        prev = sha256_hex(line.as_bytes());
        count += 1;
    }
    if count == 0 || prev != manifest.audit_chain_head_hash {
        eprintln!("FAIL: audit chain is empty or does not match the signed manifest head");
        return ExitCode::from(3);
    }
    println!(
        "  ✓ audit chain: {} entries, all signatures + links valid",
        count
    );
    println!();
    println!(
        "CLASSICAL SIGNATURES VERIFIED: case {} (examiner {}, agency {})",
        manifest.case_no, manifest.examiner_id, manifest.agency
    );
    ExitCode::SUCCESS
}

fn compact_nulls(v: serde_json::Value) -> serde_json::Value {
    if let serde_json::Value::Object(m) = v {
        let out: serde_json::Map<String, serde_json::Value> = m
            .into_iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k, compact_nulls(v)))
            .collect();
        serde_json::Value::Object(out)
    } else {
        v
    }
}

// Accept only a unique conventional sha256sum entry for the exact manifest.
// Unknown historical hash-list formats fail closed instead of claiming trust.
fn signed_manifest_matches(hashes: &[u8], manifest: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(hashes) else {
        return false;
    };
    let mut matches = 0;
    for line in text.lines() {
        if line.len() < 66 || !line.is_char_boundary(64) || !line.is_char_boundary(66) {
            continue;
        }
        let (digest, rest) = line.split_at(64);
        let name = if let Some(value) = rest.strip_prefix("  ") {
            value
        } else if let Some(value) = rest.strip_prefix(" *") {
            value
        } else {
            continue;
        };
        if name == "case-manifest.json" {
            if !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !digest.eq_ignore_ascii_case(&sha256_hex(manifest))
            {
                return false;
            }
            matches += 1;
        }
    }
    matches == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_list_must_bind_exact_manifest_once() {
        let manifest = br#"{"case_no":"A"}"#;
        let valid = format!("{}  case-manifest.json\n", sha256_hex(manifest));
        assert!(signed_manifest_matches(valid.as_bytes(), manifest));
        assert!(!signed_manifest_matches(valid.as_bytes(), b"changed"));
        assert!(!signed_manifest_matches(
            format!("{valid}{valid}").as_bytes(),
            manifest
        ));
        assert!(!signed_manifest_matches(b"unrecognized format", manifest));
        assert!(!signed_manifest_matches(&[0xff], manifest));
    }
}
