// SPDX-License-Identifier: AGPL-3.0-or-later
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};
use wincmd_search::{
    crawler::doc_id_for,
    types::{ContentQuery, IndexConfig},
    SearchEngine,
};

#[test]
fn relocated_readonly_index_uses_current_scopes_without_changing_storage() {
    let sandbox = tempfile::TempDir::new().unwrap();
    let old_root = sandbox.path().join("original-mount");
    let new_root = sandbox.path().join("current-mount");
    fs::create_dir_all(old_root.join("allowed")).unwrap();
    fs::create_dir_all(old_root.join("removed")).unwrap();
    let original_file = old_root.join("allowed").join("secret.txt");
    fs::write(
        &original_file,
        format!("relocatedtoken {}", "padding ".repeat(100)),
    )
    .unwrap();
    let removed_file = old_root.join("removed").join("secret.txt");
    fs::write(&removed_file, "relocatedtoken").unwrap();
    for number in 0..40 {
        fs::write(
            old_root
                .join("removed")
                .join(format!("relocatedtoken-{number}.txt")),
            "relocatedtoken relocatedtoken relocatedtoken",
        )
        .unwrap();
    }
    let original_id = doc_id_for(&original_file);
    let removed_id = doc_id_for(&removed_file);
    let config = IndexConfig {
        roots: vec![old_root.clone()],
        exclusions: vec![],
        skip_paths: vec![],
        max_file_bytes: 4096,
        index_dir: old_root.join(".wincommander/search"),
    };
    {
        let engine = SearchEngine::open(config.clone()).unwrap();
        assert!(
            engine
                .reconcile_sync(&AtomicBool::new(false), 128)
                .unwrap()
                .complete
        );
    }
    fs::rename(&old_root, &new_root).unwrap();
    let mut config = config;
    config.index_dir = new_root.join(".wincommander/search");
    config.roots = vec![new_root.join("allowed")];
    let snapshot = || -> BTreeMap<PathBuf, Vec<u8>> {
        fs::read_dir(&config.index_dir)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (path.clone(), fs::read(path).unwrap())
            })
            .collect()
    };
    let before = snapshot();
    let mapper_old = old_root.clone();
    let mapper_new = new_root.clone();
    let mapper: wincmd_search::PathMapper = Arc::new(move |path| {
        path.strip_prefix(&mapper_old)
            .ok()
            .map(|relative| mapper_new.join(relative))
    });
    let engine = SearchEngine::open_existing_with_mapped_paths(
        config.clone(),
        Arc::new(|_| true),
        mapper.clone(),
    )
    .unwrap();
    let mut query = ContentQuery {
        terms: "relocatedtoken".into(),
        roots: vec![],
        limit: 1,
        offset: 0,
        keyword_only: true,
    };
    let hits = engine.search_restricted(&query).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        PathBuf::from(&hits[0].path),
        new_root.join("allowed/secret.txt")
    );
    assert_eq!(hits[0].doc_id, original_id);
    assert!(!engine
        .get_chunks_restricted(hits[0].doc_id)
        .unwrap()
        .is_empty());
    assert!(engine.get_chunks_restricted(removed_id).unwrap().is_empty());
    query.roots = vec![new_root.join("removed")];
    assert!(engine.search_restricted(&query).unwrap().is_empty());
    query.roots = vec![new_root.clone()];
    assert_eq!(engine.search_restricted(&query).unwrap().len(), 1);
    assert!(engine.reconcile_sync(&AtomicBool::new(false), 10).is_err());
    drop(engine);
    let mut empty = config.clone();
    empty.roots.clear();
    let engine =
        SearchEngine::open_existing_with_mapped_paths(empty, Arc::new(|_| true), mapper).unwrap();
    assert!(engine.search_restricted(&query).unwrap().is_empty());
    assert!(engine
        .get_chunks_restricted(original_id)
        .unwrap()
        .is_empty());
    drop(engine);
    assert_eq!(snapshot(), before);
}
