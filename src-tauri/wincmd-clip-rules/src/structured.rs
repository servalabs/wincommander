// SPDX-License-Identifier: AGPL-3.0-or-later
//! Checksum-validated structural matchers — patterns that a regex alone
//! can't verify because the format has an internal check digit.
//!
//! Scope note (recorded for downstream agents): the plan's API sketch
//! names this enum's intent as "Luhn-checked card, mod-97 IBAN, org id
//! formats" — the `...` is illustrative, not a commitment to an
//! unspecified third format. Only the two concretely named checks are
//! implemented: `PaymentCard` (Luhn) and `Iban` (ISO 7064 MOD 97-10). No
//! "org id format" is implemented because none was specified.

use serde::{Deserialize, Serialize};

/// A structurally-validated pattern family — matched by a checksum over
/// candidate digit/alnum runs found in the text, not by a plain regex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum StructuredKind {
    /// A 13–19 digit run (optional space/dash separators) that passes the
    /// Luhn check digit — migrated verbatim from
    /// `commander-free/src/paste_monitor.rs::looks_like_credit_card`
    /// (`Category::PersonalData`, which had no regex in the original
    /// engine).
    PaymentCard,
    /// A 15–34 char alnum run (2 letters + 2 check digits + up to 30
    /// alnum, optional space separators as printed on statements) that
    /// passes the ISO 7064 MOD 97-10 check. New in this crate — the
    /// legacy free-tier engine never had an IBAN detector.
    Iban,
}
