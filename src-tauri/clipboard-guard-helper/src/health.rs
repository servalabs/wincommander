// SPDX-License-Identifier: AGPL-3.0-or-later
//! Aggregate on-device health booleans for Clipboard Guard.
//!
//! Field names mirror the `capability_status` catalog-id convention from
//! plan §8.1 — `clipboard_guard.helper_running`, `.listener_registered`,
//! `.policy_current`, `.rules_compiled`, `.clear_failing` — plus
//! `svc_reachable`, which isn't itself a `capability_status` key but is
//! this crate's own signal for whether the last `commander-svc` IPC round
//! trip succeeded.
//!
//! This crate has no dependency on `commander-pro` and does not call
//! `capability_status::collect()` itself — folding these booleans into
//! that map (guarded so "unknown" stays absent rather than becoming a
//! false `false`, per that function's own contract) is the embedding
//! caller's job.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperHealth {
    /// The helper's main loop is up and observing the clipboard.
    pub helper_running: bool,
    /// `true` = event-driven (`AddClipboardFormatListener` succeeded);
    /// `false` = degraded to the polling fallback, or not yet started.
    /// Set by `listener::resolve_listener_mode` — never silently.
    pub listener_registered: bool,
    /// The active policy's `policy_version` matches the most recently
    /// fetched `get_policy` response (i.e. nothing is pending/stale).
    pub policy_current: bool,
    /// The most recent policy install attempt actually compiled — see
    /// `policy::PolicyStore::rules_compiled`. `false` means the PREVIOUS
    /// good ruleset is still active (atomic install, never half-applied).
    pub rules_compiled: bool,
    /// The most recent `ClearClipboard`/`QuarantineClipboard` attempt was
    /// verified to have failed. Sticky enough to surface a real problem
    /// (e.g. some other app is fighting for the clipboard) without
    /// requiring a dashboard poll to catch a single transient blip —
    /// callers should clear it on the next verified success.
    pub clear_failing: bool,
    /// The most recent `commander-svc` IPC round trip (either
    /// `get_policy` or `report_event`) succeeded.
    pub svc_reachable: bool,
}
