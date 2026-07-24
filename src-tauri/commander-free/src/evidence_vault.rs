// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/evidence_vault.rs
//
// ═══════════════════════════════════════════════════════════════════════
// evidence.vault (B1) — WORM export of the Free evidence ledger
// ═══════════════════════════════════════════════════════════════════════
//
// The Free tier records events to a plain JSONL ledger (`evidence.rs`).
// This module is the Free-side dispatch layer for the Investigator vault
// workflow. It is deliberately absent from the everyday Privacy panel.
//
//   export_evidence_vault
//     1. Require Investigator mode.
//     2. Read all entries from the local Free ledger via evidence_read().
//     3. Ship the entries + an RFC-3339 exportedAt timestamp to the Pro
//        sidecar ("export_evidence_vault").  Pro hash-chains the entries
//        using the same SHA-256 prev-hash pattern as audit_chain.rs and
//        signs the bundle with the device Ed25519 key (flow_bundle.rs
//        get_or_create_signing_key pattern).
//     4. Write the returned signed-bundle JSON to user-chosen output_path.
//     5. Return { ok, path, entryCount, chainHead } to the frontend.
//
//   verify_evidence_vault
//     1. Read a previously-exported bundle from input_path.
//     2. Forward the raw bundle to Pro ("verify_evidence_vault") — Pro
//        checks the hash chain + Ed25519 signature.
//     3. Return { valid, error? } to the frontend.
//
// WORM note: "WORM" means the signed bundle itself is tamper-evident
// (any edit breaks the signature + hash chain) and is written once to a
// user-chosen path.  Actual OS-level immutability (WORM drives / object-
// lock) is an infrastructure concern outside this module's scope.
//
// RFC 3161 timestamping: intentionally off-by-default and NOT called
// here — a future opt-in flag can add it.  No network TSA is contacted.
//
// TPM signing + PDF affidavit: OPTIONAL sub-features; not implemented
// in this file — leave them for a future Pro-side extension.

use base64::Engine;
use chrono::Utc;
use serde_json::{json, Value};

fn require_investigator_mode() -> Result<(), String> {
    if crate::license::is_advanced_mode() {
        Ok(())
    } else {
        Err("Evidence Vault exports require Investigator mode.".to_string())
    }
}

/// Export a signed evidence vault bundle.
///
/// Reads all entries from the Free evidence ledger, dispatches them to
/// the Pro sidecar for hash-chaining + Ed25519 signing, then writes the
/// resulting bundle JSON to `output_path`.
///
/// Returns a summary object: `{ ok, path, entryCount, chainHead }`.
#[tauri::command]
pub async fn export_evidence_vault(
    output_path: String,
    use_tpm: Option<bool>,
) -> Result<Value, String> {
    require_investigator_mode()?;

    // Pull the FULL retained ledger — a signed WORM bundle must cover the
    // whole record, not the UI's 200-entry recent window (evidence_read(None)
    // silently caps at 200 while the ledger retains up to 1000).
    let entries = crate::evidence::evidence_read_all()?;
    let entry_count = entries.len();

    // Serialise entries as a JSON array so they cross the sidecar boundary
    // cleanly.  serde_json::to_value never fails on a Vec<Serialize>.
    let entries_value =
        serde_json::to_value(&entries).map_err(|e| format!("serialise entries: {}", e))?;

    let exported_at = Utc::now().to_rfc3339();

    // Dispatch to Pro: Pro receives { entries: [...], exportedAt: "..." }
    // and returns a signed bundle JSON object.
    let bundle: Value = crate::sidecar::dispatch_paid_command(
        "export_evidence_vault",
        json!({
            "entries": entries_value,
            "exportedAt": exported_at,
            "useTpm": use_tpm.unwrap_or(false),
        }),
    )
    .await
    .inspect_err(|e| {
        crate::log::log_message(
            "warn",
            &format!("evidence vault export: sidecar dispatch failed ({entry_count} entries): {e}"),
        )
    })?;

    // Extract the chain-head hash for the summary (Pro sets bundle.chainHead).
    let chain_head = bundle
        .get("chainHead")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // Write bundle JSON to the user-chosen path.
    let bundle_bytes =
        serde_json::to_vec_pretty(&bundle).map_err(|e| format!("serialise bundle: {}", e))?;
    std::fs::write(&output_path, &bundle_bytes).map_err(|e| {
        let msg = format!("write vault bundle to '{}': {}", output_path, e);
        crate::log::log_message("warn", &format!("evidence vault export: {msg}"));
        msg
    })?;

    crate::log::log_message(
        "info",
        &format!("evidence vault exported: {entry_count} entries, chainHead={chain_head}"),
    );
    Ok(json!({
        "ok": true,
        "path": output_path,
        "entryCount": entry_count,
        "chainHead": chain_head,
    }))
}

/// Verify a previously-exported signed evidence vault bundle.
///
/// Reads the bundle file at `input_path`, forwards the raw bundle to
/// the Pro sidecar ("verify_evidence_vault") for chain + signature
/// verification, and returns `{ valid: bool, error?: string }`.
#[tauri::command]
pub async fn verify_evidence_vault(input_path: String) -> Result<Value, String> {
    // Intentionally no Investigator gate here — letting a user verify a vault
    // file they already have is harmless and useful even if their licence
    // lapses. The Pro verifier explicitly permits this read-only feature.

    let raw = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("read vault bundle from '{}': {}", input_path, e))?;

    let bundle: Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse vault bundle: {}", e))?;

    // Dispatch to Pro.  Pro returns { valid: bool, error?: string }.
    match crate::sidecar::dispatch_paid_command(
        "verify_evidence_vault",
        json!({ "bundle": bundle }),
    )
    .await
    {
        Ok(result) => {
            let valid = result
                .get("valid")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            crate::log::log_message(
                if valid { "info" } else { "warn" },
                &format!("evidence vault verify: valid={valid}"),
            );
            Ok(result)
        }
        Err(e) => {
            // Surface the sidecar error as an invalid-vault response so the
            // UI can render a clear error card rather than an unhandled
            // rejection — e.g. Pro not installed, IPC timeout, etc.
            crate::log::log_message(
                "warn",
                &format!("evidence vault verify: dispatch failed: {e}"),
            );
            Ok(json!({
                "valid": false,
                "error": e,
            }))
        }
    }
}

/// Export a one-page PDF affidavit summarizing the current evidence ledger as a
/// signed bundle (chain head, signature, entry count). Writes the PDF to
/// `output_path`. Pro renders + signs; Free decodes + writes.
#[tauri::command]
pub async fn export_evidence_affidavit(output_path: String) -> Result<Value, String> {
    require_investigator_mode()?;

    let entries = crate::evidence::evidence_read_all()?;
    let entries_value =
        serde_json::to_value(&entries).map_err(|e| format!("serialise entries: {}", e))?;
    let exported_at = Utc::now().to_rfc3339();

    let result = crate::sidecar::dispatch_paid_command(
        "export_evidence_affidavit",
        json!({ "entries": entries_value, "exportedAt": exported_at }),
    )
    .await?;

    let pdf_b64 = result
        .get("pdfB64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "affidavit export: Pro returned no pdfB64".to_string())?;
    let pdf = base64::engine::general_purpose::STANDARD
        .decode(pdf_b64)
        .map_err(|e| format!("decode affidavit pdf: {}", e))?;
    std::fs::write(&output_path, &pdf)
        .map_err(|e| format!("write affidavit to '{}': {}", output_path, e))?;

    Ok(json!({ "ok": true, "path": output_path, "bytes": pdf.len() }))
}

/// IRREVERSIBLE — Delete the TPM-backed "WinCommanderEvidenceVault" CNG key.
///
/// After this call the key no longer exists in the Platform Crypto Provider;
/// any previously-signed vault bundles can no longer be hardware-attested
/// by this machine. The Ed25519 software chain signature is unaffected.
///
/// Returns `{ status: "deleted"|"no_tpm", deleted: bool }`.
/// Thin Investigator wrapper; the actual CNG call runs in the Pro sidecar.
#[tauri::command]
pub async fn delete_vault_tpm_key() -> Result<Value, String> {
    require_investigator_mode()?;
    crate::sidecar::dispatch_paid_command("delete_vault_tpm_key", serde_json::Value::Null).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_exists() {
        // Structural smoke-test: this module compiles and links.
        // Functional tests require the sidecar + licence, so they live in
        // the integration-test suite rather than here.
    }
}
