use super::{index::ScanIndex, safety, scope};
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, UNIX_EPOCH};

const ENUMERATION_BATCH_SIZE: usize = 512;
const HASH_WORKER_LIMIT: usize = 4;
const CANDIDATE_BATCH_SIZE: i64 = 128;
const HASH_PROGRESS_BATCH_BYTES: u64 = 4 * 1024 * 1024;
const SAMPLE_WINDOW_BYTES: u64 = 64 * 1024;
const HASH_BUFFER_BYTES: usize = 1024 * 1024;

type CandidateRow = (i64, String, u64, Option<u32>, Option<String>);

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
    while !cancelled.load(Ordering::Acquire) {
        let pending: Option<(String, String)> = index
            .connection
            .query_row(
                "SELECT path,root FROM directories WHERE done=0 LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((directory, root)) = pending else {
            break;
        };
        index
            .connection
            .execute("UPDATE directories SET done=1 WHERE path=?1", [&directory])
            .map_err(|error| error.to_string())?;
        let directory = PathBuf::from(directory);
        scope::update(|state| {
            state.current_path = scope::display_path(&directory).unwrap_or_default()
        });
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
        let mut directories = Vec::new();
        let mut files = Vec::new();
        let mut discovered = 0usize;
        let enumerated = enumerate_directory(&directory, cancelled, |entry| {
            if cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            match entry {
                Err((path, error)) => scope::error(&path, error),
                Ok(entry) => {
                    if matches!(entry.kind, InventoryKind::Link) {
                        scope::update(|state| state.skipped_links += 1);
                        return Ok(());
                    }
                    let Some(display_path) = entry.path.to_str() else {
                        scope::error(&directory, "An entry name cannot be decoded safely");
                        return Ok(());
                    };
                    if matches!(entry.kind, InventoryKind::Directory) {
                        directories.push(display_path.to_owned());
                    } else if matches!(entry.kind, InventoryKind::File) {
                        if entry.size < minimum_size_bytes {
                            scope::update(|state| state.skipped_small_files += 1);
                            return Ok(());
                        }
                        if entry.size > i64::MAX as u64 {
                            scope::error(&entry.path, "File size exceeds supported range");
                            return Ok(());
                        }
                        files.push((
                            display_path.to_owned(),
                            entry.size as i64,
                            // Collecting a stable Windows identity opens a handle. Most files have no
                            // same-size peer and cannot be duplicates, so defer that work until the
                            // candidate stage. The prefix cannot collide with a real volume:file key.
                            format!("path:{display_path}"),
                            None,
                            None,
                        ));
                    }
                }
            }
            if directories.len() + files.len() >= ENUMERATION_BATCH_SIZE {
                discovered += flush_enumeration_batch(index, &root, &mut directories, &mut files)?;
                scope::update(|state| {
                    state.current_path = scope::display_path(&directory).unwrap_or_default();
                    state.scanned_files += discovered;
                });
                discovered = 0;
            }
            Ok(())
        });
        if let Err(error) = enumerated {
            scope::error(&directory, error);
        }
        discovered += flush_enumeration_batch(index, &root, &mut directories, &mut files)?;
        if discovered > 0 {
            scope::update(|state| state.scanned_files += discovered);
        }
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

fn flush_enumeration_batch(
    index: &ScanIndex,
    root: &str,
    directories: &mut Vec<String>,
    files: &mut Vec<(String, i64, String, Option<u32>, Option<String>)>,
) -> Result<usize, String> {
    if directories.is_empty() && files.is_empty() {
        return Ok(0);
    }
    let transaction = index
        .connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    {
        let mut insert_directory = transaction
            .prepare("INSERT OR IGNORE INTO directories(path,root) VALUES(?1,?2)")
            .map_err(|error| error.to_string())?;
        for path in directories.drain(..) {
            insert_directory
                .execute(params![path, root])
                .map_err(|error| error.to_string())?;
        }
    }
    let mut inserted = 0usize;
    {
        let mut insert_file = transaction
            .prepare("INSERT OR IGNORE INTO files(id,path,root,size,identity,volume,file_index) VALUES(lower(hex(randomblob(16))),?1,?2,?3,?4,?5,?6)")
            .map_err(|error| error.to_string())?;
        for (path, size, identity, volume, file_index) in files.drain(..) {
            inserted += insert_file
                .execute(params![path, root, size, identity, volume, file_index])
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
            WHERE f.sample_hash IN (
                SELECT sampled.sample_hash FROM files sampled
                JOIN candidates candidate ON sampled.rowid=candidate.file_row
                WHERE length(sampled.sample_hash)=32
                GROUP BY sampled.sample_hash HAVING count(*)>1
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
    let mut cursor = 0i64;
    while !cancelled.load(Ordering::Acquire) {
        let mut statement = index.connection.prepare("SELECT f.rowid,f.path,f.size,f.volume,f.file_index FROM files f JOIN candidates c ON f.rowid=c.file_row WHERE f.rowid>?1 ORDER BY f.rowid LIMIT ?2").map_err(|error| error.to_string())?;
        let batch = statement
            .query_map(params![cursor, CANDIDATE_BATCH_SIZE], |row| {
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
        if batch.is_empty() {
            break;
        }
        if let Some((_, path, _, _, _)) = batch.first() {
            let path = PathBuf::from(path);
            scope::update(|state| {
                state.current_path = scope::display_path(&path).unwrap_or_default()
            });
        }
        for (row, path, result) in hash_candidate_batch(&batch, cancelled)? {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            cursor = row;
            let path = PathBuf::from(path);
            scope::update(|state| {
                state.current_path = scope::display_path(&path).unwrap_or_default()
            });
            match result {
                Ok((hash, modified)) => {
                    index
                        .connection
                        .execute(
                            "UPDATE files SET hash=?1,modified=?2 WHERE rowid=?3",
                            params![hash.as_slice(), modified, row],
                        )
                        .map_err(|error| error.to_string())?;
                }
                Err(error) if !cancelled.load(Ordering::Acquire) => scope::error(&path, error),
                Err(_) => {}
            }
            scope::update(|state| state.hashed_files += 1);
        }
    }
    Ok(())
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
                "SELECT f.rowid,f.path FROM files f JOIN candidates c ON f.rowid=c.file_row WHERE f.rowid>?1 ORDER BY f.rowid LIMIT 128",
            )
            .map_err(|error| error.to_string())?;
        let batch = statement
            .query_map([cursor], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        if batch.is_empty() {
            break;
        }
        for (row, path) in batch {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            cursor = row;
            let path = PathBuf::from(path);
            scope::update(|state| {
                state.current_path = scope::display_path(&path).unwrap_or_default()
            });
            let identity = crate::routine_cleaner::file_identity(&path);
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

fn sample_candidates(index: &ScanIndex, cancelled: &AtomicBool) -> Result<(), String> {
    let mut cursor = 0i64;
    while !cancelled.load(Ordering::Acquire) {
        let mut statement = index.connection.prepare("SELECT f.rowid,f.path,f.size,f.volume,f.file_index FROM files f JOIN candidates c ON f.rowid=c.file_row WHERE f.rowid>?1 ORDER BY f.rowid LIMIT ?2").map_err(|error| error.to_string())?;
        let batch = statement
            .query_map(params![cursor, CANDIDATE_BATCH_SIZE], |row| {
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
        if batch.is_empty() {
            break;
        }
        if let Some((_, path, _, _, _)) = batch.first() {
            let path = PathBuf::from(path);
            scope::update(|state| {
                state.current_path = scope::display_path(&path).unwrap_or_default()
            });
        }
        for (row, path, result) in sample_candidate_batch(&batch, cancelled)? {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            cursor = row;
            let path = PathBuf::from(path);
            scope::update(|state| {
                state.current_path = scope::display_path(&path).unwrap_or_default()
            });
            match result {
                Ok(sample_hash) => {
                    index
                        .connection
                        .execute(
                            "UPDATE files SET sample_hash=?1 WHERE rowid=?2",
                            params![sample_hash.as_slice(), row],
                        )
                        .map_err(|error| error.to_string())?;
                }
                Err(error) if !cancelled.load(Ordering::Acquire) => scope::error(&path, error),
                Err(_) => {}
            }
        }
    }
    Ok(())
}

/// Reads at most four unrelated candidate files at once. The caller applies every database
/// result after the workers finish, so SQLite remains single-writer and result ordering stays
/// deterministic. Each worker still verifies the opened handle before reading content.
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
    let chunk_size = batch.len().div_ceil(worker_count);
    std::thread::scope(|scope| {
        let handles = batch
            .chunks(chunk_size)
            .map(|chunk| {
                let process = &process;
                scope.spawn(move || {
                    let mut buffer = vec![0; HASH_BUFFER_BYTES];
                    chunk
                        .iter()
                        .map(|(row, path, size, volume, file_index)| {
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
                            (*row, path.clone(), result)
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
                    .map_err(|_| "Duplicate hashing worker failed".to_string())?,
            );
        }
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
    let mut hasher = Sha256::new();
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
    Ok(hasher.finalize().into())
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
    let mut hasher = Sha256::new();
    read_hash_segment(reader, &mut hasher, u64::MAX, cancelled, buffer)?;
    Ok(hasher.finalize().into())
}

fn read_hash_segment(
    reader: &mut impl Read,
    hasher: &mut Sha256,
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
