// SPDX-License-Identifier: AGPL-3.0-or-later
//! Closed action catalogue a matched rule can request.

use serde::{Deserialize, Serialize};

/// What a rule asks the endpoint to do on a match. Closed enum, ordered by
/// the engine (author intent, `Rule.actions` order) not enforced execution
/// order here — sequencing is the endpoint's job, this crate only carries
/// the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Toast / native notification — no clipboard content in the body.
    NotifyUser,
    /// Erase the clipboard immediately.
    ClearClipboard,
    /// Replace clipboard content with a placeholder pending review, rather
    /// than destroying it outright.
    QuarantineClipboard,
    /// Persist a content-free local receipt (ink-receipt style ticket).
    RecordLocalReceipt,
    /// Send a content-free signal to the fleet server.
    ReportFleet,
    /// Page an administrator (highest-severity rules only, by convention —
    /// not enforced by this crate).
    AlertAdmin,
}
