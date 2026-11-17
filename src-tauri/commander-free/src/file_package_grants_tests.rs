// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::path::PathBuf;

const A: Owner = Owner([1; 32]);
const B: Owner = Owner([2; 32]);

#[test]
fn owner_purpose_expiry_and_revocation_are_enforced() {
    let now = Instant::now();
    let mut store = GrantStore::default();
    let tokens = store
        .issue(
            A,
            Purpose::SourceFile,
            vec![PathBuf::from("private-name")],
            now,
        )
        .unwrap();
    let token = &tokens[0];
    assert_eq!(token.as_str().len(), 64);
    assert_eq!(
        store.resolve(A, Purpose::SourceFile, token, now),
        Some(&PathBuf::from("private-name"))
    );
    assert!(store.resolve(B, Purpose::SourceFile, token, now).is_none());
    assert!(!store.revoke(B, token));
    assert!(store.resolve(A, Purpose::Carrier, token, now).is_none());
    assert!(store
        .resolve(A, Purpose::SourceFile, token, now + LIFETIME)
        .is_none());
    assert!(store.entries.is_empty());
    let other = store
        .issue(B, Purpose::Carrier, vec![PathBuf::from("cover")], now)
        .unwrap();
    store.revoke_owner(A);
    assert!(store.resolve(B, Purpose::Carrier, &other[0], now).is_some());
    assert!(store.revoke(B, &other[0]));
    assert!(store.resolve(B, Purpose::Carrier, &other[0], now).is_none());
}

#[test]
fn failed_randomness_and_collisions_never_partially_issue() {
    let now = Instant::now();
    let paths = || vec![PathBuf::from("one"), PathBuf::from("two")];
    let mut store = GrantStore::with_limits(2, 2);
    let mut calls = 0;
    assert!(store
        .issue_with(A, Purpose::SourceFile, paths(), now, |bytes| {
            calls += 1;
            if calls == 2 {
                return Err(());
            }
            bytes.fill(1);
            Ok(())
        })
        .is_err());
    assert!(store.entries.is_empty());
    assert!(store.counts.is_empty());
    assert!(store
        .issue_with(A, Purpose::SourceFile, paths(), now, |bytes| {
            bytes.fill(3);
            Ok(())
        })
        .is_err());
    assert!(store.entries.is_empty());
    assert!(store.issue(A, Purpose::SourceFile, paths(), now).is_ok());
}

#[test]
fn limits_apply_per_owner_and_globally_and_expired_entries_free_space() {
    let now = Instant::now();
    let mut store = GrantStore::with_limits(2, 1);
    let first = store
        .issue(A, Purpose::Carrier, vec![PathBuf::from("a")], now)
        .unwrap();
    assert!(store
        .issue(A, Purpose::Carrier, vec![PathBuf::from("another")], now)
        .is_err());
    assert!(store
        .issue(B, Purpose::Carrier, vec![PathBuf::from("b")], now)
        .is_ok());
    assert!(store
        .issue(
            Owner([3; 32]),
            Purpose::Carrier,
            vec![PathBuf::from("c")],
            now
        )
        .is_err());
    let later = now + LIFETIME;
    assert!(store
        .issue(A, Purpose::Carrier, vec![PathBuf::from("fresh")], later)
        .is_ok());
    assert!(store
        .resolve(A, Purpose::Carrier, &first[0], later)
        .is_none());
    assert_eq!(store.entries.len(), 1);
}
