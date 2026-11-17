// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn selected_file_identity_is_held_until_grant_is_released() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("input.bin");
    let renamed = temp.path().join("renamed.bin");
    std::fs::write(&path, b"data").unwrap();
    let selected = BoundSelection::open(path.clone(), Purpose::SourceFile).unwrap();
    assert!(!selected.identity().parent);
    assert_eq!(selected.path(), path);
    assert!(std::fs::File::open(&path).is_ok());
    // The sidecar's retained read mode must remain compatible with the grant.
    let sidecar_read = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&path)
        .unwrap();
    drop(sidecar_read);
    assert!(std::fs::rename(&path, &renamed).is_err());
    drop(selected);
    std::fs::rename(&path, &renamed).unwrap();
}

#[test]
fn output_binds_parent_and_refuses_an_existing_name() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("destination");
    let moved = temp.path().join("moved");
    std::fs::create_dir(&parent).unwrap();
    let output = parent.join("new.png");
    let selection = BoundSelection::open(output.clone(), Purpose::Output).unwrap();
    assert!(selection.identity().parent);
    assert!(!output.exists());
    assert!(std::fs::rename(&parent, &moved).is_err());
    drop(selection);
    std::fs::write(&output, b"existing").unwrap();
    assert!(BoundSelection::open(output, Purpose::Output).is_err());
    std::fs::rename(&parent, &moved).unwrap();
}

#[test]
fn rejects_hard_links_unc_and_streams() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("input");
    std::fs::write(&path, b"data").unwrap();
    std::fs::hard_link(&path, temp.path().join("alias")).unwrap();
    assert!(BoundSelection::open(path, Purpose::Carrier).is_err());
    assert!(validate_path(Path::new(r"\\server\share\file")).is_err());
    assert!(validate_path(Path::new(r"C:\file:stream")).is_err());
    assert!(validate_path(Path::new(r"C:file")).is_err());
}
