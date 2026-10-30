// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{cache, norm_path, walker};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn display_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(network) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{network}"))
    } else {
        PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
    }
}

pub(super) fn reject_link_components(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if matches!(
            component,
            std::path::Component::Prefix(_) | std::path::Component::RootDir
        ) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| "The selected location is no longer available.")?;
        if walker::is_link(&metadata) {
            return Err("Linked or redirected locations cannot be opened from this scan. Analyze their real location instead.".into());
        }
    }
    Ok(())
}

fn scanned_path(scan_id: &str, item_id: u64) -> Result<(PathBuf, PathBuf, bool), String> {
    let guard = cache().lock().map_err(|_| "Scan cache unavailable.")?;
    let meta = guard
        .meta
        .as_ref()
        .ok_or("Analyze this location again before opening it.")?;
    if meta.scan_id != scan_id {
        return Err("This scan is no longer current. Analyze the location again.".into());
    }
    if item_id == 0 {
        return Ok((
            PathBuf::from(&meta.scan_root),
            PathBuf::from(&meta.scan_root),
            true,
        ));
    }
    let item = guard
        .children
        .values()
        .flatten()
        .find(|item| item.entry_id == item_id)
        .ok_or("This item is not part of the current scan.")?;
    Ok((
        PathBuf::from(&meta.scan_root),
        PathBuf::from(&item.full_path),
        item.is_dir,
    ))
}

fn validate_target(root: &Path, target: &Path, is_dir: bool) -> Result<PathBuf, String> {
    reject_link_components(target)?;
    let canonical_root = fs::canonicalize(root).map_err(|_| "Scan location no longer exists.")?;
    let canonical_target =
        fs::canonicalize(target).map_err(|_| "Selected item no longer exists.")?;
    let root_key = norm_path(&canonical_root);
    let target_key = norm_path(&canonical_target);
    if target_key != root_key && !target_key.starts_with(&format!("{root_key}\\")) {
        return Err("Selected item is outside the scanned location.".into());
    }
    if fs::metadata(&canonical_target)
        .map_err(|error| error.to_string())?
        .is_dir()
        != is_dir
    {
        return Err("Selected item changed. Analyze the location again.".into());
    }
    Ok(display_path(canonical_target))
}

pub async fn open_disk_item(scan_id: String, item_id: u64) -> Result<(), String> {
    let (root, target, is_dir) = scanned_path(&scan_id, item_id)?;
    let target = validate_target(&root, &target, is_dir)?;
    #[cfg(windows)]
    {
        let windows = std::env::var_os("SystemRoot").ok_or("Windows directory unavailable.")?;
        let explorer = PathBuf::from(windows).join("explorer.exe");
        let mut command = std::process::Command::new(explorer);
        if !is_dir {
            command.arg("/select,");
        }
        command
            .arg(target)
            .spawn()
            .map_err(|error| format!("Could not open Explorer: {error}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err("Open in Explorer requires Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_target_outside_scanned_scope_and_changed_kind() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(root.path().join("file"), b"test").unwrap();
        assert!(validate_target(root.path(), outside.path(), true).is_err());
        assert!(validate_target(root.path(), &root.path().join("file"), true).is_err());
        assert!(validate_target(root.path(), &root.path().join("file"), false).is_ok());
    }
    #[test]
    fn rejects_missing_or_expired_scan_ids() {
        assert!(scanned_path("not-a-current-scan", 12345).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn refuses_junction_redirects_even_inside_scope() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("real");
        fs::create_dir(&target).unwrap();
        let link = root.path().join("link");
        let output = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(validate_target(root.path(), &link, true).is_err());
        fs::remove_dir(link).unwrap();
    }
}
