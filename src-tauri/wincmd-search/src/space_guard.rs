// SPDX-License-Identifier: AGPL-3.0-or-later
//! Prototype disk-pressure state machine. Not yet connected to indexing.
//!
//! An observation is advisory, not a reservation: other processes and already
//! running index writes can consume space after it is taken.

use std::path::Path;

const GIB: u64 = 1024 * 1024 * 1024;
const PAUSE_BELOW: u64 = 2 * GIB;
const RESUME_AT: u64 = 3 * GIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpaceState {
    Ready,
    PausedLowSpace,
    PausedUnknown,
    /// A complete reconciliation must succeed before incremental updates resume.
    ReconcileRequired,
}

#[derive(Debug)]
pub(crate) struct SpaceGuard {
    state: SpaceState,
    generation: Option<u64>,
}

impl Default for SpaceGuard {
    fn default() -> Self {
        Self { state: SpaceState::Ready, generation: Some(0) }
    }
}

impl SpaceGuard {
    pub(crate) fn observe_path(&mut self, index_dir: &Path) -> SpaceState {
        self.observe(fs4::available_space(index_dir))
    }

    pub(crate) fn observe(&mut self, available: std::io::Result<u64>) -> SpaceState {
        let next = match available {
            Err(_) => SpaceState::PausedUnknown,
            Ok(bytes) if bytes < PAUSE_BELOW => SpaceState::PausedLowSpace,
            Ok(bytes) if self.state != SpaceState::Ready && bytes < RESUME_AT => SpaceState::PausedLowSpace,
            Ok(_) if self.state != SpaceState::Ready => SpaceState::ReconcileRequired,
            Ok(_) => SpaceState::Ready,
        };
        if self.state != next && matches!(next, SpaceState::PausedLowSpace | SpaceState::PausedUnknown) {
            self.note_change();
        }
        self.state = next;
        self.state
    }

    /// Call for every dropped, coalesced or concurrently observed filesystem event.
    /// Overflow deliberately makes the coordinator require an explicit restart.
    pub(crate) fn note_change(&mut self) {
        self.generation = self.generation.and_then(|generation| generation.checked_add(1));
    }

    /// An overflow or watcher failure invalidates the incremental stream.
    pub(crate) fn require_reconciliation(&mut self) {
        self.note_change();
        if self.state == SpaceState::Ready {
            self.state = SpaceState::ReconcileRequired;
        }
    }

    pub(crate) fn reconciliation_started(&self) -> Option<u64> {
        if self.state == SpaceState::ReconcileRequired { self.generation } else { None }
    }

    /// Only a full successful scan, with no intervening events or pause, can
    /// return to live updates. Call while holding the same lock used by events.
    pub(crate) fn reconciliation_finished(&mut self, token: u64, complete: bool, available: std::io::Result<u64>) -> bool {
        if self.observe(available) == SpaceState::ReconcileRequired
            && complete && self.generation == Some(token) {
            self.state = SpaceState::Ready;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_resume_requires_hysteresis_and_reconciliation() {
        let mut guard = SpaceGuard::default();
        assert_eq!(guard.observe(Ok(PAUSE_BELOW)), SpaceState::Ready);
        assert_eq!(guard.observe(Ok(PAUSE_BELOW - 1)), SpaceState::PausedLowSpace);
        assert!(guard.reconciliation_started().is_none());
        assert_eq!(guard.observe(Ok(RESUME_AT - 1)), SpaceState::PausedLowSpace);
        assert_eq!(guard.observe(Ok(RESUME_AT)), SpaceState::ReconcileRequired);
        assert_eq!(guard.observe(Ok(RESUME_AT)), SpaceState::ReconcileRequired);
        assert_eq!(guard.observe(Ok(RESUME_AT - 1)), SpaceState::PausedLowSpace);
        assert_eq!(guard.observe(Ok(RESUME_AT)), SpaceState::ReconcileRequired);
        let token = guard.reconciliation_started().unwrap();
        assert!(guard.reconciliation_finished(token, true, Ok(RESUME_AT)));
        assert_eq!(guard.observe(Ok(RESUME_AT)), SpaceState::Ready);
    }

    #[test]
    fn unavailable_reading_pauses_and_requires_reconciliation() {
        let mut guard = SpaceGuard::default();
        assert_eq!(guard.observe(Err(std::io::Error::other("unavailable"))), SpaceState::PausedUnknown);
        assert_eq!(guard.observe(Ok(RESUME_AT)), SpaceState::ReconcileRequired);
        assert_eq!(guard.observe(Ok(PAUSE_BELOW - 1)), SpaceState::PausedLowSpace);
    }

    #[test]
    fn changed_or_failed_reconciliation_cannot_resume() {
        let mut guard = SpaceGuard::default();
        guard.observe(Ok(0));
        guard.observe(Ok(RESUME_AT));
        let token = guard.reconciliation_started().unwrap();
        guard.note_change();
        assert!(!guard.reconciliation_finished(token, true, Ok(RESUME_AT)));
        let token = guard.reconciliation_started().unwrap();
        assert!(!guard.reconciliation_finished(token, false, Ok(RESUME_AT)));
        assert!(!guard.reconciliation_finished(token, true, Ok(PAUSE_BELOW - 1)));
        guard.observe(Ok(RESUME_AT));
        assert!(!guard.reconciliation_finished(token, true, Ok(RESUME_AT)));
        let token = guard.reconciliation_started().unwrap();
        assert!(guard.reconciliation_finished(token, true, Ok(RESUME_AT)));
    }

    #[test]
    fn exhausted_generation_fails_closed() {
        let mut guard = SpaceGuard { state: SpaceState::ReconcileRequired, generation: Some(u64::MAX) };
        guard.note_change();
        assert!(guard.reconciliation_started().is_none());
        assert!(!guard.reconciliation_finished(u64::MAX, true, Ok(RESUME_AT)));
    }
}
