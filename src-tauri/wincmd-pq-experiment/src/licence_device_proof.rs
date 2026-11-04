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

/// Authenticate the request for a challenge; this tag is replayable and does
/// not replace the one-use challenge or licence and device checks.
pub fn sign_challenge_request(secret: &[u8; 32], credential_id: &[u8; 32], request_digest: &[u8; 32]) -> [u8; 32] {
    let mut message = Vec::with_capacity(REQUEST_DOMAIN.len() + 64);
    message.extend_from_slice(REQUEST_DOMAIN);
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
