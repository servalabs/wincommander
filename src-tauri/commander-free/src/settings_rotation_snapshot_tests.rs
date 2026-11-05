// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use super::super::{open, derive_section_key, encode_section};
use serde_json::json;

fn fixture(root: &Path) -> [u8; 32] {
    let material = [42; 32];
    fs::create_dir(root.join("store")).unwrap();
    fs::write(root.join(".install.material"), material).unwrap();
    let key = derive_section_key(&material, None).unwrap();
    for (name, value) in [("settings", json!({"value":"private one"})), ("payments", json!({"value":"private two"}))] {
        let encoded = encode_section(&key, &serde_json::to_vec(&value).unwrap(), &format!("machine:{name}")).unwrap();
        fs::write(root.join("store").join(format!("{name}.dat")), encoded).unwrap();
    }
    material
}

#[test]
fn enumerates_all_sections_and_holds_the_key_and_files() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let key_path = dir.path().join(".install.material");
    let section = dir.path().join("store").join("settings.dat");
    let mut snapshot = Snapshot::capture(dir.path()).unwrap();
    assert_eq!(snapshot.sections.len(), 2);
    assert!(fs::write(&key_path, b"replacement").is_err());
    assert!(fs::write(&section, b"replacement").is_err());
    let generation = snapshot.prepare().unwrap();
    let values = open(&generation).unwrap();
    assert_eq!(values["settings"]["value"], "private one");
    assert_eq!(values["payments"]["value"], "private two");
    drop(snapshot);
    assert!(fs::write(&section, b"after release").is_ok());
}

#[test]
fn newly_created_section_is_detected_or_denied_while_guard_is_held() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let mut snapshot = Snapshot::capture(dir.path()).unwrap();
    let later = dir.path().join("store").join("later.dat");
    if fs::write(&later, b"new section").is_ok() {
        assert!(snapshot.verify_unchanged().is_err());
        assert!(snapshot.prepare().is_err());
    } else {
        snapshot.verify_unchanged().unwrap();
    }
}

#[test]
fn unreadable_or_invalid_section_aborts_without_publishing() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let path = dir.path().join("store").join("settings.dat");
    fs::write(&path, b"corrupt data").unwrap();
    let mut snapshot = Snapshot::capture(dir.path()).unwrap();
    assert!(snapshot.prepare().is_err());
    assert_eq!(fs::read(&path).unwrap(), b"corrupt data");
}
