// SPDX-License-Identifier: AGPL-3.0-or-later
//! Device-minted event ids.
//!
//! `wincmd_shared::fleet::ClipboardEventReport.event_id` is documented as
//! "device-minted UUIDv7 — the idempotency key" (plan §4.4). `fleet-proto`
//! itself carries zero `uuid` dependency by design (its own doc comment),
//! so minting happens here, in the endpoint crate that actually produces
//! the event — `fleet-proto`'s route layer only validates the resulting
//! string, it never mints one.

/// Mint a fresh UUIDv7 event id as its canonical string form. UUIDv7 is
/// time-ordered, which is why the wire doc calls it out specifically
/// (unlike v4, two ids minted moments apart sort adjacently — useful for
/// any future dedup/ordering work on the receiving end, though this crate
/// itself never relies on that ordering).
pub fn mint_event_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_a_parseable_uuid() {
        let id = mint_event_id();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    #[test]
    fn mints_distinct_ids() {
        let a = mint_event_id();
        let b = mint_event_id();
        assert_ne!(a, b);
    }
}
