// SPDX-License-Identifier: AGPL-3.0-or-later
use super::rules::{validate_recursive_containment, ScanTarget, TargetOperation};
use super::scanner::is_reparse_point;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 16;

pub(super) fn discover(
    root: &Path,
    cancelled: &AtomicBool,
) -> Result<(Vec<ScanTarget>, usize), String> {
    let canonical = fs::canonicalize(root).map_err(|error| error.to_string())?;
    if !validate_recursive_containment(root, &canonical) {
        return Err("Project folder must not cross a link or reparse point.".into());
    }
    let mut targets = Vec::new();
    let mut skipped = 0;
    let mut visited = 0;
    let mut stack = vec![(canonical.clone(), 0)];
    while let Some((directory, depth)) = stack.pop() {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let metadata = match fs::symlink_metadata(&directory) {
            Ok(metadata) if !metadata.file_type().is_symlink() && !is_reparse_point(&metadata) => {
                metadata
            }
            _ => {
                skipped += 1;
                continue;
            }
        };
        if !metadata.is_dir() {
            continue;
        }
        let resolved = match fs::canonicalize(&directory) {
            Ok(path) if path.starts_with(&canonical) => path,
            _ => {
                skipped += 1;
                continue;
            }
        };
        let candidates = artifact_paths(&resolved);
        for (relative, label) in &candidates {
            let path = resolved.join(relative);
            if path.is_dir() {
                let Ok(canonical_target) = fs::canonicalize(&path) else {
                    skipped += 1;
                    continue;
                };
                if !validate_recursive_containment(&path, &canonical_target) {
                    skipped += 1;
                    continue;
                }
                targets.push(ScanTarget {
                    category: "projects".into(),
                    label: (*label).into(),
                    description: None,
                    path,
                    operation: TargetOperation::Delete,
                    recommended: false,
                    minimum_age: Duration::from_secs(24 * 60 * 60),
                    containment_root: Some(canonical.clone()),
                    containment_source: Some(root.to_path_buf()),
                });
            }
        }
        let entries = match fs::read_dir(&resolved) {
            Ok(entries) => entries,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        for entry in entries {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            visited += 1;
            if visited > MAX_ENTRIES {
                return Err(
                    "Project discovery reached its entry limit. Choose a smaller project folder."
                        .into(),
                );
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            // Dependencies, VCS internals and artifact trees are not additional projects.
            if matches!(
                name.as_str(),
                ".git"
                    | ".hg"
                    | ".svn"
                    | "node_modules"
                    | "target"
                    | ".venv"
                    | "venv"
                    | ".next"
                    | "__pycache__"
            ) {
                continue;
            }
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
                skipped += 1;
                continue;
            }
            if metadata.is_dir() {
                if depth >= MAX_DEPTH {
                    return Err("Project discovery reached its depth limit. Choose a smaller project folder.".into());
                }
                stack.push((entry.path(), depth + 1));
            }
        }
    }
    Ok((targets, skipped))
}

fn artifact_paths(project: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut candidates = Vec::new();
    if regular_marker(&project.join("package.json")) {
        candidates.push((PathBuf::from(".next/cache"), "Next.js project cache"));
        candidates.push((
            PathBuf::from("node_modules/.cache"),
            "JavaScript project build cache",
        ));
    }
    if regular_marker(&project.join("Cargo.toml"))
        && valid_cargo_tag(&project.join("target/CACHEDIR.TAG"))
    {
        candidates.push((PathBuf::from("target"), "Cargo project build artifacts"));
    }
    if ["pyproject.toml", "setup.py", "setup.cfg"]
        .iter()
        .any(|name| regular_marker(&project.join(name)))
    {
        candidates.push((PathBuf::from(".pytest_cache"), "Python test cache"));
        candidates.push((PathBuf::from(".mypy_cache"), "Python type-check cache"));
        candidates.push((PathBuf::from(".ruff_cache"), "Python lint cache"));
    }
    candidates
}

fn regular_marker(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_file() && !metadata.file_type().is_symlink() && !is_reparse_point(&metadata)
    })
}

fn valid_cargo_tag(path: &Path) -> bool {
    if !regular_marker(path) {
        return false;
    }
    use std::io::Read;
    let mut header = [0; 43];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && &header == b"Signature: 8a477f597d28d172789f06886806bc55"
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
