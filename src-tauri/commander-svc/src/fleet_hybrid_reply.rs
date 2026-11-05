// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service-side verification of exact enrollment reply bytes. Verification
//! alone does not grant transport, install credentials or publish a session.
use super::{
    hybrid_enrollment::{ClaimMarker, ClaimedHybridEnrollment},
    FleetHybridTrust, FleetMachinePolicy,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use fleet_proto::windows_v2::{
    decode_windows_v2, verify_windows_hybrid_enrollment, windows_bytes_sha256, WindowsHybridTrust,
};
use serde::Deserialize;
use zeroize::Zeroize;

const INVALID: &str = "hybrid_enrollment_reply_invalid";
const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const MAX_HYBRID_HEADER: usize = 24 * 1024;

#[derive(Deserialize)]
struct Identity {
    device_id: String,
    org_id: String,
    shield_progress_generation: String,
    outcome_pubkey_b64: String,
    checkin_secret_b64: String,
    server_signing_key_b64: String,
    capabilities: Vec<String>,
}

impl Drop for Identity {
    fn drop(&mut self) {
        self.checkin_secret_b64.zeroize();
    }
}

/// A checked reply is not installation authority. Future publication must
/// atomically recheck the claim and durably commit its credential and epoch.
pub(crate) struct VerifiedHybridReply {
    marker: ClaimMarker,
    keys: FleetHybridTrust,
    epoch: u64,
    secret: zeroize::Zeroizing<Vec<u8>>,
    body: zeroize::Zeroizing<Vec<u8>>,
}

impl VerifiedHybridReply {
    pub(super) fn matches(&self, claim: &ClaimedHybridEnrollment, keys: &FleetHybridTrust) -> bool {
        &self.marker == claim.marker() && &self.keys == keys
    }
    pub(super) fn epoch(&self) -> u64 {
        self.epoch
    }
    pub(super) fn secret(&self) -> &[u8] {
        &self.secret
    }
    #[cfg(test)]
    pub(super) fn body(&self) -> &[u8] {
        &self.body
    }
}

impl FleetMachinePolicy {
    /// Inputs are exact body bytes and individual header values after the HTTP
    /// layer has rejected missing, repeated and non-ASCII proof headers.
    pub(crate) fn verify_hybrid_enrollment_reply(
        &self,
        claim: &ClaimedHybridEnrollment,
        status: u16,
        body: &[u8],
        classical_header: &str,
        hybrid_header: &str,
    ) -> Result<VerifiedHybridReply, &'static str> {
        if !self.hybrid_claim_is_current(claim)
            || status != 200
            || body.len() > MAX_RESPONSE
            || hybrid_header.len() > MAX_HYBRID_HEADER
            || classical_header.len() != 91
        {
            return Err(INVALID);
        }
        let (binding, keys) = self.hybrid_trust_snapshot()?.ok_or(INVALID)?;
        if &binding != claim.binding()
            || keys.outcome_key_b64.as_deref() != Some(claim.outcome_key())
        {
            return Err(INVALID);
        }
        let ed: [u8; 32] = STANDARD
            .decode(&binding.signer_key)
            .map_err(|_| INVALID)?
            .try_into()
            .map_err(|_| INVALID)?;
        let pq: [u8; 1952] = STANDARD
            .decode(&keys.ml_dsa_65_public_b64)
            .map_err(|_| INVALID)?
            .try_into()
            .map_err(|_| INVALID)?;
        let trusted = WindowsHybridTrust {
            minimum_epoch: keys.minimum_epoch,
            ed_key_id: &keys.ed_key_id,
            ed25519_public: &ed,
            pq_key_id: &keys.pq_key_id,
            ml_dsa_65_public: &pq,
        };
        let proof = URL_SAFE_NO_PAD.decode(hybrid_header).map_err(|_| INVALID)?;
        if URL_SAFE_NO_PAD.encode(&proof) != hybrid_header {
            return Err(INVALID);
        }
        let epoch = verify_windows_hybrid_enrollment(
            claim.challenge(),
            claim.request(),
            status,
            body,
            &binding.signer_key,
            &proof,
            &trusted,
        )
        .ok_or(INVALID)?;
        let signature = classical_header.strip_prefix("v2.").ok_or(INVALID)?;
        let signature_bytes = STANDARD.decode(signature).map_err(|_| INVALID)?;
        if signature_bytes.len() != 64 || STANDARD.encode(&signature_bytes) != signature {
            return Err(INVALID);
        }
        let preimage = fleet_proto::canonical_json_bytes(&serde_json::json!([
            "fleet.windows.enrollment.v2",
            claim.challenge(),
            windows_bytes_sha256(claim.request()),
            status,
            windows_bytes_sha256(body)
        ]));
        if !fleet_proto::verify_signature_b64(&binding.signer_key, &preimage, signature) {
            return Err(INVALID);
        }
        let identity: Identity = decode_windows_v2(body).map_err(|_| INVALID)?;
        if identity.org_id != binding.org_id
            || identity.device_id != binding.device_id
            || identity.shield_progress_generation != binding.enrollment_generation
            || identity.outcome_pubkey_b64 != claim.outcome_key()
            || identity.server_signing_key_b64 != binding.signer_key
        {
            return Err(INVALID);
        }
        let secret = zeroize::Zeroizing::new(
            STANDARD
                .decode(&identity.checkin_secret_b64)
                .map_err(|_| INVALID)?,
        );
        if secret.len() != 32 || STANDARD.encode(&*secret) != identity.checkin_secret_b64 {
            return Err(INVALID);
        }
        let _ = &identity.capabilities;
        // This is still a snapshot. Publication must be serialized with
        // cancellation; callers cannot use this return value as authority.
        if !self.hybrid_claim_is_current(claim)
            || self.hybrid_trust_snapshot()?.as_ref() != Some(&(binding.clone(), keys.clone()))
        {
            return Err(INVALID);
        }
        Ok(VerifiedHybridReply {
            marker: claim.marker().clone(),
            keys,
            epoch,
            secret,
            body: zeroize::Zeroizing::new(body.to_vec()),
        })
    }
}
