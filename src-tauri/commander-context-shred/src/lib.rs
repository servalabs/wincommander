//! Shared secure-erase implementation for WinCommander's Explorer verb and
//! its in-app caller. The caller's token is deliberately retained: selecting
//! Delete in Explorer must never grant additional access through UAC or SYSTEM.

use std::{
    collections::HashSet,
    fs,
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[cfg(windows)]
use std::io::{Read, Seek, SeekFrom};
#[cfg(not(windows))]
use std::{thread, time::Duration};

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
    if !fs::metadata(&target)
        .map_err(|error| format!("cannot inspect Explorer target: {error}"))?
        .is_file()
    {
        return Err(
            "folder shredding is disabled because handle-safe recursive deletion is unavailable"
                .into(),
        );
    }
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

#[cfg(not(windows))]
fn remove_with_retries(path: &Path) -> io::Result<()> {
    let mut last_error = None;
    for _ in 0..4 {
        let result = fs::remove_file(path);
        match result {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        thread::sleep(Duration::from_millis(150));
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("erase removal failed")))
}

#[cfg(not(windows))]
fn overwrite_and_delete_file(path: &Path) -> Result<(), String> {
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
    drop(file);
    remove_with_retries(path)
        .map_err(|error| format!("cannot delete file '{}': {error}", path.display()))
}

#[cfg(windows)]
fn overwrite_and_delete_file(path: &Path) -> Result<(), String> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfo, GetFileInformationByHandle, SetFileInformationByHandle,
        BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
    };

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .access_mode(0x8000_0000 | 0x4000_0000 | DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| format!("cannot open file for verified erase: {error}"))?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        return Err(format!(
            "cannot inspect opened erase target: {}",
            io::Error::last_os_error()
        ));
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || information.nNumberOfLinks != 1
    {
        return Err("refused linked, reparse-point, or hard-linked erase target".into());
    }
    let length = file
        .metadata()
        .map_err(|error| format!("cannot inspect opened file: {error}"))?
        .len();
    let mut offset = 0u64;
    let mut expected = [0u8; 64 * 1024];
    let mut actual = [0u8; 64 * 1024];
    while offset < length {
        let bytes = (length - offset).min(expected.len() as u64) as usize;
        OsRng.fill_bytes(&mut expected[..bytes]);
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| file.write_all(&expected[..bytes]))
            .and_then(|_| file.flush())
            .and_then(|_| file.seek(SeekFrom::Start(offset)))
            .and_then(|_| file.read_exact(&mut actual[..bytes]))
            .map_err(|error| format!("cannot overwrite and verify file: {error}"))?;
        if actual[..bytes] != expected[..bytes] {
            return Err("file overwrite read-back verification failed".into());
        }
        offset += bytes as u64;
    }
    file.sync_all()
        .map_err(|error| format!("cannot flush overwritten file: {error}"))?;
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            std::ptr::from_ref(&disposition).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(format!(
            "cannot delete verified erase target: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn secure_erase_path(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect erase target '{}': {error}", path.display()))?;
    if has_reparse_point(path)? {
        return Err("refused linked or reparse-point erase target".into());
    }
    if metadata.is_dir() {
        return Err(
            "folder shredding is disabled because handle-safe recursive deletion is unavailable"
                .into(),
        );
    }
    overwrite_and_delete_file(path)
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
    fn rejects_a_folder_batch_before_erasing_any_file() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("erase-file.txt");
        let folder = directory.path().join("erase-folder");
        fs::write(&file, b"sensitive").unwrap();
        fs::create_dir_all(folder.join("nested")).unwrap();
        fs::write(folder.join("nested").join("inside.txt"), b"sensitive").unwrap();

        let error = execute_cli(vec![
            file.to_string_lossy().into_owned(),
            folder.to_string_lossy().into_owned(),
        ])
        .unwrap_err();

        assert!(error.contains("folder shredding is disabled"));
        assert!(file.exists());
        assert!(folder.exists());
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
