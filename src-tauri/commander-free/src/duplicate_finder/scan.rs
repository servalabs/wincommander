use super::{index::ScanIndex, safety, scope};
use rusqlite::params;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Instant, UNIX_EPOCH};

// A larger write batch turns a full-drive inventory into a few thousand SQLite
// transactions instead of one transaction for every small folder.
const ENUMERATION_BATCH_SIZE: usize = 4096;
// The inventory pool persists for the whole scan. It keeps native directory
// reads concurrent without creating a new operating-system thread per folder.
const INVENTORY_WORKER_LIMIT: usize = 8;
const HASH_WORKER_LIMIT: usize = 4;
const HASH_PROGRESS_BATCH_BYTES: u64 = 4 * 1024 * 1024;
// Samples only reject non-matches; full BLAKE3 still confirms every group.
// Small start/middle/end windows avoid reading several gigabytes before full
// hashing has narrowed the field.
const SAMPLE_WINDOW_BYTES: u64 = 16 * 1024;
const HASH_BUFFER_BYTES: usize = 1024 * 1024;

type CandidateRow = (i64, String, u64, Option<u32>, Option<String>);
type DirectoryListing = Result<Vec<Result<InventoryEntry, (PathBuf, String)>>, String>;

enum InventoryKind {
    Directory,
    File,
    Link,
    #[cfg(not(windows))]
    Other,
}

struct InventoryEntry {
    path: PathBuf,
    kind: InventoryKind,
    size: u64,
}

pub(super) fn scan_roots(
    roots: &[PathBuf],
    minimum_size_bytes: u64,
    cancelled: &AtomicBool,
) -> Result<ScanIndex, String> {
    let mut index = ScanIndex::new()?;
    for root in roots {
        let root = root
            .to_str()
            .ok_or("A selected folder name cannot be decoded safely")?;
        index
            .connection
            .execute(
                "INSERT OR IGNORE INTO directories(path,root) VALUES(?1,?1)",
                [root],
            )
            .map_err(|error| error.to_string())?;
    }
    enumerate(&index, minimum_size_bytes, cancelled)?;
    if !cancelled.load(Ordering::Acquire) {
        hash_candidates(&index, cancelled)?;
    }
    index.connection.execute_batch("INSERT INTO groups(id,size,hash,count)
        SELECT lower(hex(randomblob(16))),size,hash,count(*) FROM files WHERE length(hash)=32 GROUP BY size,hash HAVING count(*)>1;
        CREATE INDEX group_hashes ON groups(size,hash);
        UPDATE files SET group_id=(SELECT id FROM groups WHERE groups.size=files.size AND groups.hash=files.hash) WHERE length(hash)=32;
        CREATE INDEX group_members ON files(group_id,id);")
        .map_err(|error| error.to_string())?;
    scope::finish(cancelled.load(Ordering::Acquire));
    index.status = scope::snapshot();
    index.created_at = Instant::now();
    Ok(index)
}

pub(super) fn enumerate(
    index: &ScanIndex,
    minimum_size_bytes: u64,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let protected = safety::protected_system_roots();
    let temporary = fs::canonicalize(index.directory.path()).map_err(|error| error.to_string())?;
    // The database is for file candidates and result paging. Keeping the
    // traversal queue there made a large volume perform a read/write round trip
    // for every folder. A single scan already owns this process, so retain the
    // queue in memory and leave only the original roots in SQLite for auditability.
    let roots = index
        .connection
        .prepare("SELECT path,root FROM directories ORDER BY rowid")
        .map_err(|error| error.to_string())?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut pending = VecDeque::new();
    let mut queued = HashSet::new();
    for (path, root) in roots {
        if queued.insert(path.clone()) {
            pending.push_back((PathBuf::from(path), root));
        }
    }
    let mut files = Vec::new();
    let mut discovered = 0usize;
    let mut skipped_small = 0usize;
    std::thread::scope(|worker_scope| -> Result<(), String> {
        let (work_sender, work_receiver) =
            mpsc::sync_channel::<(PathBuf, String)>(INVENTORY_WORKER_LIMIT);
        let (result_sender, result_receiver) =
            mpsc::channel::<(PathBuf, String, DirectoryListing)>();
        let work_receiver = Arc::new(Mutex::new(work_receiver));
        for _ in 0..INVENTORY_WORKER_LIMIT {
            let result_sender = result_sender.clone();
            let work_receiver = Arc::clone(&work_receiver);
            worker_scope.spawn(move || loop {
                if cancelled.load(Ordering::Acquire) {
                    break;
                }
                let task = match work_receiver.lock() {
                    Ok(receiver) => receiver.recv(),
                    Err(_) => break,
                };
                let Ok((directory, root)) = task else {
                    break;
                };
                let mut entries = Vec::new();
                let result = enumerate_directory(&directory, cancelled, |entry| {
                    entries.push(entry);
                    Ok(())
                })
                .map(|()| entries);
                if result_sender.send((directory, root, result)).is_err() {
                    break;
                }
            });
        }
        drop(result_sender);

        let mut active = 0usize;
        loop {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            while active < INVENTORY_WORKER_LIMIT {
                let Some((directory, root)) = pending.pop_front() else {
                    break;
                };
                if protected.iter().any(|path| directory.starts_with(path))
                    || directory.starts_with(&temporary)
                {
                    scope::error(&directory, "Protected application/system scope excluded");
                    continue;
                }
                let metadata = match fs::symlink_metadata(&directory) {
                    Ok(meta) => meta,
                    Err(error) => {
                        scope::error(&directory, error);
                        continue;
                    }
                };
                if safety::is_link_or_reparse(&metadata) {
                    scope::update(|state| state.skipped_links += 1);
                    continue;
                }
                work_sender
                    .send((directory, root))
                    .map_err(|_| "Duplicate inventory workers stopped unexpectedly")?;
                active += 1;
            }
            if active == 0 {
                break;
            }
            let (directory, root, enumerated) = result_receiver
                .recv()
                .map_err(|_| "Duplicate inventory workers stopped unexpectedly")?;
            active -= 1;
            scope::update(|state| {
                state.current_path = scope::display_path(&directory).unwrap_or_default()
            });
            let entries = match enumerated {
                Ok(entries) => entries,
                Err(error) => {
                    scope::error(&directory, error);
                    continue;
                }
            };
            for entry in entries {
                if cancelled.load(Ordering::Acquire) {
                    break;
                }
                match entry {
                    Err((path, error)) => scope::error(&path, error),
                    Ok(entry) => {
                        if matches!(entry.kind, InventoryKind::Link) {
                            scope::update(|state| state.skipped_links += 1);
                            continue;
                        }
                        let Some(display_path) = entry.path.to_str() else {
                            scope::error(&directory, "An entry name cannot be decoded safely");
                            continue;
                        };
                        if matches!(entry.kind, InventoryKind::Directory) {
                            if queued.insert(display_path.to_owned()) {
                                pending.push_back((entry.path, root.clone()));
                            }
                        } else if matches!(entry.kind, InventoryKind::File) {
                            if entry.size < minimum_size_bytes {
                                skipped_small += 1;
                                continue;
                            }
                            if entry.size > i64::MAX as u64 {
                                scope::error(&entry.path, "File size exceeds supported range");
                                continue;
                            }
                            files.push((display_path.to_owned(), root.clone(), entry.size as i64));
                        }
                    }
                }
                if files.len() >= ENUMERATION_BATCH_SIZE {
                    discovered += flush_file_batch(index, &mut files)?;
                    scope::update(|state| state.scanned_files += discovered);
                    discovered = 0;
                }
            }
            if skipped_small > 0 {
                scope::update(|state| state.skipped_small_files += skipped_small);
                skipped_small = 0;
            }
        }
        drop(work_sender);
        Ok(())
    })?;
    discovered += flush_file_batch(index, &mut files)?;
    if discovered > 0 {
        scope::update(|state| state.scanned_files += discovered);
    }
    Ok(())
}

#[cfg(not(windows))]
fn enumerate_directory(
    directory: &Path,
    cancelled: &AtomicBool,
    mut visit: impl FnMut(Result<InventoryEntry, (PathBuf, String)>) -> Result<(), String>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory).map_err(|error| error.to_string())?;
    for entry in entries {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                visit(Err((directory.to_path_buf(), error.to_string())))?;
                continue;
            }
        };
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                visit(Err((path, error.to_string())))?;
                continue;
            }
        };
        let kind = if safety::is_link_or_reparse(&metadata) {
            InventoryKind::Link
        } else if metadata.is_dir() {
            InventoryKind::Directory
        } else if metadata.is_file() {
            InventoryKind::File
        } else {
            InventoryKind::Other
        };
        visit(Ok(InventoryEntry {
            path,
            kind,
            size: metadata.len(),
        }))?;
    }
    Ok(())
}

#[cfg(windows)]
fn enumerate_directory(
    directory: &Path,
    cancelled: &AtomicBool,
    mut visit: impl FnMut(Result<InventoryEntry, (PathBuf, String)>) -> Result<(), String>,
) -> Result<(), String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::{
        Foundation::{
            GetLastError, ERROR_FILE_NOT_FOUND, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER,
            ERROR_NOT_SUPPORTED, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
        },
        Storage::FileSystem::{
            FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH,
            WIN32_FIND_DATAW,
        },
    };

    struct FindHandle(HANDLE);
    impl Drop for FindHandle {
        fn drop(&mut self) {
            // SAFETY: constructed only after FindFirstFileExW succeeds.
            unsafe { FindClose(self.0) };
        }
    }

    fn find_first(pattern: &[u16], data: &mut WIN32_FIND_DATAW, large_fetch: bool) -> HANDLE {
        // SAFETY: both buffers remain valid for the Windows call.
        unsafe {
            FindFirstFileExW(
                pattern.as_ptr(),
                FindExInfoBasic,
                data as *mut WIN32_FIND_DATAW as *mut c_void,
                FindExSearchNameMatch,
                std::ptr::null(),
                if large_fetch {
                    FIND_FIRST_EX_LARGE_FETCH
                } else {
                    0
                },
            )
        }
    }

    let mut pattern = directory.to_path_buf();
    pattern.push("*");
    let pattern: Vec<u16> = pattern.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut data = WIN32_FIND_DATAW::default();
    let mut handle = find_first(&pattern, &mut data, true);
    if handle == INVALID_HANDLE_VALUE {
        let error = unsafe { GetLastError() };
        if matches!(
            error,
            ERROR_INVALID_FUNCTION | ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED
        ) {
            handle = find_first(&pattern, &mut data, false);
        }
    }
    if handle == INVALID_HANDLE_VALUE {
        let error = unsafe { GetLastError() };
        return if error == ERROR_FILE_NOT_FOUND {
            match fs::symlink_metadata(directory) {
                Ok(metadata) if metadata.is_dir() && !safety::is_link_or_reparse(&metadata) => {
                    Ok(())
                }
                Ok(_) => Err("Directory changed before enumeration".into()),
                Err(metadata_error) => Err(metadata_error.to_string()),
            }
        } else {
            Err(std::io::Error::from_raw_os_error(error as i32).to_string())
        };
    }
    let _handle = FindHandle(handle);
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let name_length = data
            .cFileName
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(data.cFileName.len());
        let name = std::ffi::OsString::from_wide(&data.cFileName[..name_length]);
        if name != "." && name != ".." {
            let attributes = data.dwFileAttributes;
            let kind = if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                InventoryKind::Link
            } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                InventoryKind::Directory
            } else {
                InventoryKind::File
            };
            visit(Ok(InventoryEntry {
                path: directory.join(name),
                kind,
                size: (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow),
            }))?;
        }
        if unsafe { FindNextFileW(handle, &mut data) } == 0 {
            let error = unsafe { GetLastError() };
            return if error == ERROR_NO_MORE_FILES {
                Ok(())
            } else {
                Err(std::io::Error::from_raw_os_error(error as i32).to_string())
            };
        }
    }
}

fn flush_file_batch(
    index: &ScanIndex,
    files: &mut Vec<(String, String, i64)>,
) -> Result<usize, String> {
    if files.is_empty() {
        return Ok(0);
    }
    let transaction = index
        .connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut inserted = 0usize;
    {
        let mut insert_file = transaction
            .prepare("INSERT OR IGNORE INTO files(id,path,root,size) VALUES(lower(hex(randomblob(16))),?1,?2,?3)")
            .map_err(|error| error.to_string())?;
        for (path, root, size) in files.drain(..) {
            inserted += insert_file
                .execute(params![path, root, size])
                .map_err(|error| error.to_string())?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(inserted)
}

pub(super) fn hash_candidates(index: &ScanIndex, cancelled: &AtomicBool) -> Result<(), String> {
    index.connection.execute_batch("CREATE TABLE candidates AS SELECT rowid AS file_row FROM files WHERE size IN (SELECT size FROM files GROUP BY size HAVING count(*)>1); CREATE UNIQUE INDEX candidate_rows ON candidates(file_row);")
        .map_err(|error| error.to_string())?;
    resolve_candidate_identities(index, cancelled)?;
    let total: usize = index
        .connection
        .query_row("SELECT count(*) FROM candidates", [], |row| {
            super::index::count(row, 0)
        })
        .map_err(|error| error.to_string())?;
    scope::update(|state| {
        state.phase = "hashing".into();
        state.candidate_files = total;
    });
    sample_candidates(index, cancelled)?;
    index
        .connection
        .execute_batch(
            "DELETE FROM candidates WHERE file_row NOT IN (
            SELECT f.rowid FROM files f
            JOIN candidates c ON f.rowid=c.file_row
            WHERE (f.size, f.sample_hash) IN (
                SELECT sampled.size, sampled.sample_hash FROM files sampled
                JOIN candidates candidate ON sampled.rowid=candidate.file_row
                WHERE length(sampled.sample_hash)=32
                GROUP BY sampled.size, sampled.sample_hash HAVING count(*)>1
            )
        );",
        )
        .map_err(|error| error.to_string())?;
    let verified_total: usize = index
        .connection
        .query_row("SELECT count(*) FROM candidates", [], |row| {
            super::index::count(row, 0)
        })
        .map_err(|error| error.to_string())?;
    scope::update(|state| state.candidate_files = verified_total);
    let candidates = candidate_rows(index)?;
    let results = hash_candidate_batch(&candidates, cancelled)?;
    let transaction = index
        .connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut update = transaction
        .prepare("UPDATE files SET hash=?1,modified=?2 WHERE rowid=?3")
        .map_err(|error| error.to_string())?;
    for (row, path, result) in results {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let path = PathBuf::from(path);
        scope::update(|state| state.current_path = scope::display_path(&path).unwrap_or_default());
        match result {
            Ok((hash, modified)) => {
                update
                    .execute(params![hash.as_slice(), modified, row])
                    .map_err(|error| error.to_string())?;
            }
            Err(error) => scope::error(&path, error),
        }
        scope::update(|state| state.hashed_files += 1);
    }
    drop(update);
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(())
}

fn candidate_rows(index: &ScanIndex) -> Result<Vec<CandidateRow>, String> {
    let mut statement = index
        .connection
        .prepare("SELECT f.rowid,f.path,f.size,f.volume,f.file_index FROM files f JOIN candidates c ON f.rowid=c.file_row ORDER BY f.rowid")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                super::index::unsigned(row, 2)?,
                row.get::<_, Option<u32>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

/// Resolves file identities only after the cheap size filter. This keeps hard-link aliases from
/// being reported as reclaimable copies without opening every unrelated file in a full-drive scan.
fn resolve_candidate_identities(index: &ScanIndex, cancelled: &AtomicBool) -> Result<(), String> {
    let mut cursor = 0i64;
    let mut seen = HashSet::new();
    while !cancelled.load(Ordering::Acquire) {
        let mut statement = index
            .connection
            .prepare(
                "SELECT f.rowid,f.path FROM files f JOIN candidates c ON f.rowid=c.file_row WHERE f.rowid>?1 ORDER BY f.rowid LIMIT ?2",
            )
            .map_err(|error| error.to_string())?;
        let batch = statement
            .query_map(params![cursor, 512], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        if batch.is_empty() {
            break;
        }
        for (row, path, identity) in resolve_candidate_identity_batch(&batch, cancelled)? {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            cursor = row;
            let path = PathBuf::from(path);
            scope::update(|state| {
                state.current_path = scope::display_path(&path).unwrap_or_default()
            });
            if cfg!(windows) && identity.is_none() {
                index
                    .connection
                    .execute("DELETE FROM candidates WHERE file_row=?1", [row])
                    .map_err(|error| error.to_string())?;
                scope::error(&path, "Stable file identity is unavailable");
                continue;
            }
            let identity_key = identity
                .map(|(volume, file)| format!("{volume}:{file}"))
                .unwrap_or_else(|| format!("path:{}", path.to_string_lossy()));
            if !seen.insert(identity_key.clone()) {
                index
                    .connection
                    .execute("DELETE FROM candidates WHERE file_row=?1", [row])
                    .map_err(|error| error.to_string())?;
                scope::update(|state| state.scanned_files = state.scanned_files.saturating_sub(1));
                continue;
            }
            if let Err(error) = index.connection.execute(
                "UPDATE files SET identity=?1,volume=?2,file_index=?3 WHERE rowid=?4",
                params![
                    identity_key,
                    identity.map(|value| value.0),
                    identity.map(|value| value.1.to_string()),
                    row
                ],
            ) {
                index
                    .connection
                    .execute("DELETE FROM candidates WHERE file_row=?1", [row])
                    .map_err(|delete_error| delete_error.to_string())?;
                scope::error(&path, format!("Stable file identity changed: {error}"));
            }
        }
    }
    Ok(())
}

/// Stable identities are required only for same-size candidates, but opening
/// those files one at a time dominated full-drive scans. Read identities in the
/// same bounded worker shape used for sample and full-content verification;
/// the SQLite candidate updates stay on this caller thread.
fn resolve_candidate_identity_batch(
    batch: &[(i64, String)],
    cancelled: &AtomicBool,
) -> Result<Vec<(i64, String, Option<(u32, u64)>)>, String> {
    if batch.is_empty() {
        return Ok(Vec::new());
    }
    let worker_count = batch.len().min(HASH_WORKER_LIMIT);
    let chunk_size = batch.len().div_ceil(worker_count);
    std::thread::scope(|scope| {
        let handles = batch
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(row, path)| {
                            let identity = (!cancelled.load(Ordering::Acquire))
                                .then(|| crate::routine_cleaner::file_identity(Path::new(path)))
                                .flatten();
                            (*row, path.clone(), identity)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let mut results = Vec::with_capacity(batch.len());
        for handle in handles {
            results.extend(
                handle
                    .join()
                    .map_err(|_| "Duplicate identity worker failed".to_string())?,
            );
        }
        Ok(results)
    })
}

fn sample_candidates(index: &ScanIndex, cancelled: &AtomicBool) -> Result<(), String> {
    let candidates = candidate_rows(index)?;
    let results = sample_candidate_batch(&candidates, cancelled)?;
    let transaction = index
        .connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut update = transaction
        .prepare("UPDATE files SET sample_hash=?1 WHERE rowid=?2")
        .map_err(|error| error.to_string())?;
    for (row, path, result) in results {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let path = PathBuf::from(path);
        scope::update(|state| state.current_path = scope::display_path(&path).unwrap_or_default());
        match result {
            Ok(sample_hash) => {
                update
                    .execute(params![sample_hash.as_slice(), row])
                    .map_err(|error| error.to_string())?;
            }
            Err(error) => scope::error(&path, error),
        }
    }
    drop(update);
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(())
}

/// Runs a fixed set of content workers for the whole candidate set. The caller
/// remains the only SQLite writer, while workers take the next file dynamically
/// so a few large files cannot leave the other workers idle.
fn process_candidate_batch<T: Send, F>(
    batch: &[CandidateRow],
    cancelled: &AtomicBool,
    process: F,
) -> Result<Vec<(i64, String, Result<T, String>)>, String>
where
    F: Fn(&Path, u64, Option<(u32, u64)>, &AtomicBool, &mut [u8]) -> Result<T, String> + Sync,
{
    if batch.is_empty() {
        return Ok(Vec::new());
    }
    let worker_count = batch.len().min(HASH_WORKER_LIMIT);
    std::thread::scope(|scope| {
        let next = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel(worker_count.saturating_mul(2));
        let handles = (0..worker_count)
            .map(|_| {
                let sender = sender.clone();
                let process = &process;
                let next = Arc::clone(&next);
                scope.spawn(move || {
                    let mut buffer = vec![0; HASH_BUFFER_BYTES];
                    loop {
                        let position = next.fetch_add(1, Ordering::AcqRel);
                        let Some((row, path, size, volume, file_index)) = batch.get(position)
                        else {
                            break;
                        };
                        let identity = volume.zip(
                            file_index
                                .as_ref()
                                .and_then(|value| value.parse::<u64>().ok()),
                        );
                        let result = if cancelled.load(Ordering::Acquire) {
                            Err("cancelled".into())
                        } else {
                            process(Path::new(path), *size, identity, cancelled, &mut buffer)
                        };
                        if sender.send((*row, path.clone(), result)).is_err() {
                            break;
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        drop(sender);
        let mut results = Vec::with_capacity(batch.len());
        while let Ok(result) = receiver.recv() {
            results.push(result);
        }
        for handle in handles {
            handle
                .join()
                .map_err(|_| "Duplicate hashing worker failed".to_string())?;
        }
        results.sort_by_key(|(row, _, _)| *row);
        Ok(results)
    })
}

fn sample_candidate_batch(
    batch: &[CandidateRow],
    cancelled: &AtomicBool,
) -> Result<Vec<(i64, String, Result<[u8; 32], String>)>, String> {
    process_candidate_batch(batch, cancelled, sample_candidate_with_buffer)
}

fn hash_candidate_batch(
    batch: &[CandidateRow],
    cancelled: &AtomicBool,
) -> Result<Vec<(i64, String, Result<([u8; 32], Option<String>), String>)>, String> {
    process_candidate_batch(batch, cancelled, hash_candidate_with_buffer)
}

fn sample_candidate_with_buffer(
    path: &Path,
    size: u64,
    identity: Option<(u32, u64)>,
    cancelled: &AtomicBool,
    buffer: &mut [u8],
) -> Result<[u8; 32], String> {
    let before = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !before.is_file() || safety::is_link_or_reparse(&before) || before.len() != size {
        return Err("File changed before content comparison".into());
    }
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let opened = file.metadata().map_err(|error| error.to_string())?;
    if identity != super::verified_file::handle_identity(&file)?
        || opened.len() != size
        || opened.modified().ok() != before.modified().ok()
    {
        return Err("File handle identity changed before content comparison".into());
    }
    let mut hasher = blake3::Hasher::new();
    if size <= SAMPLE_WINDOW_BYTES.saturating_mul(3) {
        if read_hash_segment(&mut file, &mut hasher, size, cancelled, buffer)? != size {
            return Err("File changed during content comparison".into());
        }
    } else if read_hash_segment(
        &mut file,
        &mut hasher,
        SAMPLE_WINDOW_BYTES,
        cancelled,
        buffer,
    )? != SAMPLE_WINDOW_BYTES
    {
        return Err("File changed during content comparison".into());
    }
    if size > SAMPLE_WINDOW_BYTES.saturating_mul(3) {
        file.seek(SeekFrom::Start(size / 2 - SAMPLE_WINDOW_BYTES / 2))
            .map_err(|error| error.to_string())?;
        if read_hash_segment(
            &mut file,
            &mut hasher,
            SAMPLE_WINDOW_BYTES,
            cancelled,
            buffer,
        )? != SAMPLE_WINDOW_BYTES
        {
            return Err("File changed during content comparison".into());
        }
        file.seek(SeekFrom::Start(size - SAMPLE_WINDOW_BYTES))
            .map_err(|error| error.to_string())?;
        if read_hash_segment(
            &mut file,
            &mut hasher,
            SAMPLE_WINDOW_BYTES,
            cancelled,
            buffer,
        )? != SAMPLE_WINDOW_BYTES
        {
            return Err("File changed during content comparison".into());
        }
    }
    let finished = file.metadata().map_err(|error| error.to_string())?;
    let after = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !after.is_file()
        || safety::is_link_or_reparse(&after)
        || after.len() != size
        || before.modified().ok() != after.modified().ok()
        || opened.modified().ok() != finished.modified().ok()
        || finished.len() != size
        || identity != crate::routine_cleaner::file_identity(path)
    {
        return Err("File changed during content comparison".into());
    }
    Ok(*hasher.finalize().as_bytes())
}

fn hash_candidate_with_buffer(
    path: &Path,
    size: u64,
    identity: Option<(u32, u64)>,
    cancelled: &AtomicBool,
    buffer: &mut [u8],
) -> Result<([u8; 32], Option<String>), String> {
    let before = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !before.is_file() || safety::is_link_or_reparse(&before) || before.len() != size {
        return Err("File changed before content comparison".into());
    }
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let opened = file.metadata().map_err(|error| error.to_string())?;
    if identity != super::verified_file::handle_identity(&file)?
        || opened.len() != size
        || opened.modified().ok() != before.modified().ok()
    {
        return Err("File handle identity changed before content comparison".into());
    }
    let hash = hash_reader_with_buffer(&mut file, cancelled, buffer)?;
    let finished = file.metadata().map_err(|error| error.to_string())?;
    let after = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !after.is_file()
        || safety::is_link_or_reparse(&after)
        || after.len() != size
        || before.modified().ok() != after.modified().ok()
        || opened.modified().ok() != finished.modified().ok()
        || finished.len() != size
        || identity != crate::routine_cleaner::file_identity(path)
    {
        return Err("File changed during content comparison".into());
    }
    let modified = after
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().to_string());
    Ok((hash, modified))
}

pub(super) fn hash_reader(
    reader: &mut impl Read,
    cancelled: &AtomicBool,
) -> Result<[u8; 32], String> {
    let mut buffer = vec![0; HASH_BUFFER_BYTES];
    hash_reader_with_buffer(reader, cancelled, &mut buffer)
}

fn hash_reader_with_buffer(
    reader: &mut impl Read,
    cancelled: &AtomicBool,
    buffer: &mut [u8],
) -> Result<[u8; 32], String> {
    let mut hasher = blake3::Hasher::new();
    read_hash_segment(reader, &mut hasher, u64::MAX, cancelled, buffer)?;
    Ok(*hasher.finalize().as_bytes())
}

fn read_hash_segment(
    reader: &mut impl Read,
    hasher: &mut blake3::Hasher,
    mut remaining: u64,
    cancelled: &AtomicBool,
    buffer: &mut [u8],
) -> Result<u64, String> {
    let mut pending_progress = 0u64;
    let mut read_total = 0u64;
    while remaining > 0 {
        if cancelled.load(Ordering::Acquire) {
            return Err("cancelled".into());
        }
        let capacity = remaining.min(buffer.len() as u64) as usize;
        let read = reader
            .read(&mut buffer[..capacity])
            .map_err(|error| error.to_string())?;
        if read == 0 {
            if pending_progress > 0 && super::RUNNING.load(Ordering::Acquire) {
                scope::update(|state| {
                    state.bytes_hashed = state.bytes_hashed.saturating_add(pending_progress)
                });
            }
            return Ok(read_total);
        }
        hasher.update(&buffer[..read]);
        read_total = read_total.saturating_add(read as u64);
        remaining = remaining.saturating_sub(read as u64);
        if super::RUNNING.load(Ordering::Acquire) {
            pending_progress = pending_progress.saturating_add(read as u64);
            if pending_progress >= HASH_PROGRESS_BATCH_BYTES {
                scope::update(|state| {
                    state.bytes_hashed = state.bytes_hashed.saturating_add(pending_progress)
                });
                pending_progress = 0;
            }
        }
    }
    if pending_progress > 0 && super::RUNNING.load(Ordering::Acquire) {
        scope::update(|state| {
            state.bytes_hashed = state.bytes_hashed.saturating_add(pending_progress)
        });
    }
    Ok(read_total)
}
