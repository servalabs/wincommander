// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental, opt-in crawl plumbing. Not wired into production startup.
use std::sync::Mutex;

use crate::{crawler::{build_globset, collect_files}, error::{Result, SearchError}, indexing::index_file, space_guard::{SpaceGuard, SpaceState}, watch::FsEventAction, SearchEngine};

const PAUSED: &str = "Search indexing paused: free disk space or check disk availability. A full rescan is required.";

impl SearchEngine {
    /// Process one already-classified watcher action under the same guard used
    /// by the crawl. A false result requires full reconciliation, not replay of
    /// the current action alone.
    pub(crate) fn experimental_guarded_update<F>(&self, guard: &Mutex<SpaceGuard>, action: FsEventAction, mut available: F) -> Result<bool>
    where F: FnMut() -> std::io::Result<u64> {
        let ready = |available: std::io::Result<u64>| -> Result<bool> {
            Ok(guard.lock().map_err(|_| SearchError::Config("search disk guard unavailable".into()))?.observe(available) == SpaceState::Ready)
        };
        if !ready(available())? {
            self.status.lock().unwrap().last_error = Some(PAUSED.into());
            return Ok(false);
        }
        let mut writer = self.ci.writer()?;
        writer.set_merge_policy(Box::new(tantivy::indexer::NoMergePolicy));
        match action {
            FsEventAction::Upsert(meta) => {
                if (self.path_policy)(&meta.path) {
                    if let Err(error) = index_file(&self.ci, &mut writer, meta, &self.path_policy) {
                        writer.rollback()?;
                        return Err(error);
                    }
                }
            }
            FsEventAction::Delete(id) => self.ci.delete(&mut writer, id),
        }
        if !ready(available())? {
            writer.rollback()?;
            self.status.lock().unwrap().last_error = Some(PAUSED.into());
            return Ok(false);
        }
        self.ci.commit(&mut writer)?;
        Ok(true)
    }

    /// Return false when paused or reconciliation is required. Observations
    /// are advisory and do not reserve bytes against other writers.
    pub(crate) fn experimental_guarded_crawl<F>(&self, guard: &Mutex<SpaceGuard>, mut available: F) -> Result<bool>
    where F: FnMut() -> std::io::Result<u64> {
        let ready = |available: std::io::Result<u64>| -> Result<bool> {
            Ok(guard.lock().map_err(|_| SearchError::Config("search disk guard unavailable".into()))?.observe(available) == SpaceState::Ready)
        };
        if !ready(available())? {
            self.status.lock().unwrap().last_error = Some(PAUSED.into());
            return Ok(false);
        }
        let exclusions = build_globset(&self.config.exclusions)?;
        let files = collect_files(&self.config.roots, &exclusions, &self.config.skip_paths, self.config.max_file_bytes);
        if !ready(available())? {
            self.status.lock().unwrap().last_error = Some(PAUSED.into());
            return Ok(false);
        }
        let mut writer = self.ci.writer()?;
        // Avoid scheduling automatic merges from this experimental writer.
        writer.set_merge_policy(Box::new(tantivy::indexer::NoMergePolicy));
        for meta in files {
            if !ready(available())? {
                writer.rollback()?;
                self.status.lock().unwrap().last_error = Some(PAUSED.into());
                return Ok(false);
            }
            if !(self.path_policy)(&meta.path) { continue; }
            if let Err(error) = index_file(&self.ci, &mut writer, meta, &self.path_policy) {
                writer.rollback()?;
                return Err(error);
            }
        }
        if !ready(available())? {
            writer.rollback()?;
            self.status.lock().unwrap().last_error = Some(PAUSED.into());
            return Ok(false);
        }
        self.ci.commit(&mut writer)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IndexConfig;
    use tempfile::TempDir;

    #[test]
    fn low_space_pauses_and_recovery_requires_reconciliation() {
        let root = TempDir::new().unwrap();
        let index = TempDir::new().unwrap();
        std::fs::write(root.path().join("file.txt"), "ordinary content").unwrap();
        let engine = SearchEngine::open(IndexConfig { roots: vec![root.path().to_path_buf()], exclusions: vec![], skip_paths: vec![], max_file_bytes: 10000, index_dir: index.path().to_path_buf() }).unwrap();
        let guard = Mutex::new(SpaceGuard::default());
        assert!(!engine.experimental_guarded_crawl(&guard, || Ok(0)).unwrap());
        assert!(engine.status().last_error.unwrap().contains("paused"));
        assert!(!engine.experimental_guarded_crawl(&guard, || Ok(u64::MAX)).unwrap());
        assert_eq!(engine.indexed_document_count().unwrap(), 0);
    }

    #[test]
    fn falling_space_rolls_back_uncommitted_crawl() {
        let root = TempDir::new().unwrap();
        let index = TempDir::new().unwrap();
        std::fs::write(root.path().join("file.txt"), "ordinary content").unwrap();
        let engine = SearchEngine::open(IndexConfig { roots: vec![root.path().to_path_buf()], exclusions: vec![], skip_paths: vec![], max_file_bytes: 10000, index_dir: index.path().to_path_buf() }).unwrap();
        let guard = Mutex::new(SpaceGuard::default());
        let mut checks = 0;
        assert!(!engine.experimental_guarded_crawl(&guard, || { checks += 1; Ok(if checks < 4 { u64::MAX } else { 0 }) }).unwrap());
        assert_eq!(engine.indexed_document_count().unwrap(), 0);
    }

    #[test]
    fn watcher_update_pauses_and_rolls_back_when_space_falls() {
        let root = TempDir::new().unwrap();
        let index = TempDir::new().unwrap();
        std::fs::write(root.path().join("file.txt"), "ordinary content").unwrap();
        let engine = SearchEngine::open(IndexConfig { roots: vec![root.path().to_path_buf()], exclusions: vec![], skip_paths: vec![], max_file_bytes: 10000, index_dir: index.path().to_path_buf() }).unwrap();
        let meta = collect_files(&engine.config.roots, &build_globset(&[]).unwrap(), &[], 10000).pop().unwrap();
        let guard = Mutex::new(SpaceGuard::default());
        let mut checks = 0;
        let applied = engine.experimental_guarded_update(&guard, FsEventAction::Upsert(meta), || {
            checks += 1;
            Ok(if checks == 1 { u64::MAX } else { 0 })
        }).unwrap();
        assert!(!applied);
        assert_eq!(engine.indexed_document_count().unwrap(), 0);
        assert!(engine.status().last_error.unwrap().contains("paused"));
        assert!(guard.lock().unwrap().reconciliation_started().is_none());
    }
}
