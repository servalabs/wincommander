// SPDX-License-Identifier: AGPL-3.0-or-later
//! Strict candidate enumeration: errors are surfaced instead of silently
//! dropping inaccessible roots or metadata. Not a filesystem snapshot.
use std::{io, path::{Path, PathBuf}};
use globset::GlobSet;
use crate::{crawler::{doc_id_for, path_is_skipped}, types::FileMeta};

pub(crate) fn collect(roots: &[PathBuf], exclusions: &GlobSet, skips: &[PathBuf], max_bytes: u64) -> io::Result<(Vec<FileMeta>, usize)> {
    fn walk(dir: &Path, exclusions: &GlobSet, skips: &[PathBuf], max_bytes: u64, out: &mut Vec<FileMeta>, skipped_links: &mut usize) -> io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path_is_skipped(&path, skips) || path.file_name().is_some_and(|n| exclusions.is_match(n)) { continue; }
            let link_meta = std::fs::symlink_metadata(&path)?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if link_meta.file_attributes() & 0x400 != 0 { *skipped_links += 1; continue; }
            }
            if link_meta.file_type().is_symlink() { *skipped_links += 1; continue; }
            if link_meta.is_dir() { walk(&path, exclusions, skips, max_bytes, out, skipped_links)?; continue; }
            if !link_meta.is_file() || link_meta.len() > max_bytes { continue; }
            let name = path.file_name().ok_or_else(|| io::Error::other("missing filename"))?.to_string_lossy().into_owned();
            let ext = path.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            let mtime = link_meta.modified()?.duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| io::Error::other("invalid modification time"))?.as_secs();
            out.push(FileMeta { doc_id: doc_id_for(&path), path, name, ext, mtime, size: link_meta.len() });
        }
        Ok(())
    }
    let mut out = Vec::new();
    let mut skipped_links = 0usize;
    for root in roots { walk(root, exclusions, skips, max_bytes, &mut out, &mut skipped_links)?; }
    Ok((out, skipped_links))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inaccessible_root_is_failure_instead_of_a_partial_success() {
        let root = tempfile::tempdir().unwrap();
        let good = root.path().join("good");
        std::fs::create_dir(&good).unwrap();
        std::fs::write(good.join("a.txt"), "x").unwrap();
        let glob = crate::crawler::build_globset(&[]).unwrap();
        assert!(collect(&[good.clone(), root.path().join("missing")], &glob, &[], 100).is_err());
        assert_eq!(collect(&[good], &glob, &[], 100).unwrap().0.len(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn links_skipped_by_strict_rebuild_are_counted() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("data.txt"), "x").unwrap();
        std::os::unix::fs::symlink(root.path().join("data.txt"), root.path().join("alias.txt")).unwrap();
        let glob = crate::crawler::build_globset(&[]).unwrap();
        let (found, links) = collect(&[root.path().into()], &glob, &[], 100).unwrap();
        assert_eq!(found.len(), 1); assert_eq!(links, 1);
    }
    #[test]
    fn skip_roots_and_exclusions_are_respected() {
        let root = tempfile::tempdir().unwrap();
        let skip = root.path().join("index");
        std::fs::create_dir(&skip).unwrap();
        std::fs::write(skip.join("sensitive.txt"), "x").unwrap();
        std::fs::write(root.path().join("keep.txt"), "x").unwrap();
        let glob = crate::crawler::build_globset(&[]).unwrap();
        let (found, links) = collect(&[root.path().into()], &glob, &[skip], 100).unwrap();
        assert_eq!(links, 0);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "keep.txt");
    }
}
