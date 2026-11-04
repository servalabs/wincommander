// SPDX-License-Identifier: AGPL-3.0-or-later
//! Verifies a disposable vector made by Cloudflare's local Workers runtime.
//! This is an interoperability check, not an active product licence format.

use wincmd_pq_experiment::{verify, Purpose, Trust, VerifyError, ML_DSA_65_PUBLIC_KEY_LEN};

const VECTOR: &[u8] = include_bytes!("cloudflare-hybrid-v1.bin");
const PAYLOAD: &[u8] = b"WinCommander disposable interop probe";

#[test]
fn workers_signatures_verify_and_both_are_required() {
    assert_eq!(
        VECTOR.len(),
        32 + ML_DSA_65_PUBLIC_KEY_LEN + 52 + PAYLOAD.len() + 64 + 3309
    );
    let ed_public: &[u8; 32] = VECTOR[..32].try_into().unwrap();
    let pq_end = 32 + ML_DSA_65_PUBLIC_KEY_LEN;
    let pq_public: &[u8; ML_DSA_65_PUBLIC_KEY_LEN] = VECTOR[32..pq_end].try_into().unwrap();
    let encoded = &VECTOR[pq_end..];
    let id = [7; 32];
    let trust = Trust {
        purpose: Purpose::Licence,
        minimum_epoch: 3,
        key_id: &id,
        ed25519_public: ed_public,
        ml_dsa_65_public: pq_public,
    };
    let verified = verify(encoded, &trust).unwrap();
    assert_eq!(verified.payload, PAYLOAD);
    assert_eq!(verified.epoch, 3);

    // Alter each signature independently: one valid signature is insufficient.
    for index in [52 + PAYLOAD.len(), 52 + PAYLOAD.len() + 64] {
        let mut tampered = encoded.to_vec();
        tampered[index] ^= 1;
        assert_eq!(
            verify(&tampered, &trust).err(),
            Some(VerifyError::InvalidSignature)
        );
    }
    let mut tampered = encoded.to_vec();
    tampered[52] ^= 1;
    assert_eq!(
        verify(&tampered, &trust).err(),
        Some(VerifyError::InvalidSignature)
    );
    let wrong_purpose = Trust {
        purpose: Purpose::Release,
        ..trust
    };
    assert_eq!(
        verify(encoded, &wrong_purpose).err(),
        Some(VerifyError::UntrustedContext)
    );
}
