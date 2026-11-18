// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{contact_test_support::*, *};
use crate::fleet_contact_transfer::handle;
use serde_json::{json, Value};

fn stable_history(mut response: Value) -> Value {
    let reachability = response
        .as_object_mut()
        .unwrap()
        .remove("service_reachability")
        .unwrap();
    assert!(!reachability["state"].as_str().unwrap().is_empty());
    if let Some(timestamp) = reachability.get("observed_unix_ms") {
        assert!(timestamp.is_null() || timestamp.as_u64().is_some());
    }
    response
}

#[test]
fn chunked_maximum_evidence_verifies_once_and_exposes_only_bound_history() {
    let fixture = Fixture::new();
    let begun = handle(&fixture.store, json!({"operation":"begin"})).unwrap();
    let nonce = begun["nonce"].as_str().unwrap();
    let (mut request, mut response) = fixture.packet(nonce);
    request.resize(contact::MAX_REQUEST_BYTES, b' ');
    response.resize(contact::MAX_RESPONSE_BYTES, b' ');
    let signed = receipt(nonce, &request, &response);
    assert!(handle(
        &fixture.store,
        json!({"operation":"append","nonce":nonce,
        "stream":"response","offset":0,"data":STANDARD.encode(b" ")})
    )
    .is_err());
    for (stream, bytes) in [("request", &request), ("response", &response)] {
        let mut offset = 0;
        for chunk in bytes.chunks(1024 * 1024) {
            assert!(handle(
                &fixture.store,
                json!({"operation":"append","nonce":nonce,
                "stream":stream,"offset":offset + 1,"data":STANDARD.encode(chunk)})
            )
            .is_err());
            assert_eq!(
                handle(
                    &fixture.store,
                    json!({"operation":"append","nonce":nonce,
                "stream":stream,"offset":offset,"data":STANDARD.encode(chunk)})
                )
                .unwrap()["accepted"],
                true
            );
            offset += chunk.len();
        }
        assert!(handle(
            &fixture.store,
            json!({"operation":"append","nonce":nonce,
            "stream":stream,"offset":offset,"data":STANDARD.encode(b" ")})
        )
        .is_err());
    }
    let finished = handle(
        &fixture.store,
        json!({"operation":"finish","nonce":nonce,"receipt":signed}),
    )
    .unwrap();
    let history = handle(&fixture.store, json!({"operation":"get"})).unwrap();
    assert_eq!(history["deviceId"], trust().device_id);
    assert_eq!(history["serviceTrust"]["orgId"], trust().org_id);
    assert_eq!(history["serviceTrust"]["deviceId"], trust().device_id);
    assert_eq!(
        history["serviceTrust"]["enrollmentGeneration"],
        trust().enrollment_generation
    );
    assert_eq!(history["serviceTrust"]["signerKey"], trust().signer_key);
    assert!(history["credential_clear_token"].is_string());
    assert!(history["connection_token"].is_string());
    assert_eq!(history["observation"], finished["observation"]);
    assert!(history["observation"]["observed_unix_ms"].as_u64().unwrap() > 0);
    assert_eq!(fixture.store.snapshot().unwrap(), None);
    assert!(handle(
        &fixture.store,
        json!({"operation":"finish","nonce":nonce,"receipt":signed})
    )
    .is_err());
    let saved = fixture.fs.bytes.lock().unwrap().clone().unwrap();
    assert!(saved.len() < 32 * 1024);
    assert!(!String::from_utf8(saved).unwrap().contains(nonce));
    let new = handle(&fixture.store, json!({"operation":"begin"})).unwrap();
    assert_ne!(new["nonce"], nonce);
    assert!(handle(
        &fixture.store,
        json!({"operation":"append","nonce":nonce,
        "stream":"request","offset":0,"data":STANDARD.encode(b" ")})
    )
    .is_err());
    assert_eq!(
        stable_history(handle(&fixture.store, json!({"operation":"get"})).unwrap()),
        stable_history(history)
    );
}
