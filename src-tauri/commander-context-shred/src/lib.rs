//! Shared secure-erase implementation for WinCommander's Explorer verb and
//! its in-app caller. The caller's token is deliberately retained: selecting
//! Delete in Explorer must never grant additional access through UAC or SYSTEM.

use std::{
    fs,
    fs::OpenOptions,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

#[cfg(not(windows))]
use std::{thread, time::Duration};

#[cfg(windows)]
use std::io::{Read, Seek, SeekFrom};

use rand::{rngs::OsRng, RngCore};

#[cfg(windows)]
mod identity;

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

fn has_navigation_component(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

fn ensure_no_reparse_point_in_ancestors(path: &Path) -> Result<(), String> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if has_reparse_point(candidate)? {
            return Err("refused linked or reparse-point Explorer target".into());
        }
        current = candidate.parent();
    }
    Ok(())
}

fn resolve_existing_target_with<F>(raw: &Path, canonicalize: F) -> Result<PathBuf, String>
where
    F: FnOnce(&Path) -> io::Result<PathBuf>,
{
    match canonicalize(raw) {
        Ok(target) => Ok(target),
        Err(error) => {
            // VeraCrypt volumes can service normal file I/O while rejecting the
            // final-path query used by std::fs::canonicalize with ERROR_FILE_NOT_FOUND.
            // Keep the same traversal protection without excluding that mounted volume.
            if error.raw_os_error() != Some(2) {
                return Err(format!("cannot resolve Explorer target: {error}"));
            }
            if has_navigation_component(raw) {
                return Err("refused Explorer target containing path navigation".into());
            }
            ensure_no_reparse_point_in_ancestors(raw)?;
            fs::metadata(raw).map_err(|metadata_error| {
                format!("cannot resolve Explorer target: {metadata_error}")
            })?;
            Ok(raw.to_path_buf())
        }
    }
}

fn resolve_existing_target(raw: &Path) -> Result<PathBuf, String> {
    resolve_existing_target_with(raw, |path| fs::canonicalize(path))
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
    let target = resolve_existing_target(&raw)?;
    let metadata = fs::metadata(&target)
        .map_err(|error| format!("cannot inspect Explorer target: {error}"))?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err("refused unsupported Explorer target type".into());
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
    let mut targets: Vec<PathBuf> = Vec::with_capacity(raw_paths.len());
    for raw_path in raw_paths {
        let target = validate_target(raw_path)?;
        let target_key = normalized(&target);
        if targets.iter().any(|existing| {
            let existing_key = normalized(existing);
            target_key == existing_key || target_key.starts_with(&(existing_key + "\\"))
        }) {
            continue;
        }
        targets.retain(|existing| {
            let existing_key = normalized(existing);
            !(existing_key == target_key || existing_key.starts_with(&(target_key.clone() + "\\")))
        });
        targets.push(target);
    }
    Ok(targets)
}

#[cfg(not(windows))]
fn remove_empty_directory_with_retries(path: &Path) -> io::Result<()> {
    let mut last_error = None;
    for _ in 0..4 {
        if has_reparse_point(path).unwrap_or(true) {
            return Err(io::Error::other(
                "refused linked or reparse-point directory",
            ));
        }
        match fs::remove_dir(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        thread::sleep(Duration::from_millis(150));
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("directory removal failed")))
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
fn original_file_attributes(path: &Path) -> Result<u32, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetFileAttributesW;

    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let attributes = unsafe { GetFileAttributesW(wide_path.as_ptr()) };
    if attributes == u32::MAX {
        return Err(format!(
            "cannot inspect file attributes: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(attributes)
}

#[cfg(windows)]
fn set_file_attributes(path: &Path, attributes: u32) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;

    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe { SetFileAttributesW(wide_path.as_ptr(), attributes) } == 0 {
        return Err(format!(
            "cannot update file attributes: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn overwrite_and_delete_file_inner(path: &Path, expected: &identity::ObjectPin) -> Result<(), String> {
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
    expected.verify(&file)?;
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

#[cfg(all(windows, test))]
fn overwrite_and_delete_file(path: &Path) -> Result<(), String> {
    let _ancestors = identity::pin_ancestors(path)?;
    let expected = identity::ObjectPin::capture(path, false, false)?;
    overwrite_and_delete_expected(path, &expected)
}

#[cfg(windows)]
fn overwrite_and_delete_expected(path: &Path, expected: &identity::ObjectPin) -> Result<(), String> {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY;

    // Windows refuses a writable handle to a read-only file even when its
    // owner explicitly chose Secure Delete. Clear only that transient DOS
    // attribute; if secure erase cannot complete, restore the exact original
    // attributes so a failed operation does not silently change the file.
    let (original_attributes, was_readonly) = {
        let _name = expected.guard_name(path)?;
        let original_attributes = original_file_attributes(path)?;
        let was_readonly = original_attributes & FILE_ATTRIBUTE_READONLY != 0;
        if was_readonly {
            set_file_attributes(path, original_attributes & !FILE_ATTRIBUTE_READONLY)?;
        }
        (original_attributes, was_readonly)
    };

    let result = overwrite_and_delete_file_inner(path, expected);
    if result.is_err() && was_readonly {
        if let Ok(_name) = expected.guard_name(path) {
            let _ = set_file_attributes(path, original_attributes);
        }
    }
    result
}

#[cfg(not(windows))]
fn secure_erase_directory(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut pending = vec![root.to_path_buf()];

    // Inspect the complete tree before overwriting anything. A junction or an
    // unsupported item therefore leaves the whole requested folder intact.
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!("cannot inspect erase target '{}': {error}", path.display())
        })?;
        if has_reparse_point(&path)? {
            return Err("refused linked or reparse-point erase target".into());
        }
        if metadata.is_file() {
            files.push(path);
        } else if metadata.is_dir() {
            directories.push(path.clone());
            for child in fs::read_dir(&path)
                .map_err(|error| format!("cannot read folder '{}': {error}", path.display()))?
            {
                let child = child.map_err(|error| {
                    format!("cannot inspect folder '{}': {error}", path.display())
                })?;
                pending.push(child.path());
            }
        } else {
            return Err(format!(
                "refused unsupported erase target '{}'",
                path.display()
            ));
        }
    }

    for file in files {
        overwrite_and_delete_file(&file)?;
    }
    for directory in directories.into_iter().rev() {
        remove_empty_directory_with_retries(&directory)
            .map_err(|error| format!("cannot delete folder '{}': {error}", directory.display()))?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn secure_erase_path(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect erase target '{}': {error}", path.display()))?;
    if has_reparse_point(path)? {
        return Err("refused linked or reparse-point erase target".into());
    }
    if metadata.is_dir() {
        secure_erase_directory(path)
    } else if metadata.is_file() {
        overwrite_and_delete_file(path)
    } else {
        Err(format!(
            "refused unsupported erase target '{}'",
            path.display()
        ))
    }
}

#[cfg(not(windows))]
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
    #[cfg(windows)]
    { execute_prepared(prepare(raw_paths)?) }
    #[cfg(not(windows))]
    {
    let targets = validate_selection(&raw_paths)?;
    for initially_validated_target in targets {
        let target = validate_target(&initially_validated_target.to_string_lossy())?;
        secure_erase_target(target)?;
    }
    Ok(())
    }
}

#[cfg(windows)]
pub struct PreparedSelection {
    targets: Vec<(PathBuf, Vec<identity::ObjectPin>, identity::ObjectPin)>,
    name_guards: Vec<fs::File>,
}

#[cfg(not(windows))]
pub struct PreparedSelection { targets: Vec<PathBuf> }

#[cfg(not(windows))]
pub fn prepare(raw_paths: Vec<String>) -> Result<PreparedSelection, String> {
    Ok(PreparedSelection { targets: validate_selection(&raw_paths)? })
}

#[cfg(not(windows))]
pub fn execute_prepared(selection: PreparedSelection) -> Result<(), String> {
    for initially_validated_target in selection.targets {
        let target = validate_target(&initially_validated_target.to_string_lossy())?;
        secure_erase_target(target)?;
    }
    Ok(())
}

/// Pins approved object identities before the renderer capability is consumed.
#[cfg(windows)]
pub fn prepare(raw_paths: Vec<String>) -> Result<PreparedSelection, String> {
    let mut targets = Vec::new();
    let mut name_guards = Vec::new();
    for initially_validated_target in validate_selection(&raw_paths)? {
        let ancestors = identity::pin_ancestors(&initially_validated_target)?;
        let target = validate_target(&initially_validated_target.to_string_lossy())?;
        let directory = fs::symlink_metadata(&target).map_err(|error| error.to_string())?.is_dir();
        let pin = identity::ObjectPin::capture(&target, directory, true)?;
        if !directory { name_guards.push(pin.guard_name(&target)?); }
        targets.push((target, ancestors, pin));
    }
    Ok(PreparedSelection { targets, name_guards })
}

#[cfg(windows)]
pub fn execute_prepared(selection: PreparedSelection) -> Result<(), String> {
    let PreparedSelection { targets, name_guards } = selection;
    // The caller has consumed authority while these no-delete name guards held.
    drop(name_guards);
    for (target, _ancestors, pin) in targets {
        if pin.is_directory() {
            secure_erase_directory(&target, pin)?;
        } else {
            overwrite_and_delete_expected(&target, &pin)?;
            // Windows completes handle-based deletion after the last pin closes.
            drop(pin);
        }
        match fs::symlink_metadata(&target) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            _ => return Err("target absence could not be verified after erase".into()),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn secure_erase_directory(root: &Path, root_pin: identity::ObjectPin) -> Result<(), String> {
    let mut files = Vec::new();
    let mut directories = vec![(root.to_path_buf(), root_pin)];
    let mut cursor = 0;
    while cursor < directories.len() {
        let path = &directories[cursor].0;
        for entry in fs::read_dir(path).map_err(|error| format!("cannot inspect pinned directory: {error}"))? {
            let entry = entry.map_err(|error| error.to_string())?;
            let child = entry.path();
            let metadata = fs::symlink_metadata(&child).map_err(|error| error.to_string())?;
            if has_reparse_point(&child)? { return Err("refused reparse-point tree entry".into()); }
            if metadata.is_dir() {
                let pin = identity::ObjectPin::capture(&child, true, true)?;
                directories.push((child, pin));
            } else if metadata.is_file() {
                let pin = identity::ObjectPin::capture(&child, false, false)?;
                files.push((child, pin));
            } else { return Err("refused unsupported tree entry".into()); }
        }
        cursor += 1;
    }
    for (path, pin) in files { overwrite_and_delete_expected(&path, &pin)?; }
    for (_, pin) in directories.into_iter().rev() { pin.delete_directory()?; }
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
    fn securely_erases_mixed_file_and_folder_batches() {
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
    }

    #[test]
    fn accepts_existing_target_when_final_path_resolution_is_unavailable() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("mounted-volume.txt");
        fs::write(&file, b"sensitive").unwrap();

        let resolved =
            resolve_existing_target_with(&file, |_| Err(io::Error::from_raw_os_error(2))).unwrap();

        assert_eq!(resolved, file);
    }

    #[test]
    fn refuses_path_navigation_when_final_path_resolution_is_unavailable() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("mounted-volume.txt");
        fs::write(&file, b"sensitive").unwrap();
        let traversing_path = directory
            .path()
            .join("nested")
            .join("..")
            .join("mounted-volume.txt");

        let error = resolve_existing_target_with(&traversing_path, |_| {
            Err(io::Error::from_raw_os_error(2))
        })
        .unwrap_err();

        assert!(error.contains("path navigation"));
    }

    #[test]
    fn erases_a_folder_once_when_its_child_is_also_selected() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("erase-folder");
        let child = folder.join("nested").join("inside.txt");
        fs::create_dir_all(child.parent().unwrap()).unwrap();
        fs::write(&child, b"sensitive").unwrap();

        execute_cli(vec![
            child.to_string_lossy().into_owned(),
            folder.to_string_lossy().into_owned(),
        ])
        .unwrap();

        assert!(!folder.exists());
    }

    #[test]
    fn rejects_empty_and_oversized_selections() {
        assert!(execute_cli(Vec::new())
            .unwrap_err()
            .contains("missing Explorer target"));
        let oversized = vec!["C:\\does-not-matter".to_string(); MAX_CONTEXT_TARGETS + 1];
        assert!(execute_cli(oversized).unwrap_err().contains("larger than"));
    }

    #[cfg(windows)]
    #[test]
    fn securely_erases_a_readonly_file_selected_from_explorer() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("readonly.txt");
        fs::write(&file, b"sensitive").unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions).unwrap();

        execute_cli(vec![file.to_string_lossy().into_owned()]).unwrap();
        assert!(!file.exists());
    }
}
