//! Explorer's secure-delete verb is intentionally separate from the in-app
//! shredder. Explorer passes the selected path on the process command line,
//! so this module validates and executes it before it can reach the webview.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

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

/// Runs only for the Explorer `--context-shred` verb. No path is ever sent to
/// the frontend: every target is re-resolved immediately before the existing
/// secure-erase backend command runs.
pub(crate) async fn execute(app: AppHandle, raw_paths: Vec<String>) {
    if raw_paths.len() != 1 {
        crate::log_message_src(
            "warn",
            "core",
            "[ContextShred] refused unexpected target count",
        );
        return;
    }
    let target = match validate_target(&raw_paths[0]) {
        Ok(target) => target,
        Err(error) => {
            crate::log_message_src("warn", "core", &format!("[ContextShred] {error}"));
            return;
        }
    };
    let mut params = HashMap::new();
    params.insert("Path".to_string(), target.to_string_lossy().into_owned());
    params.insert("Type".to_string(), "File".to_string());
    match crate::backend::run_backend_script(app, "Invoke-7Erase".to_string(), params).await {
        Ok(_) => crate::log_message_src("info", "core", "[ContextShred] completed"),
        Err(error) => {
            crate::log_message_src("error", "core", &format!("[ContextShred] failed: {error}"))
        }
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
            Err(error) if error.kind() == ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to create test symlink: {error}"),
        }

        assert!(validate_target(link.to_str().unwrap())
            .unwrap_err()
            .contains("linked or reparse-point"));
    }
}
