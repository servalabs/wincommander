#[test]
fn existing_index_data_cannot_be_mistaken_for_fresh_storage() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("meta.json");
    std::fs::write(&original, b"existing index").unwrap();
    assert!(TrialDirectory::open_protected(root.path(), AccessMode::Writable, &[6; 16], false).is_err());
    assert!(!root.path().join("protected-search-key.v1").exists());
    assert_eq!(std::fs::read(original).unwrap(), b"existing index");
}
#[test]
fn protected_read_only_directories_never_create_a_key() {
    let root = tempfile::tempdir().unwrap();
    for mode in [AccessMode::SharedReader, AccessMode::GuardedReader] {
        assert!(TrialDirectory::open_protected(root.path(), mode, &[6; 16], false).is_err());
        assert!(!root.path().join("protected-search-key.v1").exists());
        assert!(TrialDirectory::open_protected(root.path(), mode, &[6; 16], true).is_err());
        assert!(!root.path().join("protected-search-key.v1").exists());
    }
}
#[test]
fn protected_key_remains_pinned_until_directory_clones_and_handles_close() {
    let root = tempfile::tempdir().unwrap();
    let directory = TrialDirectory::open_protected(root.path(), AccessMode::Writable, &[6; 16], false).unwrap();
    directory.atomic_write(Path::new("segment"), b"secret").unwrap();
    let clone = directory.clone();
    let reader = clone.get_file_handle(Path::new("segment")).unwrap();
    let writer = clone.open_write(Path::new("other-segment")).unwrap();
    let key_path = root.path().join("protected-search-key.v1");
    assert!(std::fs::remove_file(&key_path).is_err());
    drop(directory);
    drop(clone);
    assert!(std::fs::remove_file(&key_path).is_err());
    drop(reader);
    assert!(std::fs::remove_file(&key_path).is_err());
    drop(writer);
    std::fs::remove_file(&key_path).unwrap();
}
#[cfg(feature = "experimental-encrypted-search")]
#[test]
fn production_schema_can_rebuild_and_reopen_in_a_separate_encrypted_index() {
    use crate::{
        index::ContentIndex,
        types::{ContentQuery, DocProps, FileMeta},
    };
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("legacy");
    let new = root.path().join("encrypted");
    std::fs::create_dir(&old).unwrap();
    std::fs::create_dir(&new).unwrap();
    std::fs::write(old.join("meta.json"), b"legacy intact").unwrap();
    let index = ContentIndex::open_experimental_encrypted(&new, [33; 16], false).unwrap();
    let meta = FileMeta {
        doc_id: 1,
        path: root.path().join("document.txt"),
        name: "document.txt".into(),
        ext: "txt".into(),
        mtime: 1,
        size: 24,
    };
    let mut writer = index.writer().unwrap();
    index
        .upsert(
            &mut writer,
            &meta,
            "document",
            "confidential schemaindexsecret",
            &DocProps::default(),
        )
        .unwrap();
    index.commit(&mut writer).unwrap();
    drop(writer);
    drop(index);
    for entry in std::fs::read_dir(&new).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            let bytes = std::fs::read(entry.path()).unwrap();
            assert!(!bytes.windows(b"schemaindexsecret".len()).any(|w| w == b"schemaindexsecret"));
        }
    }
    let reopened = ContentIndex::open_experimental_encrypted(&new, [33; 16], true).unwrap();
    let query = ContentQuery {
        terms: "schemaindexsecret".into(),
        roots: vec![],
        limit: 10,
        offset: 0,
        keyword_only: true,
    };
    assert_eq!(reopened.search_keyword(&query).unwrap().len(), 1);
    assert_eq!(std::fs::read(old.join("meta.json")).unwrap(), b"legacy intact");
}
#[test]
fn isolated_rebuild_uses_user_key_and_preserves_legacy_index() {
    use tantivy::{
        collector::Count,
        doc,
        query::QueryParser,
        schema::{Schema, STORED, TEXT},
        Index,
    };
    let sandbox = tempfile::tempdir().unwrap();
    let old_index = sandbox.path().join("old-index");
    let fresh_index = sandbox.path().join("new-index");
    std::fs::create_dir(&old_index).unwrap();
    std::fs::create_dir(&fresh_index).unwrap();
    let legacy_file = old_index.join("meta.json");
    std::fs::write(&legacy_file, b"legacy index retained").unwrap();
    let document = sandbox.path().join("document.txt");
    std::fs::write(&document, "confidential rebuildneedleword example").unwrap();

    let directory = TrialDirectory::open_protected(&fresh_index, AccessMode::Writable, &[13; 16], false).unwrap();
    let mut schema = Schema::builder();
    let body = schema.add_text_field("body", TEXT | STORED);
    let index = Index::create(directory, schema.build(), Default::default()).unwrap();
    let mut writer = index.writer(50_000_000).unwrap();
    let contents = std::fs::read_to_string(&document).unwrap();
    writer.add_document(doc!(body => contents)).unwrap();
    writer.commit().unwrap();
    drop(writer);
    drop(index);
    assert_eq!(std::fs::read(&legacy_file).unwrap(), b"legacy index retained");
    assert!(fresh_index.join("protected-search-key.v1").is_file());
    for entry in std::fs::read_dir(&fresh_index).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            let bytes = std::fs::read(entry.path()).unwrap();
            assert!(!bytes.windows(b"rebuildneedleword".len()).any(|w| w == b"rebuildneedleword"));
        }
    }
    let directory = TrialDirectory::open_protected(&fresh_index, AccessMode::SharedReader, &[13; 16], true).unwrap();
    let reopened = Index::open(directory).unwrap();
    let query = QueryParser::for_index(&reopened, vec![body])
        .parse_query("rebuildneedleword")
        .unwrap();
    assert_eq!(reopened.reader().unwrap().searcher().search(&query, &Count).unwrap(), 1);
    assert_eq!(std::fs::read(&legacy_file).unwrap(), b"legacy index retained");
}
#[test]
fn real_tantivy_index_search_and_reopen_are_encrypted() {
    use tantivy::{
        collector::Count,
        doc,
        query::QueryParser,
        schema::{Schema, STORED, TEXT},
        Index,
    };
    let dir = tempfile::tempdir().unwrap();
    let directory = TrialDirectory::open(dir.path(), AccessMode::Writable, [9; 32]);
    let mut schema = Schema::builder();
    let body = schema.add_text_field("body", TEXT | STORED);
    let schema = schema.build();
    let index = Index::create(directory.clone(), schema, Default::default()).unwrap();
    let mut writer = index.writer(50_000_000).unwrap();
    writer
        .add_document(doc!(body => "confidential gossamersearchword example"))
        .unwrap();
    writer.commit().unwrap();
    drop(writer);
    drop(index);
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_file() {
            continue;
        }
        let bytes = std::fs::read(entry.path()).unwrap();
        assert!(!bytes.windows(18).any(|w| w == b"gossamersearchword"));
    }
    let reopened = Index::open(directory).unwrap();
    let query = QueryParser::for_index(&reopened, vec![body])
        .parse_query("gossamersearchword")
        .unwrap();
    let reader = reopened.reader().unwrap();
    assert_eq!(reader.searcher().search(&query, &Count).unwrap(), 1);
}

#[test]
fn streaming_writer_and_file_handle_use_bounded_chunks() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug)]
    struct MeasuredHandle {
        inner: Arc<dyn FileHandle>,
        largest: Arc<AtomicUsize>,
    }
    impl HasLen for MeasuredHandle {
        fn len(&self) -> usize {
            self.inner.len()
        }
    }
    impl FileHandle for MeasuredHandle {
        fn read_bytes(&self, range: Range<usize>) -> io::Result<OwnedBytes> {
            self.largest.fetch_max(range.len(), Ordering::Relaxed);
            self.inner.read_bytes(range)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let inner = MmapDirectory::open(dir.path()).unwrap();
    let path = Path::new("stream.idx");
    let key = [11; 32];
    let mut writer = TrialWriter::new(
        inner.open_write(path).unwrap(),
        Arc::new(KeyOwner::Fixture(key)),
        "stream.idx".into(),
    )
    .unwrap();
    let bytes = vec![42; CHUNK * 8 + 333];
    for part in bytes.chunks(971) {
        writer.write_all(part).unwrap();
        assert!(writer.plain.len() < CHUNK);
    }
    writer.terminate().unwrap();
    let largest = Arc::new(AtomicUsize::new(0));
    let inner_file = inner.get_file_handle(path).unwrap();
    let measured = Arc::new(MeasuredHandle {
        inner: inner_file,
        largest: largest.clone(),
    });
    let mut file = ProtectedFile::open(
        HandleReader {
            inner: measured,
            offset: 0,
        },
        &key,
        "stream.idx",
    )
    .unwrap();
    assert_eq!(file.read(CHUNK - 7..CHUNK + 7).unwrap(), bytes[CHUNK - 7..CHUNK + 7]);
    assert_eq!(file.read(0..bytes.len()).unwrap(), bytes);
    assert!(largest.load(Ordering::Relaxed) <= CHUNK + OVERHEAD);
}
