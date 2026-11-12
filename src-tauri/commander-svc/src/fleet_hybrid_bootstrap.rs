// SPDX-License-Identifier: AGPL-3.0-or-later
//! Protected first-enrollment trust, without an installed Fleet identity.
use super::{origins, trust_audit, validate_hybrid_trust, validate_transport_profile,
    validate_trust, FleetHybridTrust, FleetMachinePolicy, FleetTrust, Record, State,
    TransportProfile};

impl FleetMachinePolicy {
    /// The caller must authorize an administrator and independently approve all
    /// keys and origins. This is not exposed through the service pipe.
    pub(crate) fn provision_hybrid_bootstrap(
        &self,
        org_id: String,
        device_id: String,
        signer_key: String,
        hybrid: FleetHybridTrust,
        allowed_origins: Vec<String>,
        transport_profile: Option<TransportProfile>,
        actor: &str,
    ) -> Result<FleetTrust, &'static str> {
        let allowed_origins = origins::normalize(&allowed_origins)?;
        if allowed_origins.is_empty() || allowed_origins.iter().any(|origin| !origin.starts_with("https://")) {
            return Err("invalid_origin");
        }
        if let Some(profile) = &transport_profile { validate_transport_profile(profile)?; }
        let trust = FleetTrust { org_id, device_id, signer_key, enrollment_generation: uuid::Uuid::new_v4().to_string() };
        validate_trust(&trust)?;
        validate_hybrid_trust(&trust, &hybrid)?;
        if hybrid.outcome_key_b64.is_none() { return Err("outcome_key_required"); }
        let audit = trust_audit("hybrid_bootstrap", actor, &trust.enrollment_generation)?;
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        if let Some(current) = guard.record.as_ref() {
            if !current.hybrid_initial_pending || current.trust.org_id != trust.org_id
                || current.trust.device_id != trust.device_id || current.trust.signer_key != trust.signer_key
                || current.hybrid_trust.as_ref() != Some(&hybrid) || current.origins != allowed_origins
                || current.transport_profile != transport_profile {
                return Err("already_provisioned");
            }
            let existing = current.trust.clone();
            if current.hybrid_unconfirmed {
                let mut next = guard.clone();
                next.record.as_mut().ok_or("not_provisioned")?.hybrid_unconfirmed = false;
                self.persist(&next)?;
                *guard = next;
            }
            return Ok(existing);
        }
        let next = State {
            record: Some(Record {
                trust: trust.clone(), hybrid_trust: Some(hybrid), hybrid_epoch_floor: 0,
                hybrid_credential: None, hybrid_initial_pending: true, hybrid_unconfirmed: false,
                policy: None, audit: vec![audit], contact: None, origins: allowed_origins,
                transport_profile, credential: None, credential_revision: 0,
                connection_revision: 0, cleared: None, runtime_profile_changed: false,
                service_enabled: false,
            }),
            pending: None, contact_challenge: None, hybrid_enrollment: None, hybrid_claim: None,
        };
        match self.persist(&next) {
            Ok(()) => { *guard = next; Ok(trust) }
            Err(error) => {
                let mut fenced = next;
                fenced.record.as_mut().ok_or("not_provisioned")?.hybrid_unconfirmed = true;
                *guard = fenced;
                Err(error)
            }
        }
    }
}
