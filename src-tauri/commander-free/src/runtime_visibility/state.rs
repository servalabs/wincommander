// Persistent state manifest for runtime-visibility hides.
//
// Stored at $APPDATA/WinCommander/runtime_visibility/state.json. Writes are
// always full-rewrite via a temp-file + rename to make crashes during the
// write atomic. The state is small (one entry per hidden runtime); we don't
// need a database.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use super::registry::{RunValue, UninstallEntry};

/// One snapshot of a hide action — written BEFORE the mutation lands so a
/// crash can be recovered from. `applied` flips to true once every step
/// succeeded. If we crash mid-way `restore_all_runtimes` reverses any step
/// the snapshot says we attempted.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HideEntry {
    /// Stable user-visible key — usually the exe basename like "syncthing.exe".
    /// All entries that share the same key are restored together.
    pub key: String,
    pub hidden_at_unix_ms: i64,
    pub applied: bool,
    pub run_value_renames: Vec<RunRename>,
    pub uninstall_hides: Vec<UninstallHide>,
    #[serde(default)]
    pub shortcut_backups: Vec<ShortcutBackup>,
    #[serde(default)]
    pub app_path_backups: Vec<AppPathBackup>,
    #[serde(default)]
    pub scheduled_task_backups: Vec<ScheduledTaskBackup>,
    #[serde(default)]
    pub killed_processes: Vec<KilledProcess>,
}

fn default_hkcu() -> String {
    "HKCU".to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RunRename {
    #[serde(default = "default_hkcu")]
    pub hive: String,
    pub subkey: String,
    pub original_name: String,
    pub renamed_to: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UninstallHide {
    #[serde(default = "default_hkcu")]
    pub hive: String,
    pub subkey: String,
    /// What SystemComponent was before we touched it (None = value didn't exist).
    pub previous_value: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBackup {
    pub original_path: String,
    pub backup_path: String,
}

/// Backup of an HKLM App Paths key entry. Deleted on hide so the app
/// vanishes from Windows Search "Run commands"; recreated on restore.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppPathBackup {
    pub exe_name: String,
    pub default_value: Option<String>, // (Default) — the full path to the exe
    pub path_value: Option<String>,    // "Path" value, optional
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskBackup {
    pub task_name: String,
    pub was_enabled: bool,
}

/// A process we killed during hide so its tray icon disappears immediately.
/// `restart_cmd` holds enough info to re-launch on restore.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KilledProcess {
    pub exe_name: String,
    pub restart_cmd: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct VisibilityState {
    pub version: u32,
    pub entries: Vec<HideEntry>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StateView {
    pub state: VisibilityState,
    pub state_path: String,
}

static MUTEX: Mutex<()> = Mutex::new(());

fn state_dir() -> Result<PathBuf, String> {
    // Machine-wide (%ProgramData%) so the hide manifest is shared across all
    // Windows accounts, consistent with the per-machine settings/license.
    let dir = crate::paths::machine_data_dir()?.join("runtime_visibility");
    fs::create_dir_all(&dir).map_err(|e| format!("create_dir_all: {}", e))?;
    Ok(dir)
}

fn state_file() -> Result<PathBuf, String> {
    Ok(state_dir()?.join("state.json"))
}

pub fn load() -> Result<VisibilityState, String> {
    let path = state_file()?;
    if !path.exists() {
        return Ok(VisibilityState {
            version: 1,
            entries: Vec::new(),
        });
    }
    let bytes = fs::read(&path).map_err(|e| format!("read {:?}: {}", path, e))?;
    let parsed: VisibilityState =
        serde_json::from_slice(&bytes).map_err(|e| format!("parse state.json: {}", e))?;
    Ok(parsed)
}

fn save_locked(state: &VisibilityState) -> Result<(), String> {
    // KT: caller must already hold MUTEX — this does the raw file write only.
    let path = state_file()?;
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("create tmp: {}", e))?;
        f.write_all(&bytes)
            .map_err(|e| format!("write tmp: {}", e))?;
        f.sync_all().map_err(|e| format!("sync tmp: {}", e))?;
    }
    // Windows doesn't atomically replace via rename if the dest exists, so
    // remove first. We hold the mutex throughout, so no one else races us.
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    fs::rename(&tmp, &path).map_err(|e| format!("rename tmp → state: {}", e))?;
    Ok(())
}

pub fn upsert(entry: HideEntry) -> Result<VisibilityState, String> {
    // KT: hold the mutex across the full load+modify+save so a concurrent
    // watcher re-hide can't interleave and drop shortcut_backups/killed_processes
    // written by a parallel hide step.
    let _g = MUTEX.lock().map_err(|e| e.to_string())?;
    let mut state = load()?;
    state.version = 1;
    if let Some(existing) = state.entries.iter_mut().find(|e| e.key == entry.key) {
        *existing = entry;
    } else {
        state.entries.push(entry);
    }
    save_locked(&state)?;
    Ok(state)
}

pub fn remove(key: &str) -> Result<VisibilityState, String> {
    // KT: hold mutex across full load+modify+save — same race as upsert.
    let _g = MUTEX.lock().map_err(|e| e.to_string())?;
    let mut state = load()?;
    state.entries.retain(|e| e.key != key);
    save_locked(&state)?;
    Ok(state)
}

pub fn matches_run_value(rv: &RunValue, key: &str) -> bool {
    // Match if the executable in the command line has the same basename as
    // the key (typically "syncthing.exe"), OR if the value name itself
    // contains the key minus the .exe suffix. Both are common in the wild.
    // Both HKCU and HKLM entries are matched — the app runs as admin so
    // HKLM writes are permitted.
    let key_lower = key.to_lowercase();
    let key_stem = key_lower.trim_end_matches(".exe");
    if let Some(exe) = super::scanner::extract_exe_basename(&rv.command) {
        let exe_lower = exe.to_lowercase();
        if exe_lower == key_lower || exe_lower.trim_end_matches(".exe") == key_stem {
            return true;
        }
    }
    if rv.name.to_lowercase().contains(key_stem) {
        return true;
    }
    false
}

pub fn matches_uninstall(entry: &UninstallEntry, key: &str) -> bool {
    // Both HKCU and HKLM entries are matched — the app runs as admin so
    // HKLM writes are permitted.
    let key_lower = key.to_lowercase();
    let stem = key_lower.trim_end_matches(".exe");

    // Try DisplayName (most reliable), then UninstallString basename, then
    // DisplayIcon basename.
    if let Some(dn) = entry.display_name.as_ref() {
        if dn.to_lowercase().contains(stem) {
            return true;
        }
    }
    for v in [&entry.uninstall_string, &entry.display_icon]
        .into_iter()
        .flatten()
    {
        if let Some(base) = super::scanner::extract_exe_basename(v) {
            if base.to_lowercase() == key_lower {
                return true;
            }
        }
    }
    false
}

#[tauri::command]
pub fn runtime_visibility_state() -> Result<StateView, String> {
    let state = load()?;
    let state_path = state_file()?.to_string_lossy().to_string();
    Ok(StateView { state, state_path })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rv(name: &str, cmd: &str) -> RunValue {
        RunValue {
            hive: "HKCU".into(),
            subkey: "Software\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
            name: name.into(),
            command: cmd.into(),
        }
    }

    #[test]
    fn run_value_matches_by_exe_basename() {
        let v = rv("MyTray", r#""C:\Tools\Syncthing\syncthing.exe" -nb"#);
        assert!(matches_run_value(&v, "syncthing.exe"));
        assert!(matches_run_value(&v, "syncthing"));
    }

    #[test]
    fn run_value_matches_by_value_name_stem() {
        let v = rv("SyncthingTray", "anything.exe");
        assert!(matches_run_value(&v, "syncthing.exe"));
    }

    #[test]
    fn run_value_matches_hklm() {
        // HKLM entries are now mutated too (app runs as admin).
        let mut v = rv("MyTray", "syncthing.exe");
        v.hive = "HKLM".into();
        assert!(matches_run_value(&v, "syncthing.exe"));
    }

    fn ue(display: &str, uninst: &str) -> UninstallEntry {
        UninstallEntry {
            hive: "HKCU".into(),
            subkey: "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Foo".into(),
            display_name: Some(display.into()),
            display_icon: None,
            install_location: None,
            uninstall_string: Some(uninst.into()),
            publisher: None,
            system_component: None,
        }
    }

    #[test]
    fn uninstall_matches_by_displayname_substring() {
        let e = ue("Syncthing Tray", "C:\\\\foo\\\\unins.exe");
        assert!(matches_uninstall(&e, "syncthing.exe"));
    }

    #[test]
    fn uninstall_matches_hklm() {
        // HKLM entries are now mutated too (app runs as admin).
        let mut e = ue("Syncthing", "unins.exe");
        e.hive = "HKLM".into();
        assert!(matches_uninstall(&e, "syncthing"));
    }
}
