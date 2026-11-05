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
}

impl Default for SpaceGuard {
    fn default() -> Self {
        Self { state: SpaceState::Ready }
    }
}

impl SpaceGuard {
    pub(crate) fn observe_path(&mut self, index_dir: &Path) -> SpaceState {
        self.observe(fs4::available_space(index_dir))
    }

    fn observe(&mut self, available: std::io::Result<u64>) -> SpaceState {
        self.state = match available {
            Err(_) => SpaceState::PausedUnknown,
            Ok(bytes) if bytes < PAUSE_BELOW => SpaceState::PausedLowSpace,
            Ok(bytes) if self.state != SpaceState::Ready && bytes < RESUME_AT => SpaceState::PausedLowSpace,
            Ok(_) if self.state != SpaceState::Ready => SpaceState::ReconcileRequired,
            Ok(_) => SpaceState::Ready,
        };
        self.state
    }

    pub(crate) fn reconciliation_succeeded(&mut self) {
        if self.state == SpaceState::ReconcileRequired {
            self.state = SpaceState::Ready;
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
        guard.reconciliation_succeeded();
        assert_eq!(guard.observe(Ok(RESUME_AT - 1)), SpaceState::PausedLowSpace);
        assert_eq!(guard.observe(Ok(RESUME_AT)), SpaceState::ReconcileRequired);
        assert_eq!(guard.observe(Ok(RESUME_AT)), SpaceState::ReconcileRequired);
        assert_eq!(guard.observe(Ok(RESUME_AT - 1)), SpaceState::PausedLowSpace);
        assert_eq!(guard.observe(Ok(RESUME_AT)), SpaceState::ReconcileRequired);
        guard.reconciliation_succeeded();
        assert_eq!(guard.observe(Ok(RESUME_AT)), SpaceState::Ready);
    }

    #[test]
    fn unavailable_reading_pauses_and_requires_reconciliation() {
        let mut guard = SpaceGuard::default();
        assert_eq!(guard.observe(Err(std::io::Error::other("unavailable"))), SpaceState::PausedUnknown);
        assert_eq!(guard.observe(Ok(RESUME_AT)), SpaceState::ReconcileRequired);
        assert_eq!(guard.observe(Ok(PAUSE_BELOW - 1)), SpaceState::PausedLowSpace);
    }
}
