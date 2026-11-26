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

fn skip_dir(name: &str) -> bool {
    name == "_scrubbed" || name.starts_with(".wincommander-pdf-check-")
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
