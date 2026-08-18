// SPDX-License-Identifier: AGPL-3.0-or-later
//! Verdict/rule severity.

use serde::{Deserialize, Serialize};

/// How urgently a match should be surfaced. Four tiers rather than the
/// legacy free-tier's two ("warning"/"danger") so custom fleet-authored
/// rules have headroom above and below the migrated builtins — see
/// `BuiltinPattern::default_severity` for exactly where the old two-tier
/// scheme lands in this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational — logged, not alarmed.
    Info,
    /// The legacy free-tier "warning" tier: you copied your own secret,
    /// be careful where you paste it.
    Warn,
    /// The legacy free-tier "danger" tier: the clipboard likely holds
    /// someone else's malicious payload (ClickFix, pastejacking) or a
    /// near-certain phishing/spoofing indicator.
    High,
    /// Reserved for custom fleet-authored rules that need to outrank every
    /// migrated builtin — no builtin pattern maps here.
    Critical,
}
