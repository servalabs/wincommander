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
}
