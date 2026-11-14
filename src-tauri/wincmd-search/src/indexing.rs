// SPDX-License-Identifier: AGPL-3.0-or-later

use tantivy::IndexWriter;

use crate::{
    error::{Result, SearchError},
    extract::extract_text,
    index::ContentIndex,
    types::FileMeta,
    PathPolicy,
};

pub(crate) fn index_file(
    index: &ContentIndex,
    writer: &mut IndexWriter,
    meta: FileMeta,
    policy: &PathPolicy,
) -> Result<()> {
    if !policy(&meta.path) {
        return Ok(());
    }
    match extract_text(meta.clone()) {
        Ok(doc) => {
            if policy(&doc.meta.path) {
                index.upsert(writer, &doc.meta, &doc.title, &doc.body, &doc.props)?;
            }
        }
        Err(error) => {
            // A queued crawl/watch entry can disappear before its extractor opens it.
            let missing = matches!(&error, SearchError::Io(io) if io.kind() == std::io::ErrorKind::NotFound)
                || (error.is_per_file_skip()
                    && matches!(std::fs::metadata(&meta.path), Err(io) if io.kind() == std::io::ErrorKind::NotFound));
            if missing {
                index.delete(writer, meta.doc_id);
            } else if !error.is_per_file_skip() {
                return Err(error);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crawler::{build_globset, collect_files},
        types::{ContentQuery, IndexConfig},
        SearchEngine,
    };
    use std::{fs, path::Path};
    use tempfile::TempDir;

    fn open(index: &Path) -> SearchEngine {
        SearchEngine::open(IndexConfig {
            roots: vec![],
            exclusions: vec![],
            skip_paths: vec![],
            max_file_bytes: 10_000,
            index_dir: index.to_path_buf(),
        })
        .unwrap()
    }

    fn query(terms: &str) -> ContentQuery {
        ContentQuery {
            terms: terms.into(),
            roots: vec![],
            limit: 10,
            offset: 0,
            keyword_only: true,
        }
    }

    #[test]
    fn file_deleted_after_enumeration_removes_stale_hit_and_indexes_remaining_files() {
        let data = TempDir::new().unwrap();
        let index = TempDir::new().unwrap();
        let vanished = data.path().join("vanished.txt");
        fs::write(&vanished, "staledeletedtoken").unwrap();
        let files = || {
            collect_files(
                &[data.path().to_path_buf()],
                &build_globset(&[]).unwrap(),
                &[],
                10_000,
            )
        };
        let engine = open(index.path());
        engine.index_files_sync(files()).unwrap();
        assert_eq!(engine.search(&query("staledeletedtoken")).unwrap().len(), 1);

        fs::write(data.path().join("healthy.txt"), "healthyremainingtoken").unwrap();
        let queued = files();
        fs::remove_file(&vanished).unwrap();
        engine.index_files_sync(queued).unwrap();

        assert!(engine.status().last_error.is_none());
        assert!(!engine.status().is_indexing);
        assert_eq!(engine.status().pending_docs, 0);
        assert!(engine
            .search(&query("staledeletedtoken"))
            .unwrap()
            .is_empty());
        assert_eq!(
            engine
                .search(&query("healthyremainingtoken"))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn real_file_io_error_remains_visible_and_does_not_stop_other_files() {
        let data = TempDir::new().unwrap();
        let index = TempDir::new().unwrap();
        let blocked = data.path().join("directory.txt");
        fs::create_dir(&blocked).unwrap();
        let healthy = data.path().join("healthy.txt");
        fs::write(&healthy, "healthyremainingtoken").unwrap();
        let meta = |path: std::path::PathBuf, doc_id| FileMeta {
            doc_id,
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path,
            ext: "txt".into(),
            mtime: 0,
            size: 1,
        };
        let engine = open(index.path());
        engine
            .index_files_sync(vec![meta(blocked, 1), meta(healthy, 2)])
            .unwrap();

        assert!(engine
            .status()
            .last_error
            .unwrap()
            .starts_with("I/O error:"));
        assert_eq!(
            engine
                .search(&query("healthyremainingtoken"))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn index_writer_failure_still_reaches_the_caller() {
        let index = TempDir::new().unwrap();
        let engine = open(index.path());
        let _writer = engine.ci.writer().unwrap();
        assert!(matches!(
            engine.index_files_sync(vec![]),
            Err(SearchError::Index(_))
        ));
    }

    #[test]
    fn missing_pdf_wrapped_as_extraction_error_also_removes_stale_hit() {
        let data = TempDir::new().unwrap();
        let index = TempDir::new().unwrap();
        let path = data.path().join("vanished.pdf");
        let meta = FileMeta {
            doc_id: 1,
            path,
            name: "vanished.pdf".into(),
            ext: "pdf".into(),
            mtime: 0,
            size: 1,
        };
        let engine = open(index.path());
        let mut writer = engine.ci.writer().unwrap();
        engine
            .ci
            .upsert(
                &mut writer,
                &meta,
                "vanished",
                "stalepdftoken",
                &Default::default(),
            )
            .unwrap();
        engine.ci.commit(&mut writer).unwrap();
        drop(writer);
        assert_eq!(engine.search(&query("stalepdftoken")).unwrap().len(), 1);

        engine.index_files_sync(vec![meta]).unwrap();
        assert!(engine.status().last_error.is_none());
        assert!(engine.search(&query("stalepdftoken")).unwrap().is_empty());
    }
}
