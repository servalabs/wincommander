// SPDX-License-Identifier: AGPL-3.0-or-later
//! Serialize a caller's backup mutations without blocking read-only attestation.
use super::*;
use std::sync::{Arc, OnceLock, Weak};

type Operation = Arc<Mutex<()>>;
static OPERATIONS: OnceLock<Mutex<HashMap<String, Weak<Mutex<()>>>>> = OnceLock::new();

pub(super) fn for_target(
    caller_sid: &str,
    session: u32,
    slot: u8,
) -> Result<Operation, VaultMountReason> {
    let mut all = OPERATIONS.get_or_init(|| Mutex::new(HashMap::new()))
        .lock().map_err(|_| VaultMountReason::BrokerRejected)?;
    all.retain(|_, entry| entry.strong_count() > 0);
    let key = format!("{caller_sid}|{session}|{slot}");
    if let Some(existing) = all.get(&key).and_then(Weak::upgrade) {
        return Ok(existing);
    }
    if all.len() >= 128 {
        return Err(VaultMountReason::BrokerRejected);
    }
    let operation = Arc::new(Mutex::new(()));
    all.insert(key, Arc::downgrade(&operation));
    Ok(operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn concurrent_actions_share_only_the_same_caller_session_and_slot() {
        let operation = for_target("scope-test", 7, 12).unwrap();
        assert!(Arc::ptr_eq(&operation, &for_target("scope-test", 7, 12).unwrap()));
        for (sid, session, slot) in [("other", 7, 12), ("scope-test", 8, 12), ("scope-test", 7, 13)] {
            assert!(!Arc::ptr_eq(&operation, &for_target(sid, session, slot).unwrap()));
        }
    }

    #[test]
    fn replacement_waits_until_old_renewal_stop_dispatch_is_finished() {
        let operation = for_target("renewal-race-test", 7, 12).unwrap();
        let old_renewal = operation.lock().unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let operation = for_target("renewal-race-test", 7, 12).unwrap();
            entered_tx.send(()).unwrap();
            let _replacement = operation.lock().unwrap();
            finished_tx.send(()).unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(finished_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        // Production retains this same guard through retire + stop_record.
        drop(old_renewal);
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }
}
