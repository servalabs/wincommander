// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn target(path: std::path::PathBuf) -> ScanTarget {
    ScanTarget {
        category: "developers".into(),
        label: "Test cache".into(),
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
