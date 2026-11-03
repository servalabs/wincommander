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
fn independently_changing_reachability_does_not_change_contact_evidence() {
    let first = json!({"deviceId":"device", "observation":{"observed_unix_ms":123},
        "service_reachability":{"state":"starting", "observed_unix_ms":null}});
    let mut later = first.clone();
    later["service_reachability"] = json!({"state":"unconfirmed"});
    assert_ne!(first, later);
    assert_eq!(stable_history(first.clone()), stable_history(later.clone()));
    later["observation"]["observed_unix_ms"] = json!(124);
    assert_ne!(stable_history(first), stable_history(later));
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

#[cfg(windows)]
#[test]
fn windows_sync_then_rename_replaces_an_existing_record() {
    use std::io::Write;
    let fixture = Fixture::new();
    let root = std::env::temp_dir().join(format!("wincmd-contact-{}", fixture.nonce()));
    std::fs::create_dir(&root).unwrap();
    let live = root.join("record.json");
    let next = root.join("record.next");
    std::fs::write(&live, b"old").unwrap();
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&next)
        .unwrap();
    file.write_all(b"new").unwrap();
    file.sync_all().unwrap();
    drop(file);
    std::fs::rename(&next, &live).unwrap();
    assert_eq!(std::fs::read(&live).unwrap(), b"new");
    assert!(!next.exists());
    std::fs::remove_dir_all(root).unwrap();
}
