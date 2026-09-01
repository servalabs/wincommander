//! Explorer's secure-delete verb is intentionally separate from the in-app
//! shredder. Explorer passes the selected path on the process command line,
//! so this module validates and executes it before it can reach the webview.

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
use tauri::AppHandle;

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

/// Explorer's legacy `Player` verb model accepts up to 100 selected items.
/// Keep the same bound here so a malformed direct invocation cannot turn one
/// context-menu action into an unbounded destructive batch.
const MAX_CONTEXT_TARGETS: usize = 100;

/// Validate the complete Explorer selection before starting any erase. A
/// malformed or protected item rejects the batch before destruction begins.
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

/// Explorer launches must not depend on PowerShell. Apart from making a
/// context-menu action visibly flash a console in local/debug builds, the
/// PowerShell implementation renamed a folder before its last delete fallback.
/// A failed fallback therefore left a GUID-named directory behind, which is not
/// an acceptable outcome for a command labelled "Delete".
///
/// This direct path is deliberately small and self-contained: overwrite every
/// regular file with OS-random bytes, flush it, then remove it. Directories are
/// traversed without following reparse points and are removed only after all
/// children have been erased. Every final remove is verified before success is
/// returned to Explorer's background invocation.
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

fn make_writable(path: &Path) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
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

    // A junction/symlink must never be traversed. Removing the link itself is
    // safe; following it could erase a location outside the user selection.
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

/// Synchronous, GUI-free Explorer entry point. This intentionally runs before
/// the Tauri single-instance guard: a context-menu process must not take over,
/// exit, or otherwise disturb an already-running local/dev WinCommander.
pub(crate) fn execute_cli(raw_paths: Vec<String>) -> Result<(), String> {
    let targets = validate_selection(&raw_paths)?;
    for initially_validated_target in targets {
        // Re-resolve immediately before destruction: Explorer can leave a
        // menu open while another process replaces a selected item.
        let target = validate_target(&initially_validated_target.to_string_lossy())?;
        let target_display = target.to_string_lossy().into_owned();
        secure_erase_target(target)?;
        crate::log_message_src(
            "info",
            "core",
            &format!("[ContextShred] securely erased {target_display}"),
        );
    }
    Ok(())
}

/// In-app callers (currently the search-results context menu) use the same
/// secure implementation, off Tauri's async runtime thread.
pub(crate) async fn execute(_app: AppHandle, raw_paths: Vec<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || execute_cli(raw_paths))
        .await
        .map_err(|error| format!("secure erase worker failed: {error}"))?
}

/// The direct-shell launch paths have no webview to return an error to, so
/// retain their explicit audit logging while the in-app search command can
/// return its error to the caller.
pub(crate) fn log_result(result: Result<(), String>) {
    match result {
        Ok(()) => crate::log_message_src("info", "core", "[ContextShred] completed"),
        Err(error) => crate::log_message_src("warn", "core", &format!("[ContextShred] {error}")),
    }
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
    fn rejects_files_inside_system_locations() {
        let roots = vec![
            PathBuf::from("C:\\Windows"),
            PathBuf::from("C:\\Program Files"),
        ];
        assert!(is_descendant_of_system_root(
            Path::new("C:\\Windows\\System32\\kernel32.dll"),
            &roots
        ));
        assert!(!is_descendant_of_system_root(
            Path::new("C:\\Users\\Ada\\note.txt"),
            &roots
        ));
    }

    #[test]
    fn accepts_an_existing_regular_file_after_revalidation() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("context-shred-target.txt");
        fs::write(&target, b"test target").unwrap();

        assert_eq!(
            validate_target(target.to_str().unwrap()).unwrap(),
            fs::canonicalize(target).unwrap()
        );
    }

    #[test]
    fn accepts_a_mixed_file_and_folder_selection() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("context-shred-file.txt");
        let folder = directory.path().join("context-shred-folder");
        fs::write(&file, b"test target").unwrap();
        fs::create_dir(&folder).unwrap();

        let targets = validate_selection(&[
            file.to_string_lossy().into_owned(),
            folder.to_string_lossy().into_owned(),
        ])
        .unwrap();

        assert_eq!(
            targets,
            vec![
                fs::canonicalize(file).unwrap(),
                fs::canonicalize(folder).unwrap()
            ]
        );
    }

    #[test]
    fn rejects_an_empty_or_oversized_selection() {
        assert!(validate_selection(&[])
            .unwrap_err()
            .contains("missing Explorer target"));

        let oversized = vec!["C:\\does-not-matter".to_string(); MAX_CONTEXT_TARGETS + 1];
        assert!(validate_selection(&oversized)
            .unwrap_err()
            .contains("larger than"));
    }

    #[test]
    fn securely_erases_a_file_instead_of_only_renaming_it() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("erase-me.txt");
        fs::write(&target, b"sensitive test data").unwrap();

        secure_erase_target(target.clone()).unwrap();

        assert!(!target.exists());
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    #[test]
    fn securely_erases_nested_folder_contents_and_the_folder() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("erase-me");
        let nested = target.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("a.txt"), b"a").unwrap();
        fs::write(target.join("b.txt"), b"b").unwrap();

        secure_erase_target(target.clone()).unwrap();

        assert!(!target.exists());
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    #[test]
    fn explorer_shred_is_handled_before_single_instance_forwarding() {
        let app_startup = include_str!("lib.rs");
        let direct_runner = app_startup
            .find("context_menu_shred::execute_cli(paths)")
            .expect("Explorer shred must have a standalone runner");
        let instance_guard = app_startup
            .find("session_instance::acquire(&cli_args)")
            .expect("single-instance guard must remain present");

        assert!(direct_runner < instance_guard);
        assert!(!app_startup.contains("app_handle.exit(0);"));
    }

    #[test]
    fn direct_shred_does_not_route_back_to_powershell() {
        let source = include_str!("context_menu_shred.rs");
        let start = source
            .find("pub(crate) fn execute_cli")
            .expect("standalone Explorer runner");
        let end = source[start..]
            .find("/// In-app callers")
            .map(|offset| start + offset)
            .expect("async in-app wrapper");
        let direct_runner = &source[start..end];

        assert!(!direct_runner.contains("run_backend_script"));
        assert!(!direct_runner.contains("Invoke-7Erase"));
    }

    #[cfg(windows)]
    #[test]
    fn rejects_a_symlink_target_before_secure_erase() {
        use std::io::ErrorKind;
        use std::os::windows::fs::symlink_file;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target.txt");
        let link = directory.path().join("linked-target.txt");
        fs::write(&target, b"test target").unwrap();

        // Windows can deny symlink creation unless Developer Mode or the
        // relevant privilege is enabled. The production guard is still
        // covered above; skip only that OS capability-dependent fixture.
        match symlink_file(&target, &link) {
            Ok(()) => {}
            Err(error)
                if error.kind() == ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314) =>
            {
                return;
            }
            Err(error) => panic!("failed to create test symlink: {error}"),
        }

        assert!(validate_target(link.to_str().unwrap())
            .unwrap_err()
            .contains("linked or reparse-point"));
    }
}
