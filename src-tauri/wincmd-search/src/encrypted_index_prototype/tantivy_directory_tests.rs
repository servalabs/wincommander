#[test]
fn failed_stream_cannot_be_finalized_or_read_as_a_complete_file() {
    struct FailAfter {
        bytes: Arc<Mutex<Vec<u8>>>,
        limit: usize,
    }
    impl Write for FailAfter {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            let mut bytes = self.bytes.lock().unwrap();
            let available = self.limit.saturating_sub(bytes.len());
            if available == 0 {
                return Err(io::Error::other("injected write failure"));
            }
            let n = available.min(input.len());
            bytes.extend_from_slice(&input[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl TerminatingWrite for FailAfter {
        fn terminate_ref(&mut self, _: AntiCallToken) -> io::Result<()> {
            Ok(())
        }
    }
    let output = Arc::new(Mutex::new(Vec::new()));
    let sink: WritePtr = BufWriter::new(Box::new(FailAfter {
        bytes: output.clone(),
        limit: 100,
    }));
    let key = [17; 32];
    let mut writer = TrialWriter::new(sink, Arc::new(KeyOwner::Fixture(key)), "failed.idx".into()).unwrap();
    assert!(writer.write_all(&vec![4; CHUNK]).is_err());
    assert!(writer.write_all(b"retry").is_err());
    assert!(writer.terminate().is_err());
    let stored = output.lock().unwrap().clone();
    assert!(ProtectedFile::open(io::Cursor::new(stored), &key, "failed.idx").is_err());
}

#[test]
fn checked_shared_reader_searches_but_cannot_mutate() {
    use tantivy::{
        collector::Count,
        doc,
        query::QueryParser,
        schema::{Schema, TEXT},
        Index,
    };
    let dir = tempfile::tempdir().unwrap();
    let writer_dir = TrialDirectory::open(dir.path(), AccessMode::Writable, [23; 32]);
    let mut schema = Schema::builder();
    let body = schema.add_text_field("body", TEXT);
    let index = Index::create(writer_dir.clone(), schema.build(), Default::default()).unwrap();
    let mut writer = index.writer(50_000_000).unwrap();
    writer.add_document(doc!(body => "sharedreadtoken")).unwrap();
    writer.commit().unwrap();
    let reader_dir = TrialDirectory::open(dir.path(), AccessMode::SharedReader, [23; 32]);
    assert!(reader_dir.open_write(Path::new("forbidden")).is_err());
    assert!(reader_dir.atomic_write(Path::new("meta.json"), b"forbidden").is_err());
    assert!(reader_dir.delete(Path::new("meta.json")).is_err());
    let read_index = Index::open(reader_dir).unwrap();
    let query = QueryParser::for_index(&read_index, vec![body])
        .parse_query("sharedreadtoken")
        .unwrap();
    assert_eq!(read_index.reader().unwrap().searcher().search(&query, &Count).unwrap(), 1);
}

#[test]
fn bounded_atomic_manifest_rejects_large_input_without_replacing_existing() {
    let dir = tempfile::tempdir().unwrap();
    let directory = TrialDirectory::open(dir.path(), AccessMode::Writable, [24; 32]);
    let path = Path::new("manifest.json");
    directory.atomic_write(path, b"valid manifest").unwrap();
    assert!(directory.atomic_write(path, &vec![0; MAX_ATOMIC_PLAINTEXT + 1]).is_err());
    assert_eq!(directory.atomic_read(path).unwrap(), b"valid manifest");
    directory.inner.atomic_write(path, &vec![0; MAX_ATOMIC_CIPHERTEXT + 1]).unwrap();
    assert!(directory.atomic_read(path).is_err());
}

#[test]
fn checked_shared_reader_waits_for_writer_metadata_lock() {
    use std::{sync::mpsc, thread, time::Duration};
    use tantivy::directory::META_LOCK;
    let dir = tempfile::tempdir().unwrap();
    // The other participant uses the existing production CheckedDirectory lock.
    let writer = CheckedDirectory::open(dir.path(), AccessMode::Writable).unwrap();
    let reader = TrialDirectory::open(dir.path(), AccessMode::SharedReader, [25; 32]);
    let held = writer.acquire_lock(&META_LOCK).unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        tx.send(reader.acquire_lock(&META_LOCK).is_ok()).unwrap();
    });
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop(held);
    assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    handle.join().unwrap();
}

#[test]
fn root_anchor_blocks_replacement_during_directory_lifetime() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("index");
    std::fs::create_dir(&root).unwrap();
    let directory = TrialDirectory::open_after_pin(&root, AccessMode::Writable, [26; 32], || {
        assert!(std::fs::rename(&root, parent.path().join("moved")).is_err());
    });
    assert!(std::fs::rename(&root, parent.path().join("moved")).is_err());
    drop(directory);
    std::fs::rename(&root, parent.path().join("moved")).unwrap();
}

#[test]
fn pinned_lock_refuses_symlink_and_hardlink() {
    use tantivy::directory::META_LOCK;
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("index");
    std::fs::create_dir(&root).unwrap();
    let outside = parent.path().join("outside");
    std::fs::write(&outside, b"untouched").unwrap();
    let directory = TrialDirectory::open(&root, AccessMode::Writable, [26; 32]);
    let lock_path = root.join(&META_LOCK.filepath);
    std::fs::hard_link(&outside, &lock_path).unwrap();
    assert!(directory.acquire_lock(&META_LOCK).is_err());
    assert_eq!(std::fs::read(&outside).unwrap(), b"untouched");
    std::fs::remove_file(&lock_path).unwrap();
    std::os::windows::fs::symlink_file(&outside, &lock_path).unwrap();
    assert!(directory.acquire_lock(&META_LOCK).is_err());
    assert_eq!(std::fs::read(outside).unwrap(), b"untouched");
}

#[test]
fn checked_shared_reader_waits_for_writer_in_another_process() {
    use std::{
        process::Command,
        thread,
        time::{Duration, Instant},
    };
    use tantivy::directory::META_LOCK;
    const ENV: &str = "WINCOM_SEARCH_METADATA_LOCK_TEST";
    if let Some(path) = std::env::var_os(ENV) {
        let root = std::path::PathBuf::from(path);
        let reader = TrialDirectory::open(&root, AccessMode::SharedReader, [31; 32]);
        std::fs::write(root.join("child-started"), b"ready").unwrap();
        let _lock = reader.acquire_lock(&META_LOCK).unwrap();
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let writer = TrialDirectory::open(dir.path(), AccessMode::Writable, [31; 32]);
    let held = writer.acquire_lock(&META_LOCK).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "encrypted_index_prototype::tantivy_trial::checked_shared_reader_waits_for_writer_in_another_process",
        ])
        .env(ENV, dir.path())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !dir.path().join("child-started").exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    let started = dir.path().join("child-started").exists();
    thread::sleep(Duration::from_millis(150));
    let was_blocked = child.try_wait().unwrap().is_none();
    drop(held);
    let deadline = Instant::now() + Duration::from_secs(15);
    let result = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert!(started && was_blocked && result.is_some_and(|s| s.success()));
}

#[test]
fn atomic_manifest_reads_never_observe_a_partial_replacement() {
    use std::thread;
    let dir = tempfile::tempdir().unwrap();
    let writable = TrialDirectory::open(dir.path(), AccessMode::Writable, [27; 32]);
    let shared = TrialDirectory::open(dir.path(), AccessMode::SharedReader, [27; 32]);
    let path = Path::new("manifest.json");
    writable.atomic_write(path, &vec![1; 64 * 1024]).unwrap();
    let writer = thread::spawn(move || {
        for n in 0..20 {
            writable
                .atomic_write(path, &vec![if n % 2 == 0 { 2 } else { 1 }; 64 * 1024])
                .unwrap();
        }
    });
    for _ in 0..40 {
        let value = shared.atomic_read(path).unwrap();
        assert_eq!(value.len(), 64 * 1024);
        assert!(value.iter().all(|b| *b == 1) || value.iter().all(|b| *b == 2));
    }
    writer.join().unwrap();
}
