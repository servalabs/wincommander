// src-tauri/commander-free/src/flow_bundle.rs
//
// ═══════════════════════════════════════════════════════════════════════
// FLOW BUNDLES — Ed25519-signed export/import of user flows
// ═══════════════════════════════════════════════════════════════════════
//
// A flow bundle is a JSON document that wraps one or more `Flow` records,
// signs the canonical bytes with a device-local Ed25519 key, and embeds
// the verifying key so receivers can verify integrity in-transit without
// any out-of-band trust setup.
//
// Threat model (v1):
//
//   - In-transit tamper:  PROTECTED (verify rejects)
//   - In-transit replay:  not protected (no nonce / timestamp window —
//                         bundle is a config artefact, not a command)
//   - Operator identity:  WEAKLY anchored (pubkey is stable per-machine
//                         but TOFU — no out-of-band pubkey pinning yet)
//
// Operator identity will firm up once `evidence.vault` ships its device
// cert (pubkey + fingerprint of motherboard serial / TPM EK cert /
// Windows install ID / primary MAC); the `flow_signing_seed_b64` stored
// in settings is migration-compatible — we'll import it as the vault
// seed rather than rotating, so existing exported bundles stay
// verifiable.
//
// Schema version is hard-coded at 1 — bumps require a back-compat
// migration on the verify side (don't ever just bump and break).
//
// Canonical JSON:  This module implements a tiny RFC 8785–style
// canonicalisation (sort object keys lexicographically, no whitespace,
// no extra escapes) by routing through `serde_json::Value` and a
// `BTreeMap` re-emit. Both sides MUST canonicalise identically — the
// `canonical_json_bytes` helper is the single source of truth.

use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::flow_engine::{get_flows_from_settings, persist_flows_to_settings, Flow};
use crate::settings::{read_settings, write_settings};

/// Schema version of the bundle wire format. Bump only with a verify
/// path that handles every prior version (or refuses with a clear
/// error). The frontend mirrors this in `FLOW_BUNDLE_SCHEMA_VERSION`.
pub const SCHEMA_VERSION: u32 = 1;

/// The complete supported block-type universe as of `SCHEMA_VERSION`.
/// Receivers compare a bundle's `block_types_used` against this to flag
/// unknown blocks before import — better than failing on save with a
/// cryptic serde error.
///
/// Keep in sync with `src/types/flows.ts FLOW_BLOCK_REGISTRY`.
const SUPPORTED_BLOCK_TYPES: &[&str] = &[
    // Triggers
    "HotkeyTrigger",
    "KeySequenceTrigger",
    "USBTrigger",
    "LidCloseTrigger",
    "WebhookTrigger",
    "CameraTrigger",
    "ScheduleTrigger",
    "NetworkTrigger",
    "FileTrigger",
    "ProcessTrigger",
    "SignalReceivedTrigger",
    "PasteMonitorTrigger",
    "DecoyMonitorTrigger",
    "RansomwareMonitorTrigger",
    // Conditions
    "TimeCondition",
    "SettingCondition",
    "NetworkCondition",
    "BatteryCondition",
    "USBPresenceCondition",
    // Actions
    "CommandAction",
    "SignalAction",
    "HTTPAction",
    "NotifyAction",
    "DelayAction",
    "ShellAction",
];

/// Wire format. All fields are required on the verify path; defaulting
/// is intentionally NOT used so a tampered bundle that drops a field
/// fails parse rather than silently verifies an incomplete payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowBundle {
    /// Wire-format version. Receivers MUST reject mismatches.
    pub schema_version: u32,
    /// ISO-8601 UTC timestamp of when the bundle was created. Informational
    /// only — not validated as part of the signature window.
    pub exported_at: String,
    /// Base64 of the 32-byte Ed25519 public key that signed this bundle.
    pub signer_pubkey_b64: String,
    /// Distinct block-type strings used across all flows in this bundle.
    /// Receivers cross-check against their own supported set BEFORE
    /// attempting deserialisation to give a clean error.
    pub block_types_used: Vec<String>,
    /// The flows themselves. Stored as `serde_json::Value` so the bundle
    /// envelope round-trips through unknown future fields cleanly (the
    /// canonical-json signer normalises whatever's inside).
    pub flows: Vec<Value>,
    /// Base64 of the 64-byte Ed25519 signature over `canonical_json_bytes`
    /// of the bundle with this field temporarily set to `""`.
    pub signature_b64: String,
}

/// Result of verifying a bundle. Designed so the frontend can render a
/// rich "what's in this bundle and is it safe" dialog before the
/// operator clicks Import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleVerification {
    /// Bundle parsed cleanly and the signature is valid for the embedded
    /// pubkey. `false` does NOT necessarily mean malicious — it may mean
    /// schema mismatch or block-type unknown.
    pub signature_ok: bool,
    /// `true` iff the bundle's `schema_version` equals `SCHEMA_VERSION`.
    pub schema_version_ok: bool,
    pub schema_version_got: u32,
    pub schema_version_expected: u32,
    /// Pubkey advertised by the bundle. The receiver might pin this in
    /// future (TOFU) — for now it's surfaced for operator inspection.
    pub signer_pubkey_b64: String,
    /// Block-types in the bundle that this build does NOT understand.
    /// Importing with these will fail at serde-parse — surface up front.
    pub unsupported_block_types: Vec<String>,
    /// Count of flows the bundle CLAIMS to contain (post-parse).
    pub flow_count: usize,
    /// Names of every flow in the bundle, for the import-confirm dialog.
    pub flow_names: Vec<String>,
    /// Highest risk level across all flows in the bundle ("low" /
    /// "medium" / "high"). Drives the confirm dialog severity.
    pub max_risk_level: String,
    /// ISO-8601 timestamp the bundle was exported at.
    pub exported_at: String,
}

// ═══════════════════════════════════════════════════════════════════════
// CANONICAL JSON
// ═══════════════════════════════════════════════════════════════════════
//
// RFC 8785–lite: every object's keys are sorted in lexicographic
// (code-point) order; no insignificant whitespace; arrays preserve
// element order. Numbers use serde_json's default formatting (matches
// most canonical implementations for the typed values we emit).

/// Recursively re-build a `Value` with `BTreeMap` for every object so
/// `serde_json::to_string` emits keys in deterministic order.
fn canonicalise(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted: BTreeMap<String, Value> = BTreeMap::new();
            for (k, v) in map {
                sorted.insert(k, canonicalise(v));
            }
            // Round-trip through a fresh Map preserving BTreeMap order.
            // `serde_json::Map` with `preserve_order` feature is insertion-
            // ordered, so iterating a BTreeMap and re-inserting produces a
            // sorted-key Object on serialise. Without `preserve_order`,
            // serde_json::Map's iteration order is also stable but unsorted —
            // so we must construct via an ordered insertion sequence.
            let mut out = serde_json::Map::new();
            for (k, v) in sorted {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalise).collect()),
        other => other,
    }
}

/// The single source of truth for bytes that get signed / verified.
fn canonical_json_bytes(value: &Value) -> Vec<u8> {
    let canon = canonicalise(value.clone());
    // `to_string` on a Value produces compact (no whitespace) output.
    serde_json::to_string(&canon)
        .unwrap_or_default()
        .into_bytes()
}

// ═══════════════════════════════════════════════════════════════════════
// KEY MANAGEMENT
// ═══════════════════════════════════════════════════════════════════════

/// Returns the device's signing key, generating + persisting one on
/// first call. Idempotent.
fn get_or_create_signing_key() -> Result<SigningKey, String> {
    let mut settings = read_settings()?;

    if let Some(seed_b64) = settings.app.flow_signing_seed_b64.as_deref() {
        let seed = B64
            .decode(seed_b64)
            .map_err(|e| format!("Stored signing seed is not valid base64: {e}"))?;
        if seed.len() != 32 {
            return Err(format!(
                "Stored signing seed has wrong length: expected 32 bytes, got {}",
                seed.len()
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&seed);
        return Ok(SigningKey::from_bytes(&arr));
    }

    // First-time generation. Uses the OS CSPRNG via rand_core::OsRng.
    let signing_key = SigningKey::generate(&mut rand_core::OsRng);
    let seed_b64 = B64.encode(signing_key.to_bytes());
    settings.app.flow_signing_seed_b64 = Some(seed_b64);
    write_settings(&settings)?;
    Ok(signing_key)
}

// ═══════════════════════════════════════════════════════════════════════
// EXPORT
// ═══════════════════════════════════════════════════════════════════════

/// Collect block-type strings used across a slice of `Value` flows.
fn collect_block_types(flows: &[Value]) -> Vec<String> {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for flow in flows {
        for field in ["triggers", "conditions", "actions"] {
            if let Some(items) = flow.get(field).and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(t) = item.get("type").and_then(|v| v.as_str()) {
                        seen.insert(t.to_string());
                    }
                }
            }
        }
    }
    seen.into_iter().collect()
}

/// Sign a populated bundle in-place. The bundle's `signature_b64` field
/// MUST be `""` on entry. On success the field is filled with the
/// base64-encoded signature.
fn sign_bundle(bundle: &mut FlowBundle, signing_key: &SigningKey) -> Result<(), String> {
    bundle.signature_b64.clear();
    let value = serde_json::to_value(&*bundle).map_err(|e| e.to_string())?;
    let bytes = canonical_json_bytes(&value);
    let sig = signing_key.sign(&bytes);
    bundle.signature_b64 = B64.encode(sig.to_bytes());
    Ok(())
}

/// Build + sign a bundle from a set of flow IDs. Returns the bundle as
/// pretty-printed JSON (operator-readable in a clipboard / text file).
///
/// User flows only — system flows are deliberately excluded because
/// they're seeded on first run by the engine itself, so exporting them
/// would duplicate on import.
pub fn export_bundle(flow_ids: &[String]) -> Result<String, String> {
    if flow_ids.is_empty() {
        return Err("No flows selected for export".to_string());
    }

    let all = get_flows_from_settings();
    let mut picked: Vec<Flow> = Vec::with_capacity(flow_ids.len());
    let mut missing: Vec<String> = Vec::new();
    for fid in flow_ids {
        // Cloning via Option::cloned avoids a finicky &Flow type inference
        // path that rustc can't always resolve inside a guarded `match`.
        match all.iter().find(|f| &f.id == fid).cloned() {
            Some(f) if f.system => return Err(format!("Cannot export system flow '{}'", f.name)),
            Some(f) => picked.push(f),
            None => missing.push(fid.clone()),
        }
    }
    if !missing.is_empty() {
        return Err(format!("Unknown flow id(s): {}", missing.join(", ")));
    }

    let signing_key = get_or_create_signing_key()?;
    let verifying_key: VerifyingKey = signing_key.verifying_key();

    let flow_values: Vec<Value> = picked
        .iter()
        .map(|f| serde_json::to_value(f).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;

    let block_types_used = collect_block_types(&flow_values);

    let mut bundle = FlowBundle {
        schema_version: SCHEMA_VERSION,
        exported_at: chrono::Utc::now().to_rfc3339(),
        signer_pubkey_b64: B64.encode(verifying_key.to_bytes()),
        block_types_used,
        flows: flow_values,
        signature_b64: String::new(),
    };
    sign_bundle(&mut bundle, &signing_key)?;

    serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())
}

// ═══════════════════════════════════════════════════════════════════════
// VERIFY
// ═══════════════════════════════════════════════════════════════════════

fn verify_signature(bundle: &FlowBundle) -> Result<bool, String> {
    let pubkey_bytes = B64
        .decode(&bundle.signer_pubkey_b64)
        .map_err(|e| format!("Pubkey not valid base64: {e}"))?;
    if pubkey_bytes.len() != 32 {
        return Err(format!(
            "Pubkey wrong length: expected 32 bytes, got {}",
            pubkey_bytes.len()
        ));
    }
    let mut pk_arr = [0u8; 32];
    pk_arr.copy_from_slice(&pubkey_bytes);
    let verifying_key = VerifyingKey::from_bytes(&pk_arr).map_err(|e| e.to_string())?;

    let sig_bytes = B64
        .decode(&bundle.signature_b64)
        .map_err(|e| format!("Signature not valid base64: {e}"))?;
    if sig_bytes.len() != 64 {
        return Err(format!(
            "Signature wrong length: expected 64 bytes, got {}",
            sig_bytes.len()
        ));
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let sig = Signature::from_bytes(&sig_arr);

    // Re-canonicalise with signature blanked, matching the signer.
    let mut sig_zeroed = bundle.clone();
    sig_zeroed.signature_b64.clear();
    let value = serde_json::to_value(&sig_zeroed).map_err(|e| e.to_string())?;
    let bytes = canonical_json_bytes(&value);

    Ok(verifying_key.verify(&bytes, &sig).is_ok())
}

/// Compute the maximum risk-level string across a slice of flow JSON values.
fn max_risk(flows: &[Value]) -> String {
    let mut best = 0u8;
    for f in flows {
        let lvl = f.get("riskLevel").and_then(|v| v.as_str()).unwrap_or("low");
        let n = match lvl {
            "high" => 3,
            "medium" => 2,
            _ => 1,
        };
        if n > best {
            best = n;
        }
    }
    match best {
        3 => "high".to_string(),
        2 => "medium".to_string(),
        _ => "low".to_string(),
    }
}

/// Parse + verify a bundle string. Never throws on bad signature — the
/// boolean is returned in the report so the frontend can show a
/// "tampered" warning instead of an opaque error.
pub fn verify_bundle(bundle_json: &str) -> Result<BundleVerification, String> {
    let bundle: FlowBundle =
        serde_json::from_str(bundle_json).map_err(|e| format!("Bundle JSON malformed: {e}"))?;

    let signature_ok = verify_signature(&bundle).unwrap_or(false);

    let schema_version_ok = bundle.schema_version == SCHEMA_VERSION;
    let supported: std::collections::BTreeSet<&&str> = SUPPORTED_BLOCK_TYPES.iter().collect();
    let unsupported_block_types: Vec<String> = bundle
        .block_types_used
        .iter()
        .filter(|t| !supported.contains(&t.as_str()))
        .cloned()
        .collect();

    let flow_names: Vec<String> = bundle
        .flows
        .iter()
        .map(|f| {
            f.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("<unnamed>")
                .to_string()
        })
        .collect();

    Ok(BundleVerification {
        signature_ok,
        schema_version_ok,
        schema_version_got: bundle.schema_version,
        schema_version_expected: SCHEMA_VERSION,
        signer_pubkey_b64: bundle.signer_pubkey_b64.clone(),
        unsupported_block_types,
        flow_count: bundle.flows.len(),
        flow_names,
        max_risk_level: max_risk(&bundle.flows),
        exported_at: bundle.exported_at.clone(),
    })
}

// ═══════════════════════════════════════════════════════════════════════
// IMPORT
// ═══════════════════════════════════════════════════════════════════════

/// Import a verified bundle into the user's flow list. Always:
///   - regenerates `id`s so imports never collide with existing flows
///   - sets `enabled = false` so the operator reviews before arming
///   - sets `system = false` so imported flows can be deleted
///
/// `accept_unsupported` MUST be true if the bundle uses any block type
/// this build doesn't know about. We forward such flows verbatim and let
/// the schema-parse step fail loudly rather than silently dropping
/// blocks.
pub fn import_bundle(bundle_json: &str, accept_unsupported: bool) -> Result<usize, String> {
    let bundle: FlowBundle =
        serde_json::from_str(bundle_json).map_err(|e| format!("Bundle JSON malformed: {e}"))?;

    // Hard-stop on schema mismatch — we have no migration path yet.
    if bundle.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "Bundle schema version {} doesn't match this build's expected {}",
            bundle.schema_version, SCHEMA_VERSION
        ));
    }

    let supported: std::collections::BTreeSet<&&str> = SUPPORTED_BLOCK_TYPES.iter().collect();
    let unsupported: Vec<&String> = bundle
        .block_types_used
        .iter()
        .filter(|t| !supported.contains(&t.as_str()))
        .collect();
    if !unsupported.is_empty() && !accept_unsupported {
        return Err(format!(
            "Bundle uses block types this build doesn't support: {}. \
             Pass acceptUnsupported=true to import anyway (some flows may fail to load).",
            unsupported
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    // Verify signature — refuse on tamper, regardless of accept_unsupported.
    if !verify_signature(&bundle).unwrap_or(false) {
        return Err("Bundle signature failed verification".to_string());
    }

    // Parse each flow into a typed Flow. Anything that doesn't parse
    // gets reported as an error rather than partially imported.
    let mut imported: Vec<Flow> = Vec::with_capacity(bundle.flows.len());
    for (idx, raw) in bundle.flows.iter().enumerate() {
        let mut flow: Flow = serde_json::from_value(raw.clone()).map_err(|e| {
            format!(
                "Flow #{} ('{}') failed to parse: {}",
                idx,
                raw.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unnamed>"),
                e
            )
        })?;
        flow.id = format!(
            "flow-import-{}-{}",
            chrono::Utc::now().timestamp_millis(),
            idx
        );
        flow.enabled = false;
        flow.system = false;
        imported.push(flow);
    }

    let mut all = get_flows_from_settings();
    all.extend(imported.iter().cloned());
    persist_flows_to_settings(&all)?;

    Ok(imported.len())
}

// ═══════════════════════════════════════════════════════════════════════
// TAURI COMMANDS
// ═══════════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn export_flow_bundle(flow_ids: Vec<String>) -> Result<String, String> {
    export_bundle(&flow_ids)
}

#[tauri::command]
pub fn verify_flow_bundle(bundle_json: String) -> Result<BundleVerification, String> {
    verify_bundle(&bundle_json)
}

#[tauri::command]
pub async fn import_flow_bundle(
    app: AppHandle,
    bundle_json: String,
    accept_unsupported: bool,
) -> Result<usize, String> {
    let count = import_bundle(&bundle_json, accept_unsupported)?;
    // Surface the new flows in the UI without a full reload.
    use tauri::Emitter;
    let _ = app.emit("flows-reloaded", ());
    Ok(count)
}

#[tauri::command]
pub fn get_flow_signer_pubkey() -> Result<String, String> {
    let key = get_or_create_signing_key()?;
    Ok(B64.encode(key.verifying_key().to_bytes()))
}

// ═══════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fake_flow(name: &str) -> Value {
        json!({
            "id": format!("flow-{}", name),
            "name": name,
            "system": false,
            "enabled": false,
            "triggers": [{ "type": "HotkeyTrigger", "hotkey": "Ctrl+Alt+T" }],
            "conditions": [],
            "actions": [{ "type": "NotifyAction", "message": "ping", "severity": "info", "duration": 4000 }],
            "notes": "",
            "tags": [],
            "owner": "",
            "riskLevel": "low",
            "lastReviewedAt": "",
        })
    }

    #[test]
    fn canonical_sorts_object_keys() {
        let v = json!({ "z": 1, "a": 2, "m": { "y": 3, "x": 4 } });
        let bytes = canonical_json_bytes(&v);
        let s = String::from_utf8(bytes).unwrap();
        // 'a' before 'm' before 'z'; 'x' before 'y' inside the nested object.
        assert_eq!(s, r#"{"a":2,"m":{"x":4,"y":3},"z":1}"#);
    }

    #[test]
    fn canonical_handles_arrays() {
        let v = json!({ "list": [{ "b": 2, "a": 1 }, { "d": 4, "c": 3 }] });
        let bytes = canonical_json_bytes(&v);
        let s = String::from_utf8(bytes).unwrap();
        assert_eq!(s, r#"{"list":[{"a":1,"b":2},{"c":3,"d":4}]}"#);
    }

    #[test]
    fn collect_block_types_dedupes_and_scopes() {
        let flows = vec![
            fake_flow("a"),
            json!({
                "id": "x",
                "name": "x",
                "system": false,
                "enabled": false,
                "triggers": [{ "type": "USBTrigger", "mode": "remove" }],
                "conditions": [{ "type": "TimeCondition", "start": "09:00", "end": "17:00" }],
                "actions": [{ "type": "SignalAction", "targetRole": "admins", "signalType": "alert" }],
            }),
        ];
        let types = collect_block_types(&flows);
        // Sorted by virtue of BTreeSet.
        assert_eq!(
            types,
            vec![
                "HotkeyTrigger",
                "NotifyAction",
                "SignalAction",
                "TimeCondition",
                "USBTrigger"
            ]
        );
    }

    #[test]
    fn sign_then_verify_roundtrips() {
        let signing_key = SigningKey::generate(&mut rand_core::OsRng);
        let pubkey = signing_key.verifying_key();
        let flows = vec![fake_flow("hello")];
        let mut bundle = FlowBundle {
            schema_version: SCHEMA_VERSION,
            exported_at: "2026-05-12T00:00:00Z".to_string(),
            signer_pubkey_b64: B64.encode(pubkey.to_bytes()),
            block_types_used: collect_block_types(&flows),
            flows,
            signature_b64: String::new(),
        };
        sign_bundle(&mut bundle, &signing_key).expect("sign");
        assert!(!bundle.signature_b64.is_empty());
        assert!(verify_signature(&bundle).expect("verify"));
    }

    #[test]
    fn tamper_breaks_verify() {
        let signing_key = SigningKey::generate(&mut rand_core::OsRng);
        let pubkey = signing_key.verifying_key();
        let flows = vec![fake_flow("orig")];
        let mut bundle = FlowBundle {
            schema_version: SCHEMA_VERSION,
            exported_at: "2026-05-12T00:00:00Z".to_string(),
            signer_pubkey_b64: B64.encode(pubkey.to_bytes()),
            block_types_used: collect_block_types(&flows),
            flows,
            signature_b64: String::new(),
        };
        sign_bundle(&mut bundle, &signing_key).expect("sign");

        // Mutate a flow name AFTER signing — signature should no longer verify.
        bundle.flows[0]["name"] = json!("tampered");
        assert!(!verify_signature(&bundle).expect("verify"));
    }

    #[test]
    fn unsupported_block_types_detected() {
        let mut flows = vec![fake_flow("ok")];
        flows[0]["triggers"][0]["type"] = json!("MadeUpFutureTrigger");
        let types = collect_block_types(&flows);
        assert!(types.contains(&"MadeUpFutureTrigger".to_string()));
    }

    #[test]
    fn max_risk_picks_highest() {
        let flows = vec![
            json!({ "riskLevel": "low" }),
            json!({ "riskLevel": "high" }),
            json!({ "riskLevel": "medium" }),
        ];
        assert_eq!(max_risk(&flows), "high");

        let flows = vec![json!({ "riskLevel": "low" }), json!({})];
        assert_eq!(max_risk(&flows), "low");
    }
}
