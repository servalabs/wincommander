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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_false() {
        // The conservative starting point: nothing has been proven
        // healthy yet, so nothing should default to `true`.
        let health = HelperHealth::default();
        assert!(!health.helper_running);
        assert!(!health.listener_registered);
        assert!(!health.policy_current);
        assert!(!health.rules_compiled);
        assert!(!health.clear_failing);
        assert!(!health.svc_reachable);
    }

    #[test]
    fn round_trips_through_json() {
        let health = HelperHealth {
            helper_running: true,
            listener_registered: true,
            policy_current: true,
            rules_compiled: true,
            clear_failing: false,
            svc_reachable: true,
        };
        let json = serde_json::to_string(&health).unwrap();
        let back: HelperHealth = serde_json::from_str(&json).unwrap();
        assert_eq!(health, back);
    }
}
