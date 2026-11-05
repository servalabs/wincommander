// SPDX-License-Identifier: AGPL-3.0-or-later
//! Atomic service-owned storage of a verified hybrid enrollment credential.
//! This is not wired to the live connection worker or an IPC verb.
use super::{
    effective_hybrid_trust,
    hybrid_enrollment::{hybrid_claim_matches, ClaimedHybridEnrollment},
    hybrid_reply::VerifiedHybridReply,
    FleetMachinePolicy, StoredHybridCredential,
};
use crate::fleet_credential::{self, CredentialBinding, Protector};
use base64::{engine::general_purpose::STANDARD, Engine as _};

impl FleetMachinePolicy {
    /// Consume a verified reply and its exact current claim under the same
    /// mutex used by cancellation. The credential and epoch floor are written
    /// together. The existing classical worker stays disabled.
    pub(crate) fn stage_hybrid_credential(
        &self,
        claim: &ClaimedHybridEnrollment,
        reply: VerifiedHybridReply,
        protector: &dyn Protector,
    ) -> Result<(), &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        if !hybrid_claim_matches(&guard, claim) {
            return Err("hybrid_enrollment_not_authorized");
        }
        let current = guard.record.as_ref().ok_or("not_provisioned")?;
        let keys = effective_hybrid_trust(current).ok_or("hybrid_trust_unavailable")?;
        if !reply.matches(claim, &keys)
            || reply.epoch() < keys.minimum_epoch
            || !current
                .origins
                .iter()
                .any(|origin| origin == claim.origin())
        {
            return Err("hybrid_enrollment_not_authorized");
        }
        let binding = CredentialBinding {
            org_id: &current.trust.org_id,
            device_id: &current.trust.device_id,
            enrollment_generation: reply.generation(),
            origin: claim.origin(),
        };
        let sealed = fleet_credential::seal(protector, &binding, reply.secret())?;
        // The protector can take time; never commit an expired claim.
        if !hybrid_claim_matches(&guard, claim) {
            return Err("hybrid_enrollment_not_authorized");
        }
        let mut next = guard.clone();
        let record = next.record.as_mut().ok_or("not_provisioned")?;
        if record.hybrid_initial_pending {
            record.trust.enrollment_generation = reply.generation().to_owned();
            record.hybrid_initial_pending = false;
        } else if record.trust.enrollment_generation != reply.generation() {
            return Err("hybrid_enrollment_not_authorized");
        }
        record.credential_revision = record
            .credential_revision
            .checked_add(1)
            .ok_or("revision_exhausted")?;
        record.connection_revision = record
            .connection_revision
            .checked_add(1)
            .ok_or("revision_exhausted")?;
        record.hybrid_epoch_floor = reply.epoch();
        record.hybrid_credential = Some(StoredHybridCredential {
            origin: claim.origin().to_owned(),
            ciphertext: STANDARD.encode(&sealed),
            epoch: reply.epoch(),
        });
        record.cleared = None;
        next.hybrid_claim = None;
        next.hybrid_enrollment = None;
        match self.persist(&next) {
            Ok(()) => {
                *guard = next;
                Ok(())
            }
            Err(error) => {
                // A failed write may have reached disk. Fence all hybrid
                // trust use until an administrator confirms the exact state.
                next.record
                    .as_mut()
                    .ok_or("not_provisioned")?
                    .hybrid_unconfirmed = true;
                *guard = next;
                Err(error)
            }
        }
    }
}
