// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{contact_test_support::*, *};
use std::{sync::atomic::Ordering, time::Duration};

#[test]
fn verified_observation_is_historical_and_does_not_install_policy() {
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    assert!(nonce.starts_with("ci-") && nonce.len() == 35);
    assert!(nonce[3..]
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert!(fixture.store.contact_pending(&nonce));
    let (request, response) = fixture.packet(&nonce);
    assert_eq!(
        fixture.submit(&nonce, &request, &response, Duration::from_secs(30), 10_000),
        Ok(ContactObservation {
            observed_unix_ms: 10_000
        })
    );
    assert_eq!(fixture.store.snapshot().unwrap(), None);
    assert_eq!(
        fixture.store.contact_observation().unwrap(),
        Some((
            trust().device_id,
            ContactObservation {
                observed_unix_ms: 10_000
            }
        ))
    );
    assert!(!fixture.store.contact_pending(&nonce));
    assert_eq!(
        fixture.submit(&nonce, &request, &response, Duration::ZERO, 10_001),
        Err("contact_not_pending")
    );
}

#[test]
fn absent_trust_restart_and_replaced_challenges_cannot_accept_old_packets() {
    let empty =
        FleetMachinePolicy::open(Box::new(MemoryFs::default()), PathBuf::from("empty")).unwrap();
    assert!(empty.begin_contact().is_err());
    assert_eq!(empty.contact_observation().unwrap(), None);
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let (request, response) = fixture.packet(&nonce);
    let current = fixture.nonce();
    assert_ne!(nonce, current);
    assert_eq!(
        fixture.submit(&nonce, &request, &response, Duration::ZERO, 10_000),
        Err("contact_not_pending")
    );
    let reopened =
        FleetMachinePolicy::open(Box::new(fixture.fs.clone()), PathBuf::from("contact-test"))
            .unwrap();
    assert!(!reopened.contact_pending(&current));
    assert_eq!(
        reopened.record_contact(
            &current,
            &request,
            &response,
            &receipt(&current, &request, &response),
        ),
        Err("contact_not_pending")
    );
    let persisted = fixture.fs.bytes.lock().unwrap().clone().unwrap();
    assert!(!String::from_utf8(persisted).unwrap().contains("ci-"));
}

#[test]
fn historical_observation_survives_restart_but_active_challenge_does_not() {
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let (request, response) = fixture.packet(&nonce);
    fixture
        .submit(&nonce, &request, &response, Duration::ZERO, 12_000)
        .unwrap();
    let pending = fixture.nonce();
    let reopened =
        FleetMachinePolicy::open(Box::new(fixture.fs.clone()), PathBuf::from("contact-test"))
            .unwrap();
    assert_eq!(
        reopened
            .contact_observation()
            .unwrap()
            .unwrap()
            .1
            .observed_unix_ms,
        12_000
    );
    assert!(!reopened.contact_pending(&pending));
}

#[test]
fn timeout_and_monotonic_rollback_consume_the_nonce() {
    for time in [
        Duration::from_secs(30) + Duration::from_millis(1),
        Duration::from_secs(31),
    ] {
        let fixture = Fixture::new();
        let nonce = fixture.nonce();
        let (request, response) = fixture.packet(&nonce);
        assert_eq!(
            fixture.submit(&nonce, &request, &response, time, 1),
            Err("contact_expired")
        );
        assert!(!fixture.store.contact_pending(&nonce));
    }
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let (request, response) = fixture.packet(&nonce);
    assert_eq!(
        fixture.store.record_contact_with_clock(
            &nonce,
            &request,
            &response,
            &receipt(&nonce, &request, &response),
            || (fixture.issued - Duration::from_millis(1), Ok(1))
        ),
        Err("contact_expired")
    );
    assert_eq!(fixture.store.contact_observation().unwrap(), None);
}

#[test]
fn verification_finishing_after_deadline_and_wallclock_rollback_are_unconfirmed() {
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let (request, response) = fixture.packet(&nonce);
    let mut times = [29, 31].into_iter();
    assert_eq!(
        fixture.store.record_contact_with_clock(
            &nonce,
            &request,
            &response,
            &receipt(&nonce, &request, &response),
            || (
                fixture.issued + Duration::from_secs(times.next().unwrap()),
                Ok(100)
            )
        ),
        Err("contact_expired")
    );
    let nonce = fixture.nonce();
    let (request, response) = fixture.packet(&nonce);
    fixture
        .submit(&nonce, &request, &response, Duration::ZERO, 100)
        .unwrap();
    let nonce = fixture.nonce();
    let (request, response) = fixture.packet(&nonce);
    assert_eq!(
        fixture.submit(&nonce, &request, &response, Duration::ZERO, 99),
        Err("contact_clock_rollback")
    );
    assert_eq!(
        fixture
            .store
            .contact_observation()
            .unwrap()
            .unwrap()
            .1
            .observed_unix_ms,
        100
    );
}

#[test]
fn failed_persistence_never_publishes_an_observation_or_reuses_the_nonce() {
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let (request, response) = fixture.packet(&nonce);
    let before = fixture.fs.bytes.lock().unwrap().clone();
    fixture.fs.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        fixture.submit(&nonce, &request, &response, Duration::ZERO, 1),
        Err("store_unavailable")
    );
    assert_eq!(fixture.store.contact_observation().unwrap(), None);
    assert_eq!(*fixture.fs.bytes.lock().unwrap(), before);
    fixture.fs.fail.store(false, Ordering::SeqCst);
    assert_eq!(
        fixture.submit(&nonce, &request, &response, Duration::ZERO, 1),
        Err("contact_not_pending")
    );
}

#[test]
fn concurrent_trust_replacement_cannot_leave_old_contact_under_new_trust() {
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let (request, response) = fixture.packet(&nonce);
    let mut replacement = trust();
    replacement.enrollment_generation = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
    let proof = proof(&fixture.store, &replacement);
    std::thread::scope(|scope| {
        let record = scope.spawn(|| fixture.submit(&nonce, &request, &response, Duration::ZERO, 1));
        let replace = scope.spawn(|| {
            fixture
                .store
                .replace_trust(trust(), replacement, &proof, "S-1-5-18")
        });
        let _ = record.join().unwrap();
        replace.join().unwrap().unwrap();
    });
    assert_eq!(fixture.store.contact_observation().unwrap(), None);
    assert!(!fixture.store.contact_pending(&nonce));
}
