use super::{CachedFolder, EmptyFolder, EmptyFolderScan, MAX_FOLDERS};
use crate::empty_folder_cleaner::safety::is_link_or_reparse;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

pub(super) fn scan_roots(
    roots: &[PathBuf],
    cancelled: &AtomicBool,
) -> (EmptyFolderScan, HashMap<String, CachedFolder>) {
    let mut folders = Vec::new();
    let mut cached = HashMap::new();
    let mut scanned = 0;
    let mut truncated = false;
    for root in roots {
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            let mut has_entries = false;
            for entry in entries.flatten() {
                has_entries = true;
                let path = entry.path();
                let Ok(meta) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if !is_link_or_reparse(&meta) && meta.is_dir() {
                    if scanned >= MAX_FOLDERS {
                        truncated = true;
                        break;
                    }
                    scanned += 1;
                    stack.push(path);
                }
            }
            if !has_entries && dir != *root {
                let id = Uuid::new_v4().to_string();
                folders.push(EmptyFolder {
                    id: id.clone(),
                    name: dir
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: dir.to_string_lossy().into_owned(),
                });
                cached.insert(
                    id,
                    CachedFolder {
                        path: dir,
                        root: root.clone(),
                    },
                );
            }
            if truncated {
                break;
            }
        }
        if cancelled.load(Ordering::Acquire) || truncated {
            break;
        }
    }
    folders.sort_by(|left, right| left.path.cmp(&right.path));
    (
        EmptyFolderScan {
            folders,
            scanned_folders: scanned,
            cancelled: cancelled.load(Ordering::Acquire),
            truncated,
        },
        cached,
    )
}
