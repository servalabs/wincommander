use super::*;

static SERIAL: Mutex<()> = Mutex::new(());

fn copy_path(root: &std::path::Path, directory: &str, name: &str) -> PathBuf {
    let folder = root.join(directory);
    std::fs::create_dir_all(&folder).unwrap();
    folder.join(name)
}

#[test]
fn dot_directories_are_skipped_unless_explicitly_selected_as_roots() {
    let _serial = SERIAL.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let hidden = temp.path().join(".managed");
    std::fs::create_dir(&hidden).unwrap();
    std::fs::write(temp.path().join("visible"), b"same content").unwrap();
    std::fs::write(hidden.join("hidden-a"), b"same content").unwrap();
    std::fs::write(hidden.join("hidden-b"), b"same content").unwrap();

    let root = std::fs::canonicalize(temp.path()).unwrap();
    scope::begin(std::slice::from_ref(&root), Vec::new(), 0);
    let index = scan::scan_roots(&[root], 0, &AtomicBool::new(false)).unwrap();
    let count: i64 = index
        .connection
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "broad discovery must omit implementation directories");
    drop(index);

    let root = std::fs::canonicalize(hidden).unwrap();
    scope::begin(std::slice::from_ref(&root), Vec::new(), 0);
    let index = scan::scan_roots(&[root], 0, &AtomicBool::new(false)).unwrap();
    let count: i64 = index
        .connection
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2, "a directly selected directory must remain inspectable");
}

#[test]
fn inventory_excludes_its_own_database_inside_the_scanned_folder() {
    let _serial = SERIAL.lock().unwrap();
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
    scan::enumerate(&index, 0, &AtomicBool::new(false)).unwrap();
    let files: i64 = index
        .connection
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(files, 2);
    assert!(index.directory.path().join("scan.sqlite").exists());
}

#[test]
fn batched_inventory_keeps_every_file_and_nested_folder() {
    let _serial = SERIAL.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let nested = root.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    for index in 0..513 {
        std::fs::write(root.path().join(format!("root-{index}")), [index as u8]).unwrap();
    }
    std::fs::write(nested.join("nested-file"), b"nested").unwrap();
    let index = index::ScanIndex::new().unwrap();
    let canonical = std::fs::canonicalize(root.path()).unwrap();
    let path = canonical.to_str().unwrap();
    index
        .connection
        .execute("INSERT INTO directories(path,root) VALUES(?1,?1)", [path])
        .unwrap();

    scan::enumerate(&index, 0, &AtomicBool::new(false)).unwrap();

    let files: i64 = index
        .connection
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(files, 514);
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
    let _serial = SERIAL.lock().unwrap();
    scope::begin(roots, Vec::new(), 0);
    scan::scan_roots(roots, 0, cancelled).unwrap()
}

#[test]
fn broad_scan_minimum_excludes_small_files_without_counting_them_as_errors() {
    let _serial = SERIAL.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("small-a"), b"same").unwrap();
    std::fs::write(root.path().join("small-b"), b"same").unwrap();
    let large = vec![7u8; DEFAULT_MINIMUM_SIZE_BYTES as usize];
    std::fs::write(copy_path(root.path(), "a", "large"), &large).unwrap();
    std::fs::write(copy_path(root.path(), "b", "large"), &large).unwrap();
    let roots = [std::fs::canonicalize(root.path()).unwrap()];
    scope::begin(&roots, Vec::new(), DEFAULT_MINIMUM_SIZE_BYTES);
    let index =
        scan::scan_roots(&roots, DEFAULT_MINIMUM_SIZE_BYTES, &AtomicBool::new(false)).unwrap();
    let eligible: i64 = index
        .connection
        .query_row("SELECT count(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(eligible, 2);
    assert_eq!(index.page(0).unwrap().groups.len(), 1);
}

#[test]
fn hashes_after_size_candidates_and_keeps_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(copy_path(dir.path(), "a", "item"), b"same").unwrap();
    std::fs::write(copy_path(dir.path(), "b", "item"), b"same").unwrap();
    std::fs::write(copy_path(dir.path(), "c", "item"), b"nope").unwrap();
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
fn groups_require_matching_filename_and_full_content() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(copy_path(dir.path(), "a", "Report.TXT"), b"same").unwrap();
    std::fs::write(copy_path(dir.path(), "b", "report.txt"), b"same").unwrap();
    std::fs::write(copy_path(dir.path(), "c", "renamed.txt"), b"same").unwrap();
    std::fs::write(copy_path(dir.path(), "d", "report.txt"), b"nope").unwrap();
    let (scan, _, _) = scan_fixture(&[std::fs::canonicalize(dir.path()).unwrap()], &AtomicBool::new(false));
    assert_eq!(scan.groups.len(), 1);
    assert_eq!(scan.groups[0].file_count, 2);
    assert!(scan.groups[0].files.iter().all(|file| file.name.eq_ignore_ascii_case("report.txt")));
}

#[test]
fn duplicate_rows_expose_the_verified_modified_time_for_retention_review() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(copy_path(dir.path(), "a", "item"), b"same").unwrap();
    std::fs::write(copy_path(dir.path(), "b", "item"), b"same").unwrap();

    let (scan, _, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );

    assert!(scan.groups[0].files.iter().all(|file| file
        .modified
        .as_deref()
        .is_some_and(|value| value.parse::<u128>().is_ok())));
}

#[test]
fn only_full_hashes_same_size_files_with_matching_content_samples() {
    let dir = tempfile::tempdir().unwrap();
    let duplicate = vec![7u8; 256 * 1024];
    let distinct = vec![9u8; 256 * 1024];
    std::fs::write(copy_path(dir.path(), "a", "item"), &duplicate).unwrap();
    std::fs::write(copy_path(dir.path(), "b", "item"), &duplicate).unwrap();
    std::fs::write(copy_path(dir.path(), "c", "item"), &distinct).unwrap();
    let (scan, _, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    assert_eq!(scan.groups.len(), 1);
    assert_eq!(scan.hashed_files, 2);
}

#[test]
fn full_hash_rejects_files_that_match_only_at_the_sampled_edges() {
    let dir = tempfile::tempdir().unwrap();
    let first = vec![7u8; 512 * 1024];
    let mut second = first.clone();
    second[128 * 1024] = 9;
    std::fs::write(copy_path(dir.path(), "a", "item"), first).unwrap();
    std::fs::write(copy_path(dir.path(), "b", "item"), second).unwrap();
    let (scan, _, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    assert!(scan.groups.is_empty());
    assert_eq!(scan.hashed_files, 2);
}

#[test]
fn parallel_candidate_batches_keep_full_hash_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let duplicate = vec![7u8; 256 * 1024];
    for index in 0..4 {
        std::fs::write(copy_path(dir.path(), &format!("copy-{index}"), "item"), &duplicate).unwrap();
    }
    for index in 0..4 {
        let mut sampled_match = duplicate.clone();
        sampled_match[64 * 1024 + index] = index as u8;
        std::fs::write(copy_path(dir.path(), &format!("different-{index}"), "item"), sampled_match).unwrap();
    }

    let (scan, _, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );

    assert_eq!(scan.groups.len(), 1);
    assert_eq!(scan.groups[0].files.len(), 4);
    assert_eq!(scan.hashed_files, 8);
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
    let changed = copy_path(dir.path(), "a", "item");
    std::fs::write(&changed, b"same").unwrap();
    std::fs::write(copy_path(dir.path(), "b", "item"), b"same").unwrap();
    let (_, files, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    let candidate = files
        .values()
        .find(|file| file.path == std::fs::canonicalize(&changed).unwrap())
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
fn review_validation_allows_an_unchanged_cached_file_without_granting_removal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(copy_path(dir.path(), "a", "item"), b"same").unwrap();
    std::fs::write(copy_path(dir.path(), "b", "item"), b"same").unwrap();
    let (_, files, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    let file = files.values().next().unwrap();
    assert!(safety::validated_review_file(file).is_ok());
}

#[test]
fn explorer_selection_uses_a_normal_windows_path_after_canonicalization() {
    let argument =
        explorer_select_argument(std::path::Path::new(r"\\?\C:\Users\Example\same file.txt"))
            .unwrap();
    assert_eq!(
        argument.to_string_lossy(),
        r"/select,C:\Users\Example\same file.txt"
    );

    let unc = explorer_select_argument(std::path::Path::new(r"\\?\UNC\server\share\same file.txt"))
        .unwrap();
    assert_eq!(
        unc.to_string_lossy(),
        r"/select,\\server\share\same file.txt"
    );
    assert!(
        explorer_select_argument(std::path::Path::new(r"\\?\Volume{unsafe}\same file.txt"))
            .is_err()
    );
}

#[test]
fn refuses_removal_when_retained_copy_disappears() {
    let dir = tempfile::tempdir().unwrap();
    let a = copy_path(dir.path(), "a", "item");
    let b = copy_path(dir.path(), "b", "item");
    std::fs::write(&a, b"same").unwrap();
    std::fs::write(&b, b"same").unwrap();
    let (_, files, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    let candidate = files
        .values()
        .find(|file| file.path == std::fs::canonicalize(&a).unwrap())
        .unwrap()
        .clone();
    let keeper = files
        .values()
        .find(|file| file.path == std::fs::canonicalize(&b).unwrap())
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
    let a = copy_path(dir.path(), "a", "item");
    let b = copy_path(dir.path(), "b", "item");
    std::fs::write(&a, b"same").unwrap();
    std::fs::hard_link(a, b).unwrap();
    let (scan, _, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    assert_eq!(scan.scanned_files, 1);
    assert_eq!(scan.hashed_files, 0);
    assert!(scan.groups.is_empty());
}

#[cfg(windows)]
#[test]
fn native_inventory_skips_junctions_without_following_their_target() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("first"), b"same").unwrap();
    let junction = dir.path().join("cycle");
    let status = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(status.status.success());

    let (scan, _, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );

    assert_eq!(scan.scanned_files, 1);
    assert_eq!(scan.skipped_links, 1);
    assert!(scan.groups.is_empty());
    std::fs::remove_dir(junction).unwrap();
}

#[test]
fn verified_removal_deletes_selected_copy_and_retains_identical_copy() {
    #[cfg(windows)]
    let dir = tempfile::Builder::new()
        .prefix("duplicate-test-")
        .tempdir_in(std::env::var_os("USERPROFILE").unwrap())
        .unwrap();
    #[cfg(not(windows))]
    let dir = tempfile::tempdir().unwrap();
    let a = copy_path(dir.path(), "a", "item");
    let b = copy_path(dir.path(), "b", "item");
    std::fs::write(&a, b"same").unwrap();
    std::fs::write(&b, b"same").unwrap();
    #[cfg(windows)]
    {
        let account = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN").unwrap(),
            std::env::var("USERNAME").unwrap()
        );
        for path in [&a, &b] {
            let status = std::process::Command::new("icacls.exe")
                .arg(path)
                .arg("/setowner")
                .arg(&account)
                .status()
                .unwrap();
            assert!(
                status.success(),
                "test fixtures must be owned by the current user"
            );
        }
    }
    let (_, files, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    let candidate = files
        .values()
        .find(|file| file.path == std::fs::canonicalize(&a).unwrap())
        .unwrap()
        .clone();
    let keeper = files
        .values()
        .find(|file| file.path == std::fs::canonicalize(&b).unwrap())
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
        std::fs::write(copy_path(dir.path(), &format!("copy-{number}"), "item"), b"same").unwrap();
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
        std::fs::write(copy_path(dir.path(), &format!("copy-{number}"), "item"), b"same").unwrap();
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
    assert!(index.removal_selection(&all, None).is_err());
    let mut retained = all;
    retained.remove(&group.files[0].id);
    assert_eq!(index.removal_selection(&retained, None).unwrap().0.len(), 204);
    assert!(index
        .removal_selection(&HashSet::from(["stale-id".to_string()]), None)
        .is_err());
    assert!(index.group("stale-group", 0).is_err());
    assert!(index.page(2).is_err());
}

#[test]
fn explicit_keeper_must_match_group_and_remain_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "c"] { std::fs::write(copy_path(dir.path(), name, "item"), b"same").unwrap(); }
    let index = fixture_index(&[std::fs::canonicalize(dir.path()).unwrap()], &AtomicBool::new(false));
    let group = index.page(0).unwrap().groups.remove(0);
    let selected = HashSet::from([group.files[0].id.clone()]);
    let requested = HashSet::from([group.files[1].id.clone()]);
    assert!(index.removal_selection(&selected, Some(&HashSet::new())).is_err());
    assert!(index.removal_selection(&selected, Some(&selected)).is_err());
    assert!(index.removal_selection(&selected, Some(&HashSet::from(["stale".into()]))).is_err());
    let (candidates, keepers) = index.removal_selection(&selected, Some(&requested)).unwrap();
    assert_eq!(keepers[0].path, index.file(&group.files[1].id).unwrap().path);
    std::fs::write(&keepers[0].path, b"different").unwrap();
    let result = safety::remove_files(candidates.clone(), keepers, &AtomicBool::new(false));
    assert_eq!(result.files_removed, 0);
    assert!(candidates[0].path.exists());
}

#[test]
fn explicit_keepers_must_cover_each_selected_group_once() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a1", "a2", "a3"] { std::fs::write(copy_path(dir.path(), name, "item-a"), b"same").unwrap(); }
    for name in ["b1", "b2"] { std::fs::write(copy_path(dir.path(), name, "item-b"), b"diff").unwrap(); }
    let index = fixture_index(&[std::fs::canonicalize(dir.path()).unwrap()], &AtomicBool::new(false));
    let groups = index.page(0).unwrap().groups;
    assert_eq!(groups.len(), 2);
    let a = groups.iter().find(|group| group.file_count == 3).unwrap();
    let b = groups.iter().find(|group| group.file_count == 2).unwrap();
    let selected = HashSet::from([a.files[0].id.clone(), b.files[0].id.clone()]);
    let correct = HashSet::from([a.files[1].id.clone(), b.files[1].id.clone()]);
    let (candidates, keepers) = index.removal_selection(&selected, Some(&correct)).unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(keepers.len(), 2);
    assert!(index.removal_selection(&selected, Some(&HashSet::from([a.files[1].id.clone()]))).is_err());
    let duplicate_group = HashSet::from([a.files[1].id.clone(), a.files[2].id.clone()]);
    assert!(index.removal_selection(&selected, Some(&duplicate_group)).is_err());
    let only_a = HashSet::from([a.files[0].id.clone()]);
    assert!(index.removal_selection(&only_a, Some(&HashSet::from([b.files[1].id.clone()]))).is_err());
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

#[test]
fn protected_application_and_codec_types_cannot_be_removed() {
    for name in [
        "installer.EXE",
        "library.dll",
        "decoder.ax",
        "codec.acm",
        "driver.sys",
    ] {
        assert!(
            safety::protected_removal_reason(std::path::Path::new(name)).is_some(),
            "{name}"
        );
    }
    assert!(
        safety::protected_removal_reason(std::path::Path::new("AppData/local/settings.json"))
            .is_some()
    );
}

#[test]
fn git_object_copy_is_verified_for_review_but_cannot_be_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let object_dir = dir.path().join(".GiT").join("lfs").join("objects");
    std::fs::create_dir_all(&object_dir).unwrap();
    std::fs::write(object_dir.join("video.mp4"), b"matching video bytes").unwrap();
    std::fs::write(dir.path().join("video.mp4"), b"matching video bytes").unwrap();
    // A hidden Git root is discoverable only when the caller explicitly adds it.
    // It must remain protected even when deliberately included alongside a
    // visible copy with identical contents.
    let (page, files, _) = scan_fixture(
        &[
            std::fs::canonicalize(dir.path()).unwrap(),
            std::fs::canonicalize(dir.path().join(".GiT")).unwrap(),
        ],
        &AtomicBool::new(false),
    );
    assert_eq!(page.groups.len(), 1, "identical contents form a verified group");
    let object = page.groups[0].files.iter().find(|file| file.path.to_lowercase().contains(".git")).unwrap();
    assert!(!object.removable);
    assert_eq!(object.removal_block_reason, Some("source_control"));
    let candidate = files.values().find(|file| file.path.to_string_lossy().to_lowercase().contains(".git")).unwrap().clone();
    let keeper = files.values().find(|file| !file.path.to_string_lossy().to_lowercase().contains(".git")).unwrap().clone();
    let result = safety::remove_files(vec![candidate.clone()], vec![keeper], &AtomicBool::new(false));
    assert_eq!(result.files_removed, 0);
    assert!(candidate.path.exists());
    assert!(result.errors.iter().any(|error| error.contains("source control metadata is protected")));
}

#[test]
fn blocked_copies_expose_only_stable_non_sensitive_reason_codes() {
    assert_eq!(
        safety::removal_block_code("user profile locations could not be verified"),
        "profiles_unavailable"
    );
    assert_eq!(
        safety::removal_block_code("another user profile is protected"),
        "other_profile"
    );
    assert_eq!(
        safety::removal_block_code("unexpected private path"),
        "unknown"
    );
}

#[test]
fn backend_refuses_executable_removal_even_if_selection_is_forged() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(copy_path(dir.path(), "a", "same.exe"), b"same").unwrap();
    std::fs::write(copy_path(dir.path(), "b", "same.exe"), b"same").unwrap();
    let (page, files, _) = scan_fixture(
        &[std::fs::canonicalize(dir.path()).unwrap()],
        &AtomicBool::new(false),
    );
    let row = &page.groups[0].files[0];
    assert!(!row.removable);
    assert_eq!(row.removal_block_reason, Some("protected_type"));
    assert_eq!(
        serde_json::to_value(row).unwrap()["removalBlockReason"],
        "protected_type"
    );
    assert!(matches!(
        row.profile_scope,
        "current" | "other" | "outside" | "unavailable"
    ));
    assert_eq!(
        serde_json::to_value(row).unwrap()["profileScope"],
        row.profile_scope
    );
    let mut files = files.into_values();
    let candidate = files.next().unwrap();
    let keeper = files.next().unwrap();
    let result = safety::remove_files(
        vec![candidate.clone()],
        vec![keeper],
        &AtomicBool::new(false),
    );
    assert_eq!(result.files_removed, 0);
    assert!(!result.errors.is_empty());
    assert!(candidate.path.exists());
}
