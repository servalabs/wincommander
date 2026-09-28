// SPDX-License-Identifier: AGPL-3.0-or-later
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};
use tantivy::directory::{Directory, META_LOCK};
use tempfile::TempDir;
use wincmd_search::{
    index::ContentIndex,
    types::{ContentQuery, DocProps, FileMeta, IndexConfig},
    SearchEngine,
};

fn config(root: &Path) -> IndexConfig {
    IndexConfig {
        roots: vec![root.into()],
        exclusions: vec![],
        skip_paths: vec![],
        max_file_bytes: 1024,
        index_dir: root.join("index"),
    }
}

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
fn shared_reader_observes_metadata_lock_held_by_an_ordinary_writer() {
    let root = TempDir::new().unwrap();
    let cfg = config(root.path());
    let index = ContentIndex::open_or_create(&cfg.index_dir).unwrap();
    let lock = index.index.directory().acquire_lock(&META_LOCK).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        started_tx.send(()).unwrap();
        done_tx
            .send(SearchEngine::open_existing_shared(cfg).is_ok())
            .unwrap();
    });
    started_rx.recv().unwrap();
    assert!(matches!(
        done_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop(lock);
    assert!(done_rx.recv_timeout(Duration::from_secs(5)).unwrap());
    reader.join().unwrap();
}

#[test]
fn ordinary_readers_coexist_with_commits_and_reopen_at_the_latest_generation() {
    let root = TempDir::new().unwrap();
    let cfg = config(root.path());
    let index = Arc::new(ContentIndex::open_or_create(&cfg.index_dir).unwrap());
    let mut writer = index.writer().unwrap();
    let file = root.path().join("file.txt");
    let meta = FileMeta {
        doc_id: 1,
        path: file,
        name: "file.txt".into(),
        ext: "txt".into(),
        mtime: 0,
        size: 10,
    };
    index
        .upsert(
            &mut writer,
            &meta,
            "file",
            "commontoken first",
            &DocProps::default(),
        )
        .unwrap();
    index.commit(&mut writer).unwrap();
    let writer_index = Arc::clone(&index);
    let writer_thread = thread::spawn(move || {
        for generation in 0..8 {
            writer_index
                .upsert(
                    &mut writer,
                    &meta,
                    "file",
                    &format!("commontoken generation{generation}"),
                    &DocProps::default(),
                )
                .unwrap();
            writer_index.commit(&mut writer).unwrap();
        }
        writer.wait_merging_threads().unwrap();
    });
    let mut query = ContentQuery {
        terms: "commontoken".into(),
        roots: vec![],
        limit: 10,
        offset: 0,
        keyword_only: true,
    };
    for _ in 0..8 {
        let reader = SearchEngine::open_existing_shared(cfg.clone()).unwrap();
        assert_eq!(reader.search(&query).unwrap().len(), 1);
        assert!(reader.index_files_sync(vec![]).is_err());
    }
    writer_thread.join().unwrap();
    query.terms = "generation7".into();
    let reader = SearchEngine::open_existing_shared(cfg).unwrap();
    assert_eq!(reader.search(&query).unwrap().len(), 1);
}

#[test]
fn shared_reader_never_creates_an_index_or_migrates_an_incompatible_schema() {
    let root = TempDir::new().unwrap();
    let cfg = config(root.path());
    assert!(SearchEngine::open_existing_shared(cfg.clone()).is_err());
    assert!(!cfg.index_dir.exists());
    drop(SearchEngine::open(cfg.clone()).unwrap());
    fs::write(cfg.index_dir.join("schema.version"), "incompatible").unwrap();
    let before = snapshot(&cfg.index_dir);
    assert!(SearchEngine::open_existing_shared(cfg.clone()).is_err());
    assert_eq!(snapshot(&cfg.index_dir), before);
}
