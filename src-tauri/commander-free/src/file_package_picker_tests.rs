// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn native_results_are_scoped_to_session_and_purpose() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("example.txt");
    std::fs::write(&path, b"example").unwrap();
    let manager = SelectionManager::default();
    let first = manager.session().unwrap();
    let second = manager.session().unwrap();
    let mut picked = first
        .register_native(vec![path.clone()], PickKind::SourceFiles)
        .unwrap();
    let item = picked.pop().unwrap();
    assert_eq!(item.name, "example.txt");
    let active = first.resolve(PickKind::SourceFiles, &item.token).unwrap();
    assert_eq!(active.path(), path);
    assert!(first.resolve(PickKind::Carrier, &item.token).is_err());
    assert!(second.resolve(PickKind::SourceFiles, &item.token).is_err());
    assert!(!second.remove(&item.token));
    let renamed = temporary.path().join("renamed.txt");
    assert!(std::fs::rename(&path, &renamed).is_err());
    assert!(first.remove(&item.token));
    assert!(std::fs::rename(&path, &renamed).is_err());
    drop(active);
    std::fs::rename(&path, &renamed).unwrap();
    assert!(first.resolve(PickKind::SourceFiles, &item.token).is_err());
}

#[test]
fn closed_session_refuses_late_dialog_results() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("example.txt");
    std::fs::write(&path, b"example").unwrap();
    let manager = SelectionManager::default();
    let session = manager.session().unwrap();
    let picked = session
        .register_native(vec![path], PickKind::SourceFiles)
        .unwrap();
    session.close();
    assert!(session
        .resolve(PickKind::SourceFiles, &picked[0].token)
        .is_err());
    assert!(session
        .register_native(
            vec![PathBuf::from(r"C:\private\late.txt")],
            PickKind::SourceFiles
        )
        .is_err());
}

