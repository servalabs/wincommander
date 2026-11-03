// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disposable vector from local Workers runtime; no production keys or issuer.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use wincmd_pq_experiment::{jws, Purpose, VerifyError, ML_DSA_65_PUBLIC_KEY_LEN};

#[test]
fn workers_jws_requires_both_signatures() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("cloudflare-jws-v1.json")).unwrap();
    let ed: [u8; 32] = URL_SAFE_NO_PAD
        .decode(fixture["publicEd"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = URL_SAFE_NO_PAD
        .decode(fixture["publicPq"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let trust = jws::Trust {
        purpose: Purpose::Licence,
        minimum_epoch: 7,
        ed_key_id: "ed-key",
        ed25519_public: &ed,
        pq_key_id: "pq-key",
        ml_dsa_65_public: &pq,
    };
    let encoded = serde_json::to_vec(&fixture["jws"]).unwrap();
    assert_eq!(
        jws::verify(&encoded, &trust).unwrap().payload,
        b"disposable licence fixture"
    );
    for index in 0..2 {
        let mut changed = fixture["jws"].clone();
        let mut signature = URL_SAFE_NO_PAD
            .decode(changed["signatures"][index]["signature"].as_str().unwrap())
            .unwrap();
        signature[0] ^= 1;
        changed["signatures"][index]["signature"] = URL_SAFE_NO_PAD.encode(signature).into();
        assert_eq!(
            jws::verify(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
            Some(VerifyError::InvalidSignature)
        );
    }
}
