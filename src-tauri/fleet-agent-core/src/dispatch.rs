//! Command dispatch: the `FleetActions` trait platforms implement, and the pure
//! `process_checkin` function that verifies + routes a check-in response.

use std::collections::HashMap;

use ed25519_dalek::VerifyingKey;
use tracing::{info, warn};

use crate::verify::{
    evict_stale_nonces, verify_command, CheckinResponse, PendingSearchJob, SearchHit,
};

/// Callback interface for fleet command dispatch.
///
/// Supersedes `tc-agent`'s `FleetDispatch`. Tests inject a mock implementation
/// to verify command routing without real I/O; a platform wires a real
/// implementation that routes to its own privileged actions.
///
/// **`record_trigger_source` takes `&str`, not a platform-owned enum** — this is
/// deliberate so that no platform-specific type (e.g. TuxCommander's
/// `tc_core::TriggerSource`) leaks into this crate's dependency graph. A caller
/// that has its own `TriggerSource`-shaped enum maps the incoming `&str`
/// (constant `"fleet_command"`, see [`FLEET_TRIGGER_SOURCE`]) back to its own
/// type at the call site.
pub trait FleetActions: Send + Sync {
    /// Called when a `raise_posture` command is received (non-destructive).
    fn raise_posture(&self, level: &str);
    /// Called when a `duress_seal` command is received.
    fn duress_seal(&self);
    /// Called when a `duress_wipe` command is received (armed = live; else simulated).
    fn duress_wipe(&self);
    /// Called when an `all_clear` is received (resets the fleet dead-man).
    fn all_clear(&self);
    /// Called when a `duress_unseal` command is received (operator-signed;
    /// reverses a prior `duress_seal`). Default: no-op — a platform that
    /// hasn't wired programmatic unseal-over-fleet yet is unaffected by this
    /// method's addition rather than being force-broken by it, mirroring how
    /// `dispatch_catalog`/`on_config_epoch` default to no-op below.
    fn duress_unseal(&self) {}
    /// Called when an `all_clear_revoke` command is received (operator-signed;
    /// reverses a prior `all_clear`, re-arming the dead-man). Default: no-op,
    /// for the same reason as `duress_unseal`.
    fn all_clear_revoke(&self) {}
    /// Called when a check-in interval passes without an `all_clear` (dead-man miss).
    fn dead_man_miss(&self, consecutive_misses: u32);
    /// Called with a trigger-source label so the platform can record it (e.g. in
    /// an `AgentState.last_trigger`-shaped field). `source` is currently always
    /// [`FLEET_TRIGGER_SOURCE`] but is passed as `&str` to keep this trait
    /// dependency-free.
    fn record_trigger_source(&self, source: &str, at: String);

    /// Called for a VERIFIED ordinary (server-signed, non-duress) command —
    /// routed by `catalog_id` rather than one of the fixed duress scopes (e.g.
    /// WinCommander's `usb.storage.lockdown.enable`, `vault.dismount_all`). The
    /// caller has already verified the signature against the pinned SERVER key.
    /// Returns `true` if the platform handled it. Default: no-op (`false`) — a
    /// duress-only agent (TuxCommander/Android) has no open catalog.
    fn dispatch_catalog(
        &self,
        _catalog_id: &str,
        _action_class: &str,
        _payload: &serde_json::Value,
    ) -> bool {
        false
    }

    /// Called with the device's resolved config epoch (opaque server-signed
    /// policy snapshot) when a check-in response carries one. The platform
    /// verifies its signature + applies/queues it. Default: no-op — duress-only
    /// agents (TuxCommander/Android) have no policy epoch.
    fn on_config_epoch(&self, _epoch: &serde_json::Value) {}

    /// Sample live device-resource telemetry (CPU/RAM/disk/network) to attach
    /// to this check-in round-trip, if the platform has a collector wired up.
    /// Default: `None` — duress-only agents (TuxCommander/Android) have no
    /// local resource collector; WinCommander overrides this with its
    /// existing `system_metrics`/`net_traffic_alert` samplers. Called once
    /// per non-decoy check-in cycle — keep this cheap/non-blocking (the
    /// existing collectors already are: kernel-counter reads, no process
    /// spawn on the hot path).
    fn sample_resources(&self) -> Option<fleet_proto::DeviceResourceSample> {
        None
    }

    /// Sample a device-health snapshot (encryption/patch/AV/OS/sovereignty)
    /// to attach to this check-in round-trip, if the platform has a health
    /// collector wired up. Default: `None` — duress-only agents
    /// (TuxCommander/Android) have no local health collector.
    ///
    /// Mirrors [`FleetActions::sample_resources`]: called once per non-decoy
    /// check-in cycle, so an implementation should keep this cheap/non-
    /// blocking — read cached/previously-probed values rather than spawning
    /// a fresh scan on the hot path. **Must be resilient**: a failed or
    /// unavailable individual probe should degrade only that field to `None`
    /// on the returned [`crate::verify::HealthSnapshot`], never panic or
    /// block the check-in.
    ///
    /// WinCommander's Windows implementation (Pro's `fleet_push.rs`) is
    /// expected to source each field best-effort from probes that already
    /// exist on the Free side, reusing them rather than re-implementing:
    /// `encryption_on` from `commander-free/src/backend.rs`'s
    /// `Get-EncryptionStatus` PS handler (encrypted-volume presence),
    /// `patch_state` from the same file's `Get-UpdateStatus` handler
    /// (Windows Update service/pause state, mapped to
    /// `"current"`/`"pending"`/`"unknown"`), and `av_on` from
    /// `commander-free/src/pro_install.rs`'s `get_defender_status`
    /// (`DefenderStatus::real_time_monitoring`). A probe that errors or
    /// isn't reachable from the sidecar simply leaves its field `None` —
    /// it must never fail the whole snapshot or the check-in.
    fn sample_health(&self) -> Option<crate::verify::HealthSnapshot> {
        None
    }

    /// The local content-search executor this agent has wired up, if any.
    /// Default: `None` — an agent with no local search capability (TuxCommander/
    /// Android today, or a WinCommander build before the real engine is linked)
    /// simply reports every dispatched job as an error rather than fabricating
    /// hits. See [`SearchRunner`] and [`execute_pending_search_jobs`].
    fn search_runner(&self) -> Option<&dyn SearchRunner> {
        None
    }
}

/// Executes a local content-search query for one dispatched
/// [`PendingSearchJob`]. Implemented by the platform — WinCommander Pro is
/// expected to wire this to the real `wincmd-search` keyword engine against
/// the Free side's on-disk tantivy index; an agent with no local search
/// capability simply never wires one (`FleetActions::search_runner` default
/// `None`).
///
/// Mirrors [`FleetActions::sample_health`]'s discipline: called synchronously
/// from the check-in loop, so an implementation should keep the query
/// bounded/cheap rather than run an unbounded scan on the hot path.
/// **Must be resilient**: any internal failure (index not yet built, engine
/// not initialized, query error) must surface as `Err(reason)` — never
/// panic — so [`execute_pending_search_jobs`] can report it to the job
/// instead of crashing or stalling the check-in loop.
pub trait SearchRunner: Send + Sync {
    /// Run `query` over the local content index, returning at most
    /// `max_hits` hits.
    fn search(&self, query: &str, max_hits: usize) -> Result<Vec<SearchHit>, String>;
}

/// One dispatched search job's outcome, ready to report to
/// `POST /v1/agents/search-result` (`SearchResultReport::job_id`/`hits`/
/// `error`; `device_id`/`ts`/`nonce`/`hmac` are filled in by the transport
/// layer that actually sends it — see `transport::report_search_results`).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchJobReport {
    pub job_id: String,
    pub hits: Vec<SearchHit>,
    pub error: Option<String>,
}

/// Execute every pending search job from a check-in response against the
/// platform's [`SearchRunner`] (if any), producing one [`SearchJobReport`]
/// per job. **Pure** (no I/O, no async) — the same testability discipline as
/// [`process_checkin`]: a mock [`SearchRunner`] wired through
/// `FleetActions::search_runner` lets this be unit-tested without a network
/// stack. `jobs` empty (the common case: no server dispatch, or nothing
/// currently owed) is a true no-op — returns an empty `Vec` without calling
/// `dispatch.search_runner()` at all.
pub fn execute_pending_search_jobs(
    jobs: &[PendingSearchJob],
    dispatch: &dyn FleetActions,
) -> Vec<SearchJobReport> {
    if jobs.is_empty() {
        return Vec::new();
    }
    let runner = dispatch.search_runner();
    jobs.iter()
        .map(|job| match runner {
            None => {
                warn!(
                    "fleet: search job '{}' dispatched but no local SearchRunner is wired — reporting error",
                    job.job_id
                );
                SearchJobReport {
                    job_id: job.job_id.clone(),
                    hits: Vec::new(),
                    error: Some("no local content-search engine wired on this agent".to_string()),
                }
            }
            Some(r) => match r.search(&job.query, job.max_hits_per_device) {
                Ok(hits) => {
                    info!(
                        "fleet: search job '{}' executed ({} hits)",
                        job.job_id,
                        hits.len()
                    );
                    SearchJobReport {
                        job_id: job.job_id.clone(),
                        hits,
                        error: None,
                    }
                }
                Err(e) => {
                    warn!("fleet: search job '{}' failed: {e}", job.job_id);
                    SearchJobReport {
                        job_id: job.job_id.clone(),
                        hits: Vec::new(),
                        error: Some(e),
                    }
                }
            },
        })
        .collect()
}

/// The trigger-source label passed to [`FleetActions::record_trigger_source`]
/// for every command dispatched from a fleet check-in. Platforms with their own
/// `TriggerSource` enum should map this string back to their `FleetCommand`
/// variant (or equivalent).
pub const FLEET_TRIGGER_SOURCE: &str = "fleet_command";

/// What the fleet command handler did with a verified command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FleetDispatchOutcome {
    /// Command was verified and the platform action was requested.
    Dispatched { kind: String },
    /// Command had an invalid signature, bad ts, replay, or scope mismatch.
    AuthFailed(String),
    /// Profile gate rejected the command.
    ProfileRejected,
    /// Pinned-key mismatch: server returned a different key on re-enroll.
    PinnedKeyMismatch,
}

/// Process one check-in response against a pinned key + nonce cache.
///
/// Returns a [`FleetDispatchOutcome`] for each command in the response.
/// An `all_clear: true` in the response resets the dead-man; a response with
/// `all_clear: false` (or missing) is a dead-man miss (accounted by the caller).
///
/// This function is **pure** (no I/O, no async) so it can be unit-tested without
/// a network stack.
pub fn process_checkin(
    response: &CheckinResponse,
    operator_key: Option<&VerifyingKey>,
    server_key: Option<&VerifyingKey>,
    now: i64,
    max_skew_secs: i64,
    seen_nonces: &mut HashMap<String, i64>,
    dispatch: &dyn FleetActions,
) -> Vec<FleetDispatchOutcome> {
    let mut outcomes = Vec::new();

    // Evict stale nonces before processing (bounds memory growth).
    evict_stale_nonces(seen_nonces, now, max_skew_secs);

    for cmd in &response.commands {
        // Dual-key routing (fixed, local — never client/server-supplied): an
        // operator-control command must use the operator key; every other
        // command uses the server signing key. A missing required key is a
        // fail-closed reject, never a fallback to the other key.
        let is_operator_command = fleet_proto::is_duress_catalog(&cmd.catalog_id);
        let selected = if is_operator_command {
            operator_key
        } else {
            server_key
        };
        let verify = match selected {
            None => Err(format!(
                "no pinned {} key for command '{}'",
                if is_operator_command {
                    "operator"
                } else {
                    "server"
                },
                cmd.catalog_id
            )),
            Some(k) => verify_command(cmd, k, now, max_skew_secs, seen_nonces),
        };
        let outcome = match verify {
            Err(e) => {
                warn!("fleet command verification failed: {e}");
                FleetDispatchOutcome::AuthFailed(e)
            }
            Ok(()) => {
                dispatch.record_trigger_source(FLEET_TRIGGER_SOURCE, crate::util::now_rfc3339());
                // The server's `fleet_proto::SignedCommand` carries no `scope`
                // field; for operator commands `scope == catalog_id`, so fall
                // back to it when `scope` is absent/empty (the real server case).
                let scope = if cmd.scope.is_empty() {
                    cmd.catalog_id.as_str()
                } else {
                    cmd.scope.as_str()
                };
                match scope {
                    "raise_posture" => {
                        let level = cmd
                            .payload
                            .get("level")
                            .and_then(|v| v.as_str())
                            .unwrap_or("elevated");
                        info!("fleet: raise_posture → {level}");
                        dispatch.raise_posture(level);
                    }
                    "duress_seal" => {
                        info!("fleet: duress_seal");
                        dispatch.duress_seal();
                    }
                    "duress_wipe" => {
                        info!("fleet: duress_wipe");
                        dispatch.duress_wipe();
                    }
                    "all_clear" => {
                        info!("fleet: all_clear");
                        dispatch.all_clear();
                    }
                    "duress_unseal" => {
                        info!("fleet: duress_unseal");
                        dispatch.duress_unseal();
                    }
                    "all_clear_revoke" => {
                        info!("fleet: all_clear_revoke");
                        dispatch.all_clear_revoke();
                    }
                    other => {
                        // A verified ordinary (server-signed) command routes by
                        // catalog_id to the platform's open catalog. An
                        // operator-control-only agent's default
                        // `dispatch_catalog` no-ops it.
                        if !is_operator_command
                            && dispatch.dispatch_catalog(
                                &cmd.catalog_id,
                                &cmd.action_class,
                                &cmd.payload,
                            )
                        {
                            info!("fleet: dispatched catalog command '{}'", cmd.catalog_id);
                        } else {
                            warn!("fleet: unhandled command scope '{other}' — ignoring");
                        }
                    }
                }
                FleetDispatchOutcome::Dispatched {
                    kind: scope.to_string(),
                }
            }
        };
        outcomes.push(outcome);
    }

    outcomes
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::verify::SignedCommand;
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    use ed25519_dalek::{Signature, Signer, SigningKey};
    use std::sync::Mutex as StdMutex;

    fn test_signing_key() -> SigningKey {
        SigningKey::generate(&mut rand::rngs::OsRng)
    }

    /// Signs over `idempotency_key`, not `command_id` — mirroring the real
    /// flow (operator/server signs the stable idempotency key before a
    /// server-assigned UUID exists). See the identical helper doc in
    /// `verify.rs` for why `command_id` and `idempotency_key` are
    /// deliberately distinct here rather than reusing one string for both.
    #[allow(clippy::too_many_arguments)]
    fn make_signed_command(
        key: &SigningKey,
        scope: &str,
        payload: serde_json::Value,
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
        SignedCommand {
            command_id: command_id.to_string(),
            idempotency_key: idempotency_key.to_string(),
            device_id: device_id.to_string(),
            catalog_id: catalog_id.to_string(),
            action_class: action_class.to_string(),
            payload,
            epoch_version,
            signature: B64.encode(sig.to_bytes()),
            signer_key: B64.encode(key.verifying_key().to_bytes()),
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

    /// A `FleetActions` mock that records calls.
    #[derive(Default)]
    pub struct MockDispatch {
        pub posture_calls: StdMutex<Vec<String>>,
        pub seal_calls: StdMutex<u32>,
        pub wipe_calls: StdMutex<u32>,
        pub all_clear_calls: StdMutex<u32>,
        pub unseal_calls: StdMutex<u32>,
        pub all_clear_revoke_calls: StdMutex<u32>,
        pub miss_calls: StdMutex<Vec<u32>>,
        pub trigger_sources: StdMutex<Vec<(String, String)>>,
        /// Optional mock `SearchRunner` — `None` reproduces "no engine wired".
        pub runner: Option<Box<dyn SearchRunner>>,
    }

    impl FleetActions for MockDispatch {
        fn raise_posture(&self, level: &str) {
            self.posture_calls.lock().unwrap().push(level.to_string());
        }
        fn duress_seal(&self) {
            *self.seal_calls.lock().unwrap() += 1;
        }
        fn duress_wipe(&self) {
            *self.wipe_calls.lock().unwrap() += 1;
        }
        fn all_clear(&self) {
            *self.all_clear_calls.lock().unwrap() += 1;
        }
        fn duress_unseal(&self) {
            *self.unseal_calls.lock().unwrap() += 1;
        }
        fn all_clear_revoke(&self) {
            *self.all_clear_revoke_calls.lock().unwrap() += 1;
        }
        fn dead_man_miss(&self, misses: u32) {
            self.miss_calls.lock().unwrap().push(misses);
        }
        fn record_trigger_source(&self, source: &str, at: String) {
            self.trigger_sources
                .lock()
                .unwrap()
                .push((source.to_string(), at));
        }
        fn search_runner(&self) -> Option<&dyn SearchRunner> {
            self.runner.as_deref()
        }
    }

    /// A mock `SearchRunner` that always returns a fixed hit set or a fixed error.
    pub struct MockSearchRunner {
        pub result: Result<Vec<SearchHit>, String>,
    }

    impl SearchRunner for MockSearchRunner {
        fn search(&self, _query: &str, _max_hits: usize) -> Result<Vec<SearchHit>, String> {
            self.result.clone()
        }
    }

    #[test]
    fn raise_posture_is_dispatched() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;
        let cmd = make_signed_command(
            &key,
            "raise_posture",
            serde_json::json!({ "level": "elevated" }),
            now,
            "rp-nonce-1",
            "cmd-rp-server-uuid",
            "idem-rp-key",
            "dev-test",
            "lc.raise_posture",
            "safe",
            1,
        );
        let resp = CheckinResponse {
            all_clear: false,
            commands: vec![cmd],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        let dispatch = MockDispatch::default();
        let mut seen = HashMap::new();
        let outcomes = process_checkin(&resp, Some(&vk), Some(&vk), now, 300, &mut seen, &dispatch);
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(
            outcomes[0],
            FleetDispatchOutcome::Dispatched { .. }
        ));
        let postures = dispatch.posture_calls.lock().unwrap();
        assert_eq!(postures.as_slice(), &["elevated"]);
    }

    #[test]
    fn duress_seal_is_dispatched() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;
        let cmd = make_cmd(&key, "duress_seal", "seal-nonce-1", now);
        let resp = CheckinResponse {
            all_clear: false,
            commands: vec![cmd],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        let dispatch = MockDispatch::default();
        let mut seen = HashMap::new();
        process_checkin(&resp, Some(&vk), Some(&vk), now, 300, &mut seen, &dispatch);
        assert_eq!(*dispatch.seal_calls.lock().unwrap(), 1);
    }

    /// P0 regression: the real server's `fleet_proto::SignedCommand` carries
    /// only 9 fields — NO `scope`/`ts`/`nonce`. Before the `#[serde(default)]`
    /// on `scope`, a command-carrying check-in failed to deserialize
    /// ("missing field `scope`") and was silently treated as a network error
    /// (dead-man miss, command never processed). This asserts the 9-field
    /// server shape deserializes, verifies, and routes on `catalog_id`.
    #[test]
    fn server_9field_command_without_scope_deserializes_verifies_and_routes() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;
        // Signature is over (idempotency_key, device_id, catalog_id,
        // action_class, payload, epoch_version) — none of scope/ts/nonce — so
        // dropping those from the wire cannot affect verification.
        let signed = make_signed_command(
            &key,
            "duress_seal", // scope (dropped from the server JSON below)
            serde_json::json!({}),
            now,
            "seal-nonce",
            "cmd-uuid",
            "idem-key",
            "dev-test",
            "duress_seal", // catalog_id — the routing key after fallback
            "destructive",
            1,
        );
        let server_json = serde_json::json!({
            "command_id": signed.command_id,
            "idempotency_key": signed.idempotency_key,
            "device_id": signed.device_id,
            "catalog_id": signed.catalog_id,
            "action_class": signed.action_class,
            "payload": signed.payload,
            "epoch_version": signed.epoch_version,
            "signature": signed.signature,
            "signer_key": signed.signer_key,
        });
        let cmd: SignedCommand =
            serde_json::from_value(server_json).expect("9-field server command must deserialize");
        assert!(cmd.scope.is_empty() && cmd.ts == 0 && cmd.nonce.is_empty());

        let resp = CheckinResponse {
            all_clear: false,
            commands: vec![cmd],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        let dispatch = MockDispatch::default();
        let mut seen = HashMap::new();
        let outcomes = process_checkin(&resp, Some(&vk), Some(&vk), now, 300, &mut seen, &dispatch);
        assert!(
            matches!(outcomes[0], FleetDispatchOutcome::Dispatched { .. }),
            "must verify + dispatch (not AuthFailed) from the 9-field server shape"
        );
        assert_eq!(
            *dispatch.seal_calls.lock().unwrap(),
            1,
            "routed on catalog_id fallback when scope is absent"
        );
    }

    /// Security: dual-key routing is fixed and local. An operator-control
    /// command verifies ONLY against the operator key (never the server key),
    /// and an ordinary command ONLY against the server key — so the server
    /// can't forge an operator command and an operator-signed blob can't
    /// masquerade as ordinary.
    #[test]
    fn dual_key_routing_rejects_wrong_key_class() {
        let op = test_signing_key();
        let srv = test_signing_key();
        let op_vk = op.verifying_key();
        let srv_vk = srv.verifying_key();
        let now = 1_700_000_000_i64;

        // Operator-control command, operator-signed.
        let duress = make_signed_command(
            &op,
            "duress_seal",
            serde_json::json!({}),
            now,
            "n1",
            "c1",
            "i1",
            "d1",
            "duress_seal",
            "destructive",
            1,
        );
        let resp = CheckinResponse {
            all_clear: false,
            commands: vec![duress],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        // Both keys pinned → routes to operator → dispatched.
        let d = MockDispatch::default();
        let mut seen = HashMap::new();
        process_checkin(&resp, Some(&op_vk), Some(&srv_vk), now, 300, &mut seen, &d);
        assert_eq!(*d.seal_calls.lock().unwrap(), 1);
        // ONLY the server key pinned → operator command rejected (no fallback).
        let d2 = MockDispatch::default();
        let mut seen2 = HashMap::new();
        let out = process_checkin(&resp, None, Some(&srv_vk), now, 300, &mut seen2, &d2);
        assert!(matches!(out[0], FleetDispatchOutcome::AuthFailed(_)));
        assert_eq!(*d2.seal_calls.lock().unwrap(), 0);

        // Ordinary command, server-signed → routes to the server key.
        let ordinary = make_signed_command(
            &srv,
            "",
            serde_json::json!({}),
            now,
            "n2",
            "c2",
            "i2",
            "d1",
            "usb.storage.lockdown.enable",
            "destructive",
            1,
        );
        let resp2 = CheckinResponse {
            all_clear: false,
            commands: vec![ordinary],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        let out3 = process_checkin(
            &resp2,
            Some(&op_vk),
            Some(&srv_vk),
            now,
            300,
            &mut HashMap::new(),
            &MockDispatch::default(),
        );
        assert!(matches!(out3[0], FleetDispatchOutcome::Dispatched { .. }));
        // ONLY the operator key pinned → ordinary must be rejected.
        let out4 = process_checkin(
            &resp2,
            Some(&op_vk),
            None,
            now,
            300,
            &mut HashMap::new(),
            &MockDispatch::default(),
        );
        assert!(matches!(out4[0], FleetDispatchOutcome::AuthFailed(_)));
    }

    #[test]
    fn duress_wipe_is_dispatched_and_trigger_source_recorded() {
        // On an unarmed system the platform enforces simulation — the dispatch mock
        // records the call but does NOT perform real destruction (that gate lives
        // in the platform, not this crate).
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;
        let cmd = make_cmd(&key, "duress_wipe", "wipe-nonce-1", now);
        let resp = CheckinResponse {
            all_clear: false,
            commands: vec![cmd],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        let dispatch = MockDispatch::default();
        let mut seen = HashMap::new();
        process_checkin(&resp, Some(&vk), Some(&vk), now, 300, &mut seen, &dispatch);
        assert_eq!(*dispatch.wipe_calls.lock().unwrap(), 1);
        let sources = dispatch.trigger_sources.lock().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].0, FLEET_TRIGGER_SOURCE);
    }

    #[test]
    fn all_clear_resets_dead_man() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;
        let cmd = make_cmd(&key, "all_clear", "ac-nonce-1", now);
        let resp = CheckinResponse {
            all_clear: true,
            commands: vec![cmd],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        let dispatch = MockDispatch::default();
        let mut seen = HashMap::new();
        process_checkin(&resp, Some(&vk), Some(&vk), now, 300, &mut seen, &dispatch);
        assert_eq!(*dispatch.all_clear_calls.lock().unwrap(), 1);
    }

    #[test]
    fn invalid_signature_produces_auth_failed_outcome() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;
        let mut cmd = make_cmd(&key, "duress_wipe", "bad-sig-1", now);
        cmd.signature = B64.encode([0u8; 64]);
        let resp = CheckinResponse {
            all_clear: false,
            commands: vec![cmd],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        let dispatch = MockDispatch::default();
        let mut seen = HashMap::new();
        let outcomes = process_checkin(&resp, Some(&vk), Some(&vk), now, 300, &mut seen, &dispatch);
        assert!(matches!(outcomes[0], FleetDispatchOutcome::AuthFailed(_)));
        assert_eq!(*dispatch.wipe_calls.lock().unwrap(), 0);
    }

    /// P0 regression: `duress_unseal` and `all_clear_revoke` are 2 of the 5
    /// entries in `fleet_proto::DURESS_CATALOG_IDS` — a verified,
    /// operator-signed command for either must actually reach the platform
    /// instead of falling into the generic `other` arm (which used to just
    /// warn "unhandled command scope … — ignoring" and silently do nothing).
    #[test]
    fn duress_unseal_and_all_clear_revoke_are_dispatched() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;

        let unseal_cmd = make_cmd(&key, "duress_unseal", "unseal-nonce-1", now);
        let resp = CheckinResponse {
            all_clear: false,
            commands: vec![unseal_cmd],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        let dispatch = MockDispatch::default();
        let mut seen = HashMap::new();
        let outcomes = process_checkin(&resp, Some(&vk), Some(&vk), now, 300, &mut seen, &dispatch);
        assert!(matches!(
            outcomes[0],
            FleetDispatchOutcome::Dispatched { .. }
        ));
        assert_eq!(*dispatch.unseal_calls.lock().unwrap(), 1);

        let revoke_cmd = make_cmd(&key, "all_clear_revoke", "revoke-nonce-1", now);
        let resp2 = CheckinResponse {
            all_clear: false,
            commands: vec![revoke_cmd],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        let dispatch2 = MockDispatch::default();
        let mut seen2 = HashMap::new();
        let outcomes2 = process_checkin(
            &resp2,
            Some(&vk),
            Some(&vk),
            now,
            300,
            &mut seen2,
            &dispatch2,
        );
        assert!(matches!(
            outcomes2[0],
            FleetDispatchOutcome::Dispatched { .. }
        ));
        assert_eq!(*dispatch2.all_clear_revoke_calls.lock().unwrap(), 1);
    }

    #[test]
    fn dead_man_miss_on_no_all_clear() {
        let key = test_signing_key();
        let vk = key.verifying_key();
        let now = 1_700_000_000_i64;
        let resp = CheckinResponse {
            all_clear: false,
            commands: vec![],
            config_epoch: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
        };
        let dispatch = MockDispatch::default();
        let mut seen = HashMap::new();
        process_checkin(&resp, Some(&vk), Some(&vk), now, 300, &mut seen, &dispatch);
        assert_eq!(*dispatch.all_clear_calls.lock().unwrap(), 0);
    }

    // ── execute_pending_search_jobs ───────────────────────────────────────────

    fn make_job(job_id: &str, query: &str, max_hits: usize) -> PendingSearchJob {
        PendingSearchJob {
            job_id: job_id.to_string(),
            query: query.to_string(),
            max_hits_per_device: max_hits,
        }
    }

    #[test]
    fn absent_pending_search_jobs_is_a_noop() {
        // An absent/empty `pending_search_jobs` (the common case — no server
        // dispatch, or nothing currently owed) must be a true no-op: no
        // report produced, and — critically — no call into the search
        // runner at all (a `None` runner here would otherwise be
        // indistinguishable from "ran zero jobs", so use a runner that
        // panics if invoked to prove it's genuinely never called).
        struct PanicIfCalled;
        impl SearchRunner for PanicIfCalled {
            fn search(&self, _q: &str, _m: usize) -> Result<Vec<SearchHit>, String> {
                panic!("search_runner must not be invoked when there are no pending jobs");
            }
        }
        let dispatch = MockDispatch {
            runner: Some(Box::new(PanicIfCalled)),
            ..Default::default()
        };
        let reports = execute_pending_search_jobs(&[], &dispatch);
        assert!(reports.is_empty());
    }

    #[test]
    fn checkin_response_pending_search_jobs_defaults_to_empty_when_absent() {
        // A server response with no `pending_search_jobs` key at all must
        // deserialize (not error) and yield an empty Vec — this is the
        // wire-level half of the no-op guarantee above.
        let json = serde_json::json!({ "all_clear": true, "commands": [] });
        let resp: CheckinResponse = serde_json::from_value(json).unwrap();
        assert!(resp.pending_search_jobs.is_empty());
    }

    #[test]
    fn one_pending_job_with_mock_runner_produces_hits_report() {
        // A check-in response carrying 1 pending job + a mock runner
        // returning 2 hits must produce exactly one report carrying those 2
        // hits and no error — the shape that feeds
        // `transport::report_search_results`'s `/agents/search-result` POST.
        let hits = vec![
            SearchHit {
                path: "C:\\docs\\a.txt".to_string(),
                snippet: "...needle...".to_string(),
                score: 0.9,
            },
            SearchHit {
                path: "C:\\docs\\b.txt".to_string(),
                snippet: "...needle again...".to_string(),
                score: 0.5,
            },
        ];
        let dispatch = MockDispatch {
            runner: Some(Box::new(MockSearchRunner {
                result: Ok(hits.clone()),
            })),
            ..Default::default()
        };
        let jobs = vec![make_job("csj_abc123", "needle", 50)];
        let reports = execute_pending_search_jobs(&jobs, &dispatch);

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].job_id, "csj_abc123");
        assert_eq!(reports[0].hits, hits);
        assert!(reports[0].error.is_none());
    }

    #[test]
    fn runner_error_produces_error_report_with_no_hits() {
        let dispatch = MockDispatch {
            runner: Some(Box::new(MockSearchRunner {
                result: Err("index not yet built".to_string()),
            })),
            ..Default::default()
        };
        let jobs = vec![make_job("csj_err1", "needle", 50)];
        let reports = execute_pending_search_jobs(&jobs, &dispatch);

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].job_id, "csj_err1");
        assert!(reports[0].hits.is_empty());
        assert_eq!(reports[0].error.as_deref(), Some("index not yet built"));
    }

    #[test]
    fn no_runner_wired_produces_error_report_not_panic() {
        // No `SearchRunner` wired (the default on every `FleetActions`
        // impl until a platform opts in) must degrade to an error report
        // for the job — never panic, never silently fabricate hits.
        let dispatch = MockDispatch::default();
        let jobs = vec![make_job("csj_noeng", "needle", 50)];
        let reports = execute_pending_search_jobs(&jobs, &dispatch);

        assert_eq!(reports.len(), 1);
        assert!(reports[0].hits.is_empty());
        assert!(reports[0].error.is_some());
    }

    #[test]
    fn multiple_pending_jobs_each_get_their_own_report() {
        let dispatch = MockDispatch {
            runner: Some(Box::new(MockSearchRunner {
                result: Ok(vec![SearchHit {
                    path: "x".to_string(),
                    snippet: "y".to_string(),
                    score: 1.0,
                }]),
            })),
            ..Default::default()
        };
        let jobs = vec![
            make_job("csj_1", "a", 10),
            make_job("csj_2", "b", 10),
            make_job("csj_3", "c", 10),
        ];
        let reports = execute_pending_search_jobs(&jobs, &dispatch);
        assert_eq!(reports.len(), 3);
        assert_eq!(
            reports.iter().map(|r| r.job_id.clone()).collect::<Vec<_>>(),
            vec!["csj_1", "csj_2", "csj_3"]
        );
    }
}
