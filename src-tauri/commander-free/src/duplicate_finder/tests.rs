use super::*;

#[test]
fn hashes_after_size_candidates_and_keeps_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), b"same").unwrap();
    std::fs::write(dir.path().join("b"), b"same").unwrap();
    std::fs::write(dir.path().join("c"), b"nope").unwrap();
    let (scan, files, groups) = scan::scan_roots(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    assert_eq!(scan.groups.len(), 1);
    assert_eq!(scan.groups[0].files.len(), 2);
    let members = groups.values().next().unwrap();
    assert_eq!(members.len(), 2);
    assert!(safety::retains_live_member(
        members,
        &HashSet::from([members[0].clone()]),
        &files
    ));
    assert!(!safety::retains_live_member(
        members,
        &HashSet::from_iter(members.iter().cloned()),
        &files
    ));
}

#[test]
fn rejects_empty_root_list() {
    assert!(safety::validate_roots(Vec::new()).is_err());
}

#[test]
fn overlapping_roots_do_not_turn_one_file_into_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("child")).unwrap();
    std::fs::write(dir.path().join("child/only"), b"same").unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let (scan, _, _) =
        scan::scan_roots(&[root.clone(), root.join("child")], &AtomicBool::new(false));
    assert_eq!(scan.scanned_files, 1);
    assert!(scan.groups.is_empty());
}

#[test]
fn refuses_changed_content_even_when_size_and_timestamp_match() {
    let dir = tempfile::tempdir().unwrap();
    let changed = dir.path().join("a");
    std::fs::write(&changed, b"same").unwrap();
    std::fs::write(dir.path().join("b"), b"same").unwrap();
    let (_, files, _) = scan::scan_roots(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    let candidate = files
        .values()
        .find(|file| file.path.ends_with("a"))
        .unwrap();
    std::fs::write(&changed, b"nope").unwrap();
    let times = std::fs::FileTimes::new().set_modified(candidate.modified.unwrap());
    std::fs::File::options()
        .write(true)
        .open(&changed)
        .unwrap()
        .set_times(times)
        .unwrap();
    assert!(verified_file::open_verified(candidate, false, &AtomicBool::new(false)).is_err());
    assert!(changed.exists());
}

#[test]
fn refuses_removal_when_retained_copy_disappears() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), b"same").unwrap();
    std::fs::write(dir.path().join("b"), b"same").unwrap();
    let (_, files, _) = scan::scan_roots(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    let candidate = files
        .values()
        .find(|file| file.path.ends_with("a"))
        .unwrap()
        .clone();
    let keeper = files
        .values()
        .find(|file| file.path.ends_with("b"))
        .unwrap()
        .clone();
    std::fs::remove_file(&keeper.path).unwrap();
    let result = safety::remove_files(
        vec![candidate.clone()],
        vec![keeper],
        &AtomicBool::new(false),
    );
    assert_eq!(result.files_removed, 0);
    assert_eq!(result.errors.len(), 1);
    assert!(candidate.path.exists());
}

#[cfg(windows)]
#[test]
fn hard_links_are_not_reported_as_reclaimable_copies() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), b"same").unwrap();
    std::fs::hard_link(dir.path().join("a"), dir.path().join("b")).unwrap();
    let (scan, _, _) = scan::scan_roots(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    assert_eq!(scan.scanned_files, 1);
    assert!(scan.groups.is_empty());
}

#[test]
fn verified_removal_deletes_selected_copy_and_retains_identical_copy() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), b"same").unwrap();
    std::fs::write(dir.path().join("b"), b"same").unwrap();
    let (_, files, _) = scan::scan_roots(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    let candidate = files
        .values()
        .find(|file| file.path.ends_with("a"))
        .unwrap()
        .clone();
    let keeper = files
        .values()
        .find(|file| file.path.ends_with("b"))
        .unwrap()
        .clone();
    let result = safety::remove_files(
        vec![candidate.clone()],
        vec![keeper.clone()],
        &AtomicBool::new(false),
    );
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.files_removed, 1);
    assert_eq!(result.bytes_recovered, 4);
    assert!(!candidate.path.exists());
    assert_eq!(std::fs::read(&keeper.path).unwrap(), b"same");
}
