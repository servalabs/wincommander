// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, synchronous reconciliation for indexes opened only during a mount operation.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tantivy::{schema::Value, DocAddress, TantivyDocument};

use crate::{
    crawler::doc_id_for,
    error::{Result, SearchError},
    extract::extract_text,
    restricted::safe_path,
    types::{DocId, DocProps, FileMeta},
    SearchEngine,
};

#[derive(Debug, Default, Clone)]
pub struct ReconcileReport {
    pub visited: u64,
    pub updated: u64,
    pub removed: u64,
    pub indexed_docs: u64,
    /// False on cancellation or when the changed-document budget was consumed.
    pub complete: bool,
}

pub(crate) fn revision_ns(path: &Path) -> std::io::Result<u64> {
    Ok(std::fs::metadata(path)?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u64::MAX as u128) as u64)
}

struct StoredFile {
    id: DocId,
    path: PathBuf,
    size: u64,
    revision_ns: u64,
}

impl SearchEngine {
    fn stored_files(&self) -> Result<Vec<StoredFile>> {
        let reader = self.ci.index_reader()?;
        let searcher = reader.searcher();
        let mut files = Vec::new();
        for (segment_ord, segment) in searcher.segment_readers().iter().enumerate() {
            for doc_id in segment.doc_ids_alive() {
                let doc: TantivyDocument =
                    searcher.doc(DocAddress::new(segment_ord as u32, doc_id))?;
                let number = |field| doc.get_first(field).and_then(|v| v.as_u64()).unwrap_or(0);
                files.push(StoredFile {
                    id: number(self.ci.f_doc_id),
                    path: PathBuf::from(
                        doc.get_first(self.ci.f_path)
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                    ),
                    size: number(self.ci.f_size),
                    revision_ns: number(self.ci.f_revision_ns),
                });
            }
        }
        Ok(files)
    }

    /// Total committed document count, including unsupported files indexed by name.
    pub fn indexed_document_count(&self) -> Result<u64> {
        Ok(self.ci.index_reader()?.searcher().num_docs())
    }

    /// Reconcile up to `max_files` changed files; unchanged files don't consume the budget.
    /// Repeated calls therefore make progress without an off-volume checkpoint. Cancellation
    /// is checked between filesystem entries and extractions, not within a document parser.
    /// After progress, yield between entries once the batch exceeds 500 ms. Scanning
    /// unchanged entries and a single parser call may exceed that soft time budget.
    /// Deletions apply only after every root was fully scanned and remains accessible.
    pub fn reconcile_sync(&self, cancel: &AtomicBool, max_files: usize) -> Result<ReconcileReport> {
        let mut report = ReconcileReport::default();
        if max_files == 0 || cancel.load(Ordering::Relaxed) {
            report.indexed_docs = self.indexed_document_count()?;
            return Ok(report);
        }
        for root in &self.config.roots {
            if !safe_path(root) || !root.is_dir() || !(self.path_policy)(root) {
                return Err(SearchError::Config(
                    "index root is unavailable or disallowed".into(),
                ));
            }
        }
        let existing = self.stored_files()?;
        let by_id: HashMap<_, _> = existing.iter().map(|f| (f.id, f)).collect();
        let mut seen = HashSet::new();
        let mut directories = self.config.roots.clone();
        let mut writer = self.ci.writer()?;
        let mut complete = true;
        let started = std::time::Instant::now();
        'scan: while let Some(directory) = directories.pop() {
            if cancel.load(Ordering::Relaxed) {
                complete = false;
                break;
            }
            for entry in std::fs::read_dir(&directory)? {
                if cancel.load(Ordering::Relaxed)
                    || (report.updated > 0
                        && started.elapsed() >= std::time::Duration::from_millis(500))
                {
                    complete = false;
                    break 'scan;
                }
                let path = entry?.path();
                if !self.permits_path(&path) {
                    continue;
                }
                let metadata = std::fs::symlink_metadata(&path)?;
                if metadata.is_dir() {
                    directories.push(path);
                    continue;
                }
                if !metadata.is_file() {
                    continue;
                }
                let id = doc_id_for(&path);
                if !seen.insert(id) {
                    continue;
                }
                report.visited += 1;
                let stamp = revision_ns(&path)?;
                if by_id
                    .get(&id)
                    .is_some_and(|f| f.size == metadata.len() && f.revision_ns == stamp)
                {
                    continue;
                }
                if report.updated >= max_files as u64 {
                    complete = false;
                    break 'scan;
                }
                let meta = FileMeta {
                    doc_id: id,
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    ext: path
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase(),
                    mtime: metadata
                        .modified()?
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    size: metadata.len(),
                    path,
                };
                let extracted = if meta.size > self.config.max_file_bytes {
                    Err(SearchError::Unsupported(
                        "file exceeds extraction size limit".into(),
                    ))
                } else {
                    extract_text(meta.clone())
                };
                let (title, body, props) = match extracted {
                    Ok(doc) => (doc.title, doc.body, doc.props),
                    Err(error) if error.is_per_file_skip() => {
                        (String::new(), String::new(), DocProps::default())
                    }
                    Err(error) => return Err(error),
                };
                if !self.permits_path(&meta.path) || revision_ns(&meta.path)? != stamp {
                    complete = false;
                    continue;
                }
                self.ci
                    .upsert_at_revision(&mut writer, &meta, &title, &body, &props, stamp)?;
                report.updated += 1;
            }
        }
        if cancel.load(Ordering::Relaxed) {
            complete = false;
        }
        if complete {
            for root in &self.config.roots {
                if !safe_path(root) || !root.is_dir() || !(self.path_policy)(root) {
                    return Err(SearchError::Config("index root became unavailable".into()));
                }
            }
            for file in &existing {
                if !seen.contains(&file.id) || !self.permits_path(&file.path) {
                    self.ci.delete(&mut writer, file.id);
                    report.removed += 1;
                }
            }
        }
        self.ci.commit(&mut writer)?;
        writer.wait_merging_threads()?;
        report.complete = complete;
        report.indexed_docs = self.indexed_document_count()?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContentQuery, IndexConfig};

    #[test]
    fn file_edit_after_extraction_cannot_hide_behind_a_newer_revision_stamp() {
        let volume = tempfile::TempDir::new().unwrap();
        let path = volume.path().join("edited.txt");
        std::fs::write(&path, "oldtoken").unwrap();
        let stamp = revision_ns(&path).unwrap();
        let meta = FileMeta {
            doc_id: doc_id_for(&path),
            path: path.clone(),
            name: "edited.txt".into(),
            ext: "txt".into(),
            mtime: 0,
            size: 8,
        };
        let config = IndexConfig {
            roots: vec![volume.path().into()],
            exclusions: vec![],
            skip_paths: vec![],
            max_file_bytes: 1024,
            index_dir: volume.path().join("index"),
        };
        let engine = SearchEngine::open(config).unwrap();
        let mut writer = engine.ci.writer().unwrap();
        std::fs::write(&path, "newtoken").unwrap();
        engine
            .ci
            .upsert_at_revision(
                &mut writer,
                &meta,
                "edited",
                "oldtoken",
                &DocProps::default(),
                stamp,
            )
            .unwrap();
        engine.ci.commit(&mut writer).unwrap();
        writer.wait_merging_threads().unwrap();
        assert_eq!(
            engine
                .reconcile_sync(&AtomicBool::new(false), 10)
                .unwrap()
                .updated,
            1
        );
        let query = ContentQuery {
            terms: "newtoken".into(),
            roots: vec![],
            limit: 10,
            offset: 0,
            keyword_only: true,
        };
        assert_eq!(engine.search_restricted(&query).unwrap().len(), 1);
    }
}
