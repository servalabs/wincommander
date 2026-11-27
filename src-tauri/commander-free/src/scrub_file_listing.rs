// SPDX-License-Identifier: AGPL-3.0-or-later
// Lists the files of a Share Safely run with the same rules the scrub engine
// uses, but keeps what it cannot read instead of dropping it.

use std::fs;
use std::path::{Path, PathBuf};

pub struct Listed {
    pub path: PathBuf,
    pub root: String,
    /// Set for a folder that could not be read; holds the OS error code.
    pub unreadable_dir: Option<i32>,
}

// Version-control internals and dependency caches are tool state, not content to share, and must never be rewritten.
const TOOL_DIRS: [&str; 6] = [".git", ".svn", ".hg", ".bzr", "node_modules", "__pycache__"];

fn skip_dir(name: &str) -> bool {
    name == "_scrubbed"
        || name.starts_with(".wincommander-pdf-check-")
        || name.starts_with(".wincommander-scrub-stage-")
        || TOOL_DIRS.iter().any(|tool| name.eq_ignore_ascii_case(tool))
}

// Windows writes these into folders on its own; they are not content the user shares.
fn is_shell_file(name: &str, meta: &fs::Metadata) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "desktop.ini" | "thumbs.db" | "ehthumbs.db"
    ) || is_hidden_system(meta)
}

#[cfg(windows)]
fn is_hidden_system(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN_SYSTEM: u32 = 0x2 | 0x4;
    meta.file_attributes() & HIDDEN_SYSTEM == HIDDEN_SYSTEM
}

#[cfg(not(windows))]
fn is_hidden_system(_: &fs::Metadata) -> bool {
    false
}

fn walk(dir: &Path, root: &str, recursive: bool, out: &mut Vec<Listed>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            let code = e.raw_os_error().unwrap_or(-1);
            out.push(Listed {
                path: dir.to_path_buf(),
                root: root.to_string(),
                unreadable_dir: Some(code),
            });
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            out.push(Listed {
                path,
                root: root.to_string(),
                unreadable_dir: None,
            });
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_file() {
            if is_shell_file(&entry.file_name().to_string_lossy(), &meta) {
                continue;
            }
            out.push(Listed {
                path,
                root: root.to_string(),
                unreadable_dir: None,
            });
        } else if meta.is_dir() && recursive && !skip_dir(&entry.file_name().to_string_lossy()) {
            walk(&path, root, recursive, out);
        }
    }
}

pub fn list_run(paths: &[String], recursive: bool) -> Vec<Listed> {
    let mut out = Vec::new();
    for root in paths {
        let path = PathBuf::from(root);
        if path.is_dir() {
            walk(&path, root, recursive, &mut out);
        } else {
            out.push(Listed {
                path,
                root: root.clone(),
                unreadable_dir: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(listed: &[Listed]) -> Vec<String> {
        let mut names: Vec<String> = listed
            .iter()
            .map(|l| l.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn folder_walk_skips_windows_shell_files_and_scrub_staging() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["photo.jpg", "desktop.ini", "Thumbs.db", "EHTHUMBS.DB"] {
            fs::write(dir.path().join(name), b"x").unwrap();
        }
        let stage = dir.path().join(".wincommander-scrub-stage-abc123");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("staged.jpg"), b"x").unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        assert_eq!(names(&list_run(&[root], true)), ["photo.jpg"]);
    }

    #[cfg(windows)]
    #[test]
    fn folder_walk_skips_hidden_system_files_but_keeps_merely_hidden_ones() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM,
        };
        let dir = tempfile::tempdir().unwrap();
        let set = |name: &str, attributes: u32| {
            let path = dir.path().join(name);
            fs::write(&path, b"x").unwrap();
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
            assert_ne!(unsafe { SetFileAttributesW(wide.as_ptr(), attributes) }, 0);
        };
        set("shell.dat", FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM);
        set("hidden.jpg", FILE_ATTRIBUTE_HIDDEN);
        let root = dir.path().to_string_lossy().into_owned();
        assert_eq!(names(&list_run(&[root], true)), ["hidden.jpg"]);
    }

    #[test]
    fn folder_walk_skips_version_control_and_dependency_folders() {
        let dir = tempfile::tempdir().unwrap();
        for tool in [".git", ".SVN", ".hg", ".bzr", "node_modules", "__pycache__"] {
            let nested = dir.path().join("project").join(tool).join("hooks");
            fs::create_dir_all(&nested).unwrap();
            fs::write(nested.join("pre-commit.sample"), b"#!/bin/sh").unwrap();
        }
        fs::write(dir.path().join("project").join("notes.md"), b"# notes").unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        assert_eq!(names(&list_run(&[root], true)), ["notes.md"]);
    }

    #[test]
    fn a_version_control_folder_or_file_the_user_picked_directly_is_still_listed() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir_all(git.join("info")).unwrap();
        fs::write(git.join("config"), b"[core]").unwrap();
        fs::write(git.join("info").join("exclude"), b"# exclude").unwrap();
        let picked_folder = list_run(&[git.to_string_lossy().into_owned()], true);
        assert_eq!(names(&picked_folder), ["config", "exclude"]);
        let picked_file = list_run(&[git.join("config").to_string_lossy().into_owned()], true);
        assert_eq!(names(&picked_file), ["config"]);
    }

    #[test]
    fn a_shell_file_the_user_picked_directly_is_still_listed() {
        let dir = tempfile::tempdir().unwrap();
        let picked = dir.path().join("desktop.ini");
        fs::write(&picked, b"x").unwrap();
        let listed = list_run(&[picked.to_string_lossy().into_owned()], true);
        assert_eq!(names(&listed), ["desktop.ini"]);
    }
}
