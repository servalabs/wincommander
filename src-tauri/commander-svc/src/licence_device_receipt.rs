// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental dual-signed enrolment receipt. No production issuer exists.
use super::{Binding, PendingCredential, INVALID};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use wincmd_pq_experiment::{jws, Purpose};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    kind: String,
    licence_id: String,
    device_hash: String,
    issuer_origin: String,
    credential_id: String,
    secret_commitment: String,
    iat: u64,
    exp: u64,
}

pub(super) fn verify(
    encoded: &str,
    trust: &jws::Trust<'_>,
    binding: &Binding<'_>,
    credential: &PendingCredential,
    now: u64,
) -> Result<(), &'static str> {
    if trust.purpose != Purpose::LicenceEnrolment || encoded.len() > 32 * 1024 {
        return Err(INVALID);
    }
    let signed = jws::verify(encoded.as_bytes(), trust).map_err(|_| INVALID)?;
    let claims: Claims = serde_json::from_slice(&signed.payload).map_err(|_| INVALID)?;
    let mut hash = Sha256::new();
    hash.update(b"wincommander:licence-device-secret:v1\0");
    hash.update(credential.secret());
    let commitment = URL_SAFE_NO_PAD.encode(hash.finalize());
    if claims.kind != "licence-device-enrolment-v1"
        || claims.licence_id != binding.licence_id
        || claims.device_hash != binding.device_hash
        || claims.issuer_origin != binding.issuer_origin
        || claims.credential_id != URL_SAFE_NO_PAD.encode(credential.id)
        || claims.secret_commitment != commitment
        || claims.iat > now
        || claims.exp < now
        || claims.exp <= claims.iat
        || claims.exp - claims.iat > 600
    {
        return Err(INVALID);
    }
    Ok(())
}
