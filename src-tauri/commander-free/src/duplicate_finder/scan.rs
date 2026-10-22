use super::{index::ScanIndex, safety, scope};
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, UNIX_EPOCH};

const ENUMERATION_BATCH_SIZE: usize = 512;
const HASH_PROGRESS_BATCH_BYTES: u64 = 4 * 1024 * 1024;

pub(super) fn scan_roots(roots: &[PathBuf], cancelled: &AtomicBool) -> Result<ScanIndex, String> {
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
    enumerate(&index, cancelled)?;
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

pub(super) fn enumerate(index: &ScanIndex, cancelled: &AtomicBool) -> Result<(), String> {
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
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                scope::error(&directory, error);
                continue;
            }
        };
        let mut directories = Vec::new();
        let mut files = Vec::new();
        let mut discovered = 0usize;
        for entry in entries {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    scope::error(&directory, error);
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(error) => {
                    scope::error(&path, error);
                    continue;
                }
            };
            if safety::is_link_or_reparse(&metadata) {
                scope::update(|state| state.skipped_links += 1);
                continue;
            }
            let Some(display_path) = path.to_str() else {
                scope::error(&directory, "An entry name cannot be decoded safely");
                continue;
            };
            if metadata.is_dir() {
                directories.push(display_path.to_owned());
            } else if metadata.is_file() {
                if metadata.len() > i64::MAX as u64 {
                    scope::error(&path, "File size exceeds supported range");
                    continue;
                }
                let identity = crate::routine_cleaner::file_identity(&path);
                let identity_key = identity
                    .map(|(volume, file)| format!("{volume}:{file}"))
                    .unwrap_or_else(|| display_path.to_string());
                files.push((
                    display_path.to_owned(),
                    metadata.len() as i64,
                    identity_key,
                    identity.map(|value| value.0),
                    identity.map(|value| value.1.to_string()),
                ));
            }
            if directories.len() + files.len() >= ENUMERATION_BATCH_SIZE {
                discovered += flush_enumeration_batch(index, &root, &mut directories, &mut files)?;
                scope::update(|state| {
                    state.current_path = scope::display_path(&directory).unwrap_or_default();
                    state.scanned_files += discovered;
                });
                discovered = 0;
            }
        }
        discovered += flush_enumeration_batch(index, &root, &mut directories, &mut files)?;
        if discovered > 0 {
            scope::update(|state| state.scanned_files += discovered);
        }
    }
    Ok(())
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
    let mut cursor = 0i64;
    while !cancelled.load(Ordering::Acquire) {
        let mut statement = index.connection.prepare("SELECT f.rowid,f.path,f.size,f.volume,f.file_index FROM files f JOIN candidates c ON f.rowid=c.file_row WHERE f.rowid>?1 ORDER BY f.rowid LIMIT 128").map_err(|error| error.to_string())?;
        let batch = statement
            .query_map([cursor], |row| {
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
        for (row, path, size, volume, file_index) in batch {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            cursor = row;
            let path = PathBuf::from(path);
            scope::update(|state| {
                state.current_path = scope::display_path(&path).unwrap_or_default()
            });
            let identity = volume.zip(file_index.and_then(|value| value.parse::<u64>().ok()));
            match hash_candidate(&path, size, identity, cancelled) {
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

fn hash_candidate(
    path: &Path,
    size: u64,
    identity: Option<(u32, u64)>,
    cancelled: &AtomicBool,
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
    let hash = hash_reader(&mut file, cancelled)?;
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
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    let mut pending_progress = 0u64;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("cancelled".into());
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            if pending_progress > 0 && super::RUNNING.load(Ordering::Acquire) {
                scope::update(|state| {
                    state.bytes_hashed = state.bytes_hashed.saturating_add(pending_progress)
                });
            }
            return Ok(hasher.finalize().into());
        }
        hasher.update(&buffer[..read]);
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
}
