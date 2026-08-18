// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure per-rule cooldown bookkeeping.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::RuleId;

/// Whether a match should actually be emitted right now, or has been
/// folded into a running suppression count instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emit {
    /// Emit now — either the rule has never fired, or its cooldown window
    /// has elapsed since the last emission.
    Now,
    /// Still inside the rule's cooldown window since the last emission.
    /// `count` is the total number of matches folded into this window so
    /// far (including this one) — content-free, so the endpoint can report
    /// "N suppressed" instead of either an alert storm or silence.
    Suppressed { count: u32 },
}

struct LedgerEntry {
    last_emit: Instant,
    suppressed_since: u32,
}

/// Per-rule-id cooldown state. Pure and in-memory — no I/O, no clock
/// reads of its own (the caller supplies `now`), no persistence. A fresh
/// `CooldownLedger` treats every rule as never having fired.
#[derive(Default)]
pub struct CooldownLedger {
    entries: HashMap<RuleId, LedgerEntry>,
}

impl CooldownLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decide whether `rule_id`'s match at `now` should be emitted, given
    /// its `cooldown` duration.
    ///
    /// First-ever match for a rule: always `Emit::Now`. Otherwise: `Now`
    /// if at least `cooldown` has elapsed since the last emission (and the
    /// suppression counter resets to 0 for the next window); `Suppressed`
    /// otherwise, with `count` incremented.
    pub fn should_emit(&mut self, rule_id: RuleId, now: Instant, cooldown: Duration) -> Emit {
        match self.entries.entry(rule_id) {
            Entry::Vacant(slot) => {
                slot.insert(LedgerEntry { last_emit: now, suppressed_since: 0 });
                Emit::Now
            }
            Entry::Occupied(mut slot) => {
                let entry = slot.get_mut();
                // `saturating_duration_since` rather than `duration_since`:
                // this is caller-supplied time (tests may construct
                // `Instant`s that aren't monotonically increasing relative
                // to a prior call), and this ledger has no business
                // panicking over clock arithmetic either way.
                if now.saturating_duration_since(entry.last_emit) >= cooldown {
                    entry.last_emit = now;
                    entry.suppressed_since = 0;
                    Emit::Now
                } else {
                    entry.suppressed_since += 1;
                    Emit::Suppressed { count: entry.suppressed_since }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rid(s: &str) -> RuleId {
        RuleId::new(s).unwrap()
    }

    #[test]
    fn first_match_always_emits() {
        let mut ledger = CooldownLedger::new();
        let now = Instant::now();
        assert_eq!(
            ledger.should_emit(rid("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b"), now, Duration::from_secs(60)),
            Emit::Now
        );
    }

    #[test]
    fn second_match_within_cooldown_is_suppressed_and_counted() {
        let mut ledger = CooldownLedger::new();
        let id = rid("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b");
        let t0 = Instant::now();
        assert_eq!(ledger.should_emit(id.clone(), t0, Duration::from_secs(60)), Emit::Now);

        let t1 = t0 + Duration::from_secs(10);
        assert_eq!(
            ledger.should_emit(id.clone(), t1, Duration::from_secs(60)),
            Emit::Suppressed { count: 1 }
        );

        let t2 = t0 + Duration::from_secs(20);
        assert_eq!(
            ledger.should_emit(id, t2, Duration::from_secs(60)),
            Emit::Suppressed { count: 2 }
        );
    }

    #[test]
    fn match_after_cooldown_elapses_emits_and_resets_counter() {
        let mut ledger = CooldownLedger::new();
        let id = rid("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b");
        let t0 = Instant::now();
        ledger.should_emit(id.clone(), t0, Duration::from_secs(60));
        ledger.should_emit(id.clone(), t0 + Duration::from_secs(10), Duration::from_secs(60));

        let after_cooldown = t0 + Duration::from_secs(61);
        assert_eq!(
            ledger.should_emit(id.clone(), after_cooldown, Duration::from_secs(60)),
            Emit::Now
        );

        // Counter reset — the very next suppressed match starts back at 1.
        assert_eq!(
            ledger.should_emit(id, after_cooldown + Duration::from_secs(1), Duration::from_secs(60)),
            Emit::Suppressed { count: 1 }
        );
    }

    #[test]
    fn zero_cooldown_always_emits() {
        let mut ledger = CooldownLedger::new();
        let id = rid("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b");
        let t0 = Instant::now();
        ledger.should_emit(id.clone(), t0, Duration::ZERO);
        assert_eq!(ledger.should_emit(id, t0, Duration::ZERO), Emit::Now);
    }

    #[test]
    fn independent_rules_have_independent_cooldowns() {
        let mut ledger = CooldownLedger::new();
        let a = rid("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b");
        let b = rid("1a2b3c4d5e6f7a8b9c0d1e2f3a4b0e8f");
        let t0 = Instant::now();
        assert_eq!(ledger.should_emit(a.clone(), t0, Duration::from_secs(60)), Emit::Now);
        // A fresh rule id, same instant — its own cooldown hasn't started.
        assert_eq!(ledger.should_emit(b, t0, Duration::from_secs(60)), Emit::Now);
        // `a` is still in its own cooldown window.
        assert_eq!(
            ledger.should_emit(a, t0 + Duration::from_secs(1), Duration::from_secs(60)),
            Emit::Suppressed { count: 1 }
        );
    }
}
