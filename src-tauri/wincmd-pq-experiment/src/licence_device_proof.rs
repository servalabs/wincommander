// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental transcript for device possession, not an authentication protocol.
//!
//! A valid tag alone is replayable. An issuer must bind the credential to a
//! licence and device, authenticate the challenge, enforce expiry, and consume
//! the challenge atomically before granting anything. Enrolment, key custody,
//! recovery and those checks are not implemented here.
use aws_lc_rs::hmac;

const DOMAIN: &[u8] = b"wincommander:licence-device-proof:v1\0";
const REQUEST_DOMAIN: &[u8] = b"wincommander:licence-challenge-request:v1\0";
const RELEASE_REQUEST_DOMAIN: &[u8] = b"wincommander:licence-release-challenge-request:v1\0";
const RELEASE_ACK_DOMAIN: &[u8] = b"wincommander:licence-release-ack:v1\0";
const ACTIVATION_ACK_DOMAIN: &[u8] = b"wincommander:licence-activation-ack:v1\0";
const REACTIVATION_DOMAIN: &[u8] = b"wincommander:licence-reactivation:v1\0";
const TRIAL_SETTLEMENT_DOMAIN: &[u8] = b"wincommander:licence-trial-settlement:v1\0";
const TRIAL_RETIREMENT_DOMAIN: &[u8] = b"wincommander:licence-trial-retirement:v1\0";

/// Candidate trial retirement transcript; no server endpoint is enabled yet.
pub fn sign_trial_retirement(secret: &[u8; 32], payload: &[u8]) -> Result<[u8; 32], &'static str> {
    if payload.is_empty() || payload.len() > 16 * 1024 { return Err("invalid trial retirement"); }
    let mut message = Vec::with_capacity(TRIAL_RETIREMENT_DOMAIN.len() + payload.len());
    message.extend_from_slice(TRIAL_RETIREMENT_DOMAIN);
    message.extend_from_slice(payload);
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    Ok(hmac::sign(&key, &message).as_ref().try_into().unwrap())
}

/// Candidate trial settlement transcript; no server endpoint is enabled yet.
pub fn sign_trial_settlement(secret: &[u8; 32], payload: &[u8]) -> Result<[u8; 32], &'static str> {
    if payload.is_empty() || payload.len() > 16 * 1024 { return Err("invalid trial settlement"); }
    let mut message = Vec::with_capacity(TRIAL_SETTLEMENT_DOMAIN.len() + payload.len());
    message.extend_from_slice(TRIAL_SETTLEMENT_DOMAIN);
    message.extend_from_slice(payload);
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    Ok(hmac::sign(&key, &message).as_ref().try_into().unwrap())
}

pub fn sign_reactivation(secret: &[u8; 32], payload: &[u8]) -> Result<[u8; 32], &'static str> {
    if payload.is_empty() || payload.len() > 16 * 1024 { return Err("invalid reactivation witness"); }
    let mut message = Vec::with_capacity(REACTIVATION_DOMAIN.len() + payload.len());
    message.extend_from_slice(REACTIVATION_DOMAIN);
    message.extend_from_slice(payload);
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    Ok(hmac::sign(&key, &message).as_ref().try_into().unwrap())
}

/// Bind an idempotent activation acknowledgement to exact canonical bytes.
pub fn sign_activation_acknowledgement(secret: &[u8; 32], payload: &[u8]) -> Result<[u8; 32], &'static str> {
    if payload.is_empty() || payload.len() > 16 * 1024 { return Err("invalid activation acknowledgement"); }
    let mut message = Vec::with_capacity(ACTIVATION_ACK_DOMAIN.len() + payload.len());
    message.extend_from_slice(ACTIVATION_ACK_DOMAIN);
    message.extend_from_slice(payload);
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    Ok(hmac::sign(&key, &message).as_ref().try_into().unwrap())
}

/// Authenticate the request for a challenge; this tag is replayable and does
/// not replace the one-use challenge or licence and device checks.
pub fn sign_challenge_request(secret: &[u8; 32], credential_id: &[u8; 32], request_digest: &[u8; 32]) -> [u8; 32] {
    sign_request(REQUEST_DOMAIN, secret, credential_id, request_digest)
}

/// Prepare a release challenge request. This does not release a seat.
pub fn sign_release_challenge_request(secret: &[u8; 32], credential_id: &[u8; 32], request_digest: &[u8; 32]) -> [u8; 32] {
    sign_request(RELEASE_REQUEST_DOMAIN, secret, credential_id, request_digest)
}

fn sign_request(domain: &[u8], secret: &[u8; 32], credential_id: &[u8; 32], request_digest: &[u8; 32]) -> [u8; 32] {
    let mut message = Vec::with_capacity(domain.len() + 64);
    message.extend_from_slice(domain);
    message.extend_from_slice(credential_id);
    message.extend_from_slice(request_digest);
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::sign(&key, &message).as_ref().try_into().unwrap()
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Operation {
    Refresh = 1,
    Deactivate = 2,
}

pub struct Challenge<'a> {
    pub operation: Operation,
    pub credential_id: &'a [u8; 32],
    pub nonce: &'a [u8; 32],
    /// SHA-256 of protocol-defined request bytes, excluding the proof itself.
    pub request_digest: &'a [u8; 32],
    pub expires_at: u64,
}

fn transcript(challenge: &Challenge<'_>) -> Vec<u8> {
    let mut result = Vec::with_capacity(DOMAIN.len() + 105);
    result.extend_from_slice(DOMAIN);
    result.push(challenge.operation as u8);
    result.extend_from_slice(challenge.credential_id);
    result.extend_from_slice(challenge.nonce);
    result.extend_from_slice(challenge.request_digest);
    result.extend_from_slice(&challenge.expires_at.to_be_bytes());
    result
}

/// The secret must be an independently generated, high-entropy 256-bit value.
pub fn sign(secret: &[u8; 32], challenge: &Challenge<'_>) -> [u8; 32] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::sign(&key, &transcript(challenge))
        .as_ref()
        .try_into()
        .unwrap()
}

/// Only acknowledge after the release outcome has been durably recorded.
pub fn sign_release_acknowledgement(secret: &[u8; 32], challenge: &Challenge<'_>) -> Result<[u8; 32], &'static str> {
    if !matches!(challenge.operation, Operation::Deactivate) { return Err("invalid release acknowledgement"); }
    let mut message = Vec::with_capacity(RELEASE_ACK_DOMAIN.len() + DOMAIN.len() + 105);
    message.extend_from_slice(RELEASE_ACK_DOMAIN);
    message.extend_from_slice(&transcript(challenge));
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    Ok(hmac::sign(&key, &message).as_ref().try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_acknowledgement_matches_independent_hmac_vector() {
        let secret = std::array::from_fn(|i| i as u8);
        let proof = sign_activation_acknowledgement(&secret, br#"{"kind":"test"}"#).unwrap();
        assert_eq!(proof, [0x69, 0xa6, 0xdb, 0xd2, 0x70, 0x89, 0xc9, 0x83,
            0xb5, 0xd3, 0xd0, 0x50, 0xbb, 0xca, 0x15, 0x9a, 0xb5, 0xbc, 0xe4, 0x55,
            0x6b, 0x14, 0xc3, 0x04, 0x10, 0x91, 0x97, 0xbf, 0x77, 0x56, 0x53, 0xf5]);
        assert_ne!(proof, sign_activation_acknowledgement(&secret, br#"{"kind":"TEST"}"#).unwrap());
    }

    #[test]
    fn challenge_request_proof_binds_device_and_request() {
        let id = std::array::from_fn(|i| i as u8);
        let digest = std::array::from_fn(|i| (i + 64) as u8);
        let secret = std::array::from_fn(|i| (i + 96) as u8);
        let expected = [0xb6, 0xed, 0x81, 0x59, 0x09, 0x80, 0x84, 0x7a, 0xf6, 0x7b, 0x53, 0x08, 0xa8, 0x05, 0x7d, 0xa0,
            0x65, 0xd5, 0xe1, 0x47, 0xc4, 0xf5, 0x10, 0xc2, 0xf8, 0x43, 0xec, 0x00, 0xc3, 0x38, 0xfa, 0xc2];
        assert_eq!(sign_challenge_request(&secret, &id, &digest), expected);
        let mut changed = digest;
        changed[0] ^= 1;
        assert_ne!(sign_challenge_request(&secret, &id, &changed), expected);
    }

    #[test]
    fn release_request_uses_a_distinct_domain() {
        let id = std::array::from_fn(|i| i as u8);
        let digest = std::array::from_fn(|i| (i + 64) as u8);
        let secret = std::array::from_fn(|i| (i + 96) as u8);
        let expected = [0x83, 0xc7, 0x15, 0x10, 0xbf, 0x85, 0x93, 0x79, 0xb1, 0x57, 0xe5, 0xcc, 0x6f, 0xa5, 0xb5, 0xa9,
            0x3c, 0x76, 0xde, 0x87, 0x8b, 0x02, 0xb2, 0xb0, 0xfa, 0x92, 0xf8, 0xa5, 0xf1, 0x70, 0x94, 0x26];
        assert_eq!(sign_release_challenge_request(&secret, &id, &digest), expected);
        assert_ne!(sign_challenge_request(&secret, &id, &digest), expected);
    }

    #[test]
    fn fixture_binds_every_field() {
        let credential_id = std::array::from_fn(|i| i as u8);
        let nonce = std::array::from_fn(|i| (i + 32) as u8);
        let request_digest = std::array::from_fn(|i| (i + 64) as u8);
        let secret = std::array::from_fn(|i| (i + 96) as u8);
        let challenge = Challenge {
            operation: Operation::Refresh,
            credential_id: &credential_id,
            nonce: &nonce,
            request_digest: &request_digest,
            expires_at: 2_000_000_000,
        };
        let expected = [
            0xba, 0x41, 0x19, 0xfd, 0xff, 0x8a, 0x65, 0x80, 0x3f, 0xab, 0x3f, 0xa7, 0xe8, 0xfc,
            0xcd, 0xd1, 0x91, 0xa3, 0x55, 0x9a, 0xb6, 0x89, 0x48, 0xe1, 0x97, 0x40, 0x86, 0x02,
            0x7f, 0xad, 0xd9, 0xa9,
        ];
        assert_eq!(sign(&secret, &challenge), expected);
        let key = hmac::Key::new(hmac::HMAC_SHA256, &secret);
        assert!(hmac::verify(&key, &transcript(&challenge), &expected).is_ok());
        let mut changed = nonce;
        changed[0] ^= 1;
        let altered = Challenge {
            nonce: &changed,
            ..challenge
        };
        assert!(hmac::verify(&key, &transcript(&altered), &expected).is_err());
        let release = Challenge { operation: Operation::Deactivate, ..challenge };
        let ack = [0x06, 0xd6, 0x6f, 0xdb, 0x87, 0x34, 0xac, 0xcf, 0x5c, 0xb1, 0x57, 0x08, 0x36, 0x57, 0xb4, 0x8c,
            0x8e, 0x79, 0x17, 0x0e, 0x03, 0x09, 0x40, 0x7c, 0x4e, 0x57, 0x90, 0x85, 0xd7, 0xd4, 0x03, 0x7c];
        assert_eq!(sign_release_acknowledgement(&secret, &release).unwrap(), ack);
        assert_ne!(sign(&secret, &release), ack);
        assert!(sign_release_acknowledgement(&secret, &challenge).is_err());
    }
}
