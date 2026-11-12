// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{cache, DiskNode};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskFileType {
    pub category: String,
    pub file_count: u64,
    pub logical_size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskFileTypes {
    pub scan_id: String,
    pub scope_path: String,
    pub categories: Vec<DiskFileType>,
    pub extensions: Vec<DiskFileType>,
    pub complete: bool,
}

const CATEGORIES: [&str; 7] = [
    "images",
    "videos",
    "audio",
    "documents",
    "code",
    "archives",
    "unclassified",
];

fn category(name: &str) -> usize {
    let lower = name.to_ascii_lowercase();
    let extension = lower
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or("");
    match extension {
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "heic" | "svg"
        | "ico" | "raw" => 0,
        "mp4" | "mkv" | "mov" | "avi" | "wmv" | "webm" | "m4v" | "mpeg" | "mpg" => 1,
        "mp3" | "wav" | "flac" | "m4a" | "ogg" | "aac" | "wma" | "opus" => 2,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "rtf" | "odt"
        | "csv" | "epub" => 3,
        "rs" | "ts" | "tsx" | "js" | "jsx" | "vue" | "py" | "go" | "c" | "cpp" | "h" | "cs"
        | "java" | "kt" | "swift" | "html" | "css" | "scss" | "sql" | "sh" | "ps1" | "json"
        | "yaml" | "yml" | "toml" | "xml" => 4,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "cab" | "zst" => 5,
        _ => 6,
    }
}

fn summarize(children: &HashMap<String, Vec<DiskNode>>, path: &str) -> Vec<DiskFileType> {
    let mut totals: Vec<_> = CATEGORIES
        .iter()
        .map(|name| DiskFileType {
            category: (*name).into(),
            file_count: 0,
            logical_size: 0,
        })
        .collect();
    let mut pending = vec![path];
    while let Some(directory) = pending.pop() {
        if let Some(items) = children.get(directory) {
            for item in items {
                if item.is_dir {
                    pending.push(&item.full_path);
                } else {
                    let total = &mut totals[category(&item.name)];
                    total.file_count += 1;
                    total.logical_size = total.logical_size.saturating_add(item.size);
                }
            }
        }
    }
    totals
}

fn summarize_extensions(
    children: &HashMap<String, Vec<DiskNode>>,
    path: &str,
) -> Vec<DiskFileType> {
    let mut totals: HashMap<String, (u64, u64)> = HashMap::new();
    let mut pending = vec![path];
    while let Some(directory) = pending.pop() {
        if let Some(items) = children.get(directory) {
            for item in items {
                if item.is_dir {
                    pending.push(&item.full_path);
                    continue;
                }
                let extension = std::path::Path::new(&item.name)
                    .extension()
                    .and_then(|value| value.to_str())
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(".{}", value.to_lowercase()))
                    .unwrap_or_else(|| "No extension".to_string());
                let entry = totals.entry(extension).or_default();
                entry.0 += 1;
                entry.1 = entry.1.saturating_add(item.size);
            }
        }
    }
    let mut rows: Vec<_> = totals
        .into_iter()
        .map(|(category, (file_count, logical_size))| DiskFileType {
            category,
            file_count,
            logical_size,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.logical_size
            .cmp(&a.logical_size)
            .then_with(|| a.category.cmp(&b.category))
    });
    rows
}

pub async fn get_disk_file_types(scan_id: String, path: String) -> Result<DiskFileTypes, String> {
    tokio::task::spawn_blocking(move || {
        let guard = cache().lock().map_err(|_| "Scan cache unavailable.")?;
        let meta = guard
            .meta
            .as_ref()
            .ok_or("Analyze a drive or folder first.")?;
        if meta.scan_id != scan_id {
            return Err("This scan is no longer current. Analyze the location again.".into());
        }
        if !guard.children.contains_key(&path) {
            return Err("This folder is not part of the current scan.".into());
        }
        Ok(DiskFileTypes {
            categories: summarize(&guard.children, &path),
            extensions: summarize_extensions(&guard.children, &path),
            scan_id,
            scope_path: path,
            complete: meta.complete,
        })
    })
    .await
    .map_err(|error| format!("File statistics task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(name: &str, path: &str, size: u64, is_dir: bool) -> DiskNode {
        DiskNode {
            entry_id: 1,
            name: name.into(),
            full_path: path.into(),
            size,
            allocated: None,
            is_dir,
            last_modified: String::new(),
            file_count: 0,
            folder_count: 0,
        }
    }
    #[test]
    fn types_count_scanned_files_once_and_follow_the_requested_folder() {
        let data = HashMap::from([
            (
                "root".into(),
                vec![
                    item("nested", "root/nested", 123, true),
                    item("photo.JPG", "root/photo.JPG", 10, false),
                    item("empty.xyz", "root/empty.xyz", 0, false),
                ],
            ),
            (
                "root/nested".into(),
                vec![
                    item("main.rs", "root/nested/main.rs", 20, false),
                    item("backup.zip", "root/nested/backup.zip", 30, false),
                ],
            ),
        ]);
        let root = summarize(&data, "root");
        assert_eq!(root.iter().map(|row| row.file_count).sum::<u64>(), 4);
        assert_eq!(root.iter().map(|row| row.logical_size).sum::<u64>(), 60);
        assert_eq!((root[0].file_count, root[0].logical_size), (1, 10));
        assert_eq!((root[6].file_count, root[6].logical_size), (1, 0));
        let nested = summarize(&data, "root/nested");
        assert_eq!(nested.iter().map(|row| row.file_count).sum::<u64>(), 2);
        assert_eq!(nested.iter().map(|row| row.logical_size).sum::<u64>(), 50);
    }
    #[test]
    fn extension_totals_include_nested_and_extensionless_files() {
        let data = HashMap::from([
            (
                "root".into(),
                vec![
                    item("nested", "root/nested", 0, true),
                    item("a.JPG", "root/a.JPG", 10, false),
                    item("readme", "root/readme", 2, false),
                ],
            ),
            (
                "root/nested".into(),
                vec![item("b.jpg", "root/nested/b.jpg", 20, false)],
            ),
        ]);
        let rows = summarize_extensions(&data, "root");
        assert_eq!(
            (
                rows[0].category.as_str(),
                rows[0].file_count,
                rows[0].logical_size
            ),
            (".jpg", 2, 30)
        );
        assert_eq!(
            (
                rows[1].category.as_str(),
                rows[1].file_count,
                rows[1].logical_size
            ),
            ("No extension", 1, 2)
        );
    }
    #[test]
    fn every_supported_type_and_unclassified_file_have_an_explicit_bucket() {
        for (name, expected) in [
            ("photo.png", 0),
            ("movie.mkv", 1),
            ("song.flac", 2),
            ("notes.pdf", 3),
            ("source.tsx", 4),
            ("archive.7z", 5),
            ("unknown", 6),
            ("png", 6),
        ] {
            assert_eq!(category(name), expected);
        }
    }
}
