//! Wake-up for the Fleet apply loop when the Pro agent reports that a command
//! finished on this device.
//!
//! The apply loop in `lib.rs` runs on a 60 s timer. Pro cannot push policy or
//! command state into Free, but it can send a notification over the existing
//! authenticated sidecar pipe; this module turns that notice into an immediate
//! pass instead of a wait for the next tick. The notice is only a trigger: the
//! pass still reads the verified hand-off from Pro and the UI still re-reads
//! settings, so no value travels in the notice.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Pro notification that a Fleet command or policy epoch changed this device.
pub(crate) const DEVICE_STATE_CHANGED_EVENT: &str = "device-state-changed";

/// Minimum gap between two passes when something other than a device-state
/// notice (trust established, a Shield tick behind the epoch) woke the loop.
const GENERIC_WAKE_GAP: Duration = Duration::from_secs(10);
/// Minimum gap after a device-state notice. Commands arrive at most once per
/// 10 s check-in, so this still bounds load on a failing sidecar.
const STATE_WAKE_GAP: Duration = Duration::from_secs(2);

static STATE_NOTICE_PENDING: AtomicBool = AtomicBool::new(false);

/// Handle a verified Pro notification. Returns whether it asked for a pass.
/// Safe from any thread: it only sets a flag and signals the loop.
pub(crate) fn on_pro_notification(event: &str) -> bool {
    if event != DEVICE_STATE_CHANGED_EVENT {
        return false;
    }
    STATE_NOTICE_PENDING.store(true, Ordering::SeqCst);
    crate::fleet_agent::request_epoch_adoption();
    true
}

/// Consume the pending flag at the start of a pass. `true` means the pass was
/// asked for by a device-state notice and should apply first.
pub(crate) fn take_state_notice() -> bool {
    STATE_NOTICE_PENDING.swap(false, Ordering::SeqCst)
}

/// Minimum spacing between an early-woken pass and the previous one.
pub(crate) fn early_wake_gap(state_notice: bool) -> Duration {
    if state_notice {
        STATE_WAKE_GAP
    } else {
        GENERIC_WAKE_GAP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test owns the process-wide flag so parallel tests cannot race it.
    #[test]
    fn only_the_device_state_notice_requests_a_fast_pass_and_it_is_consumed_once() {
        assert!(!take_state_notice());
        assert!(!on_pro_notification("wifi-guard-detected"));
        assert!(!on_pro_notification("privacy-shield-event"));
        assert!(!take_state_notice(), "unrelated events must not set the flag");

        assert!(on_pro_notification("device-state-changed"));
        assert!(on_pro_notification("device-state-changed"));
        assert!(take_state_notice(), "a notice is remembered until a pass takes it");
        assert!(!take_state_notice(), "and consumed exactly once");
    }

    #[test]
    fn a_state_notice_shortens_the_early_wake_gap_but_never_removes_it() {
        assert_eq!(early_wake_gap(false), Duration::from_secs(10));
        assert_eq!(early_wake_gap(true), Duration::from_secs(2));
        assert!(early_wake_gap(true) > Duration::ZERO);
    }
}
