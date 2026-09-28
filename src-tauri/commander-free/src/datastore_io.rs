use std::io::{ErrorKind, Write};
use std::path::Path;

use tempfile::NamedTempFile;

fn stage(path: &Path, data: &[u8]) -> Result<NamedTempFile, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Path has no parent directory".to_string())?;
    // Each writer owns its staging file, including cleanup after a failed publish.
    let mut temporary = tempfile::Builder::new()
        .prefix(".datastore-")
        .tempfile_in(parent)
        .map_err(|error| format!("Create temp: {error}"))?;
    temporary
        .write_all(data)
        .map_err(|error| format!("Write temp: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("Fsync temp: {error}"))?;
    Ok(temporary)
}

pub(crate) fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    let mut temporary = stage(path, data)?;
    for attempt in 0..=20 {
        match temporary.persist(path) {
            Ok(_) => return Ok(()),
            Err(error) if retry_transient_publish(&error.error, attempt) => {
                temporary = error.file;
            }
            Err(error) => return Err(format!("Atomic replace: {}", error.error)),
        }
    }
    unreachable!("the final publish attempt returns its result")
}

/// Publishes complete material only if absent; a losing caller must load the winner.
pub(crate) fn publish_new(path: &Path, data: &[u8]) -> Result<bool, String> {
    let mut temporary = stage(path, data)?;
    for attempt in 0..=20 {
        match temporary.persist_noclobber(path) {
            Ok(_) => return Ok(true),
            Err(error) if error.error.kind() == ErrorKind::AlreadyExists => return Ok(false),
            Err(error) if retry_transient_publish(&error.error, attempt) => {
                temporary = error.file;
            }
            Err(error) => return Err(format!("Atomic create: {}", error.error)),
        }
    }
    unreachable!("the final publish attempt returns its result")
}

fn retry_transient_publish(error: &std::io::Error, attempt: usize) -> bool {
    // Concurrent Windows renames can briefly deny replacement; retain our own
    // complete staging file while allowing at most 300 ms for the handle to close.
    if cfg!(windows) && attempt < 20 && matches!(error.raw_os_error(), Some(5 | 32 | 33)) {
        std::thread::sleep(std::time::Duration::from_millis(15));
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{atomic_write, publish_new};
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn payload(index: usize) -> Vec<u8> {
        vec![index as u8; 4_097 + index * 8_191]
    }

    #[test]
    fn concurrent_replacements_publish_one_entire_payload_and_clean_staging_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.dat");
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|index| {
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let data = payload(index);
                    barrier.wait();
                    atomic_write(&path, &data).unwrap();
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let saved = fs::read(&path).unwrap();
        assert!((0..8).any(|index| saved == payload(index)));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_initializers_observe_one_complete_winner_without_replacing_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("material");
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|index| {
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let data = payload(index);
                    barrier.wait();
                    let published = publish_new(&path, &data).unwrap();
                    let observed = fs::read(&path).unwrap();
                    (index, published, observed)
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        let winners: Vec<_> = results
            .iter()
            .filter(|(_, published, _)| *published)
            .collect();
        assert_eq!(winners.len(), 1);
        let expected = payload(winners[0].0);
        for (_, _, observed) in results {
            assert_eq!(observed, expected);
        }
        assert_eq!(fs::read(&path).unwrap(), expected);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_replace_preserves_destination_and_cleans_only_its_staging_file() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("settings.dat");
        fs::create_dir(&destination).unwrap();
        let original = destination.join("original");
        fs::write(&original, b"preserve these bytes").unwrap();
        let unrelated = directory.path().join(".other-writer.tmp");
        fs::write(&unrelated, b"another writer owns this").unwrap();

        assert!(atomic_write(&destination, b"replacement").is_err());
        assert_eq!(fs::read(original).unwrap(), b"preserve these bytes");
        assert_eq!(fs::read(unrelated).unwrap(), b"another writer owns this");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn publishing_existing_material_preserves_its_bytes_and_cleans_staging_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("material");
        fs::write(&path, b"existing material").unwrap();

        assert!(!publish_new(&path, b"different material").unwrap());
        assert_eq!(fs::read(path).unwrap(), b"existing material");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn failed_replace_of_locked_file_preserves_original_bytes() {
        use std::os::windows::fs::OpenOptionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.dat");
        fs::write(&path, b"existing settings").unwrap();
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();

        assert!(atomic_write(&path, b"replacement settings").is_err());
        drop(locked);
        assert_eq!(fs::read(path).unwrap(), b"existing settings");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
