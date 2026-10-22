use super::*;

#[test]
fn inventory_excludes_its_own_database_inside_the_scanned_folder() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("one.bin"), b"same").unwrap();
    std::fs::write(root.path().join("two.bin"), b"same").unwrap();
    let scratch = tempfile::Builder::new()
        .prefix("inventory-")
        .tempdir_in(root.path())
        .unwrap();
    let index = index::ScanIndex::from_directory(scratch).unwrap();
    let canonical = std::fs::canonicalize(root.path()).unwrap();
    let path = canonical.to_str().unwrap();
    index
        .connection
        .execute("INSERT INTO directories(path,root) VALUES(?1,?1)", [path])
        .unwrap();
    scan::enumerate(&index, &AtomicBool::new(false)).unwrap();
    let files: i64 = index
        .connection
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(files, 2);
    assert!(index.directory.path().join("scan.sqlite").exists());
}

fn scan_fixture(
    roots: &[PathBuf],
    cancelled: &AtomicBool,
) -> (
    DuplicateScan,
    std::collections::HashMap<String, CachedFile>,
    std::collections::HashMap<String, Vec<String>>,
) {
    let index = fixture_index(roots, cancelled);
    let page = index.page(0).unwrap();
    let mut files = std::collections::HashMap::new();
    let mut groups = std::collections::HashMap::new();
    for group in &page.groups {
        groups.insert(
            group.id.clone(),
            group.files.iter().map(|file| file.id.clone()).collect(),
        );
        for file in &group.files {
            files.insert(file.id.clone(), index.file(&file.id).unwrap());
        }
    }
    (page, files, groups)
}

fn fixture_index(roots: &[PathBuf], cancelled: &AtomicBool) -> index::ScanIndex {
    static SERIAL: Mutex<()> = Mutex::new(());
    let _serial = SERIAL.lock().unwrap();
    scope::begin(roots, Vec::new());
    scan::scan_roots(roots, cancelled).unwrap()
}

#[test]
fn hashes_after_size_candidates_and_keeps_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), b"same").unwrap();
    std::fs::write(dir.path().join("b"), b"same").unwrap();
    std::fs::write(dir.path().join("c"), b"nope").unwrap();
    let (scan, files, groups) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    assert_eq!(scan.groups.len(), 1);
    assert_eq!(scan.groups[0].files.len(), 2);
    let members = groups.values().next().unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(files.len(), 2);
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
    let (scan, _, _) = scan_fixture(&[root.clone(), root.join("child")], &AtomicBool::new(false));
    assert_eq!(scan.scanned_files, 1);
    assert!(scan.groups.is_empty());
}

#[test]
fn refuses_changed_content_even_when_size_and_timestamp_match() {
    let dir = tempfile::tempdir().unwrap();
    let changed = dir.path().join("a");
    std::fs::write(&changed, b"same").unwrap();
    std::fs::write(dir.path().join("b"), b"same").unwrap();
    let (_, files, _) = scan_fixture(
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
    let (_, files, _) = scan_fixture(
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
    let (scan, _, _) = scan_fixture(
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
    let (_, files, _) = scan_fixture(
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

#[test]
fn member_paging_preserves_true_group_total_and_same_keeper() {
    let dir = tempfile::tempdir().unwrap();
    for number in 0..205 {
        std::fs::write(dir.path().join(format!("copy-{number}")), b"same").unwrap();
    }
    let (scan, _, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    assert_eq!(scan.scanned_files, 205);
    assert_eq!(scan.total_groups, 1);
    assert_eq!(scan.total_duplicate_bytes, 204 * 4);
    assert_eq!(scan.groups[0].file_count, 205);
    assert_eq!(scan.groups[0].files.len(), MEMBER_PAGE_SIZE + 1);
    assert!(scan.groups[0].has_more_files);
    assert!(scan.complete);
    assert!(!scan.truncated);
}

#[test]
fn cancelled_inventory_never_claims_complete() {
    let dir = tempfile::tempdir().unwrap();
    let (scan, _, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(true),
    );
    assert!(scan.cancelled);
    assert!(!scan.complete);
}

#[test]
fn selection_uses_whole_group_and_rejects_stale_ids() {
    let dir = tempfile::tempdir().unwrap();
    for number in 0..205 {
        std::fs::write(dir.path().join(format!("copy-{number}")), b"same").unwrap();
    }
    let roots = [std::fs::canonicalize(dir.path()).unwrap()];
    let index = fixture_index(&roots, &AtomicBool::new(false));
    let group = index.page(0).unwrap().groups.remove(0);
    let next = index.group(&group.id, MEMBER_PAGE_SIZE).unwrap();
    assert_eq!(next.files[0].id, group.files[0].id);
    assert_eq!(next.files.len(), 5);
    assert!(!next.has_more_files);
    let all: HashSet<String> = group
        .files
        .iter()
        .chain(next.files.iter())
        .map(|file| file.id.clone())
        .collect();
    assert_eq!(all.len(), 205);
    assert!(index.removal_selection(&all).is_err());
    let mut retained = all;
    retained.remove(&group.files[0].id);
    assert_eq!(index.removal_selection(&retained).unwrap().0.len(), 204);
    assert!(index
        .removal_selection(&HashSet::from(["stale-id".to_string()]))
        .is_err());
    assert!(index.group("stale-group", 0).is_err());
    assert!(index.page(2).is_err());
}

#[test]
fn result_paging_rejects_a_different_scan_capability() {
    let dir = tempfile::tempdir().unwrap();
    let index = fixture_index(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    let id = index.scan_id.clone();
    *cache().lock().unwrap() = Some(index);
    assert!(with_index("previous-scan", |index| index.page(0)).is_err());
    assert!(with_index(&id, |index| index.page(0)).is_ok());
    *cache().lock().unwrap() = None;
}
