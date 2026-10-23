// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{
    scope, CachedFile, DuplicateFile, DuplicateGroup, DuplicateScan, GROUP_PAGE_SIZE,
    MEMBER_PAGE_SIZE,
};
use rusqlite::{params, Connection, OpenFlags};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, UNIX_EPOCH};
#[path = "scratch.rs"]
mod scratch;

pub(super) fn unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}
pub(super) fn count(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<usize> {
    usize::try_from(unsigned(row, column)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

pub(super) struct ScanIndex {
    pub connection: Connection,
    _database: std::fs::File,
    _directories: Vec<std::fs::File>,
    pub directory: tempfile::TempDir,
    pub created_at: Instant,
    pub scan_id: String,
    pub status: scope::DuplicateProgress,
}
impl ScanIndex {
    pub fn new() -> Result<Self, String> {
        Self::from_directory(scratch::create()?)
    }
    pub(super) fn from_directory(directory: tempfile::TempDir) -> Result<Self, String> {
        let directories = scratch::directories(directory.path())?;
        let path = directory.path().join("scan.sqlite");
        let database = scratch::reserve_database(&path)?;
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|error| error.to_string())?;
        connection.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA temp_store=FILE; PRAGMA cache_size=-4096;
            CREATE TABLE directories(path TEXT PRIMARY KEY, root TEXT NOT NULL, done INTEGER NOT NULL DEFAULT 0);
            CREATE INDEX pending_directories ON directories(done);
            CREATE TABLE files(id TEXT PRIMARY KEY, path TEXT UNIQUE NOT NULL, root TEXT NOT NULL, size INTEGER NOT NULL,
                identity TEXT UNIQUE NOT NULL, volume INTEGER, file_index TEXT, modified TEXT, sample_hash BLOB, hash BLOB, group_id TEXT);
            CREATE INDEX file_sizes ON files(size);
            CREATE TABLE groups(id TEXT PRIMARY KEY, size INTEGER NOT NULL, hash BLOB NOT NULL, count INTEGER NOT NULL);")
            .map_err(|error| error.to_string())?;
        let status = scope::snapshot();
        Ok(Self {
            connection,
            _database: database,
            _directories: directories,
            directory,
            created_at: Instant::now(),
            scan_id: status.scan_id.clone(),
            status,
        })
    }

    pub fn page(&self, offset: usize) -> Result<DuplicateScan, String> {
        let total: usize = self
            .connection
            .query_row("SELECT count(*) FROM groups", [], |row| count(row, 0))
            .map_err(|error| error.to_string())?;
        if offset > total {
            return Err("Invalid duplicate result page".into());
        }
        let mut statement = self
            .connection
            .prepare("SELECT id FROM groups ORDER BY size*(count-1) DESC,id LIMIT ?1 OFFSET ?2")
            .map_err(|error| error.to_string())?;
        let ids = statement
            .query_map(params![GROUP_PAGE_SIZE as i64, offset as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| error.to_string())?;
        let groups = ids
            .map(|id| self.group(&id.map_err(|error| error.to_string())?, 0))
            .collect::<Result<Vec<_>, _>>()?;
        let mut total_duplicate_bytes = 0u64;
        let mut sizes = self
            .connection
            .prepare("SELECT size,count FROM groups")
            .map_err(|error| error.to_string())?;
        for row in sizes
            .query_map([], |row| Ok((unsigned(row, 0)?, unsigned(row, 1)?)))
            .map_err(|error| error.to_string())?
        {
            let (size, count) = row.map_err(|error| error.to_string())?;
            total_duplicate_bytes =
                total_duplicate_bytes.saturating_add(size.saturating_mul(count.saturating_sub(1)));
        }
        let state = &self.status;
        Ok(DuplicateScan {
            has_more_groups: offset + groups.len() < total,
            groups,
            group_offset: offset,
            total_groups: total,
            total_duplicate_bytes,
            scan_id: self.scan_id.clone(),
            scope_roots: state.scope_roots.clone(),
            excluded_volumes: state.excluded_volumes.clone(),
            scanned_files: state.scanned_files,
            hashed_files: state.hashed_files,
            skipped_entries: state.skipped_entries,
            skipped_links: state.skipped_links,
            errors: state.errors.clone(),
            cancelled: state.phase == "cancelled",
            complete: state.phase == "ready"
                && state.skipped_entries == 0
                && state.skipped_links == 0
                && !state
                    .excluded_volumes
                    .iter()
                    .any(|entry| entry.contains("Fixed drive unavailable")),
            truncated: false,
            minimum_size_bytes: state.minimum_size_bytes,
            skipped_small_files: state.skipped_small_files,
        })
    }

    pub fn group(&self, group_id: &str, offset: usize) -> Result<DuplicateGroup, String> {
        let (size, count): (u64, usize) = self
            .connection
            .query_row(
                "SELECT size,count FROM groups WHERE id=?1",
                [group_id],
                |row| Ok((unsigned(row, 0)?, count(row, 1)?)),
            )
            .map_err(|_| "Duplicate group is stale or invalid")?;
        if offset > count.saturating_sub(1) {
            return Err("Invalid duplicate member page".into());
        }
        let mut statement = self
            .connection
            .prepare("SELECT id,path,modified FROM files WHERE group_id=?1 ORDER BY id LIMIT ?2 OFFSET ?3")
            .map_err(|error| error.to_string())?;
        let mut files = Vec::new();
        // A stable original copy is repeated on every member page to keep retention explicit.
        let keeper: (String, String, Option<String>) = self
            .connection
            .query_row(
                "SELECT id,path,modified FROM files WHERE group_id=?1 ORDER BY id LIMIT 1",
                [group_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| error.to_string())?;
        let display = |(id, path, modified): (String, String, Option<String>)| DuplicateFile {
            name: PathBuf::from(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Name unavailable")
                .into(),
            id,
            path,
            size,
            modified,
        };
        files.push(display(keeper));
        for row in statement
            .query_map(
                params![group_id, MEMBER_PAGE_SIZE as i64, offset as i64 + 1],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .map_err(|error| error.to_string())?
        {
            files.push(display(row.map_err(|error| error.to_string())?));
        }
        Ok(DuplicateGroup {
            id: group_id.into(),
            size,
            reclaimable_bytes: size.saturating_mul(count.saturating_sub(1) as u64),
            has_more_files: offset + files.len() < count,
            files,
            file_count: count,
            member_offset: offset,
        })
    }

    pub fn file(&self, id: &str) -> Result<CachedFile, String> {
        self.connection.query_row("SELECT path,root,group_id,size,modified,hash,volume,file_index FROM files WHERE id=?1 AND group_id IS NOT NULL", [id], |row| {
            let modified: Option<String> = row.get(4)?;
            let nanos = modified.and_then(|value| value.parse::<u128>().ok());
            let modified = nanos.and_then(|value| UNIX_EPOCH.checked_add(Duration::new((value / 1_000_000_000) as u64, (value % 1_000_000_000) as u32)));
            let hash: Vec<u8> = row.get(5)?;
            let hash = hash.try_into().map_err(|_| rusqlite::Error::InvalidQuery)?;
            let volume: Option<u32> = row.get(6)?;
            let file_index: Option<String> = row.get(7)?;
            Ok(CachedFile { path: PathBuf::from(row.get::<_, String>(0)?), root: PathBuf::from(row.get::<_, String>(1)?),
                group_id: row.get(2)?, bytes: unsigned(row, 3)?, modified, hash, identity: volume.zip(file_index.and_then(|value| value.parse().ok())) })
        }).map_err(|_| "Duplicate selection is stale or invalid; scan again".into())
    }

    pub fn removal_selection(
        &self,
        selected: &HashSet<String>,
    ) -> Result<(Vec<CachedFile>, Vec<CachedFile>), String> {
        let mut candidates = Vec::new();
        let mut groups: HashMap<String, usize> = HashMap::new();
        for id in selected {
            let file = self.file(id)?;
            *groups.entry(file.group_id.clone()).or_default() += 1;
            candidates.push(file);
        }
        let mut keepers = Vec::new();
        for (group, selected_count) in groups {
            let count: usize = self
                .connection
                .query_row("SELECT count FROM groups WHERE id=?1", [&group], |row| {
                    count(row, 0)
                })
                .map_err(|error| error.to_string())?;
            if selected_count >= count {
                return Err("Every group must retain at least one verified copy".into());
            }
            let mut statement = self
                .connection
                .prepare("SELECT id FROM files WHERE group_id=?1 ORDER BY id")
                .map_err(|error| error.to_string())?;
            let mut keeper = None;
            for id in statement
                .query_map([&group], |row| row.get::<_, String>(0))
                .map_err(|error| error.to_string())?
            {
                let id = id.map_err(|error| error.to_string())?;
                if !selected.contains(&id) {
                    let file = self.file(&id)?;
                    if super::safety::validated_file(&file).is_ok() {
                        keeper = Some(file);
                        break;
                    }
                }
            }
            keepers.push(keeper.ok_or("No unchanged copy remains; scan again")?);
        }
        Ok((candidates, keepers))
    }
}
