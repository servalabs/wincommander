use super::{CachedFile, DuplicateFile, DuplicateGroup, DuplicateScan, MAX_FILES};
use crate::duplicate_finder::safety::is_link_or_reparse;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

pub(super) fn scan_roots(
    roots: &[PathBuf],
    cancelled: &AtomicBool,
) -> (
    DuplicateScan,
    HashMap<String, CachedFile>,
    HashMap<String, Vec<String>>,
) {
    let mut by_size: HashMap<u64, Vec<(PathBuf, PathBuf)>> = HashMap::new();
    let mut scanned = 0;
    let mut truncated = false;
    for root in roots {
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if scanned >= MAX_FILES {
                    truncated = true;
                    break;
                }
                let path = entry.path();
                let Ok(meta) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if is_link_or_reparse(&meta) {
                    continue;
                }
                if meta.is_dir() {
                    stack.push(path);
                } else if meta.is_file() {
                    scanned += 1;
                    by_size
                        .entry(meta.len())
                        .or_default()
                        .push((path, root.clone()));
                }
            }
            if truncated {
                break;
            }
        }
        if cancelled.load(Ordering::Acquire) || truncated {
            break;
        }
    }
    let mut public = Vec::new();
    let mut cached = HashMap::new();
    let mut groups = HashMap::new();
    for (size, files) in by_size.into_iter().filter(|(_, files)| files.len() > 1) {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let mut by_hash: HashMap<[u8; 32], Vec<(PathBuf, PathBuf)>> = HashMap::new();
        for (path, root) in files {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            if let Ok(hash) = hash_file(&path, cancelled) {
                by_hash.entry(hash).or_default().push((path, root));
            }
        }
        for identical in by_hash.into_values().filter(|files| files.len() > 1) {
            let group_id = Uuid::new_v4().to_string();
            let mut ids = Vec::new();
            let mut display = Vec::new();
            for (path, root) in identical {
                let id = Uuid::new_v4().to_string();
                let modified = fs::metadata(&path)
                    .ok()
                    .and_then(|metadata| metadata.modified().ok());
                ids.push(id.clone());
                display.push(DuplicateFile {
                    id: id.clone(),
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: path.to_string_lossy().into_owned(),
                    size,
                });
                cached.insert(
                    id,
                    CachedFile {
                        path,
                        root,
                        group_id: group_id.clone(),
                        bytes: size,
                        modified,
                    },
                );
            }
            groups.insert(group_id.clone(), ids);
            public.push(DuplicateGroup {
                id: group_id,
                size,
                reclaimable_bytes: size.saturating_mul((display.len() as u64).saturating_sub(1)),
                files: display,
            });
        }
    }
    public.sort_by_key(|group| std::cmp::Reverse(group.reclaimable_bytes));
    (
        DuplicateScan {
            groups: public,
            scanned_files: scanned,
            cancelled: cancelled.load(Ordering::Acquire),
            truncated,
        },
        cached,
        groups,
    )
}

fn hash_file(path: &Path, cancelled: &AtomicBool) -> Result<[u8; 32], String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 128];
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("cancelled".into());
        }
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            return Ok(hasher.finalize().into());
        }
        hasher.update(&buffer[..read]);
    }
}
