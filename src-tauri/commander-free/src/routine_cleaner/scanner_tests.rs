// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn target(path: std::path::PathBuf) -> ScanTarget {
    ScanTarget {
        category: "developers".into(),
        label: "Test cache".into(),
        description: Some("A test description from the target rule.".into()),
        path,
        operation: TargetOperation::Delete,
        recommended: false,
        minimum_age: Duration::ZERO,
        containment_root: None,
        containment_source: None,
    }
}

#[test]
fn overlapping_targets_count_and_cache_each_file_once() {
    let temp = tempfile::tempdir().unwrap();
    let child = temp.path().join("nested");
    fs::create_dir(&child).unwrap();
    let path = child.join("cache");
    fs::write(&path, b"123456").unwrap();
    let modified = SystemTime::now()
        .checked_sub(Duration::from_secs(7200))
        .unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let (scan, cached) = scan_targets(
        vec![target(temp.path().to_path_buf()), target(child)],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!((scan.total_bytes, scan.total_files), (6, 1));
    assert_eq!(cached.iter().map(|item| item.files.len()).sum::<usize>(), 1);
    assert_eq!(scan.items[0].entry_count, 1);
    assert_eq!(scan.items[0].entries.len(), 1);
    assert!(!scan.items[0].entries_truncated);
}

#[test]
fn file_preview_exposes_backend_path_and_actual_logical_size() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cache item.bin");
    fs::write(&path, b"123456").unwrap();
    let modified = SystemTime::now()
        .checked_sub(Duration::from_secs(7200))
        .unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();

    let (scan, _) = scan_targets(
        vec![target(temp.path().to_path_buf())],
        &AtomicBool::new(false),
    )
    .unwrap();

    let preview = &scan.items[0];
    assert_eq!(preview.path_display.as_deref(), temp.path().to_str());
    assert_eq!(
        preview.description.as_deref(),
        Some("A test description from the target rule.")
    );
    assert_eq!(preview.entry_count, 1);
    assert_eq!(preview.entries.len(), 1);
    assert_eq!(
        preview.entries[0].path.as_deref(),
        fs::canonicalize(&path).unwrap().to_str()
    );
    assert_eq!(preview.entries[0].actual_size_bytes, 6);
    assert_eq!(preview.entries[0].estimated_reclaim_bytes, None);
}

#[test]
fn file_preview_limit_is_distinct_from_backend_partial_scan() {
    let temp = tempfile::tempdir().unwrap();
    let modified = SystemTime::now()
        .checked_sub(Duration::from_secs(7200))
        .unwrap();
    for index in 0..=MAX_PREVIEW_ENTRIES_PER_TARGET {
        let path = temp.path().join(format!("cache-{index:03}.bin"));
        fs::write(&path, b"x").unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }

    let (scan, _) = scan_targets(
        vec![target(temp.path().to_path_buf())],
        &AtomicBool::new(false),
    )
    .unwrap();

    let preview = &scan.items[0];
    assert_eq!(preview.entry_count, MAX_PREVIEW_ENTRIES_PER_TARGET + 1);
    assert_eq!(preview.entries.len(), MAX_PREVIEW_ENTRIES_PER_TARGET);
    assert!(preview.entries_truncated);
    assert!(!preview.truncated);
}

#[test]
fn vacuum_preview_separates_database_size_from_estimated_reclaim() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cache.sqlite");
    let mut database = b"SQLite format 3\0".to_vec();
    database.extend([0u8; 32]);
    fs::write(&path, &database).unwrap();
    fs::write(format!("{}-wal", path.to_string_lossy()), b"waldata").unwrap();
    let mut vacuum_target = target(path.clone());
    vacuum_target.operation = TargetOperation::Vacuum;

    let (scan, _) = scan_targets(vec![vacuum_target], &AtomicBool::new(false)).unwrap();

    let item = &scan.items[0];
    assert_eq!(item.bytes, 7 + database.len() as u64 / 10);
    assert_eq!(item.entries.len(), 1);
    assert_eq!(item.entries[0].actual_size_bytes, database.len() as u64);
    assert_eq!(item.entries[0].estimated_reclaim_bytes, Some(item.bytes));
}

#[test]
fn inaccessible_target_is_counted_as_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let (scan, cached) = scan_targets(
        vec![target(temp.path().join("missing"))],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(scan.skipped_targets, 1);
    assert!(cached.is_empty());
}

#[test]
fn entry_limit_bounds_scanning_even_when_every_file_is_too_recent() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("first"), b"1").unwrap();
    fs::write(temp.path().join("second"), b"2").unwrap();
    let (files, bytes, partial) =
        collect_deletable_files(temp.path(), Duration::ZERO, &AtomicBool::new(false), 1);
    assert!(files.is_empty());
    assert_eq!(bytes, 0);
    assert!(partial);
}
#[test]
fn recognizes_sqlite_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    fs::write(&path, b"SQLite format 3\0rest").unwrap();
    assert!(has_sqlite_header(&path));
}
#[test]
fn rejects_non_sqlite_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    fs::write(&path, b"not a database").unwrap();
    assert!(!has_sqlite_header(&path));
}

#[test]
fn preserves_files_that_become_recent_during_a_cleaner_session() {
    let now = SystemTime::now();
    assert!(!is_old_enough(Some(now), now, Duration::from_secs(0)));
    assert!(is_old_enough(
        Some(now.checked_sub(Duration::from_secs(60 * 60 + 1)).unwrap()),
        now,
        Duration::from_secs(0),
    ));
    assert!(!is_old_enough(
        Some(now.checked_sub(Duration::from_secs(60 * 60 + 1)).unwrap()),
        now,
        Duration::from_secs(24 * 60 * 60),
    ));
    assert!(!is_old_enough(None, now, Duration::ZERO));
    assert!(!is_old_enough(
        Some(now.checked_add(Duration::from_secs(1)).unwrap()),
        now,
        Duration::ZERO,
    ));
    assert!(is_old_enough(
        Some(now.checked_sub(RECENT_FILE_AGE).unwrap()),
        now,
        Duration::ZERO,
    ));
}
