// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service-owned Shield intent journal. This is not an IPC endpoint: callers
//! must first authenticate the peer and independently verify launch identity
//! and any Fleet authority. Until that wiring exists this module grants no
//! permission to start or stop a detector.
#![allow(dead_code)]

use crate::policy_store::PolicyFs;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Mutex};

// Compile the detector source into the service image. A future broker must
// never accept Python source supplied by an IPC caller.
const SHIELD_MODULE: &str =
    include_str!("../../commander-free/scripts/modules/privacy/privacy_shield.ps1");

fn detector_payload(module: &str) -> Result<String, &'static str> {
    let mut found = false;
    let mut payload = String::new();
    for line in module.lines() {
        if !found {
            if line.trim() == "$embeddedScript = @'" {
                found = true;
            }
        } else if line == "'@" {
            return if payload.is_empty() {
                Err("invalid_detector_payload")
            } else {
                Ok(payload)
            };
        } else {
            payload.push_str(line);
            payload.push('\n');
        }
    }
    Err("invalid_detector_payload")
}

pub(crate) fn compiled_detector_payload() -> Result<String, &'static str> {
    detector_payload(SHIELD_MODULE)
}

/// Builds arguments from bounded, independently authorized values. The
/// caller must compare these values with the signed policy before launch.
pub(crate) fn detector_arguments(
    camera: u32,
    mode: &str,
    options: &fleet_proto::ShieldNativeOptions,
    owner_pid: u32,
) -> Result<Vec<String>, &'static str> {
    if camera > 32
        || owner_pid == 0
        || !matches!(mode, "blur_notify" | "notify_only")
        || !options.is_bounded()
        || options.device_wake_multiplier.fract() != 0.0
        || options.multi_face_wake_multiplier.fract() != 0.0
    {
        return Err("invalid_detector_options");
    }
    let mut args = vec!["-".into(), "--wc-privacy-shield".into()];
    for (key, value) in [
        ("--camera", camera.to_string()),
        ("--mode", mode.to_string()),
        ("--model-level", options.model_level.clone()),
        ("--confidence", options.confidence.to_string()),
        ("--overlay-opacity", options.overlay_opacity.to_string()),
        ("--wake-delay-ms", options.wake_delay_ms.to_string()),
        (
            "--device-wake-multiplier",
            (options.device_wake_multiplier as u32).to_string(),
        ),
        (
            "--multi-face-wake-multiplier",
            (options.multi_face_wake_multiplier as u32).to_string(),
        ),
        ("--buffer-frames", options.buffer_frames.to_string()),
        ("--capture-speed", options.capture_speed.to_string()),
        ("--owner-pid", owner_pid.to_string()),
    ] {
        args.push(key.into());
        args.push(value);
    }
    for (flag, enabled) in [
        ("--check-gaze", options.check_gaze),
        ("--check-faces", options.check_faces),
        ("--check-phone", options.check_phone),
        ("--blur-gaze", options.blur_gaze),
        ("--blur-faces", options.blur_faces),
        ("--blur-phone", options.blur_phone),
        ("--capture-on-device", options.capture_on_device),
        ("--capture-on-multi-face", options.capture_on_multi_face),
    ] {
        if enabled {
            args.push(flag.into());
        }
    }
    Ok(args)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LaunchIdentity {
    pub sid: String,
    pub session_id: u32,
    pub process_id: u32,
    /// OS process creation time, not a client-supplied timestamp.
    pub creation_time: u64,
}

impl LaunchIdentity {
    fn valid(&self) -> bool {
        !self.sid.is_empty()
            && self.session_id != 0
            && self.process_id != 0
            && self.creation_time != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LaunchReceipt {
    pub identity: LaunchIdentity,
    pub fleet_owned: bool,
    pub sequence: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingLaunch {
    sid: String,
    session_id: u32,
    fleet_owned: bool,
    sequence: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    sequence: u64,
    launch: Option<LaunchReceipt>,
    stop_sequence: Option<u64>,
    #[serde(default)]
    pending: Option<PendingLaunch>,
}

pub(crate) struct ShieldJournal {
    fs: Box<dyn PolicyFs>,
    path: PathBuf,
    state: Mutex<Option<State>>,
}

impl ShieldJournal {
    pub(crate) fn open(fs: Box<dyn PolicyFs>, dir: PathBuf) -> Result<Self, &'static str> {
        fs.ensure_dir_secure(&dir).map_err(|_| "insecure_journal")?;
        let path = dir.join("shield-intent.json");
        let state = match fs.read(&path) {
            Ok(bytes) => serde_json::from_slice::<State>(&bytes).map_err(|_| "invalid_journal")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(_) => return Err("journal_unavailable"),
        };
        if state
            .launch
            .as_ref()
            .is_some_and(|r| !r.identity.valid() || r.sequence == 0 || r.sequence > state.sequence)
            || state.stop_sequence.is_some_and(|s| {
                s == 0 || s > state.sequence || (state.launch.is_none() && state.pending.is_none())
            })
            || state.pending.as_ref().is_some_and(|p| {
                p.sid.is_empty()
                    || p.session_id == 0
                    || p.sequence == 0
                    || p.sequence > state.sequence
            })
        {
            return Err("invalid_journal");
        }
        Ok(Self {
            fs,
            path,
            state: Mutex::new(Some(state)),
        })
    }

    fn update<T>(
        &self,
        apply: impl FnOnce(&mut State) -> Result<T, &'static str>,
    ) -> Result<T, &'static str> {
        let mut current = self.state.lock().map_err(|_| "journal_unavailable")?;
        let mut next = current.as_ref().ok_or("journal_unavailable")?.clone();
        let result = apply(&mut next)?;
        let bytes = serde_json::to_vec(&next).map_err(|_| "invalid_journal")?;
        if self.fs.atomic_write(&self.path, &bytes).is_err() {
            // A failed write can still have reached disk; refuse further
            // decisions until a fresh open rereads the protected journal.
            *current = None;
            return Err("journal_unavailable");
        }
        *current = Some(next);
        Ok(result)
    }

    /// The caller MUST derive identity from a held process handle and the
    /// captured pipe token; a process command line or UI field is not proof.
    pub(crate) fn sequence(&self) -> Result<u64, &'static str> {
        Ok(self
            .state
            .lock()
            .map_err(|_| "journal_unavailable")?
            .as_ref()
            .ok_or("journal_unavailable")?
            .sequence)
    }

    /// Reserve a launch only after authenticating the session and verifying
    /// any Fleet instruction in the service. Stop can supersede this even if
    /// the detector process has not yet produced a receipt.
    pub(crate) fn reserve_launch(
        &self,
        sid: &str,
        session_id: u32,
        fleet_owned: bool,
    ) -> Result<u64, &'static str> {
        if sid.is_empty() || session_id == 0 {
            return Err("invalid_identity");
        }
        self.update(|state| {
            state.sequence = state.sequence.checked_add(1).ok_or("sequence_exhausted")?;
            state.pending = Some(PendingLaunch {
                sid: sid.into(),
                session_id,
                fleet_owned,
                sequence: state.sequence,
            });
            Ok(state.sequence)
        })
    }

    pub(crate) fn record_verified_launch(
        &self,
        expected_sequence: u64,
        identity: LaunchIdentity,
        fleet_owned: bool,
    ) -> Result<LaunchReceipt, &'static str> {
        if !identity.valid() {
            return Err("invalid_identity");
        }
        self.update(|state| {
            if state.sequence != expected_sequence {
                return Err("intent_superseded");
            }
            let pending = state.pending.as_ref().ok_or("ownership_unknown")?;
            if pending.sequence != expected_sequence
                || pending.sid != identity.sid
                || pending.session_id != identity.session_id
                || pending.fleet_owned != fleet_owned
            {
                return Err("ownership_unverified");
            }
            state.sequence = state.sequence.checked_add(1).ok_or("sequence_exhausted")?;
            let receipt = LaunchReceipt {
                identity,
                fleet_owned,
                sequence: state.sequence,
            };
            state.launch = Some(receipt.clone());
            state.pending = None;
            Ok(receipt)
        })
    }

    /// Peer SID/session must come from the service's authenticated pipe token.
    /// A local user cannot supersede another session or a Fleet-owned launch.
    pub(crate) fn record_local_stop(
        &self,
        sid: &str,
        session_id: u32,
    ) -> Result<u64, &'static str> {
        self.update(|state| {
            let allowed = if let Some(pending) = &state.pending {
                !pending.fleet_owned && pending.sid == sid && pending.session_id == session_id
            } else if let Some(launch) = &state.launch {
                !launch.fleet_owned
                    && launch.identity.sid == sid
                    && launch.identity.session_id == session_id
            } else {
                return Err("ownership_unknown");
            };
            if !allowed {
                return Err("ownership_unverified");
            }
            state.sequence = state.sequence.checked_add(1).ok_or("sequence_exhausted")?;
            state.stop_sequence = Some(state.sequence);
            Ok(state.sequence)
        })
    }

    /// Call only after independently verifying a newer signed Fleet Stop. A
    /// receipt must match exactly so a stale Stop cannot target a later launch.
    pub(crate) fn record_verified_fleet_stop(
        &self,
        receipt: &LaunchReceipt,
    ) -> Result<u64, &'static str> {
        self.update(|state| {
            if !receipt.fleet_owned
                || state.launch.as_ref() != Some(receipt)
                || state.pending.is_some()
                || state
                    .stop_sequence
                    .is_some_and(|stop| stop > receipt.sequence)
            {
                return Err("ownership_unverified");
            }
            state.sequence = state.sequence.checked_add(1).ok_or("sequence_exhausted")?;
            state.stop_sequence = Some(state.sequence);
            Ok(state.sequence)
        })
    }

    /// Check immediately before and after any restart. A later stop supersedes
    /// the captured launch; unavailable/corrupt storage must never mean current.
    pub(crate) fn launch_still_current(
        &self,
        receipt: &LaunchReceipt,
    ) -> Result<bool, &'static str> {
        let state = self.state.lock().map_err(|_| "journal_unavailable")?;
        let state = state.as_ref().ok_or("journal_unavailable")?;
        Ok(state.launch.as_ref() == Some(receipt)
            && state
                .pending
                .as_ref()
                .is_none_or(|p| p.sequence < receipt.sequence)
            && state.stop_sequence.is_none_or(|s| s < receipt.sequence))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_store::PolicyStoreError;
    use std::{io, path::Path, sync::Arc};
    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<Option<Vec<u8>>>>);
    impl PolicyFs for Memory {
        fn read(&self, _: &Path) -> io::Result<Vec<u8>> {
            self.0
                .lock()
                .unwrap()
                .clone()
                .ok_or(io::ErrorKind::NotFound.into())
        }
        fn atomic_write(&self, _: &Path, bytes: &[u8]) -> io::Result<()> {
            *self.0.lock().unwrap() = Some(bytes.to_vec());
            Ok(())
        }
        fn ensure_dir_secure(&self, _: &Path) -> Result<(), PolicyStoreError> {
            Ok(())
        }
    }
    fn identity() -> LaunchIdentity {
        LaunchIdentity {
            sid: "S-1-5-21-1".into(),
            session_id: 1,
            process_id: 42,
            creation_time: 55,
        }
    }
    #[test]
    fn payload_is_built_into_service_and_must_be_complete() {
        let payload = compiled_detector_payload().unwrap();
        assert!(payload.contains("class ShieldApp"));
        assert!(payload.contains("if __name__ == \"__main__\":"));
        assert_eq!(
            detector_payload("$embeddedScript = @'\nmissing terminator"),
            Err("invalid_detector_payload")
        );
        assert_eq!(
            detector_payload("$embeddedScript = @'\n'@"),
            Err("invalid_detector_payload")
        );
    }
    #[test]
    fn launch_arguments_reject_unbounded_and_non_integral_options() {
        let mut options = fleet_proto::ShieldNativeOptions {
            check_gaze: true,
            check_faces: false,
            check_phone: false,
            blur_gaze: true,
            blur_faces: false,
            blur_phone: false,
            capture_on_device: false,
            capture_on_multi_face: false,
            model_level: "nano".into(),
            confidence: 0.5,
            overlay_opacity: 200,
            wake_delay_ms: 150,
            device_wake_multiplier: 2.0,
            multi_face_wake_multiplier: 2.0,
            buffer_frames: 2,
            capture_speed: 1,
        };
        let args = detector_arguments(0, "blur_notify", &options, 12).unwrap();
        assert!(args.contains(&"--check-gaze".to_string()));
        assert!(!args.contains(&"--check-phone".to_string()));
        assert!(detector_arguments(33, "blur_notify", &options, 12).is_err());
        assert!(detector_arguments(0, "other", &options, 12).is_err());
        options.device_wake_multiplier = 1.5;
        assert!(detector_arguments(0, "blur_notify", &options, 12).is_err());
    }
    #[test]
    fn stop_is_durable_and_bound_to_session() {
        let fs = Memory::default();
        let journal = ShieldJournal::open(Box::new(fs.clone()), "test".into()).unwrap();
        let reserved = journal.reserve_launch("S-1-5-21-1", 1, false).unwrap();
        let launch = journal
            .record_verified_launch(reserved, identity(), false)
            .unwrap();
        assert_eq!(
            journal.record_local_stop("S-1-5-21-1", 2),
            Err("ownership_unverified")
        );
        assert_eq!(
            journal.record_local_stop("other", 1),
            Err("ownership_unverified")
        );
        journal.record_local_stop("S-1-5-21-1", 1).unwrap();
        assert!(!journal.launch_still_current(&launch).unwrap());
        drop(journal);
        let restored = ShieldJournal::open(Box::new(fs), "test".into()).unwrap();
        assert!(!restored.launch_still_current(&launch).unwrap());
        assert_eq!(
            restored.record_verified_launch(reserved, identity(), false),
            Err("intent_superseded")
        );
        let reserved = restored.reserve_launch("S-1-5-21-1", 1, false).unwrap();
        let later = restored
            .record_verified_launch(reserved, identity(), false)
            .unwrap();
        assert!(restored.launch_still_current(&later).unwrap());
        assert!(!restored.launch_still_current(&launch).unwrap());
    }
    #[test]
    fn fleet_owner_cannot_be_locally_superseded() {
        let journal = ShieldJournal::open(Box::new(Memory::default()), "test".into()).unwrap();
        let reserved = journal.reserve_launch("S-1-5-21-1", 1, true).unwrap();
        let receipt = journal
            .record_verified_launch(reserved, identity(), true)
            .unwrap();
        assert_eq!(
            journal.record_local_stop("S-1-5-21-1", 1),
            Err("ownership_unverified")
        );
        assert!(journal.launch_still_current(&receipt).unwrap());
        journal.record_verified_fleet_stop(&receipt).unwrap();
        assert!(!journal.launch_still_current(&receipt).unwrap());
    }
    #[test]
    fn stop_during_first_launch_survives_restart_and_rejects_receipt() {
        let fs = Memory::default();
        let journal = ShieldJournal::open(Box::new(fs.clone()), "test".into()).unwrap();
        let reserved = journal.reserve_launch("S-1-5-21-1", 1, false).unwrap();
        journal.record_local_stop("S-1-5-21-1", 1).unwrap();
        drop(journal);
        let restored = ShieldJournal::open(Box::new(fs), "test".into()).unwrap();
        assert_eq!(
            restored.record_verified_launch(reserved, identity(), false),
            Err("intent_superseded")
        );
    }
    #[test]
    fn malformed_persisted_state_fails_closed() {
        let fs = Memory::default();
        *fs.0.lock().unwrap() = Some(br#"{"sequence":0,"launch":null,"stop_sequence":1}"#.to_vec());
        assert!(matches!(
            ShieldJournal::open(Box::new(fs), "test".into()),
            Err("invalid_journal")
        ));
    }

    #[test]
    fn concurrent_stops_are_serialized() {
        let journal =
            Arc::new(ShieldJournal::open(Box::new(Memory::default()), "test".into()).unwrap());
        let reserved = journal.reserve_launch("S-1-5-21-1", 1, false).unwrap();
        let launch = journal
            .record_verified_launch(reserved, identity(), false)
            .unwrap();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let journal = Arc::clone(&journal);
                std::thread::spawn(move || journal.record_local_stop("S-1-5-21-1", 1).unwrap())
            })
            .collect();
        let mut sequences: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        sequences.sort_unstable();
        assert_eq!(sequences, (3..=10).collect::<Vec<_>>());
        assert!(!journal.launch_still_current(&launch).unwrap());
    }

    #[test]
    fn ambiguous_write_failure_disables_further_decisions() {
        struct FailWrite;
        impl PolicyFs for FailWrite {
            fn read(&self, _: &Path) -> io::Result<Vec<u8>> {
                Err(io::ErrorKind::NotFound.into())
            }
            fn atomic_write(&self, _: &Path, _: &[u8]) -> io::Result<()> {
                Err(io::ErrorKind::Other.into())
            }
            fn ensure_dir_secure(&self, _: &Path) -> Result<(), PolicyStoreError> {
                Ok(())
            }
        }
        let journal = ShieldJournal::open(Box::new(FailWrite), "test".into()).unwrap();
        assert_eq!(
            journal.reserve_launch("S-1-5-21-1", 1, false),
            Err("journal_unavailable")
        );
        assert_eq!(journal.sequence(), Err("journal_unavailable"));
    }
}
