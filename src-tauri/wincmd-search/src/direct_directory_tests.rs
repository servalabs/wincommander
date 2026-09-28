// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::read_only::{AccessMode, CheckedDirectory};
use std::{collections::BTreeMap, os::windows::fs::OpenOptionsExt};
use tantivy::{
    directory::{INDEX_WRITER_LOCK, META_LOCK},
    schema::{Schema, TEXT},
    Index, ReloadPolicy,
};

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (path.clone(), fs::read(path).unwrap())
        })
        .collect()
}

#[test]
fn atomic_replace_preserves_open_snapshot_and_old_file_on_failure() {
    let root = tempfile::TempDir::new().unwrap();
    let directory = DirectDirectory::new(root.path());
    let path = Path::new("meta.json");
    directory.atomic_write(path, b"first").unwrap();
    let old = directory.get_file_handle(path).unwrap();
    directory.atomic_write(path, b"second").unwrap();
    assert_eq!(old.read_bytes(0..5).unwrap().as_slice(), b"first");
    assert!(old.read_bytes(0..6).is_err());
    assert_eq!(old.read_bytes(5..5).unwrap().len(), 0);
    assert_eq!(directory.atomic_read(path).unwrap(), b"second");
    drop(old);
    let held = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(root.path().join(path))
        .unwrap();
    assert!(directory.atomic_write(path, b"must-not-replace").is_err());
    drop(held);
    assert_eq!(directory.atomic_read(path).unwrap(), b"second");
    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        1,
        "failed write must remove its own temporary file"
    );
}

#[test]
fn writer_lock_excludes_another_thread_and_releases_on_drop() {
    let root = tempfile::TempDir::new().unwrap();
    let directory = DirectDirectory::new(root.path());
    let held = directory.acquire_lock(&INDEX_WRITER_LOCK).unwrap();
    let other = directory.clone();
    assert!(std::thread::spawn(move || matches!(
        other.acquire_lock(&INDEX_WRITER_LOCK),
        Err(LockError::LockBusy)
    ))
    .join()
    .unwrap());
    drop(held);
    assert!(directory.acquire_lock(&INDEX_WRITER_LOCK).is_ok());
}

#[test]
fn direct_backend_indexes_and_guarded_readonly_reopen_never_writes() {
    let root = tempfile::TempDir::new().unwrap();
    let directory = CheckedDirectory::direct_for_test(root.path(), AccessMode::Writable).unwrap();
    let mut schema = Schema::builder();
    let body = schema.add_text_field("body", TEXT);
    let index = Index::create(directory, schema.build(), Default::default()).unwrap();
    let mut writer = index.writer(15_000_000).unwrap();
    writer
        .add_document(tantivy::doc!(body => "orchidcheck"))
        .unwrap();
    writer.commit().unwrap();
    writer.wait_merging_threads().unwrap();
    drop(index);
    let before = snapshot(root.path());
    let reader_dir =
        CheckedDirectory::direct_for_test(root.path(), AccessMode::GuardedReader).unwrap();
    assert!(reader_dir
        .atomic_write(Path::new("meta.json"), b"denied")
        .is_err());
    assert!(reader_dir.open_write(Path::new("new-file")).is_err());
    assert!(reader_dir.delete(Path::new("meta.json")).is_err());
    assert!(reader_dir.acquire_lock(&INDEX_WRITER_LOCK).is_err());
    assert!(reader_dir.acquire_lock(&META_LOCK).is_ok());
    assert!(reader_dir.atomic_read(Path::new("../outside")).is_err());
    let index = Index::open(reader_dir).unwrap();
    let reader: tantivy::IndexReader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()
        .unwrap();
    let query = tantivy::query::QueryParser::for_index(&index, vec![body])
        .parse_query("orchidcheck")
        .unwrap();
    assert_eq!(
        reader
            .searcher()
            .search(&query, &tantivy::collector::Count)
            .unwrap(),
        1
    );
    drop(reader);
    drop(index);
    assert_eq!(snapshot(root.path()), before);
}
