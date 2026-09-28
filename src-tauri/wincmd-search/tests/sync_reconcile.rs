// SPDX-License-Identifier: AGPL-3.0-or-later
use std::{fs, path::Path, sync::atomic::AtomicBool};
use wincmd_search::{
    types::{ContentQuery, IndexConfig},
    SearchEngine,
};

fn open(root: &Path) -> SearchEngine {
    SearchEngine::open(IndexConfig {
        roots: vec![root.into()],
        exclusions: vec![],
        skip_paths: vec![],
        max_file_bytes: 1024,
        index_dir: root.join(".wincommander").join("search"),
    })
    .unwrap()
}

fn hits(engine: &SearchEngine, terms: &str) -> usize {
    engine
        .search_restricted(&ContentQuery {
            terms: terms.into(),
            roots: vec![],
            limit: 20,
            offset: 0,
            keyword_only: true,
        })
        .unwrap()
        .len()
}

#[cfg(windows)]
#[test]
fn locked_file_does_not_discard_other_progress_and_is_retried_after_release() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::TempDir::new().unwrap();
    let blocked = root.path().join("blocked.txt");
    fs::write(&blocked, "lockretrytoken").unwrap();
    fs::write(root.path().join("available.txt"), "availabletoken").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&blocked)
        .unwrap();
    let engine = open(root.path());
    let report = engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
    assert_eq!(report.indexed_docs, 1);
    assert!(!report.complete);
    assert_eq!(hits(&engine, "availabletoken"), 1);
    drop(lock);
    let report = engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
    assert!(report.complete);
    assert_eq!(report.indexed_docs, 2);
    assert_eq!(hits(&engine, "lockretrytoken"), 1);
}

#[test]
fn sync_style_create_replace_rename_and_delete_reconcile_without_rebuild() {
    let root = tempfile::TempDir::new().unwrap();
    let file = root.path().join("orchidcheck.txt");
    fs::write(&file, "initialbodytoken").unwrap();
    let engine = open(root.path());
    assert!(
        engine
            .reconcile_sync(&AtomicBool::new(false), 10)
            .unwrap()
            .complete
    );
    assert_eq!(hits(&engine, "orchidcheck"), 1);
    assert_eq!(hits(&engine, "initialbodytoken"), 1);
    let temporary = root.path().join(".syncthing.orchidcheck.tmp");
    fs::write(&temporary, "replacementbodytoken").unwrap();
    fs::remove_file(&file).unwrap();
    fs::rename(temporary, &file).unwrap();
    assert!(
        engine
            .reconcile_sync(&AtomicBool::new(false), 10)
            .unwrap()
            .complete
    );
    assert_eq!(hits(&engine, "replacementbodytoken"), 1);
    assert_eq!(hits(&engine, "initialbodytoken"), 0);
    let renamed = root.path().join("movedorchid.txt");
    fs::rename(file, &renamed).unwrap();
    assert!(
        engine
            .reconcile_sync(&AtomicBool::new(false), 10)
            .unwrap()
            .complete
    );
    assert_eq!(hits(&engine, "orchidcheck"), 0);
    assert_eq!(hits(&engine, "movedorchid"), 1);
    fs::remove_file(renamed).unwrap();
    let report = engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
    assert!(report.complete);
    assert_eq!(report.indexed_docs, 0);
    assert_eq!(hits(&engine, "replacementbodytoken"), 0);
}
