// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded candidate verification and fenced credential replacement.
use super::*;
use crate::fleet_machine_policy::CandidateContext;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

struct Candidate {
    sequence: u64,
    digest: [u8; 32],
    context: CandidateContext,
    secret: Zeroizing<Vec<u8>>,
}
#[derive(Default)]
struct CandidateQueue {
    sequence: u64,
    pending: Option<Candidate>,
    active: Option<(u64, [u8; 32])>,
}
impl CandidateQueue {
    fn push(
        &mut self,
        context: CandidateContext,
        secret: Zeroizing<Vec<u8>>,
        digest: [u8; 32],
    ) -> Result<bool, &'static str> {
        if self
            .active
            .as_ref()
            .is_some_and(|(sequence, active)| *sequence == self.sequence && *active == digest)
            || self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.digest == digest)
        {
            return Ok(false);
        }
        self.sequence = self.sequence.checked_add(1).ok_or("revision_exhausted")?;
        self.pending = Some(Candidate {
            sequence: self.sequence,
            digest,
            context,
            secret,
        });
        Ok(true)
    }
    fn take(&mut self) -> Option<Candidate> {
        let candidate = self.pending.take()?;
        self.active = Some((candidate.sequence, candidate.digest));
        Some(candidate)
    }
    fn finish(&mut self, sequence: u64) {
        if self
            .active
            .as_ref()
            .is_some_and(|(active, _)| *active == sequence)
        {
            self.active = None;
        }
    }
}
fn candidate_slot() -> &'static Mutex<CandidateQueue> {
    static SLOT: OnceLock<Mutex<CandidateQueue>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(CandidateQueue::default()))
}
fn candidate_notify() -> &'static tokio::sync::Notify {
    static NOTIFY: OnceLock<tokio::sync::Notify> = OnceLock::new();
    NOTIFY.get_or_init(tokio::sync::Notify::new)
}

/// Called while holding the queue lock: the most recent candidate alone may
/// commit, and an unsuccessful proof leaves the previous credential intact.
fn finish_candidate(
    slot: &mut CandidateQueue,
    candidate: &Candidate,
    observed: bool,
    store: &FleetMachinePolicy,
    protector: &dyn crate::fleet_credential::Protector,
    updates: &tokio::sync::watch::Sender<()>,
) {
    if observed
        && slot.sequence == candidate.sequence
        && store
            .commit_candidate(&candidate.context, &candidate.secret, protector)
            .is_ok()
    {
        // Notify only after durable installation. Failed or superseded offers
        // cannot interrupt the current credential's bounded retry cadence.
        updates.send_replace(());
    }
    slot.finish(candidate.sequence);
}

/// At most one candidate is active and one newer candidate is retained.
/// A failed candidate never replaces a working protected credential.
#[cfg(windows)]
pub(crate) fn offer(
    store: &FleetMachinePolicy,
    trust: FleetTrust,
    origins: Vec<String>,
    profile: Option<TransportProfile>,
    origin: String,
    secret: Zeroizing<Vec<u8>>,
    connection_token: String,
) -> Result<(), &'static str> {
    if !store.observe_runtime_transport(&trust, &origins, profile.as_ref(), &connection_token)? {
        return Err("transport_profile_changed");
    }
    let profile = profile.ok_or("transport_profile_changed")?;
    if crate::fleet_runtime_env::fingerprint().as_deref()
        != Some(profile.environment_fingerprint.as_str())
    {
        // Suspension preserves any old credential but makes the mismatch
        // visible even before the service has accepted its first candidate.
        let _ = store.observe_runtime_transport(&trust, &origins, None, &connection_token)?;
        return Err("network_configuration_differs");
    }
    let Some(context) = store.prepare_candidate(
        (&trust, &connection_token),
        &origins,
        &profile,
        &origin,
        &secret,
        &crate::fleet_credential::MachineDpapi,
    )?
    else {
        return Ok(());
    };
    let mut hash = Sha256::new();
    hash.update(b"wincommander:fleet-service-offer:v1\0");
    hash.update(&*secret);
    for field in [
        &context.trust.org_id,
        &context.trust.device_id,
        &context.trust.enrollment_generation,
        &context.trust.signer_key,
        &context.origin,
        &context.profile.spki_pins,
        &context.profile.environment_fingerprint,
    ] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field.as_bytes());
    }
    for origin in &context.origins {
        hash.update((origin.len() as u64).to_be_bytes());
        hash.update(origin.as_bytes());
    }
    hash.update(context.revision.to_be_bytes());
    hash.update(context.connection_revision.to_be_bytes());
    let digest: [u8; 32] = hash.finalize().into();
    let mut slot = candidate_slot().lock().map_err(|_| "service_unavailable")?;
    let queued = slot.push(context, secret, digest)?;
    drop(slot);
    if queued {
        candidate_notify().notify_one();
    }
    Ok(())
}

#[cfg(windows)]
pub(super) async fn candidate_worker() {
    loop {
        candidate_notify().notified().await;
        loop {
            let candidate = {
                let Ok(mut slot) = candidate_slot().lock() else {
                    break;
                };
                let Some(candidate) = slot.take() else {
                    break;
                };
                candidate
            };
            let profile = &candidate.context.profile;
            let pins = fleet_agent_core::SpkiPinSet::from_hex_list(&profile.spki_pins);
            let valid_profile = crate::fleet_runtime_env::fingerprint().as_deref()
                == Some(profile.environment_fingerprint.as_str())
                && (profile.spki_pins.trim().is_empty() || pins.is_some());
            let store = crate::fleet_machine_policy::system_store().ok();
            let observed = if valid_profile {
                match (
                    fleet_agent_core::build_client_without_redirects(pins),
                    store,
                ) {
                    (Ok(client), Some(store)) => {
                        attempt(
                            &client,
                            &candidate.context.trust,
                            std::slice::from_ref(&candidate.context.origin),
                            &candidate.secret,
                            || {
                                candidate_slot()
                                    .lock()
                                    .is_ok_and(|slot| slot.sequence == candidate.sequence)
                                    && store.candidate_is_current(&candidate.context)
                            },
                        )
                        .await
                            == Outcome::Observed
                    }
                    _ => false,
                }
            } else {
                false
            };
            if let Ok(mut slot) = candidate_slot().lock() {
                // Hold the queue lock through commit so a newer offer cannot race
                // between this fence and protected credential replacement.
                if let Ok(store) = crate::fleet_machine_policy::system_store() {
                    finish_candidate(
                        &mut slot,
                        &candidate,
                        observed,
                        store,
                        &crate::fleet_credential::MachineDpapi,
                        credential_updates(),
                    );
                } else {
                    slot.finish(candidate.sequence);
                }
                if slot.pending.is_none() {
                    break;
                }
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
#[path = "fleet_transport_candidate_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "fleet_transport_worker_tests.rs"]
mod worker_tests;
