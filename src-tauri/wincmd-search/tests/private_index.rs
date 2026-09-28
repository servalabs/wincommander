// SPDX-License-Identifier: AGPL-3.0-or-later
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};
use tempfile::TempDir;
use wincmd_search::{
    crawler::doc_id_for,
    types::{ContentQuery, IndexConfig},
    SearchEngine,
};

fn config(root: &Path) -> IndexConfig {
    IndexConfig {
        roots: vec![root.to_owned()],
        exclusions: vec![],
        skip_paths: vec![],
        max_file_bytes: 1_000_000,
        index_dir: root.join(".wincommander").join("search"),
    }
}

fn query(terms: &str) -> ContentQuery {
    ContentQuery {
        terms: terms.into(),
        roots: vec![],
        limit: 100,
        offset: 0,
        keyword_only: true,
    }
}

fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (path.clone(), fs::read(path).unwrap())
        })
        .collect()
}

#[test]
fn readonly_queries_never_create_locks_or_migrate_incompatible_indexes() {
    let volume = TempDir::new().unwrap();
    fs::write(volume.path().join("secret.txt"), "privatevolumetoken").unwrap();
    let cfg = config(volume.path());
    {
        let engine = SearchEngine::open(cfg.clone()).unwrap();
        assert!(
            engine
                .reconcile_sync(&AtomicBool::new(false), 10)
                .unwrap()
                .complete
        );
    }
    for entry in fs::read_dir(&cfg.index_dir).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".lock")
        {
            fs::remove_file(path).unwrap();
        }
    }
    let before = snapshot(&cfg.index_dir);
    {
        let reader = SearchEngine::open_existing(cfg.clone()).unwrap();
        assert_eq!(
            reader
                .search_restricted(&query("privatevolumetoken"))
                .unwrap()
                .len(),
            1
        );
        assert!(reader.reconcile_sync(&AtomicBool::new(false), 10).is_err());
    }
    assert_eq!(snapshot(&cfg.index_dir), before);
    fs::write(cfg.index_dir.join("schema.version"), "old-generation").unwrap();
    let before = snapshot(&cfg.index_dir);
    assert!(SearchEngine::open_existing(cfg.clone()).is_err());
    assert_eq!(snapshot(&cfg.index_dir), before);
    let mut missing = cfg;
    missing.index_dir = volume.path().join("absent");
    assert!(SearchEngine::open_existing(missing.clone()).is_err());
    assert!(!missing.index_dir.exists());
}

#[test]
fn bounded_reconciliation_makes_progress_and_prunes_only_after_a_complete_scan() {
    let volume = TempDir::new().unwrap();
    for name in ["one.txt", "two.txt", "three.txt"] {
        fs::write(volume.path().join(name), "boundedtoken").unwrap();
    }
    let engine = SearchEngine::open(config(volume.path())).unwrap();
    for expected in 1..=3 {
        let report = engine.reconcile_sync(&AtomicBool::new(false), 1).unwrap();
        assert_eq!(report.indexed_docs, expected);
        assert_eq!(report.updated, 1);
    }
    fs::remove_file(volume.path().join("one.txt")).unwrap();
    let cancelled = engine.reconcile_sync(&AtomicBool::new(true), 1).unwrap();
    assert!(!cancelled.complete);
    assert_eq!(cancelled.indexed_docs, 3);
    let report = engine.reconcile_sync(&AtomicBool::new(false), 1).unwrap();
    assert!(report.complete);
    assert_eq!(report.removed, 1);
    assert_eq!(report.indexed_docs, 2);
}

#[test]
fn removed_roots_and_empty_roots_cannot_retrieve_historical_text() {
    let volume = TempDir::new().unwrap();
    let a = volume.path().join("a");
    let b = volume.path().join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    let file = a.join("secret.txt");
    fs::write(&file, "historicaltoken").unwrap();
    let cfg = config(volume.path());
    let id = doc_id_for(&file);
    {
        let engine = SearchEngine::open(cfg.clone()).unwrap();
        engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
        assert!(!engine.get_chunks_restricted(id).unwrap().is_empty());
    }
    for roots in [vec![b], vec![]] {
        let mut narrowed = cfg.clone();
        narrowed.roots = roots;
        let engine = SearchEngine::open_existing(narrowed).unwrap();
        assert!(engine
            .search_restricted(&query("historicaltoken"))
            .unwrap()
            .is_empty());
        assert!(engine.get_chunks_restricted(id).unwrap().is_empty());
        assert_eq!(engine.search(&query("historicaltoken")).unwrap().len(), 1);
    }
}

#[test]
fn index_contents_are_never_indexed_and_unsupported_files_are_searchable_by_name() {
    let volume = TempDir::new().unwrap();
    let cfg = config(volume.path());
    let engine = SearchEngine::open(cfg.clone()).unwrap();
    fs::write(cfg.index_dir.join("leak.txt"), "indexselfleaktoken").unwrap();
    fs::write(volume.path().join("rarephotograph.xyz"), [0, 1, 2]).unwrap();
    let report = engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
    assert!(report.complete);
    assert_eq!(report.indexed_docs, 1);
    assert!(engine
        .search(&query("indexselfleaktoken"))
        .unwrap()
        .is_empty());
    assert_eq!(
        engine
            .search_restricted(&query("rarephotograph"))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn policy_changes_block_both_search_and_previews_and_prevent_ingestion() {
    let volume = TempDir::new().unwrap();
    let file = volume.path().join("secret.txt");
    fs::write(&file, "restrictedtoken").unwrap();
    let permitted = Arc::new(AtomicBool::new(true));
    let flag = permitted.clone();
    let engine = SearchEngine::open_with_policy(
        config(volume.path()),
        Arc::new(move |_| flag.load(std::sync::atomic::Ordering::Relaxed)),
    )
    .unwrap();
    engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
    permitted.store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(engine
        .search_restricted(&query("restrictedtoken"))
        .unwrap()
        .is_empty());
    assert!(engine
        .get_chunks_restricted(doc_id_for(&file))
        .unwrap()
        .is_empty());
    assert!(engine.reconcile_sync(&AtomicBool::new(false), 10).is_err());
}

#[test]
fn missing_root_cannot_delete_the_previous_generation() {
    let volume = TempDir::new().unwrap();
    let root = volume.path().join("mounted");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("file.txt"), "preservedtoken").unwrap();
    let mut cfg = config(volume.path());
    cfg.roots = vec![root.clone()];
    let engine = SearchEngine::open(cfg).unwrap();
    engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
    fs::rename(root, volume.path().join("unmounted")).unwrap();
    assert!(engine.reconcile_sync(&AtomicBool::new(false), 10).is_err());
    assert_eq!(engine.indexed_document_count().unwrap(), 1);
    assert!(engine
        .search_restricted(&query("preservedtoken"))
        .unwrap()
        .is_empty());
}

#[test]
fn same_size_edits_in_one_second_are_reconciled() {
    let volume = TempDir::new().unwrap();
    let file = volume.path().join("edit.txt");
    fs::write(&file, "oldtoken").unwrap();
    let engine = SearchEngine::open(config(volume.path())).unwrap();
    engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
    fs::write(&file, "newtoken").unwrap();
    assert_eq!(
        engine
            .reconcile_sync(&AtomicBool::new(false), 10)
            .unwrap()
            .updated,
        1
    );
    assert!(engine
        .search_restricted(&query("oldtoken"))
        .unwrap()
        .is_empty());
    assert_eq!(
        engine.search_restricted(&query("newtoken")).unwrap().len(),
        1
    );
}

#[test]
fn child_query_scope_cannot_widen_configured_roots() {
    let volume = TempDir::new().unwrap();
    for name in ["first", "second"] {
        let dir = volume.path().join(name);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("file.txt"), "scopetoken").unwrap();
    }
    let engine = SearchEngine::open(config(volume.path())).unwrap();
    engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
    let mut scoped = query("scopetoken");
    scoped.roots = vec![volume.path().join("second")];
    let hits = engine.search_restricted(&scoped).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].path.contains("second"));
}

#[test]
fn files_over_extraction_limit_remain_searchable_by_name_without_body() {
    let volume = TempDir::new().unwrap();
    let file = volume.path().join("large-recording.txt");
    fs::write(&file, "oversizedbodytoken").unwrap();
    let mut cfg = config(volume.path());
    cfg.max_file_bytes = 1;
    let engine = SearchEngine::open(cfg).unwrap();
    engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
    assert_eq!(
        engine.search_restricted(&query("recording")).unwrap().len(),
        1
    );
    assert!(engine
        .search_restricted(&query("oversizedbodytoken"))
        .unwrap()
        .is_empty());
}
