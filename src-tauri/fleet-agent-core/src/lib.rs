// SPDX-License-Identifier: AGPL-3.0-or-later
//! Generic fleet-client loop: enroll, HMAC-authenticated check-in, ed25519 command
//! verification, and dispatch — shared by every product's fleet agent
//! (TuxCommander, WinCommander Pro, and future fleet-managed agents).
//!
//! This crate is the canonical copy of the client loop previously hand-maintained
//! in `tc-agent/src/fleet.rs`. All canonical signing byte-layout logic (the actual
//! preimage construction + raw ed25519 verify) lives in [`fleet_proto`] — this
//! crate never reimplements it; see [`verify::verify_command`].
//!
//! # Feature split (AV-hygiene / Free-tier safety)
//!
//! - `types` (always available, even with `default-features = false`): the wire
//!   types + [`verify::verify_command`] + [`verify::evict_stale_nonces`] +
//!   [`dispatch::process_checkin`]. **No `reqwest`, no `tokio`.** A types-only
//!   consumer (e.g. WinCommander Free) can verify a signed command locally
//!   without linking any transport/network code, keeping the AV-hygiene
//!   `strings-grep-free` gate clean.
//! - `transport` (default): adds [`transport::enroll`],
//!   [`transport::run_checkin_cycle`], [`transport::spawn_fleet_client`], and the
//!   HTTP/backoff/jitter helpers. This is the part of the crate that depends on
//!   `reqwest`/`tokio`.
//!
//! # Security invariants preserved from `tc-agent/fleet.rs` (NON-NEGOTIABLE)
//!
//! - Signatures are verified against the **pinned** `command_pubkey` stored at
//!   enroll time. A server that returns a different key on re-enroll is refused.
//! - Capped-exponential + full-jitter backoff; `is_transient_status` = 5xx/429 only.
//! - Trailing slash on the configured URL is trimmed.
//! - `checkin_secret` is taken as the **raw UTF-8 bytes** of the env var (not
//!   hashed, not hex/base64-decoded) — matches the server's verbatim storage.
//! - A per-device enroll secret (`checkin_secret_b64` from the enroll response)
//!   **overrides** the config-supplied secret for all subsequent check-ins.
//! - The five destruction/trigger gates (arming, profile, live-primitives, etc.)
//!   are enforced by the platform via its [`dispatch::FleetActions`]
//!   implementation, NOT by this crate — `fleet-agent-core` only requests
//!   actions through the same authenticated path as every other trigger source.
//! - TLS is server-pinned by SPKI SHA-256 when a pin is configured (see
//!   [`pinning::build_client`]); otherwise default WebPKI TLS verification
//!   applies unchanged. Pinning is OPTIONAL and config-driven — it is never
//!   required for the client to be constructed.

pub mod config;
pub mod dispatch;
pub mod state;
pub mod util;
pub mod verify;

#[cfg(feature = "transport")]
pub mod pinning;
#[cfg(feature = "transport")]
pub mod transport;

// Re-export fleet-proto so consumers of the `types` slice have a single import.
pub use fleet_proto;

pub use config::FleetConfig;
pub use dispatch::{
    execute_pending_search_jobs, process_checkin, FleetActions, FleetDispatchOutcome,
    SearchJobReport, SearchRunner, FLEET_TRIGGER_SOURCE,
};
pub use state::{
    new_shared_fleet_state, new_shared_fleet_state_with_persist, FleetClientState, FleetStatus,
    SecretStore, SharedFleetState,
};
pub use verify::{
    compute_checkin_hmac, compute_request_hmac_v2, decode_verifying_key, evict_stale_nonces,
    request_hmac_preimage_v2, verify_command, CheckinRequest, CheckinResponse, EnrollRequest,
    EnrollResponse, PendingSearchJob, SearchHit, SearchResultReport, SignedCommand,
    ENROLL_PROTOCOL_VERSION, HMAC_BODY_V2_CAPABILITY, HMAC_VERSION_V2,
};

#[cfg(feature = "transport")]
pub use transport::{
    backoff_duration, enroll, fleet_post, is_transient_status, report_search_results,
    run_checkin_cycle, spawn_fleet_client,
};

#[cfg(feature = "transport")]
pub use pinning::{build_client, SpkiPin, SpkiPinSet};
