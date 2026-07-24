// SPDX-License-Identifier: AGPL-3.0-or-later
//! Filesystem crawler for wincmd-search.
//!
//! Produces a flat `Vec<FileMeta>` from a set of root directories,
//! respecting glob exclusions, hard-skip prefix paths, and a file-size cap.

use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};

use crate::error::{Result, SearchError};
use crate::types::{DocId, FileMeta};

/// FNV-1a 64-bit hash of the canonicalized path string.
pub fn doc_id_for(path: &Path) -> DocId {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = canonical.to_string_lossy();
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Build a GlobSet from a list of glob pattern strings.
pub fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p).map_err(|e| SearchError::Config(e.to_string()))?);
    }
    builder
        .build()
        .map_err(|e| SearchError::Config(e.to_string()))
}

/// Collect all indexable files under `roots`, respecting exclusions and size cap.
pub fn collect_files(
    roots: &[PathBuf],
    exclusions: &GlobSet,
    skip_paths: &[PathBuf],
    max_file_bytes: u64,
) -> Vec<FileMeta> {
    let mut out = Vec::new();
    for root in roots {
        collect_recursive(root, exclusions, skip_paths, max_file_bytes, &mut out);
    }
    out
}

fn collect_recursive(
    dir: &Path,
    exclusions: &GlobSet,
    skip_paths: &[PathBuf],
    max_file_bytes: u64,
    out: &mut Vec<FileMeta>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();

        // Hard skip (decoy paths, etc.)
        if skip_paths.iter().any(|s| path.starts_with(s)) {
            continue;
        }

        // Glob exclusions match on the file name component.
        if let Some(name) = path.file_name() {
            if exclusions.is_match(name) {
                continue;
            }
        }

        if path.is_dir() {
            if entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(false) {
                continue; // don't follow symlinks/junctions — avoids cycles
            }
            collect_recursive(&path, exclusions, skip_paths, max_file_bytes, out);
            continue;
        }

        // Use fs::metadata (follows symlinks/reparse points), not
        // DirEntry::metadata (lstat-like — reports the link's own size,
        // not the target's), so the size cap and mtime reflect the real
        // file content, matching watch.rs's fs::metadata usage.
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() > max_file_bytes {
            continue;
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        out.push(FileMeta {
            doc_id: doc_id_for(&path),
            path,
            name,
            ext,
            mtime,
            size: meta.len(),
        });
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn empty_globset() -> GlobSet {
        build_globset(&[]).unwrap()
    }

    // ── doc_id_for stability ──────────────────────────────────────────

    #[test]
    fn doc_id_stable_across_calls() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("stable.txt");
        fs::write(&file, b"hi").unwrap();
        let id1 = doc_id_for(&file);
        let id2 = doc_id_for(&file);
        assert_eq!(id1, id2);
    }

    #[test]
    fn doc_id_differs_for_different_paths() {
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        assert_ne!(doc_id_for(&a), doc_id_for(&b));
    }

    #[test]
    fn doc_id_canonicalization_tolerant() {
        // Two paths that resolve to the same file (one via a `..` component)
        // should produce the same id once both exist (canonicalize resolves the real path).
        let dir = TempDir::new().unwrap();
        let canonical = dir.path().join("canon.txt");
        fs::write(&canonical, b"x").unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        let dotdot = dir.path().join("sub").join("..").join("canon.txt");
        assert_eq!(doc_id_for(&canonical), doc_id_for(&dotdot));
    }

    // ── build_globset ─────────────────────────────────────────────────

    #[test]
    fn build_globset_valid_patterns() {
        let gs = build_globset(&["*.log".into(), "*.tmp".into()]);
        assert!(gs.is_ok());
    }

    #[test]
    fn build_globset_invalid_pattern_returns_error() {
        let gs = build_globset(&["[invalid".into()]);
        assert!(gs.is_err());
    }

    // ── collect_files: basic ──────────────────────────────────────────

    #[test]
    fn collects_files_in_subdirs() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(dir.path().join("root.txt"), b"r").unwrap();
        fs::write(sub.join("nested.txt"), b"n").unwrap();

        let gs = empty_globset();
        let files = collect_files(&[dir.path().to_path_buf()], &gs, &[], u64::MAX);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert!(
            names.contains(&"root.txt"),
            "expected root.txt, got {:?}",
            names
        );
        assert!(
            names.contains(&"nested.txt"),
            "expected nested.txt, got {:?}",
            names
        );
    }

    // ── collect_files: glob exclusions ───────────────────────────────

    #[test]
    fn glob_exclusion_skips_matching_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("keep.txt"), b"k").unwrap();
        fs::write(dir.path().join("skip.log"), b"s").unwrap();

        let gs = build_globset(&["*.log".into()]).unwrap();
        let files = collect_files(&[dir.path().to_path_buf()], &gs, &[], u64::MAX);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"keep.txt"));
        assert!(!names.contains(&"skip.log"), "*.log should be excluded");
    }

    #[test]
    fn glob_exclusion_skips_matching_dirs() {
        let dir = TempDir::new().unwrap();
        let hidden = dir.path().join(".git");
        fs::create_dir(&hidden).unwrap();
        fs::write(hidden.join("config"), b"cfg").unwrap();
        fs::write(dir.path().join("visible.rs"), b"code").unwrap();

        let gs = build_globset(&[".git".into()]).unwrap();
        let files = collect_files(&[dir.path().to_path_buf()], &gs, &[], u64::MAX);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert!(!names.contains(&"config"), ".git dir should be excluded");
        assert!(names.contains(&"visible.rs"));
    }

    // ── collect_files: skip_paths ─────────────────────────────────────

    #[test]
    fn skip_paths_prefix_match_excludes_subtree() {
        let dir = TempDir::new().unwrap();
        let secret = dir.path().join("secret");
        fs::create_dir(&secret).unwrap();
        fs::write(secret.join("private.txt"), b"p").unwrap();
        fs::write(dir.path().join("public.txt"), b"pub").unwrap();

        let gs = empty_globset();
        let files = collect_files(
            &[dir.path().to_path_buf()],
            &gs,
            std::slice::from_ref(&secret),
            u64::MAX,
        );
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert!(
            !names.contains(&"private.txt"),
            "skip_paths should exclude subtree"
        );
        assert!(names.contains(&"public.txt"));
    }

    // ── collect_files: max_file_bytes ────────────────────────────────

    #[test]
    fn max_file_bytes_excludes_oversized_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("small.txt"), b"hi").unwrap(); // 2 bytes
        fs::write(dir.path().join("large.txt"), b"hello world!").unwrap(); // 12 bytes

        let gs = empty_globset();
        let files = collect_files(&[dir.path().to_path_buf()], &gs, &[], 5);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"small.txt"));
        assert!(
            !names.contains(&"large.txt"),
            "oversized file should be excluded"
        );
    }

    // ── collect_files: metadata correctness ──────────────────────────

    #[test]
    fn file_meta_fields_populated_correctly() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.pdf");
        fs::write(&file, b"pdfcontent").unwrap();

        let gs = empty_globset();
        let files = collect_files(&[dir.path().to_path_buf()], &gs, &[], u64::MAX);
        let m = files.iter().find(|f| f.name == "test.pdf").unwrap();
        assert_eq!(m.ext, "pdf");
        assert_eq!(m.size, 10);
        assert_eq!(m.doc_id, doc_id_for(&file));
    }
}
