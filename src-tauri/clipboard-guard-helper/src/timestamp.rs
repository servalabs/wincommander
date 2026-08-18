// SPDX-License-Identifier: AGPL-3.0-or-later
//! Wall-clock RFC3339 timestamps for the wire report.
//!
//! `wincmd_shared::fleet::ClipboardEventReport.occurred_at` is documented
//! as "RFC3339, agent clock" — a plain `String`, matching every other
//! `occurred_at`/`window_start` producer in this codebase
//! (`paste_monitor.rs`'s `chrono::Utc::now().to_rfc3339()`,
//! `InkReceiptReport.occurred_at`'s doc). `fleet-proto` itself avoids
//! `chrono` for its own dependency-budget reasons, but nothing stops the
//! endpoint crate that actually stamps the timestamp from using it.

/// Current UTC time as RFC3339 text.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_a_parseable_rfc3339_timestamp() {
        let ts = now_rfc3339();
        assert!(chrono::DateTime::parse_from_rfc3339(&ts).is_ok());
    }
}
