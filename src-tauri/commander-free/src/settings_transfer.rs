// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native, user-selected settings backup I/O. No renderer-supplied paths or bytes.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri_plugin_dialog::DialogExt;

const MAX_IMPORT_BYTES: u64 = 4 * 1024 * 1024;
static TRANSFER_ACTIVE: AtomicBool = AtomicBool::new(false);

struct TransferGuard;
impl TransferGuard {
    fn begin() -> Result<Self, String> {
        if crate::cli::tauri_runtime_active() {
            return Err("Settings file selection requires an interactive window".into());
        }
        TRANSFER_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "A settings file dialog is already open".to_string())?;
        Ok(Self)
    }
}
impl Drop for TransferGuard {
    fn drop(&mut self) {
        TRANSFER_ACTIVE.store(false, Ordering::Release);
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsImportFile {
    file_name: String,
    json: String,
}

fn require_json_file(path: &Path) -> Result<(), String> {
    if path
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("json"))
    {
        Ok(())
    } else {
        Err("Choose a .json settings file".into())
    }
}

fn read_import(reader: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_IMPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Settings file could not be read".to_string())?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err("Settings file exceeds the 4 MiB limit".into());
    }
    let json =
        String::from_utf8(bytes).map_err(|_| "Settings file must be UTF-8 JSON".to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&json).map_err(|_| "Settings file is not valid JSON".to_string())?;
    if !value.is_object() {
        return Err("Settings file must contain a JSON object".into());
    }
    Ok(json)
}

async fn select_file(app: &tauri::AppHandle, save: bool) -> Result<Option<PathBuf>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let picker = app
        .dialog()
        .file()
        .add_filter("WinCommander settings", &["json"]);
    if save {
        picker
            .set_file_name("wincommander-settings.json")
            .save_file(move |path| {
                let _ = tx.send(path);
            });
    } else {
        picker.pick_file(move |path| {
            let _ = tx.send(path);
        });
    }
    rx.await
        .map_err(|_| "Settings file dialog closed unexpectedly".to_string())?
        .map(|path| {
            path.into_path()
                .map_err(|_| "Choose a filesystem path".to_string())
        })
        .transpose()
}

#[tauri::command]
pub async fn write_settings_export_file(app: tauri::AppHandle) -> Result<bool, String> {
    let _guard = TransferGuard::begin()?;
    let Some(path) = select_file(&app, true).await? else {
        return Ok(false);
    };
    require_json_file(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let json = crate::settings::export_settings_cmd()?;
        std::fs::write(path, json).map_err(|_| "Settings export could not be saved".to_string())?;
        Ok(true)
    })
    .await
    .map_err(|_| "Settings export task failed".to_string())?
}

#[tauri::command]
pub async fn read_settings_import_file(
    app: tauri::AppHandle,
) -> Result<Option<SettingsImportFile>, String> {
    let _guard = TransferGuard::begin()?;
    let Some(path) = select_file(&app, false).await? else {
        return Ok(None);
    };
    require_json_file(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let file = std::fs::File::open(&path)
            .map_err(|_| "Settings file could not be opened".to_string())?;
        if !file
            .metadata()
            .map_err(|_| "Settings file could not be inspected".to_string())?
            .is_file()
        {
            return Err("Choose a regular settings file".into());
        }
        let json = read_import(file)?;
        Ok(Some(SettingsImportFile {
            file_name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            json,
        }))
    })
    .await
    .map_err(|_| "Settings import task failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_json_settings_and_preserves_bytes() {
        let json = "{\"settingsVersion\": 2}";
        assert_eq!(read_import(json.as_bytes()).unwrap(), json);
    }

    #[test]
    fn rejects_non_json_non_object_and_invalid_utf8() {
        for input in [b"not json".as_slice(), b"[]", b"null", &[0xff]] {
            assert!(read_import(input).is_err());
        }
    }

    #[test]
    fn rejects_oversized_input_without_unbounded_reading() {
        let error = read_import(std::io::repeat(b' ')).unwrap_err();
        assert!(error.contains("4 MiB"));
    }

    #[test]
    fn rejects_non_json_extensions_and_alternate_data_streams() {
        assert!(require_json_file(Path::new("backup.JSON")).is_ok());
        for name in [
            "script.ps1",
            "backup.json.exe",
            "backup",
            "backup.json:stream",
        ] {
            assert!(require_json_file(Path::new(name)).is_err());
        }
    }
}
