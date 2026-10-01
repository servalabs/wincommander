// SPDX-License-Identifier: AGPL-3.0-or-later
use std::sync::atomic::{AtomicU8, Ordering};

const KNOWN: u8 = 1;
const PIN_ARMED: u8 = 2;
const LOCK_ON_CLOSE: u8 = 4;
const BORROWED_PANELS: u8 = 8;
static COMMITTED: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WindowPolicy {
    pub pin_armed: bool,
    pub lock_on_close: bool,
    pub has_borrowed_panels: bool,
}

pub(super) fn cached() -> Option<WindowPolicy> {
    decode(COMMITTED.load(Ordering::Acquire))
}

fn decode(bits: u8) -> Option<WindowPolicy> {
    (bits & KNOWN != 0).then_some(WindowPolicy {
        pin_armed: bits & PIN_ARMED != 0,
        lock_on_close: bits & LOCK_ON_CLOSE != 0,
        has_borrowed_panels: bits & BORROWED_PANELS != 0,
    })
}

pub(super) fn publish(settings: &super::AppSettings) {
    let pin_armed = crate::startup_auth::gate_enabled(&settings.ideal.privacy.startup_pin);
    let lock_on_close = settings.app.lock_panel_on_close.unwrap_or(pin_armed);
    let has_borrowed_panels = settings
        .app
        .locked_panel_ids
        .as_ref()
        .is_some_and(|ids| !ids.is_empty());
    // Publish all three decisions together, without any window-thread lock or I/O.
    COMMITTED.store(
        KNOWN
            | if pin_armed { PIN_ARMED } else { 0 }
            | if lock_on_close { LOCK_ON_CLOSE } else { 0 }
            | if has_borrowed_panels {
                BORROWED_PANELS
            } else {
                0
            },
        Ordering::Release,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{
        self, create_default_settings, patch_settings_with, GLOBAL_STATE_TEST_LOCK, SETTINGS_CACHE,
    };
    use std::{sync::mpsc, time::Duration};

    fn seed(pin_armed: bool) {
        let mut settings = create_default_settings();
        settings.snapshot_revision = Some(uuid::Uuid::new_v4());
        if pin_armed {
            settings.ideal.privacy.startup_pin.real_hash = Some("test-hash".into());
        }
        publish(&settings);
        *SETTINGS_CACHE.lock().unwrap() = Some(settings);
    }

    #[test]
    fn unknown_window_policy_is_explicitly_absent() {
        assert_eq!(decode(0), None);
    }

    #[test]
    fn invalidation_preserves_both_no_pin_and_armed_pin_without_inventing_either() {
        let _global = GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for pin_armed in [false, true] {
            seed(pin_armed);
            settings::invalidate_cache();
            assert!(settings::cached_settings().is_none());
            assert_eq!(
                settings::cached_window_policy(),
                Some(WindowPolicy {
                    pin_armed,
                    lock_on_close: pin_armed,
                    has_borrowed_panels: false,
                })
            );
        }
    }

    #[test]
    fn failed_candidate_does_not_replace_the_last_confirmed_window_policy() {
        let _global = GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        seed(false);
        let before = cached();
        let result = patch_settings_with(
            serde_json::json!({
                "ideal": {"privacy": {"startupPin": {"realHash": "uncommitted"}}},
                "app": {"lockPanelOnClose": true, "lockedPanelIds": ["privacy"]}
            }),
            false,
            |_| Err("unknown write result".into()),
            |_, _| {},
        );
        assert!(result.is_err());
        assert!(settings::cached_settings().is_none());
        assert_eq!(cached(), before);
    }

    #[test]
    fn window_policy_reads_progress_during_pending_persistence_then_observe_commit() {
        let _global = GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        seed(true);
        let before = cached();
        let (started, ready) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            patch_settings_with(
                serde_json::json!({
                    "app": {"lockPanelOnClose": false, "lockedPanelIds": ["privacy"]}
                }),
                false,
                |_| {
                    started.send(()).unwrap();
                    blocked.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(())
                },
                |_, _| {},
            )
        });
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let (send, receive) = mpsc::channel();
        let reader =
            std::thread::spawn(move || send.send(settings::cached_window_policy()).unwrap());
        let observed = receive.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        writer.join().unwrap().unwrap();
        reader.join().unwrap();
        assert_eq!(observed.unwrap(), before);
        assert_eq!(
            cached(),
            Some(WindowPolicy {
                pin_armed: true,
                lock_on_close: false,
                has_borrowed_panels: true,
            })
        );
    }

    #[test]
    fn successful_full_write_publishes_resolved_policy() {
        let _global = GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        seed(true);
        let mut updated = settings::read_settings().unwrap();
        updated.ideal.privacy.startup_pin.enabled = Some(false);
        updated.app.locked_panel_ids = Some(vec!["privacy".into()]);
        settings::write_settings_with(&updated, |_| Ok(())).unwrap();
        assert_eq!(
            cached(),
            Some(WindowPolicy {
                pin_armed: false,
                lock_on_close: false,
                has_borrowed_panels: true,
            })
        );
    }
}
