//! Shared secure-erase implementation for WinCommander's Explorer verb and
//! its in-app caller. The caller's token is deliberately retained: selecting
//! Delete in Explorer must never grant additional access through UAC or SYSTEM.

use std::{
    collections::HashSet,
    fs,
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use rand::{rngs::OsRng, RngCore};

fn normalized(path: &Path) -> String {
    let value = path.to_string_lossy().replace('/', "\\");
    value
        .strip_prefix("\\\\?\\")
        .unwrap_or(&value)
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn protected_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for variable in [
        "SystemRoot",
        "windir",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "PUBLIC",
        "USERPROFILE",
    ] {
        if let Ok(value) = std::env::var(variable) {
            if !value.is_empty() {
                roots.push(PathBuf::from(value));
            }
        }
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    roots
}

fn system_roots() -> Vec<PathBuf> {
    let mut roots = protected_roots();
    roots.retain(|root| {
        let value = normalized(root);
        !std::env::var("USERPROFILE")
            .ok()
            .is_some_and(|profile| value == normalized(Path::new(&profile)))
            && !std::env::var("PUBLIC")
                .ok()
                .is_some_and(|public| value == normalized(Path::new(&public)))
    });
    roots
}

fn is_target_or_ancestor_of_protected_root(target: &Path, roots: &[PathBuf]) -> bool {
    if target.parent().is_none() {
        return true;
    }
    let target = normalized(target);
    let target_prefix = format!("{target}\\");
    roots.iter().any(|root| {
        let root = fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        let root = normalized(&root);
        !root.is_empty() && (target == root || root.starts_with(&target_prefix))
    })
}

fn is_descendant_of_system_root(target: &Path, roots: &[PathBuf]) -> bool {
    let target = normalized(target);
    roots.iter().any(|root| {
        let root = fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        let root = normalized(&root);
        !root.is_empty() && target.starts_with(&format!("{root}\\"))
    })
}

fn has_reparse_point(path: &Path) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

fn validate_target(raw_path: &str) -> Result<PathBuf, String> {
    if raw_path.is_empty() || raw_path.starts_with('-') {
        return Err("refused missing or malformed Explorer target".into());
    }
    let raw = PathBuf::from(raw_path);
    if !raw.is_absolute() {
        return Err("refused non-absolute Explorer target".into());
    }
    if has_reparse_point(&raw)? {
        return Err("refused linked or reparse-point Explorer target".into());
    }
    let target = fs::canonicalize(&raw)
        .map_err(|error| format!("cannot resolve Explorer target: {error}"))?;
    if is_target_or_ancestor_of_protected_root(&target, &protected_roots())
        || is_descendant_of_system_root(&target, &system_roots())
    {
        return Err("refused protected Explorer target".into());
    }
    Ok(target)
}

/// Bounds an Explorer `Player` verb to a deliberate, finite selection.
pub const MAX_CONTEXT_TARGETS: usize = 100;

fn validate_selection(raw_paths: &[String]) -> Result<Vec<PathBuf>, String> {
    if raw_paths.is_empty() {
        return Err("refused missing Explorer target".into());
    }
    if raw_paths.len() > MAX_CONTEXT_TARGETS {
        return Err(format!(
            "refused Explorer selection larger than {MAX_CONTEXT_TARGETS} items"
        ));
    }
    let mut seen = HashSet::new();
    let mut targets = Vec::with_capacity(raw_paths.len());
    for raw_path in raw_paths {
        let target = validate_target(raw_path)?;
        if seen.insert(normalized(&target)) {
            targets.push(target);
        }
    }
    Ok(targets)
}

fn remove_with_retries(path: &Path, directory: bool) -> io::Result<()> {
    let mut last_error = None;
    for _ in 0..4 {
        let result = if directory {
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        };
        match result {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        thread::sleep(Duration::from_millis(150));
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("erase removal failed")))
}

#[cfg(windows)]
#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly(permissions: &mut fs::Permissions) {
    // This helper ships only for Windows Explorer, where readonly is a file attribute.
    permissions.set_readonly(false);
}

#[cfg(unix)]
fn clear_readonly(permissions: &mut fs::Permissions) {
    use std::os::unix::fs::PermissionsExt;

    permissions.set_mode(permissions.mode() | 0o200);
}

fn make_writable(path: &Path) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        clear_readonly(&mut permissions);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn overwrite_file(path: &Path) -> Result<(), String> {
    make_writable(path).map_err(|error| format!("cannot make file writable: {error}"))?;
    let length = fs::metadata(path)
        .map_err(|error| format!("cannot inspect file: {error}"))?
        .len();
    if length == 0 {
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("cannot open file for overwrite: {error}"))?;
    let mut remaining = length;
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        let bytes = remaining.min(buffer.len() as u64) as usize;
        OsRng.fill_bytes(&mut buffer[..bytes]);
        file.write_all(&buffer[..bytes])
            .map_err(|error| format!("cannot overwrite file: {error}"))?;
        remaining -= bytes as u64;
    }
    file.sync_all()
        .map_err(|error| format!("cannot flush overwritten file: {error}"))?;
    Ok(())
}

fn secure_erase_path(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect erase target '{}': {error}", path.display()))?;
    if has_reparse_point(path)? {
        return remove_with_retries(path, metadata.is_dir())
            .map_err(|error| format!("cannot remove linked target '{}': {error}", path.display()));
    }
    if metadata.is_dir() {
        let entries = fs::read_dir(path)
            .map_err(|error| format!("cannot enumerate folder '{}': {error}", path.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("cannot read folder entry: {error}"))?;
            secure_erase_path(&entry.path())?;
        }
        make_writable(path).map_err(|error| format!("cannot make folder writable: {error}"))?;
        return remove_with_retries(path, true)
            .map_err(|error| format!("cannot delete folder '{}': {error}", path.display()));
    }
    overwrite_file(path)?;
    remove_with_retries(path, false)
        .map_err(|error| format!("cannot delete file '{}': {error}", path.display()))
}

fn secure_erase_target(target: PathBuf) -> Result<(), String> {
    secure_erase_path(&target)?;
    match fs::symlink_metadata(&target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(format!(
            "target still exists after secure erase: {}",
            target.display()
        )),
        Err(error) => Err(format!(
            "cannot verify secure erase for '{}': {error}",
            target.display()
        )),
    }
}

/// Executes a complete Explorer selection without UI, PowerShell, elevation,
/// or a service hand-off. Every target is validated before any erase begins.
pub fn execute_cli(raw_paths: Vec<String>) -> Result<(), String> {
    let targets = validate_selection(&raw_paths)?;
    for initially_validated_target in targets {
        let target = validate_target(&initially_validated_target.to_string_lossy())?;
        secure_erase_target(target)?;
    }
    Ok(())
}

/// The helper has no UI. Keep a durable diagnostic for an Explorer action that
/// fails (for example, a file open in another process) without showing a pop-up.
pub fn log_result(error: &str) {
    let directory = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("WinCommander");
    let _ = fs::create_dir_all(&directory);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("context-shred.log"))
        .and_then(|mut file| writeln!(file, "[ContextShred] {error}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_protected_roots_and_their_ancestors() {
        let roots = vec![
            PathBuf::from("C:\\Windows"),
            PathBuf::from("C:\\Program Files"),
        ];
        assert!(is_target_or_ancestor_of_protected_root(
            Path::new("C:\\"),
            &roots
        ));
        assert!(is_target_or_ancestor_of_protected_root(
            Path::new("C:\\Windows"),
            &roots
        ));
        assert!(!is_target_or_ancestor_of_protected_root(
            Path::new("C:\\Users\\Ada\\note.txt"),
            &roots
        ));
    }

    #[test]
    fn erases_a_mixed_file_and_folder_batch_without_partial_rename() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("erase-file.txt");
        let folder = directory.path().join("erase-folder");
        fs::write(&file, b"sensitive").unwrap();
        fs::create_dir_all(folder.join("nested")).unwrap();
        fs::write(folder.join("nested").join("inside.txt"), b"sensitive").unwrap();

        execute_cli(vec![
            file.to_string_lossy().into_owned(),
            folder.to_string_lossy().into_owned(),
        ])
        .unwrap();

        assert!(!file.exists());
        assert!(!folder.exists());
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    #[test]
    fn rejects_empty_and_oversized_selections() {
        assert!(execute_cli(Vec::new())
            .unwrap_err()
            .contains("missing Explorer target"));
        let oversized = vec!["C:\\does-not-matter".to_string(); MAX_CONTEXT_TARGETS + 1];
        assert!(execute_cli(oversized).unwrap_err().contains("larger than"));
    }
}
