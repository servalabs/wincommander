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

#[test]
fn staging_does_not_accept_new_section_created_during_publication() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let original = fs::read(dir.path().join("store").join("settings.dat")).unwrap();
    let mut snapshot = Snapshot::capture(dir.path()).unwrap();
    let later = dir.path().join("store").join("later.dat");
    let result = snapshot.stage_unactivated(&|_| {
        fs::write(&later, b"late old-writer data").map_err(|_| "old writer blocked".to_string())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(dir.path().join("store").join("settings.dat")).unwrap(), original);
}

#[test]
fn failure_after_staging_does_not_authorize_cutover_or_modify_sources() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let section = dir.path().join("store").join("settings.dat");
    let original = fs::read(&section).unwrap();
    let staged = dir.path().join("unactivated-generation");
    let mut snapshot = Snapshot::capture(dir.path()).unwrap();
    let result = snapshot.stage_unactivated(&|candidate| {
        fs::write(&staged, candidate).map_err(|e| e.to_string())?;
        Err("publication outcome unknown".into())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&section).unwrap(), original);
    assert!(staged.exists());
}
