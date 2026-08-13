//! Wire types for the agent-facing fleet check-in protocol + verification.
//!
//! This module is available under the `types` feature alone (no `reqwest`/`tokio`)
//! so a types-only consumer (e.g. WinCommander Free) can verify a `SignedCommand`
//! locally without linking the transport loop.
//!
//! The signature covers the canonical JSON preimage produced by
//! [`fleet_proto::canonical_command_bytes`] — this crate does NOT reimplement
//! canonical-byte construction; it calls the SSOT crate for both the byte layout
//! and the raw ed25519 verify (`fleet_proto::verify_signature_b64`).
//!
//! **The preimage's `command_id` slot is filled with `idempotency_key`, NOT
//! `SignedCommand.command_id`.** The signer (operator's offline tool for
//! operator-control commands, or the server for ordinary commands) signs over the
//! stable `idempotency_key` because the server-assigned UUID does not exist
//! yet at signing time. See [`verify_command`] and `fleet_proto::SignedCommand`.

use std::collections::HashMap;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use ed25519_dalek::VerifyingKey;
use hmac::{KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Enrollment capability that opts a device into request-body-bound HMACs.
/// Once recorded, the server rejects v1 identity-only MACs for that device.
pub const HMAC_BODY_V2_CAPABILITY: &str = "hmac_body_v2";
pub const HMAC_VERSION_V2: i64 = 2;

// ── Wire types (agent check-in protocol) ─────────────────────────────────────

/// Enroll request body (POST `/v1/agents/enroll`). Unified check-in transport
/// (2026-07-09): the client OWNS its identity — it sends a `device_id`
/// (UUIDv4), a `device_hash` (seat/blocklist + re-enroll-guard key; equal to
/// `device_id` on self-host), and a `device_kind`
/// (`wincommander` | `tuxcommander` | `android`). NO device public key is sent:
/// the check-in transport authenticates by the per-device HMAC `checkin_secret`
/// and verifies inbound commands against the operator's pinned key.
#[derive(Debug, Serialize)]
pub struct EnrollRequest {
    pub device_id: String,
    pub device_hash: String,
    pub device_kind: String,
    pub hostname: String,
    pub platform: String,
    pub agent_version: String,
    pub protocol_version: i64,
    pub capabilities: Vec<String>,
}

/// Enroll response from the fleet server.
#[derive(Debug, Clone, Deserialize)]
pub struct EnrollResponse {
    pub device_id: String,
    /// Base64 ed25519 OPERATOR key — verifies operator-control commands. `None`
    /// when the org has no operator key configured yet; such an agent then
    /// can't verify those commands (fail-closed) and should present itself as
    /// not-fully-enrolled. `#[serde(default)]` lets an older response still parse.
    #[serde(default)]
    pub command_pubkey_b64: Option<String>,
    /// Base64 ed25519 SERVER signing key — verifies ordinary (server-signed)
    /// commands. Present from any current server. `#[serde(default)]` for
    /// tolerance of older responses.
    #[serde(default)]
    pub server_signing_key_b64: Option<String>,
    /// Per-device HMAC secret (raw bytes, base64-encoded) returned by the server at
    /// enroll time. Used to authenticate subsequent check-in requests.
    pub checkin_secret_b64: Option<String>,
}

/// Check-in request body (POST `/v1/agents/checkin`).
///
/// Version 2 binds the method, route, and canonical JSON payload (excluding
/// only `hmac`), so acknowledgements, telemetry, padding, and `decoy` all have
/// integrity rather than merely authenticating a device identity.
#[derive(Debug, Serialize)]
pub struct CheckinRequest {
    pub device_id: String,
    pub hostname: String,
    pub posture: String,
    /// Unix epoch seconds for the freshness check (and HMAC preimage).
    pub ts: i64,
    /// Single-use nonce for replay defence (and HMAC preimage).
    pub nonce: String,
    pub hmac_version: i64,
    /// HMAC-SHA256 over the v2 request preimage, STANDARD base64 encoded.
    pub hmac: String,
    /// Opaque, ignored-by-server filler bytes (base64) so every check-in
    /// request lands in the same size bucket regardless of what it actually
    /// carries. It is covered by the v2 request MAC. Empty string when padding
    /// is disabled (`FleetConfig::checkin_padding_bytes == 0`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub padding: String,
    /// Marks this as a cover-traffic decoy check-in: authenticate normally,
    /// but the server must return no commands and record nothing sensitive
    /// for it (see `fleet-server`'s `checkin` handler). Since the request body
    /// is sent over TLS, this flag is never visible to a network observer —
    /// it exists purely so the server can distinguish decoys from real
    /// check-ins *after* decryption, without that distinction leaking on the
    /// wire (size/timing are identical either way).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub decoy: bool,
    /// Optional live resource sample (CPU/RAM/disk/network) — see
    /// `FleetActions::sample_resources`. Always-on, no consent gate; omitted
    /// entirely when the platform has no collector wired up (duress-only
    /// agents) so older/other check-ins are byte-identical to before this
    /// field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<fleet_proto::DeviceResourceSample>,
    /// Optional device-health snapshot (encryption/patch/AV/OS/sovereignty) —
    /// see [`HealthSnapshot`] and `FleetActions::sample_health`. Transport-
    /// envelope only, exactly like `resources` immediately above; omitted
    /// entirely when the platform has no health collector
    /// wired up (duress-only agents, or before a platform implements
    /// `sample_health`) so older/other check-ins are byte-identical to before
    /// this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthSnapshot>,
}

/// Agent-reported device-health snapshot, folded into the SAME check-in
/// round-trip as `resources`/`posture`. Deliberately ROUTE-LOCAL — not a
/// `fleet_proto` wire type — mirroring the fleet server's own
/// `device_health::HealthSnapshot` (`fleet-server/src/routes/device_health.rs`),
/// which is intentionally kept out of the shared `fleet_proto` crate because
/// this is an admin-visibility roll-up, not a signed/verified command or
/// policy type. Field names/shape are pinned to match that server struct
/// exactly (`encryption_on`, `patch_state` ∈ {"current","pending","unknown"},
/// `av_on`, `os_version`, `sovereignty_score`).
///
/// PII-free: every field is a scalar or a coarse label — never a filename,
/// path, URL, or username, matching the `ArgusSignal` discipline.
///
/// Every field is independently optional: a platform's `sample_health`
/// implementation should degrade a single failed/unavailable probe to `None`
/// for that field alone rather than dropping the whole snapshot — see the
/// `FleetActions::sample_health` doc.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HealthSnapshot {
    pub encryption_on: Option<bool>,
    pub patch_state: Option<String>,
    pub av_on: Option<bool>,
    pub os_version: Option<String>,
    pub sovereignty_score: Option<i64>,
}

/// Check-in response: zero or more signed commands to execute.
#[derive(Debug, Deserialize)]
pub struct CheckinResponse {
    /// Whether the server is issuing an all-clear (resets the dead-man).
    #[serde(default)]
    pub all_clear: bool,
    /// Signed commands to dispatch.
    #[serde(default)]
    pub commands: Vec<SignedCommand>,
    /// The device's resolved config epoch (server-signed policy snapshot),
    /// `None` when the org has published no policy. Opaque `Value` here — the
    /// platform (WinCommander) verifies + applies it via
    /// [`FleetActions::on_config_epoch`]; duress-only agents ignore it.
    #[serde(default)]
    pub config_epoch: Option<serde_json::Value>,
    /// Opaque, ignored filler bytes (base64) mirroring `CheckinRequest::padding`
    /// — pads the response into the same size bucket regardless of whether it
    /// carries zero or several signed commands. Never covered by any
    /// signature; purely transport-envelope. Absent/empty on servers that
    /// don't shape responses.
    #[serde(default)]
    pub padding: String,
    /// On-device content-search jobs this device currently owes (fleet
    /// server's `search_job_devices` rows in `pending` state for this
    /// device). **Optional / null-absent-safe**: `#[serde(default)]` so a
    /// server that hasn't shipped dispatch yet, or a response that simply
    /// has none to hand out, deserializes to an empty `Vec` rather than a
    /// parse error — this must never break check-in parsing or block the
    /// check-in loop. See [`dispatch::execute_pending_search_jobs`] for how
    /// these are executed and reported.
    ///
    /// [`dispatch::execute_pending_search_jobs`]: crate::dispatch::execute_pending_search_jobs
    #[serde(default)]
    pub pending_search_jobs: Vec<PendingSearchJob>,
}

/// One on-device content-search job owed by this device, as carried in
/// [`CheckinResponse::pending_search_jobs`]. Route-local (like
/// [`HealthSnapshot`]) — mirrors the fleet server's
/// `content_search::ContentSearchJob`/`NewSearchJob` shape, not a signed
/// `fleet_proto` command (a search job is dispatched over the check-in
/// response, not the signed-command channel — it carries no signature to
/// verify).
#[derive(Debug, Clone, Deserialize)]
pub struct PendingSearchJob {
    /// The fleet server's job id (`csj_<uuid>` — see fleet-server's
    /// `CONTENT_JOB_ID_PREFIX`). Echoed back verbatim in the result report.
    pub job_id: String,
    /// The search query text to run against the local content index.
    pub query: String,
    /// Cap on the number of hits to report for this job on this device.
    /// `#[serde(default)]` with a conservative fallback so a malformed/
    /// missing value never blocks execution — matches the server's own
    /// `max_hits_per_device` ceiling (1..=1000).
    #[serde(default = "default_max_hits_per_device")]
    pub max_hits_per_device: usize,
}

fn default_max_hits_per_device() -> usize {
    50
}

/// One content-search hit — field names pinned to match the fleet server's
/// `content_search::ContentSearchHit` exactly (`path`, `snippet`, `score`).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SearchHit {
    pub path: String,
    pub snippet: String,
    pub score: f64,
}

/// Body of `POST /v1/agents/search-result` — reports one job's outcome for
/// this device. Same HMAC envelope as [`CheckinRequest`] (`device_id`/`ts`/
/// `nonce`/`hmac`, computed via [`compute_request_hmac_v2`] over the SAME
/// per-device `checkin_secret` used
/// for check-in — this is a dedicated event-driven route, not a literal
/// `/checkin` call, per the server's `authenticate_device_hmac` reuse) plus
/// the job payload. Field names pinned to match the fleet server's
/// `content_search::SearchResultRequest` exactly.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResultReport {
    pub device_id: String,
    pub ts: i64,
    pub nonce: String,
    pub hmac_version: i64,
    pub hmac: String,
    pub job_id: String,
    pub hits: Vec<SearchHit>,
    pub error: Option<String>,
}

/// A command signed by the fleet server's ed25519 key.
///
/// The ed25519 signature covers the canonical JSON preimage produced by
/// [`fleet_proto::canonical_command_bytes`] (alphabetically-sorted object keys,
/// no spaces):
///
/// ```json
/// {"action_class":"…","catalog_id":"…","command_id":"…","device_id":"…","epoch_version":N,"payload":{…}}
/// ```
///
/// `action_class` wire strings: `"safe"`, `"destructive"`, `"irreversible"`.
#[derive(Debug, Clone, Deserialize)]
pub struct SignedCommand {
    /// Unique command id (assigned by the server). Delivery/dedup/ack id
    /// ONLY — NOT used in the signature preimage (see `idempotency_key`).
    pub command_id: String,
    /// The stable id the signer actually signed over (operator's offline
    /// signing tool, or the server's own signing path, both bind
    /// `idempotency_key` — see `fleet_proto::SignedCommand` doc). Used in the
    /// signature preimage in place of `command_id`, which is unknown to an
    /// offline operator at signing time.
    pub idempotency_key: String,
    /// Device UUID string (must match the enrolled device_id). Used in the preimage.
    pub device_id: String,
    /// Catalog id — references a COMMAND_CATALOG entry, never a raw command string.
    pub catalog_id: String,
    /// Risk class wire string: `"safe"`, `"destructive"`, or `"irreversible"`.
    pub action_class: String,
    /// Arbitrary JSON payload for the command (passed to the platform action).
    #[serde(default)]
    pub payload: Value,
    /// Monotonically-increasing config epoch version (anti-rollback).
    pub epoch_version: i64,
    /// Base64-encoded ed25519 signature over the canonical preimage.
    pub signature: String,
    /// Base64-encoded ed25519 verifying key that produced `signature`.
    /// The agent pins this key at enroll time and refuses any change.
    pub signer_key: String,
    /// Unix seconds — freshness check (must be within ±window of local clock).
    /// Carried outside the signed payload so the agent can check staleness
    /// without a round-trip and the server can set it independently.
    #[serde(default)]
    pub ts: i64,
    /// Single-use nonce — replay defence.
    #[serde(default)]
    pub nonce: String,
    /// Human-readable scope string (e.g. `"raise_posture"`, `"duress_seal"`,
    /// `"duress_wipe"`, `"all_clear"`). Used by the agent to route the command
    /// to the right platform action after signature verification.
    ///
    /// `#[serde(default)]`: the server's `fleet_proto::SignedCommand` does NOT
    /// carry `scope` (for operator commands `scope == catalog_id`), so it
    /// arrives empty and the dispatcher falls back to `catalog_id` — see
    /// `dispatch::process_checkin`. Without this default, a non-empty `commands`
    /// array from the real server fails to deserialize (`missing field scope`),
    /// silently turning every command-carrying check-in into a network error.
    #[serde(default)]
    pub scope: String,
}

// ── Signature verification ────────────────────────────────────────────────────

/// Decode a base64-encoded ed25519 verifying key.
pub fn decode_verifying_key(b64: &str) -> Result<VerifyingKey, String> {
    let bytes = B64
        .decode(b64)
        .map_err(|e| format!("fleet pubkey base64 decode: {e}"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "fleet pubkey is not 32 bytes".to_string())?;
    VerifyingKey::from_bytes(&arr).map_err(|e| format!("fleet pubkey invalid: {e}"))
}

/// Verify a [`SignedCommand`] against the pinned verifying key.
///
/// Checks (in order):
/// 1. `signer_key` matches the pinned key (key-swap prevention).
/// 2. Timestamp is within `max_skew_secs` of `now`.
/// 3. ed25519 signature over `fleet_proto::canonical_command_bytes(…)` is valid
///    (verified via `fleet_proto::verify_signature_b64` — no local crypto).
/// 4. Nonce has not been seen before (replay defence).
pub fn verify_command(
    cmd: &SignedCommand,
    key: &VerifyingKey,
    now: i64,
    max_skew_secs: i64,
    seen_nonces: &mut HashMap<String, i64>,
) -> Result<(), String> {
    // 1. Signer-key pinning: `cmd.signer_key` must encode the same key as `key`.
    let cmd_key = decode_verifying_key(&cmd.signer_key)
        .map_err(|e| format!("fleet command signer_key decode: {e}"))?;
    if cmd_key.to_bytes() != key.to_bytes() {
        return Err("fleet command signer_key does not match pinned pubkey".to_string());
    }

    // 2. Timestamp freshness.
    let skew = if cmd.ts != 0 { (now - cmd.ts).abs() } else { 0 };
    if cmd.ts != 0 && skew > max_skew_secs {
        return Err(format!(
            "fleet command timestamp skew {skew}s exceeds max {max_skew_secs}s"
        ));
    }

    // 3. ed25519 signature over the canonical preimage — delegated to fleet-proto
    //    (SSOT for both the byte layout and the raw verify).
    //
    //    P0 fix: the preimage MUST be rebuilt from `idempotency_key`, NOT
    //    `command_id`. The signer (operator's offline tool, or the server)
    //    signs over the stable idempotency_key because the server-assigned
    //    UUID (`command_id`) does not exist yet at signing time. Using
    //    `command_id` here made every duress-wipe signature fail to verify
    //    on real devices (see fleet_proto::SignedCommand doc).
    let msg = fleet_proto::canonical_command_bytes(
        &cmd.idempotency_key,
        &cmd.device_id,
        &cmd.catalog_id,
        &cmd.action_class,
        &cmd.payload,
        cmd.epoch_version,
    );
    let pinned_key_b64 = B64.encode(key.to_bytes());
    if !fleet_proto::verify_signature_b64(&pinned_key_b64, &msg, &cmd.signature) {
        return Err("fleet command ed25519 signature invalid".to_string());
    }

    // 4. Nonce replay defence.
    //    Nonce may be empty for server implementations that don't set it;
    //    only check replay when non-empty.
    if !cmd.nonce.is_empty() {
        if seen_nonces.contains_key(&cmd.nonce) {
            return Err(format!("fleet command nonce '{}' replayed", cmd.nonce));
        }
        seen_nonces.insert(cmd.nonce.clone(), now);
    }

    Ok(())
}

/// Evict nonces older than `ttl_secs` from `seen_nonces`.
///
/// Call this periodically (e.g. once per check-in cycle) to bound memory growth.
/// A nonce is safe to evict once the server's replay window has passed; using the
/// same `ttl_secs` as `max_skew_secs` is the correct choice.
pub fn evict_stale_nonces(seen_nonces: &mut HashMap<String, i64>, now: i64, ttl_secs: i64) {
    seen_nonces.retain(|_, &mut inserted_at| now - inserted_at <= ttl_secs);
}

// ── Check-in HMAC ─────────────────────────────────────────────────────────────

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

/// Compute the check-in HMAC per the fleet server spec:
///
/// ```text
/// preimage = "{device_id}:{ts}:{nonce}"   (UTF-8 bytes)
/// secret   = raw checkin_secret bytes     (NOT hashed)
/// HMAC     = HMAC-SHA256(secret, preimage)
/// output   = STANDARD base64(HMAC output) — 44 chars
/// ```
pub fn compute_checkin_hmac(secret: &[u8], device_id: &str, ts: i64, nonce: &str) -> String {
    let preimage = format!("{device_id}:{ts}:{nonce}");
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(preimage.as_bytes());
    B64.encode(mac.finalize().into_bytes())
}

fn normalize_json_number(input: &str) -> Result<String, String> {
    if input.len() > 1_024 {
        return Err("JSON number token exceeds canonicalization limit".to_string());
    }
    let (negative, unsigned) = input
        .strip_prefix('-')
        .map_or((false, input), |rest| (true, rest));
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => {
            let parsed = exponent
                .parse::<i32>()
                .map_err(|_| "invalid JSON number exponent".to_string())?;
            if parsed.unsigned_abs() > 1_000 {
                return Err("JSON number exponent exceeds canonicalization limit".to_string());
            }
            (mantissa, parsed)
        }
        None => (unsigned, 0),
    };
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{integer}{fraction}");
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("JSON number exceeds canonicalization limit".to_string());
    }
    let Some(first) = digits.bytes().position(|byte| byte != b'0') else {
        return Ok("0".to_string());
    };
    let last = digits
        .bytes()
        .rposition(|byte| byte != b'0')
        .expect("first non-zero digit exists");
    let significant = &digits[first..=last];
    let decimal_position = i64::try_from(integer.len())
        .map_err(|_| "JSON number exceeds canonicalization limit".to_string())?
        + i64::from(exponent)
        - i64::try_from(first)
            .map_err(|_| "JSON number exceeds canonicalization limit".to_string())?;
    let mut normalized = if decimal_position <= 0 {
        let zeros = usize::try_from(-decimal_position)
            .map_err(|_| "JSON number exceeds canonicalization limit".to_string())?;
        format!("0.{}{significant}", "0".repeat(zeros))
    } else {
        let position = usize::try_from(decimal_position)
            .map_err(|_| "JSON number exceeds canonicalization limit".to_string())?;
        if position >= significant.len() {
            format!("{significant}{}", "0".repeat(position - significant.len()))
        } else {
            format!("{}.{}", &significant[..position], &significant[position..])
        }
    };
    if negative {
        normalized.insert(0, '-');
    }
    Ok(normalized)
}

fn canonical_json(value: &Value, output: &mut String) -> Result<(), String> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&normalize_json_number(&value.to_string())?),
        Value::String(value) => output.push_str(
            &serde_json::to_string(value)
                .map_err(|error| format!("request HMAC string serialization failed: {error}"))?,
        ),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                canonical_json(value, output)?;
            }
            output.push(']');
        }
        Value::Object(object) => {
            output.push('{');
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key).map_err(|error| {
                    format!("request HMAC object-key serialization failed: {error}")
                })?);
                output.push(':');
                canonical_json(&object[key], output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

/// Canonical v2 device-request preimage:
/// `fleet-hmac-v2\n<METHOD>\n<path>\n<canonical JSON without hmac>`.
/// Object keys are recursively sorted; arrays retain their order.
pub fn request_hmac_preimage_v2<T: Serialize>(
    method: &str,
    path: &str,
    payload: &T,
) -> Result<Vec<u8>, String> {
    let mut value = serde_json::to_value(payload)
        .map_err(|error| format!("request HMAC payload serialization failed: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "request HMAC payload must be a JSON object".to_string())?;
    object.remove("hmac");
    let mut canonical = String::new();
    canonical_json(&value, &mut canonical)?;
    Ok(format!(
        "fleet-hmac-v2\n{}\n{}\n{}",
        method.to_ascii_uppercase(),
        path,
        canonical
    )
    .into_bytes())
}

pub fn compute_request_hmac_v2<T: Serialize>(
    secret: &[u8],
    method: &str,
    path: &str,
    payload: &T,
) -> Result<String, String> {
    let preimage = request_hmac_preimage_v2(method, path, payload)?;
    let mut mac =
        HmacSha256::new_from_slice(secret).map_err(|_| "request HMAC key error".to_string())?;
    mac.update(&preimage);
    Ok(B64.encode(mac.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Signer, SigningKey};

    fn test_signing_key() -> SigningKey {
        SigningKey::generate(&mut rand::rngs::OsRng)
    }

    /// Builds a `SignedCommand` the way the REAL system does: the signer signs
    /// over `idempotency_key` (the only stable id it has), while `command_id`
    /// is a SEPARATE, independently-chosen delivery/dedup/ack id (mirroring
    /// the server assigning a fresh UUID after the operator/server already
    /// signed offline over the idempotency key). Deliberately NOT reusing one
    /// string for both — a test that does so cannot distinguish "verifies
    /// correctly" from "verifies only because both ids happened to match",
    /// which is exactly how the P0 preimage-mismatch bug hid from every prior
    /// test in this file.
    #[allow(clippy::too_many_arguments)]
    fn make_signed_command(
        key: &SigningKey,
        scope: &str,
        payload: Value,
        ts: i64,
        nonce: &str,
        command_id: &str,
        idempotency_key: &str,
        device_id: &str,
        catalog_id: &str,
        action_class: &str,
        epoch_version: i64,
    ) -> SignedCommand {
        let msg = fleet_proto::canonical_command_bytes(
            idempotency_key,
            device_id,
            catalog_id,
            action_class,
            &payload,
            epoch_version,
        );
        let sig: Signature = key.sign(&msg);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        SignedCommand {
            command_id: command_id.to_string(),
            idempotency_key: idempotency_key.to_string(),
            device_id: device_id.to_string(),
            catalog_id: catalog_id.to_string(),
            action_class: action_class.to_string(),
            payload,
            epoch_version,
            signature: B64.encode(sig.to_bytes()),
            signer_key: pubkey_b64,
            ts,
            nonce: nonce.to_string(),
            scope: scope.to_string(),
        }
    }

    fn make_cmd(key: &SigningKey, scope: &str, nonce: &str, ts: i64) -> SignedCommand {
        make_signed_command(
            key,
            scope,
            serde_json::json!({}),
            ts,
            nonce,
            "cmd-test-server-uuid",
            "idem-test-key",
            "dev-test",
            "lc.cascade",
            "irreversible",
            1,
        )
    }

    // ── Health snapshot wire shape ────────────────────────────────────────────
    //
    // These pin `HealthSnapshot`'s JSON shape to exactly what the fleet
    // server's `device_health::HealthSnapshot`
    // (`fleet-server/src/routes/device_health.rs`) deserializes: field names
    // `encryption_on` / `patch_state` / `av_on` / `os_version` /
    // `sovereignty_score`, values null-safe on a per-field basis.

    #[test]
    fn health_snapshot_serializes_to_server_expected_shape() {
        let health = HealthSnapshot {
            encryption_on: Some(true),
            patch_state: Some("current".to_string()),
            av_on: Some(true),
            os_version: Some("Windows 11 Pro 24H2".to_string()),
            sovereignty_score: Some(82),
        };
        let json = serde_json::to_value(&health).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "encryption_on": true,
                "patch_state": "current",
                "av_on": true,
                "os_version": "Windows 11 Pro 24H2",
                "sovereignty_score": 82
            }),
            "field names/values must match the server's HealthSnapshot exactly"
        );
    }

    #[test]
    fn health_snapshot_unavailable_probes_serialize_as_explicit_null_not_omitted() {
        // "Gather best-effort: any probe that is expensive/unavailable -> send
        // null for that field, never block or fail the check-in." Each inner
        // field is a plain `Option<T>` (no `skip_serializing_if`), so an
        // all-None snapshot round-trips as explicit JSON nulls the server's
        // `#[derive(Deserialize)]` `Option<T>` fields accept natively.
        let health = HealthSnapshot::default();
        let json = serde_json::to_value(&health).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "encryption_on": null,
                "patch_state": null,
                "av_on": null,
                "os_version": null,
                "sovereignty_score": null
            })
        );
    }

    #[test]
    fn checkin_request_omits_health_key_entirely_when_no_collector() {
        // Byte-identical to a pre-health check-in when the platform has no
        // health collector wired up (`sample_health` default `None`) — same
        // `skip_serializing_if` discipline as `resources`.
        let req = CheckinRequest {
            device_id: "dev-1".to_string(),
            hostname: "host".to_string(),
            posture: "nominal".to_string(),
            ts: 1_700_000_000,
            nonce: "n".to_string(),
            hmac_version: HMAC_VERSION_V2,
            hmac: "h".to_string(),
            padding: String::new(),
            decoy: false,
            resources: None,
            health: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(
            json.get("health").is_none(),
            "health key must be OMITTED (not null) when the platform has no collector"
        );
    }

    #[test]
    fn checkin_request_carries_partial_health_when_some_probes_fail() {
        // Simulates a real best-effort gather: only encryption_on and
        // os_version were readable this cycle; patch_state/av_on/
        // sovereignty_score stayed None. The whole snapshot must still be
        // sent — a single failed probe degrades only its own field, not the
        // rest of the report.
        let health = HealthSnapshot {
            encryption_on: Some(true),
            patch_state: None,
            av_on: None,
            os_version: Some("Windows 11 Pro 24H2".to_string()),
            sovereignty_score: None,
        };
        let req = CheckinRequest {
            device_id: "dev-1".to_string(),
            hostname: "host".to_string(),
            posture: "nominal".to_string(),
            ts: 1_700_000_000,
            nonce: "n".to_string(),
            hmac_version: HMAC_VERSION_V2,
            hmac: "h".to_string(),
            padding: String::new(),
            decoy: false,
            resources: None,
            health: Some(health),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["health"]["encryption_on"], serde_json::json!(true));
        assert_eq!(json["health"]["patch_state"], serde_json::Value::Null);
        assert_eq!(json["health"]["av_on"], serde_json::Value::Null);
        assert_eq!(
            json["health"]["os_version"],
            serde_json::json!("Windows 11 Pro 24H2")
        );
        assert_eq!(json["health"]["sovereignty_score"], serde_json::Value::Null);
    }

    // ── check-in HMAC tests ───────────────────────────────────────────────────

    #[test]
    fn checkin_hmac_matches_independently_computed_value() {
        let secret = b"fleet-checkin-secret-32-bytes-ok";
        let device_id = "550e8400-e29b-41d4-a716-446655440000";
        let ts: i64 = 1_700_000_000;
        let nonce = "ci-deadbeef1234";

        let preimage = format!("{device_id}:{ts}:{nonce}");
        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        mac.update(preimage.as_bytes());
        let expected = B64.encode(mac.finalize().into_bytes());

        let produced = compute_checkin_hmac(secret, device_id, ts, nonce);
        assert_eq!(
            produced, expected,
            "HMAC must match independent computation"
        );
        assert_eq!(produced.len(), 44);
    }

    #[test]
    fn checkin_hmac_changes_with_each_input() {
        let secret = b"some-secret-bytes-here-for-test!";
        let base = compute_checkin_hmac(secret, "dev-1", 1000, "nonce-a");
        assert_ne!(
            base,
            compute_checkin_hmac(secret, "dev-2", 1000, "nonce-a"),
            "device_id change"
        );
        assert_ne!(
            base,
            compute_checkin_hmac(secret, "dev-1", 1001, "nonce-a"),
            "ts change"
        );
        assert_ne!(
            base,
            compute_checkin_hmac(secret, "dev-1", 1000, "nonce-b"),
            "nonce change"
        );
        let other_secret = b"other-secret-bytes-here-for-tst!";
        assert_ne!(
            base,
            compute_checkin_hmac(other_secret, "dev-1", 1000, "nonce-a"),
            "secret change"
        );
    }

    #[test]
    fn request_hmac_v2_binds_method_path_and_payload_except_hmac() {
        let secret = b"request-body-secret";
        let base = serde_json::json!({
            "device_id": "dev-1", "ts": 1000, "nonce": "nonce-a",
            "hmac_version": 2, "hmac": "ignored-a", "decoy": false,
            "acks": [{"command_id":"cmd-1","success":false}]
        });
        let mac = compute_request_hmac_v2(secret, "POST", "/v1/agents/checkin", &base).unwrap();

        let mut changed_hmac = base.clone();
        changed_hmac["hmac"] = serde_json::json!("ignored-b");
        assert_eq!(
            mac,
            compute_request_hmac_v2(secret, "POST", "/v1/agents/checkin", &changed_hmac).unwrap()
        );

        for changed in [
            serde_json::json!({"device_id":"dev-1","ts":1000,"nonce":"nonce-a","hmac_version":2,"hmac":"ignored-a","decoy":true,"acks":[{"command_id":"cmd-1","success":false}]}),
            serde_json::json!({"device_id":"dev-1","ts":1000,"nonce":"nonce-a","hmac_version":2,"hmac":"ignored-a","decoy":false,"acks":[{"command_id":"cmd-1","success":true}]}),
        ] {
            assert_ne!(
                mac,
                compute_request_hmac_v2(secret, "POST", "/v1/agents/checkin", &changed).unwrap()
            );
        }
        assert_ne!(
            mac,
            compute_request_hmac_v2(secret, "GET", "/v1/agents/checkin", &base).unwrap()
        );
        assert_ne!(
            mac,
            compute_request_hmac_v2(secret, "POST", "/v1/agents/duress-event", &base).unwrap()
        );
    }

    #[test]
    fn request_hmac_v2_matches_cross_repo_golden_vector() {
        let body = serde_json::json!({
            "device_id": "dev-1",
            "ts": 1_700_000_000_i64,
            "nonce": "nonce-a",
            "hmac_version": 2,
            "hmac": "ignored",
            "decoy": false,
            "acks": [{"command_id": "cmd-1", "success": false}],
        });
        assert_eq!(
            compute_request_hmac_v2(b"test-secret", "POST", "/v1/agents/checkin", &body).unwrap(),
            "SUK6rX+K24Phz6abnI/V1m0oiQ8giDP4pILVEHxs3HY="
        );
    }

    #[test]
    fn request_hmac_v2_cross_language_strings_and_numbers_golden_vector() {
        let body = serde_json::json!({
            "device_id": "dev/1",
            "ts": 1_700_000_000_i64,
            "nonce": "nonce-a",
            "hmac_version": 2,
            "hmac": "ignored",
            "path": "build/device/blazer",
            "unicode": "café雪",
            "control": "line\n\t",
            "numbers": [1, 1.0, 1.25e2, -0.0, 1.2e-3],
        });
        let preimage = request_hmac_preimage_v2("POST", "/v1/agents/checkin", &body).unwrap();
        assert_eq!(
            String::from_utf8(preimage).unwrap(),
            "fleet-hmac-v2\nPOST\n/v1/agents/checkin\n{\"control\":\"line\\n\\t\\u0001\",\"device_id\":\"dev/1\",\"hmac_version\":2,\"nonce\":\"nonce-a\",\"numbers\":[1,1,125,0,0.0012],\"path\":\"build/device/blazer\",\"ts\":1700000000,\"unicode\":\"café雪\"}"
        );
        assert_eq!(
            compute_request_hmac_v2(b"test-secret", "POST", "/v1/agents/checkin", &body).unwrap(),
            "nvu2PQQzcwFNQLuX6MlOkdkil8OTwilR1OE7SKVuiC4="
        );
    }

    // ── verify_command tests ──────────────────────────────────────────────────

    #[test]
    fn valid_command_passes_verification() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;
        let cmd = make_cmd(&key, "all_clear", "nonce-1", now);
        let mut seen = HashMap::new();
        assert!(verify_command(&cmd, &vk, now, 300, &mut seen).is_ok());
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;
        let mut cmd = make_cmd(&key, "all_clear", "nonce-2", now);
        cmd.signature = B64.encode([0u8; 64]);
        let mut seen = HashMap::new();
        assert!(verify_command(&cmd, &vk, now, 300, &mut seen).is_err());
    }

    #[test]
    fn stale_timestamp_is_rejected() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let ts = 1_700_000_000_i64;
        let cmd = make_cmd(&key, "all_clear", "nonce-3", ts);
        let mut seen = HashMap::new();
        let now = ts + 600;
        assert!(verify_command(&cmd, &vk, now, 300, &mut seen).is_err());
    }

    #[test]
    fn replayed_nonce_is_rejected() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;
        let cmd = make_cmd(&key, "all_clear", "nonce-4", now);
        let mut seen = HashMap::new();

        assert!(verify_command(&cmd, &vk, now, 300, &mut seen).is_ok());
        assert!(verify_command(&cmd, &vk, now, 300, &mut seen).is_err());
    }

    #[test]
    fn wrong_key_is_rejected() {
        let key = test_signing_key();
        let other_key = test_signing_key();
        let now = 1_700_000_000_i64;
        let cmd = make_cmd(&key, "all_clear", "nonce-6", now);
        let wrong_vk = other_key.verifying_key();
        let mut seen = HashMap::new();
        assert!(verify_command(&cmd, &wrong_vk, now, 300, &mut seen).is_err());
    }

    #[test]
    fn signer_key_mismatch_is_rejected() {
        let key = test_signing_key();
        let other_key = test_signing_key();
        let now = 1_700_000_000_i64;
        let mut cmd = make_cmd(&key, "all_clear", "nonce-7", now);
        cmd.signer_key = B64.encode(other_key.verifying_key().to_bytes());
        let wrong_vk = other_key.verifying_key();
        let mut seen = HashMap::new();
        assert!(verify_command(&cmd, &wrong_vk, now, 300, &mut seen).is_err());
    }

    // ── Seen-nonce TTL eviction test ──────────────────────────────────────────

    #[test]
    fn stale_nonces_are_evicted_by_ttl() {
        let mut seen: HashMap<String, i64> = HashMap::new();
        let t0 = 1_000_000i64;
        seen.insert("old-nonce".to_string(), t0);
        seen.insert("recent-nonce".to_string(), t0 + 290);

        evict_stale_nonces(&mut seen, t0 + 310, 300);
        assert!(!seen.contains_key("old-nonce"), "old nonce must be evicted");
        assert!(
            seen.contains_key("recent-nonce"),
            "recent nonce must be kept"
        );
    }

    #[test]
    fn nonce_growth_is_bounded_across_cycles() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let mut seen: HashMap<String, i64> = HashMap::new();
        let base_ts = 1_700_000_000i64;

        for i in 0..200i64 {
            let ts = base_ts + i * 60;
            let nonce = format!("nonce-cycle-{i}");
            let cmd = make_cmd(&key, "all_clear", &nonce, ts);
            let _ = verify_command(&cmd, &vk, ts, 300, &mut seen);
            evict_stale_nonces(&mut seen, ts, 300);
        }
        assert!(
            seen.len() <= 10,
            "seen_nonces must not grow unboundedly; got {} entries",
            seen.len()
        );
    }

    // ── Interop test: verify against fleet-proto's own signing format ────────

    #[test]
    fn command_signed_with_server_canonical_format_verifies_correctly() {
        let key = test_signing_key();
        let now = 1_700_000_000_i64;

        let command_id = "cmd-1234";
        let device_id = "dev-5678";
        let catalog_id = "lc.duress_wipe";
        let action_class = "irreversible";
        let payload = serde_json::json!({});
        let epoch_version = 7i64;

        let msg = fleet_proto::canonical_command_bytes(
            command_id,
            device_id,
            catalog_id,
            action_class,
            &payload,
            epoch_version,
        );
        let sig: Signature = key.sign(&msg);

        let cmd = SignedCommand {
            command_id: command_id.to_string(),
            idempotency_key: command_id.to_string(),
            device_id: device_id.to_string(),
            catalog_id: catalog_id.to_string(),
            action_class: action_class.to_string(),
            payload,
            epoch_version,
            signature: B64.encode(sig.to_bytes()),
            signer_key: B64.encode(key.verifying_key().to_bytes()),
            ts: now,
            nonce: "server-nonce-abc".to_string(),
            scope: "duress_wipe".to_string(),
        };

        let vk = key.verifying_key();
        let mut seen = HashMap::new();
        assert!(
            verify_command(&cmd, &vk, now, 300, &mut seen).is_ok(),
            "command signed with fleet-proto's canonical format must verify"
        );
    }

    /// ── P0 REGRESSION: the real duress-wipe flow, end to end ─────────────────
    ///
    /// This is the test the nation-state security review found missing: every
    /// prior test signed and verified with the SAME self-consistent id, so the
    /// preimage-mismatch bug never surfaced. This test simulates the REAL
    /// sequence:
    ///   1. The operator signs OFFLINE over `idempotency_key = K` (the only
    ///      stable id available before the command exists server-side) —
    ///      exactly what `tools/sign-duress-command.rs` does.
    ///   2. The server creates the command with a FRESH UUID `command_id`
    ///      (distinct from `K`) and dispatches
    ///      `SignedCommand { command_id: UUID, idempotency_key: K, signature }`.
    ///   3. The on-device verifier (this crate) rebuilds the preimage from
    ///      `idempotency_key` and the signature MUST verify.
    #[test]
    fn real_duress_wipe_flow_operator_signs_idempotency_key_server_assigns_different_uuid() {
        let operator_key = test_signing_key();
        let now = 1_700_000_000_i64;

        // Step 1: operator signs offline over the idempotency_key ONLY — the
        // server-assigned UUID does not exist yet.
        let idempotency_key = "duress-wipe-2026-07-05-ab12cd34";
        let device_id = "dev-real-device";
        let catalog_id = "duress_wipe";
        let action_class = "irreversible";
        let payload = serde_json::json!({});
        let epoch_version = 3i64;

        let operator_preimage = fleet_proto::canonical_command_bytes(
            idempotency_key,
            device_id,
            catalog_id,
            action_class,
            &payload,
            epoch_version,
        );
        let operator_signature: Signature = operator_key.sign(&operator_preimage);

        // Step 2: the server assigns a FRESH UUID as command_id — genuinely
        // different from idempotency_key, exactly as it does in production
        // (`CommandRecord.command_id` is a server-generated UUID; see
        // `fleet-server/src/store/memory.rs` / `pg.rs`).
        let server_assigned_command_id = "550e8400-e29b-41d4-a716-446655440099";
        assert_ne!(
            server_assigned_command_id, idempotency_key,
            "precondition: the UUID and the signed id must be different, \
             or this test cannot distinguish the fix from the bug"
        );

        let dispatched = SignedCommand {
            command_id: server_assigned_command_id.to_string(),
            idempotency_key: idempotency_key.to_string(),
            device_id: device_id.to_string(),
            catalog_id: catalog_id.to_string(),
            action_class: action_class.to_string(),
            payload,
            epoch_version,
            signature: B64.encode(operator_signature.to_bytes()),
            signer_key: B64.encode(operator_key.verifying_key().to_bytes()),
            ts: now,
            nonce: "duress-nonce-real-flow".to_string(),
            scope: "duress_wipe".to_string(),
        };

        // Step 3: the on-device verifier must accept this — proving the wipe
        // signature actually verifies under the real dispatch shape.
        let vk = operator_key.verifying_key();
        let mut seen = HashMap::new();
        assert!(
            verify_command(&dispatched, &vk, now, 300, &mut seen).is_ok(),
            "duress_wipe signed offline over idempotency_key must verify even though \
             the dispatched command_id is a different, server-assigned UUID"
        );
    }

    /// ── P0 NEGATIVE: a signature computed over `command_id` (the UUID) must
    /// fail. This is the exact shape of the original bug: if a signer (or a
    /// stale build of the signing tool) mistakenly signs over the delivery
    /// UUID instead of `idempotency_key`, on-device verification must REFUSE
    /// it, not silently accept — silent acceptance here would mean the
    /// verifier is no longer binding the signature to the field it claims to.
    #[test]
    fn signature_computed_over_command_id_instead_of_idempotency_key_is_rejected() {
        let operator_key = test_signing_key();
        let now = 1_700_000_000_i64;

        let idempotency_key = "duress-wipe-idem-key";
        let server_assigned_command_id = "550e8400-e29b-41d4-a716-446655440042";
        let device_id = "dev-real-device";
        let catalog_id = "duress_wipe";
        let action_class = "irreversible";
        let payload = serde_json::json!({});
        let epoch_version = 3i64;

        // Sign over command_id (the WRONG field) — this is the bug we are
        // guarding against ever silently working again.
        let wrong_preimage = fleet_proto::canonical_command_bytes(
            server_assigned_command_id,
            device_id,
            catalog_id,
            action_class,
            &payload,
            epoch_version,
        );
        let wrong_signature: Signature = operator_key.sign(&wrong_preimage);

        let dispatched = SignedCommand {
            command_id: server_assigned_command_id.to_string(),
            idempotency_key: idempotency_key.to_string(),
            device_id: device_id.to_string(),
            catalog_id: catalog_id.to_string(),
            action_class: action_class.to_string(),
            payload,
            epoch_version,
            signature: B64.encode(wrong_signature.to_bytes()),
            signer_key: B64.encode(operator_key.verifying_key().to_bytes()),
            ts: now,
            nonce: "duress-nonce-negative".to_string(),
            scope: "duress_wipe".to_string(),
        };

        let vk = operator_key.verifying_key();
        let mut seen = HashMap::new();
        assert!(
            verify_command(&dispatched, &vk, now, 300, &mut seen).is_err(),
            "a signature computed over command_id (not idempotency_key) must be rejected"
        );
    }

    #[test]
    fn canonical_command_bytes_golden_vector_matches_fleet_proto() {
        // Sanity check: fleet-agent-core does not reimplement canonical bytes —
        // it must produce identical output to fleet_proto for the same inputs
        // (this is implicit since we call fleet_proto directly, but pin it here
        // so a future refactor that reintroduces a local copy is caught).
        let bytes = fleet_proto::canonical_command_bytes(
            "cmd-abc",
            "dev-xyz",
            "status.read",
            "safe",
            &serde_json::json!({}),
            3,
        );
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "7b22616374696f6e5f636c617373223a2273616665222c22636174616c6f675f6964223a227374617475732e72656164222c22636f6d6d616e645f6964223a22636d642d616263222c226465766963655f6964223a226465762d78797a222c2265706f63685f76657273696f6e223a332c227061796c6f6164223a7b7d7d",
            "must match the fleet-proto golden vector"
        );
    }
}
