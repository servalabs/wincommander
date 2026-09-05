//! The background enroll/check-in HTTP loop. Gated behind `feature = "transport"`
//! — this is the part of the crate that pulls in `reqwest`/`tokio`; a `types`-only
//! consumer never sees this module.

use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use fleet_proto::{verify_signature_b64, PolicyEnvelope};
use serde::Serialize;
use tracing::{debug, info, warn};

use crate::config::FleetConfig;
use crate::dispatch::{execute_pending_search_jobs, process_checkin, FleetActions};
use crate::state::SharedFleetState;
use crate::util::now_rfc3339;
use crate::verify::{
    compute_request_hmac_v2, decode_verifying_key, CheckinRequest, CheckinResponse, EnrollRequest,
    EnrollResponse, SearchResultReport, ENROLL_PROTOCOL_VERSION, HMAC_BODY_V2_CAPABILITY,
    HMAC_VERSION_V2,
};

// ── HTTP client helper ─────────────────────────────────────────────────────────

/// POST to the fleet server, returning the JSON response.
///
/// Best-effort: network errors are logged and propagated as `Err`.
pub async fn fleet_post<Req, Resp>(
    client: &reqwest::Client,
    url: &str,
    bearer: &str,
    body: &Req,
) -> Result<Resp, String>
where
    Req: Serialize,
    Resp: serde::de::DeserializeOwned,
{
    let resp = client
        .post(url)
        .bearer_auth(bearer)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("fleet POST {url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("fleet {url}: HTTP {}", resp.status()));
    }

    resp.json()
        .await
        .map_err(|e| format!("fleet {url} JSON: {e}"))
}

/// Is this HTTP status transient (worth a capped-exponential retry) — 5xx or 429 only.
/// 4xx (other than 429) is treated as a permanent rejection (bad auth, bad request, …).
pub fn is_transient_status(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// Capped-exponential backoff with full jitter.
///
/// `attempt` is 0-based. Returns a duration in `[0, min(cap, base * 2^attempt)]`.
pub fn backoff_duration(attempt: u32, base: Duration, cap: Duration) -> Duration {
    use rand::Rng;
    let exp = base.as_millis().saturating_mul(1u128 << attempt.min(20));
    let capped_ms = exp.min(cap.as_millis());
    let capped_ms_u64: u64 = capped_ms.try_into().unwrap_or(u64::MAX);
    let jittered = if capped_ms_u64 == 0 {
        0
    } else {
        rand::thread_rng().gen_range(0..=capped_ms_u64)
    };
    Duration::from_millis(jittered)
}

/// Generate a compact cryptographically-random nonce string for check-in requests.
fn checkin_nonce() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("ci-{}", hex::encode(bytes))
}

/// Mint a fresh, client-owned device identity (RFC 4122 v4 UUID string). The
/// device owns its identity in the unified check-in transport (the server
/// echoes it back at enroll); a UUIDv4 is unguessable, so it cannot be used to
/// enumerate or hijack other devices' ids.
fn new_device_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant 10xx
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Map the agent `platform` tag to the fleet server's `device_kind` enum value.
/// Unknown platforms pass through unchanged so the server's fail-closed
/// `DeviceKind::parse` rejects them rather than this silently guessing.
fn platform_device_kind(platform: &str) -> &str {
    match platform {
        "windows" => "wincommander",
        "linux" => "tuxcommander",
        other => other,
    }
}

/// Draw the NEXT check-in sleep duration from a genuinely randomized window,
/// not a fixed beacon with a small wobble on top.
///
/// Traffic-shaping (LIGHT tier): returns a value uniformly distributed in
/// `[interval * min_frac, interval * max_frac]` seconds, so the cadence
/// itself is not a fixed fingerprint a network observer could key on. Bounded
/// below at 1 second so a degenerate config (e.g. `min_frac == max_frac == 0`)
/// can never produce a busy-loop.
///
/// `min_frac`/`max_frac` are validated (min > 0, min <= max) by
/// `FleetConfig::from_env` before this is ever called with them, but this
/// function defends independently: an inverted/degenerate pair still produces
/// a sane bounded value rather than panicking `gen_range` on an empty/invalid
/// range.
fn randomized_interval_secs(interval: u64, min_frac: f64, max_frac: f64) -> u64 {
    use rand::Rng;

    let (lo_frac, hi_frac) = if min_frac > 0.0 && min_frac <= max_frac {
        (min_frac, max_frac)
    } else {
        (0.5, 1.5)
    };

    let lo = ((interval as f64) * lo_frac).round().max(1.0);
    let hi = ((interval as f64) * hi_frac).round().max(lo);

    if lo >= hi {
        return lo as u64;
    }
    rand::thread_rng().gen_range(lo as u64..=hi as u64)
}

/// Generate an opaque, cryptographically-random base64 padding string sized
/// so the check-in request lands in a fixed `target_bytes` bucket regardless
/// of what real content it carries.
///
/// `already_used_bytes` is a rough estimate of the non-padding JSON already in
/// the request (so padding only tops the body up to the bucket rather than
/// always adding the full `target_bytes`, which would make every request grow
/// unboundedly under a small `target_bytes`). When the real content already
/// meets or exceeds the bucket, returns an empty string (no padding — the
/// bucket is a floor, not a hard cap; `fleet-agent-core` never truncates real
/// data to hit a size target). `target_bytes == 0` disables padding entirely.
///
/// This is a pure size-shaping helper — the returned value is carried in
/// `CheckinRequest::padding`, which is covered by the v2 request MAC (see the
/// module doc on `CheckinRequest`).
fn make_padding(target_bytes: usize, already_used_bytes: usize) -> String {
    if target_bytes == 0 || already_used_bytes >= target_bytes {
        return String::new();
    }
    // Base64 (STANDARD, no padding stripped) expands 3 raw bytes -> 4 chars,
    // so ask for enough raw bytes to cover the remaining budget.
    let remaining = target_bytes - already_used_bytes;
    let raw_len = remaining.div_ceil(4) * 3;
    let mut bytes = vec![0u8; raw_len];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    B64.encode(bytes)
}

/// Best-effort hostname (falls back to `"unknown"`).
fn hostname_str() -> String {
    if let Ok(h) = std::env::var("HOSTNAME") {
        return h;
    }
    if let Ok(h) = std::env::var("COMPUTERNAME") {
        return h;
    }
    #[cfg(target_os = "linux")]
    if let Ok(s) = std::fs::read_to_string("/etc/hostname") {
        return s.trim().to_string();
    }
    "unknown".to_string()
}

// ── Enroll ──────────────────────────────────────────────────────────────────

/// Enroll this device with the fleet server, if not already enrolled.
///
/// On success, sets `fleet_state.device_id` and `fleet_state.pinned_pubkey_b64`.
/// Refuses if the server returns a different pubkey than already pinned.
///
/// `platform` is a short platform tag sent in the enroll request (e.g. `"linux"`,
/// `"windows"`); `agent_version` is the caller's own version string.
///
/// Returns `Ok(device_id)` on success, `Err` on network/auth failure.
pub async fn enroll(
    client: &reqwest::Client,
    config: &FleetConfig,
    fleet_state: &SharedFleetState,
    platform: &str,
    agent_version: &str,
) -> Result<String, String> {
    // Client-owned identity: if already enrolled, short-circuit; otherwise mint
    // a fresh UUIDv4 the server will echo back at enroll.
    let device_id = {
        let state = fleet_state.lock().unwrap();
        if let Some(ref id) = state.device_id {
            debug!("fleet: already enrolled as {id}");
            return Ok(id.clone());
        }
        new_device_id()
    };

    let hostname = hostname_str();
    // `device_hash == device_id`: on a self-hosted fleet the hash is only a
    // seat/blocklist + re-enroll-guard key, never an auth secret (the
    // per-device `checkin_secret` authenticates). No device public key — the
    // check-in transport never uses one.
    let req = EnrollRequest {
        device_id: device_id.clone(),
        device_hash: device_id.clone(),
        device_kind: platform_device_kind(platform).to_string(),
        hostname: hostname.clone(),
        platform: platform.to_string(),
        agent_version: agent_version.to_string(),
        protocol_version: ENROLL_PROTOCOL_VERSION,
        capabilities: vec![HMAC_BODY_V2_CAPABILITY.to_string()],
    };

    let url = format!("{}/v1/agents/enroll", config.url);
    let resp: EnrollResponse = fleet_post(client, &url, &config.enroll_token, &req).await?;

    // Dual-key pinning: pin the OPERATOR key (operator-control commands) and the
    // SERVER signing key (ordinary commands) independently. Each is pinned on first
    // sight and a silent change on re-enroll is REFUSED (only a verified
    // `rotate_key` may change a key). The operator key may legitimately be
    // absent (`None`) until the org configures it — an operator-control-only
    // agent then can't verify those commands yet (fail-closed at dispatch).
    {
        let mut state = fleet_state.lock().unwrap();
        if let Some(ref op_key) = resp.command_pubkey_b64 {
            if let Some(ref existing) = state.pinned_pubkey_b64 {
                if existing != op_key {
                    let msg = format!(
                        "fleet server returned a different operator command_pubkey on \
                         re-enroll — REFUSING (pinned={existing}, server={op_key})"
                    );
                    warn!("{msg}");
                    return Err(msg);
                }
            } else {
                decode_verifying_key(op_key)?;
                info!("fleet: operator command key pinned");
                state.pinned_pubkey_b64 = Some(op_key.clone());
            }
        }
        if let Some(ref srv_key) = resp.server_signing_key_b64 {
            if let Some(ref existing) = state.pinned_server_key_b64 {
                if existing != srv_key {
                    let msg = "fleet server returned a different server signing key on \
                               re-enroll — REFUSING"
                        .to_string();
                    warn!("{msg}");
                    return Err(msg);
                }
            } else {
                decode_verifying_key(srv_key)?;
                info!("fleet: server signing key pinned");
                state.pinned_server_key_b64 = Some(srv_key.clone());
            }
        }
        info!("fleet: enrolled as {}", resp.device_id);
        state.device_id = Some(resp.device_id.clone());

        // Store per-device checkin_secret returned by the server if present.
        if let Some(ref b64) = resp.checkin_secret_b64 {
            match B64.decode(b64) {
                Ok(secret_bytes) => {
                    info!(
                        "fleet: per-device checkin_secret stored ({} bytes)",
                        secret_bytes.len()
                    );
                    state.checkin_secret = Some(secret_bytes);
                }
                Err(e) => {
                    warn!("fleet: checkin_secret_b64 decode failed ({e}) — using config secret");
                }
            }
        }
    }

    Ok(resp.device_id)
}

// ── On-device content search ──────────────────────────────────────────────────

/// Report every dispatched search job's outcome to
/// `POST /v1/agents/search-result` — one POST per job, HMAC-authed exactly
/// like check-in (fresh `ts`/`nonce`/`hmac` per job, over the SAME
/// per-device `checkin_secret`; see [`SearchResultReport`]). Best-effort: a
/// POST failure for one job is logged and does not affect the others or the
/// caller — a search-result report failing to reach the server is no more
/// fatal to the check-in loop than a single dropped check-in.
pub async fn report_search_results(
    client: &reqwest::Client,
    config: &FleetConfig,
    device_id: &str,
    checkin_secret: &[u8],
    reports: Vec<crate::dispatch::SearchJobReport>,
) {
    let url = format!("{}/v1/agents/search-result", config.url);
    for report in reports {
        let now = crate::util::now_unix();
        let nonce = checkin_nonce();
        let mut body = SearchResultReport {
            device_id: device_id.to_string(),
            ts: now,
            nonce,
            hmac_version: HMAC_VERSION_V2,
            hmac: String::new(),
            job_id: report.job_id.clone(),
            hits: report.hits,
            error: report.error,
        };
        body.hmac = match compute_request_hmac_v2(
            checkin_secret,
            "POST",
            "/v1/agents/search-result",
            &body,
        ) {
            Ok(hmac) => hmac,
            Err(error) => {
                warn!(
                    "fleet: search-result report for job '{}' could not be authenticated: {error}",
                    report.job_id
                );
                continue;
            }
        };
        match client.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                debug!("fleet: search-result reported for job '{}'", report.job_id);
            }
            Ok(resp) => {
                warn!(
                    "fleet: search-result report for job '{}': HTTP {}",
                    report.job_id,
                    resp.status()
                );
            }
            Err(e) => {
                warn!(
                    "fleet: search-result report for job '{}' failed: {e}",
                    report.job_id
                );
            }
        }
    }
}

// ── Check-in cycle ────────────────────────────────────────────────────────────

/// Perform one check-in cycle: enroll-if-needed, POST check-in, process response.
///
/// Returns `true` if the check-in was successful (all_clear or commands processed ok),
/// `false` on network error. Dead-man accounting is always updated.
///
/// `decoy` marks this as a cover-traffic check-in (see
/// [`spawn_fleet_client`]/`FleetConfig::checkin_decoy_enabled`): it is
/// authenticated exactly like a real check-in (same HMAC, same padding
/// bucket) so it is indistinguishable on the wire, but the server is told via
/// `CheckinRequest::decoy` to skip recording anything sensitive for it. Use
/// [`run_checkin_cycle`] with `decoy = false` for the ordinary real-poll path.
#[allow(clippy::too_many_arguments)]
pub async fn run_checkin_cycle_inner(
    client: &reqwest::Client,
    config: &FleetConfig,
    fleet_state: &SharedFleetState,
    dispatch: &dyn FleetActions,
    now: i64,
    platform: &str,
    agent_version: &str,
    decoy: bool,
) -> bool {
    // 1. Ensure enrolled.
    let device_id = match enroll(client, config, fleet_state, platform, agent_version).await {
        Ok(id) => id,
        Err(e) => {
            warn!("fleet: enrollment failed: {e}");
            let misses = {
                let mut s = fleet_state.lock().unwrap();
                s.dead_man_misses += 1;
                s.dead_man_misses
            };
            dispatch.dead_man_miss(misses);
            return false;
        }
    };

    // 2. Resolve the check-in secret: per-device secret takes precedence.
    let checkin_secret: Vec<u8> = {
        let s = fleet_state.lock().unwrap();
        s.checkin_secret
            .clone()
            .unwrap_or_else(|| config.checkin_secret.clone())
    };

    // 3. Build HMAC-authenticated check-in request.
    //
    //    HMAC v2 binds the method, path, and complete canonical JSON body
    //    (excluding only the `hmac` field). Padding is sized against a rough
    //    estimate of the non-padding JSON so the WHOLE body (not just the
    //    padding value) lands near the configured bucket.
    let nonce = checkin_nonce();
    let hostname = hostname_str();
    let base_len_estimate =
        device_id.len() + hostname.len() + nonce.len() + 44 + 64 /* JSON scaffolding */;
    let padding = make_padding(config.checkin_padding_bytes, base_len_estimate);
    let url = format!("{}/v1/agents/checkin", config.url);
    // Resources/health are skipped entirely on a decoy check-in — a decoy is
    // cover traffic only and must never carry real device state (mirrors why
    // decoys never touch dead-man/posture/productivity state either).
    let resources = if decoy {
        None
    } else {
        dispatch.sample_resources()
    };
    let health = if decoy {
        None
    } else {
        dispatch.sample_health()
    };
    let mut req = CheckinRequest {
        device_id: device_id.clone(),
        hostname,
        posture: "nominal".to_string(),
        ts: now,
        nonce,
        hmac_version: HMAC_VERSION_V2,
        hmac: String::new(),
        padding,
        decoy,
        resources,
        health,
    };
    req.hmac = match compute_request_hmac_v2(&checkin_secret, "POST", "/v1/agents/checkin", &req) {
        Ok(hmac) => hmac,
        Err(error) => {
            warn!("fleet: could not authenticate check-in body: {error}");
            return false;
        }
    };
    let resp: CheckinResponse = match fleet_post(client, &url, &config.fleet_token, &req).await {
        Ok(r) => r,
        Err(e) => {
            warn!("fleet: check-in failed: {e}");
            let misses = {
                let mut s = fleet_state.lock().unwrap();
                s.dead_man_misses += 1;
                s.dead_man_misses
            };
            dispatch.dead_man_miss(misses);
            return false;
        }
    };

    // A decoy check-in is pure cover traffic: it is authenticated and sent
    // exactly like a real check-in (same HMAC, same padding bucket), but a
    // decoy-aware server returns no commands and no meaningful `all_clear`
    // for it (see `fleet-server`'s `checkin` handler). Folding that response
    // into command dispatch or dead-man accounting would let cover traffic
    // corrupt the REAL dead-man clock (e.g. a server that always answers
    // decoys with `all_clear: false` would inject spurious misses). Decoys
    // therefore stop here, after the authenticated round-trip completes —
    // the round-trip itself is what makes them indistinguishable on the wire;
    // nothing past that point should observably differ from a real poll's
    // network shape, but it must not feed back into local state.
    if decoy {
        debug!("fleet: decoy check-in completed (cover traffic, no state applied)");
        return true;
    }

    // 4. Process commands (dual-key): operator-control commands verify against
    //    the OPERATOR key (the provisioned `config.cmd_pubkey`), ordinary
    //    server-signed commands against the SERVER key (pinned at enroll). The
    //    catalog→key routing (`fleet_proto::is_duress_catalog`) is fixed and
    //    local, so the server can never forge an operator command and an
    //    operator-signed blob can't masquerade as an ordinary one.
    let operator_key = config.cmd_pubkey;
    let pinned_server_key_b64 = {
        let s = fleet_state.lock().unwrap();
        s.pinned_server_key_b64.clone()
    };
    let server_key = pinned_server_key_b64
        .as_deref()
        .and_then(|b| decode_verifying_key(b).ok());
    let mut seen_nonces = {
        let s = fleet_state.lock().unwrap();
        s.seen_nonces.clone()
    };
    process_checkin(
        &resp,
        Some(&operator_key),
        server_key.as_ref(),
        now,
        config.max_cmd_skew_secs,
        &mut seen_nonces,
        dispatch,
    );
    {
        let mut s = fleet_state.lock().unwrap();
        s.seen_nonces = seen_nonces;
    }

    // 4b. Verify the one signed policy envelope before exposing either
    // section to a platform.  The signer is the enrollment-pinned key, never
    // a key merely supplied inside the received packet.
    match serde_json::from_value::<PolicyEnvelope>(resp.policy.clone()) {
        Ok(policy)
            if pinned_server_key_b64.as_deref() == Some(policy.signer_key.as_str())
                && verify_signature_b64(
                    &policy.signer_key,
                    &policy.preimage(),
                    &policy.signature,
                ) =>
        {
            dispatch.on_policy_envelope(&resp.policy)
        }
        Ok(_) => warn!("fleet: rejected policy envelope signed by an unpinned or invalid key"),
        Err(error) => warn!("fleet: rejected malformed policy envelope: {error}"),
    }

    // 4c. Execute any on-device content-search jobs this check-in handed us,
    //     and report each job's result back over the SAME per-device
    //     `checkin_secret` HMAC used above. `execute_pending_search_jobs` is
    //     pure and null-safe (empty `resp.pending_search_jobs` short-circuits
    //     to nothing); a runner error or a missing `SearchRunner` becomes an
    //     `{error}` report rather than aborting the check-in — a search
    //     failure must never crash or stall this loop.
    if !resp.pending_search_jobs.is_empty() {
        let reports = execute_pending_search_jobs(&resp.pending_search_jobs, dispatch);
        report_search_results(client, config, &device_id, &checkin_secret, reports).await;
    }

    // 5. Dead-man accounting.
    if resp.all_clear {
        let mut s = fleet_state.lock().unwrap();
        s.dead_man_misses = 0;
        s.last_checkin_at = Some(now_rfc3339());
        dispatch.all_clear();
    } else {
        let misses = {
            let mut s = fleet_state.lock().unwrap();
            s.dead_man_misses += 1;
            s.dead_man_misses
        };
        dispatch.dead_man_miss(misses);
    }

    true
}

/// Perform one REAL (non-decoy) check-in cycle. Thin wrapper over
/// [`run_checkin_cycle_inner`] with `decoy = false`, kept as the stable public
/// entry point every existing caller (background loop, tests, other
/// platforms) already uses.
#[allow(clippy::too_many_arguments)]
pub async fn run_checkin_cycle(
    client: &reqwest::Client,
    config: &FleetConfig,
    fleet_state: &SharedFleetState,
    dispatch: &dyn FleetActions,
    now: i64,
    platform: &str,
    agent_version: &str,
) -> bool {
    run_checkin_cycle_inner(
        client,
        config,
        fleet_state,
        dispatch,
        now,
        platform,
        agent_version,
        false,
    )
    .await
}

// ── Background loop ────────────────────────────────────────────────────────────

/// Spawn the background fleet check-in loop.
///
/// Only spawned when `FleetConfig::from_env(prefix)` returns `Some` and the
/// caller's own profile/policy gate allows remote triggers — that gate is the
/// platform's responsibility, not this crate's.
///
/// `platform` / `agent_version` are forwarded to [`enroll`] on every cycle.
pub fn spawn_fleet_client(
    config: FleetConfig,
    fleet_state: SharedFleetState,
    dispatch: Arc<dyn FleetActions>,
    platform: &'static str,
    agent_version: &'static str,
) {
    tokio::spawn(async move {
        // Honor an optional TLS SPKI pin (`{prefix}_CMD_PIN_SPKI`); falls back
        // to reqwest's default client (default TLS verification) on `None`
        // pin or on any internal TLS-config build error — pinning must never
        // prevent the client from being constructed.
        let client = match crate::pinning::build_client(config.cmd_pin_spki.clone()) {
            Ok(c) => c,
            Err(e) => {
                warn!("fleet client: TLS pin setup failed ({e}) — using default TLS verification");
                reqwest::Client::new()
            }
        };

        info!(
            "fleet client: starting (url={}, interval={}s [{:.2}x-{:.2}x randomized], \
             tls_pinned={}, padding_bytes={}, decoys={})",
            config.url,
            config.checkin_interval_secs,
            config.checkin_jitter_min_frac,
            config.checkin_jitter_max_frac,
            config.cmd_pin_spki.is_some(),
            config.checkin_padding_bytes,
            config.checkin_decoy_enabled,
        );

        loop {
            let now = crate::util::now_unix();
            run_checkin_cycle(
                &client,
                &config,
                &fleet_state,
                dispatch.as_ref(),
                now,
                platform,
                agent_version,
            )
            .await;

            // Traffic-shaping: draw the NEXT interval from a genuinely
            // randomized window rather than a fixed beacon with small ±10%
            // wobble (see `randomized_interval_secs`).
            let next_secs = randomized_interval_secs(
                config.checkin_interval_secs,
                config.checkin_jitter_min_frac,
                config.checkin_jitter_max_frac,
            );
            let next_interval = Duration::from_secs(next_secs);

            // Optional cover traffic: with probability `checkin_decoy_rate`
            // per real interval, fire ONE decoy check-in at a random offset
            // within this interval. The decoy is authenticated + shaped
            // identically to a real check-in (same HMAC round-trip, same
            // padding bucket) — see `run_checkin_cycle_inner(.., decoy=true)`
            // — so it is indistinguishable on the wire; it just doesn't feed
            // back into local dead-man/command state.
            if config.checkin_decoy_enabled && config.checkin_decoy_rate > 0.0 {
                let fire_decoy = {
                    use rand::Rng;
                    rand::thread_rng().gen::<f64>() < config.checkin_decoy_rate
                };
                if fire_decoy && next_secs > 1 {
                    use rand::Rng;
                    let offset_secs = rand::thread_rng().gen_range(1..next_secs);
                    tokio::time::sleep(Duration::from_secs(offset_secs)).await;
                    let decoy_now = crate::util::now_unix();
                    debug!("fleet client: firing decoy (cover-traffic) check-in");
                    run_checkin_cycle_inner(
                        &client,
                        &config,
                        &fleet_state,
                        dispatch.as_ref(),
                        decoy_now,
                        platform,
                        agent_version,
                        true,
                    )
                    .await;
                    tokio::time::sleep(Duration::from_secs(next_secs - offset_secs)).await;
                    continue;
                }
            }

            tokio::time::sleep(next_interval).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_transient_status_covers_5xx_and_429_only() {
        assert!(is_transient_status(500));
        assert!(is_transient_status(503));
        assert!(is_transient_status(429));
        assert!(!is_transient_status(400));
        assert!(!is_transient_status(401));
        assert!(!is_transient_status(404));
        assert!(!is_transient_status(200));
    }

    #[test]
    fn platform_device_kind_maps_to_server_enum_values() {
        // Must match the fleet server's fail-closed `DeviceKind::parse`
        // (`wincommander` | `tuxcommander` | `android`) — a wrong value 400s.
        assert_eq!(platform_device_kind("windows"), "wincommander");
        assert_eq!(platform_device_kind("linux"), "tuxcommander");
        // An exact device_kind passes through unchanged (server validates).
        assert_eq!(platform_device_kind("android"), "android");
    }

    #[test]
    fn new_device_id_is_uuid_v4_shaped() {
        let id = new_device_id();
        assert_eq!(id.len(), 36, "UUID canonical form is 36 chars");
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(id.as_bytes()[14] == b'4', "version nibble is 4");
        assert!(new_device_id() != new_device_id(), "ids are unique");
    }

    #[test]
    fn backoff_duration_is_capped() {
        let base = Duration::from_millis(100);
        let cap = Duration::from_secs(30);
        for attempt in 0..30 {
            let d = backoff_duration(attempt, base, cap);
            assert!(d <= cap, "attempt {attempt} exceeded cap: {d:?}");
        }
    }

    #[test]
    fn backoff_duration_grows_with_attempt_upper_bound() {
        let base = Duration::from_millis(100);
        let cap = Duration::from_secs(30);
        // The upper bound (before jitter) should be non-decreasing until capped.
        let upper = |attempt: u32| -> u128 {
            (base.as_millis().saturating_mul(1u128 << attempt.min(20))).min(cap.as_millis())
        };
        assert!(upper(1) >= upper(0));
        assert!(upper(5) >= upper(1));
        assert_eq!(upper(10), cap.as_millis());
    }

    // ── Traffic shaping: randomized interval ─────────────────────────────────

    #[test]
    fn randomized_interval_stays_within_configured_bounds() {
        let interval = 100u64;
        let (min_frac, max_frac) = (0.5, 1.5);
        for _ in 0..500 {
            let secs = randomized_interval_secs(interval, min_frac, max_frac);
            assert!(
                (50..=150).contains(&secs),
                "interval {secs} outside [50,150] window"
            );
        }
    }

    #[test]
    fn randomized_interval_actually_varies_across_calls() {
        // The whole point of this change: NOT a fixed beacon with ±10% wobble.
        // Over enough draws from a wide window we must see more than a
        // handful of distinct values (a fixed-interval regression would
        // produce exactly one value every time).
        let interval = 200u64;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            seen.insert(randomized_interval_secs(interval, 0.5, 1.5));
        }
        assert!(
            seen.len() > 20,
            "expected wide variance in randomized interval, got {} distinct values",
            seen.len()
        );
    }

    #[test]
    fn randomized_interval_never_goes_below_one_second() {
        // Degenerate config (both fractions collapse to ~0) must still
        // produce a positive sleep — never a busy loop.
        for _ in 0..50 {
            let secs = randomized_interval_secs(10, 0.0, 0.0);
            assert!(secs >= 1);
        }
    }

    #[test]
    fn randomized_interval_falls_back_when_bounds_inverted() {
        // min > max at the call site is defended independently of the config
        // layer's own validation — must not panic, must stay sane.
        for _ in 0..50 {
            let secs = randomized_interval_secs(100, 2.0, 1.0);
            assert!((50..=150).contains(&secs));
        }
    }

    #[test]
    fn randomized_interval_narrow_window_still_bounded() {
        // A configured window narrower than the historical ±10% jitter must
        // still respect its own (tighter) bounds — proves this isn't just
        // reusing the old jitter behavior under a new name.
        let interval = 60u64;
        for _ in 0..200 {
            let secs = randomized_interval_secs(interval, 0.95, 1.05);
            assert!(
                (57..=63).contains(&secs),
                "secs={secs} outside tight window"
            );
        }
    }

    // ── Traffic shaping: fixed-size padding ──────────────────────────────────

    #[test]
    fn padding_disabled_when_target_is_zero() {
        assert_eq!(make_padding(0, 0), "");
        assert_eq!(make_padding(0, 500), "");
    }

    #[test]
    fn padding_empty_when_real_content_already_meets_bucket() {
        assert_eq!(make_padding(100, 100), "");
        assert_eq!(make_padding(100, 200), "");
    }

    #[test]
    fn padding_tops_up_towards_target_bucket() {
        let target = 512;
        let used = 120;
        let padding = make_padding(target, used);
        assert!(!padding.is_empty());
        // Padding is base64; decode to confirm it's real filler bytes, then
        // check the combined estimate lands at-or-above the bucket (a floor,
        // not a truncation).
        let decoded = B64.decode(&padding).expect("padding must be valid base64");
        assert!(!decoded.is_empty());
        assert!(used + padding.len() >= target);
    }

    #[test]
    fn padding_is_random_across_calls_not_a_fixed_filler() {
        let a = make_padding(256, 0);
        let b = make_padding(256, 0);
        assert_ne!(
            a, b,
            "padding must be freshly randomized, not a static filler string"
        );
    }

    // ── Traffic shaping: padding must never alter the HMAC preimage ──────────

    #[test]
    fn padding_decoy_and_health_alter_request_hmac_v2() {
        let secret = b"fleet-checkin-secret-32-bytes-ok";
        let device_id = "dev-pad-test";
        let ts: i64 = 1_700_000_000;
        let nonce = "ci-padtest";

        let mut req_a = CheckinRequest {
            device_id: device_id.to_string(),
            hostname: "host-a".to_string(),
            posture: "nominal".to_string(),
            ts,
            nonce: nonce.to_string(),
            hmac_version: HMAC_VERSION_V2,
            hmac: String::new(),
            padding: make_padding(512, 0),
            decoy: false,
            resources: None,
            health: None,
        };
        req_a.hmac = compute_request_hmac_v2(secret, "POST", "/v1/agents/checkin", &req_a).unwrap();

        let mut req_b = CheckinRequest {
            device_id: device_id.to_string(),
            hostname: "host-b-different-length".to_string(),
            posture: "nominal".to_string(),
            ts,
            nonce: nonce.to_string(),
            hmac_version: HMAC_VERSION_V2,
            hmac: String::new(),
            padding: make_padding(4096, 0),
            decoy: true,
            resources: None,
            health: Some(crate::verify::HealthSnapshot {
                encryption_on: Some(true),
                patch_state: Some("current".to_string()),
                av_on: Some(false),
                platform_facts: None,
                os_version: Some("Windows 11 Pro 24H2".to_string()),
                sovereignty_score: Some(50),
            }),
        };

        req_b.hmac = compute_request_hmac_v2(secret, "POST", "/v1/agents/checkin", &req_b).unwrap();

        assert_ne!(req_a.hmac, req_b.hmac);
    }
}
