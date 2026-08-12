use std::path::{Path, PathBuf};
use std::process::Command;

use clipboard_win::{
    formats::{FileList, RawData},
    Clipboard, Setter,
};

fn existing_path(raw: &str) -> Result<&Path, String> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err("Search actions require an absolute filesystem path.".to_string());
    }
    if !path.exists() {
        return Err("The selected file or folder no longer exists.".to_string());
    }
    Ok(path)
}

#[tauri::command]
pub fn search_copy_path(path: String) -> Result<(), String> {
    existing_path(&path)?;
    clipboard_win::set_clipboard_string(&path)
        .map_err(|error| format!("Failed to copy the path: {error}"))
}

#[tauri::command]
pub fn search_set_file_clipboard(path: String, cut: bool) -> Result<(), String> {
    existing_path(&path)?;

    let _clipboard = Clipboard::new_attempts(10)
        .map_err(|error| format!("Failed to open the clipboard: {error}"))?;
    clipboard_win::empty().map_err(|error| format!("Failed to clear the clipboard: {error}"))?;
    FileList
        .write_clipboard(&[path.as_str()])
        .map_err(|error| format!("Failed to copy the selected path: {error}"))?;

    // Explorer reads this registered format to distinguish Copy from Cut.
    let drop_effect = clipboard_win::register_format("Preferred DropEffect")
        .ok_or_else(|| "Failed to register the Explorer clipboard format.".to_string())?;
    let effect = if cut { 2_u32 } else { 1_u32 }.to_le_bytes();
    RawData(drop_effect.get())
        .write_clipboard(&effect)
        .map_err(|error| format!("Failed to set the clipboard operation: {error}"))
}

#[tauri::command]
pub fn search_open_containing_folder(path: String) -> Result<(), String> {
    existing_path(&path)?;
    Command::new("explorer.exe")
        .arg("/select,")
        .arg(&path)
        .spawn()
        .map_err(|error| format!("Failed to open the containing folder: {error}"))?;
    Ok(())
}

fn vscode_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local_app_data).join("Programs/Microsoft VS Code/Code.exe"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(program_files).join("Microsoft VS Code/Code.exe"));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        candidates.push(PathBuf::from(program_files_x86).join("Microsoft VS Code/Code.exe"));
    }
    candidates
}

#[tauri::command]
pub fn search_open_in_vscode(path: String) -> Result<(), String> {
    existing_path(&path)?;
    let executable = vscode_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "Visual Studio Code is not installed in a standard location.".to_string())?;
    Command::new(executable)
        .arg("--")
        .arg(path)
        .spawn()
        .map_err(|error| format!("Failed to open Visual Studio Code: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn search_delete_to_recycle_bin(path: String) -> Result<(), String> {
    let target = existing_path(&path)?;
    trash::delete(target).map_err(|error| format!("Failed to move to the Recycle Bin: {error}"))
}

/// Search has already resolved the exact result the user chose. Shred it
/// immediately, using the same canonical-path, protected-root and reparse
/// point checks as Explorer's direct WinCommander context-menu action.
/// Deliberately does not use the normal `shred-requested` event: that event is
/// the in-app confirmation path and would reopen a modal over the search UI.
#[tauri::command]
pub async fn search_shred_direct(app: tauri::AppHandle, path: String) -> Result<(), String> {
    crate::context_menu_shred::execute(app, vec![path]).await
}

fn validate_file_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("The new name cannot be empty.".to_string());
    }
    if name.contains('\\') || name.contains('/') {
        return Err("The new name cannot contain path separators.".to_string());
    }
    if name == "." || name == ".." {
        return Err("That name isn't allowed.".to_string());
    }
    const RESERVED_CHARS: [char; 8] = ['<', '>', ':', '"', '|', '?', '*', '\0'];
    if name.chars().any(|c| RESERVED_CHARS.contains(&c)) {
        return Err(
            "The new name contains characters Windows doesn't allow in file names.".to_string(),
        );
    }
    Ok(())
}

#[tauri::command]
pub fn search_rename_file(path: String, new_name: String) -> Result<String, String> {
    let target = existing_path(&path)?;
    validate_file_name(&new_name)?;
    let new_path = target
        .parent()
        .ok_or_else(|| "The selected item has no parent folder.".to_string())?
        .join(&new_name);
    if new_path.exists() {
        return Err("An item with that name already exists.".to_string());
    }
    std::fs::rename(target, &new_path).map_err(|error| format!("Failed to rename: {error}"))?;
    Ok(new_path.to_string_lossy().into_owned())
}

#[cfg(windows)]
fn show_native_properties(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb: Vec<u16> = "properties".encode_utf16().chain(Some(0)).collect();
    let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut request = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_INVOKEIDLIST,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };

    // KT: The "properties" verb must be invoked through the Shell context-menu
    // handler; Explorer /select only reveals the item and never opens Properties.
    if unsafe { ShellExecuteExW(&mut request) } == 0 {
        return Err(format!(
            "Failed to open Properties: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn show_native_properties(_path: &Path) -> Result<(), String> {
    Err("The native Properties dialog is only available on Windows.".to_string())
}

#[tauri::command]
pub fn search_show_properties(path: String) -> Result<(), String> {
    show_native_properties(existing_path(&path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_actions_reject_relative_paths() {
        assert!(existing_path("relative.txt").is_err());
    }

    #[test]
    fn search_actions_reject_missing_paths() {
        let missing = std::env::temp_dir().join("wincommander-search-action-missing");
        assert!(existing_path(&missing.to_string_lossy()).is_err());
    }

    #[test]
    fn rename_rejects_empty_or_path_like_names() {
        assert!(validate_file_name("").is_err());
        assert!(validate_file_name("   ").is_err());
        assert!(validate_file_name("sub\\name.txt").is_err());
        assert!(validate_file_name("sub/name.txt").is_err());
        assert!(validate_file_name(".").is_err());
        assert!(validate_file_name("..").is_err());
        assert!(validate_file_name("name.txt").is_ok());
    }
}
