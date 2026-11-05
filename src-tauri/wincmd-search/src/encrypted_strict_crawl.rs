// SPDX-License-Identifier: AGPL-3.0-or-later
//! Strict candidate enumeration: errors are surfaced instead of silently
//! dropping inaccessible roots or metadata. Not a filesystem snapshot.
use std::{io, path::{Path, PathBuf}};
use globset::GlobSet;
use crate::{crawler::{doc_id_for, path_is_skipped}, types::FileMeta};

pub(crate) fn collect(roots: &[PathBuf], exclusions: &GlobSet, skips: &[PathBuf], max_bytes: u64) -> io::Result<Vec<FileMeta>> {
    fn walk(dir: &Path, exclusions: &GlobSet, skips: &[PathBuf], max_bytes: u64, out: &mut Vec<FileMeta>) -> io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path_is_skipped(&path, skips) || path.file_name().is_some_and(|n| exclusions.is_match(n)) { continue; }
            let link_meta = std::fs::symlink_metadata(&path)?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if link_meta.file_attributes() & 0x400 != 0 { continue; }
            }
            if link_meta.file_type().is_symlink() { continue; }
            if link_meta.is_dir() { walk(&path, exclusions, skips, max_bytes, out)?; continue; }
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
    for root in roots { walk(root, exclusions, skips, max_bytes, &mut out)?; }
    Ok(out)
}
/// Hash the exact set of enumerated file names and bytes. Repeating this detects
/// observed additions, removals and content changes; it is not a snapshot or a
/// fence against changes between verification and publication.
pub(crate) fn fingerprint(files: &[FileMeta]) -> io::Result<[u8; 32]> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use zeroize::Zeroizing;
    let mut ordered: Vec<_> = files.iter().collect();
    ordered.sort_by(|a, b| a.path.as_os_str().as_encoded_bytes().cmp(b.path.as_os_str().as_encoded_bytes()));
    let mut total = Sha256::new();
    total.update(b"wincommander-search-source-fingerprint-v1");
    for file in ordered {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
            options.share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let mut opened = options.open(&file.path)?;
        let meta = opened.metadata()?;
        if !meta.is_file() || meta.len() != file.size { return Err(io::Error::other("source changed during fingerprint")); }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 { return Err(io::Error::other("linked source refused")); }
        }
        let mut content = Sha256::new();
        let mut buffer = Zeroizing::new([0u8; 65536]);
        let mut count = 0u64;
        loop {
            let n = opened.read(buffer.as_mut())?;
            if n == 0 { break; }
            count = count.checked_add(n as u64).ok_or_else(|| io::Error::other("source too large"))?;
            if count > file.size { return Err(io::Error::other("source changed during fingerprint")); }
            content.update(&buffer[..n]);
        }
        if count != file.size { return Err(io::Error::other("source changed during fingerprint")); }
        let path = file.path.as_os_str().as_encoded_bytes();
        total.update((path.len() as u64).to_le_bytes());
        total.update(path);
        total.update(file.size.to_le_bytes());
        total.update(content.finalize());
    }
    Ok(total.finalize().into())
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
        assert_eq!(collect(&[good], &glob, &[], 100).unwrap().len(), 1);
    }
    #[test]
    fn fingerprint_detects_content_and_file_set_changes() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("a.txt");
        std::fs::write(&first, b"abc").unwrap();
        let glob = crate::crawler::build_globset(&[]).unwrap();
        let snapshot = || collect(&[root.path().into()], &glob, &[], 100).unwrap();
        let initial = fingerprint(&snapshot()).unwrap();
        assert_eq!(initial, fingerprint(&snapshot()).unwrap());
        std::fs::write(&first, b"xyz").unwrap();
        assert_ne!(initial, fingerprint(&snapshot()).unwrap());
        std::fs::write(&first, b"abc").unwrap();
        std::fs::write(root.path().join("new.txt"), b"new").unwrap();
        assert_ne!(initial, fingerprint(&snapshot()).unwrap());
    }
    #[test]
    fn skip_roots_and_exclusions_are_respected() {
        let root = tempfile::tempdir().unwrap();
        let skip = root.path().join("index");
        std::fs::create_dir(&skip).unwrap();
        std::fs::write(skip.join("sensitive.txt"), "x").unwrap();
        std::fs::write(root.path().join("keep.txt"), "x").unwrap();
        let glob = crate::crawler::build_globset(&[]).unwrap();
        let found = collect(&[root.path().into()], &glob, &[skip], 100).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "keep.txt");
    }
}
