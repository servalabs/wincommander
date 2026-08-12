// Hide / restore orchestration.
//
// Each `hide_runtime` call:
//   1. Walks HKCU Run + RunOnce values that match the key.
//   2. Walks HKCU Uninstall entries that match the key.
//   3. Writes the snapshot (applied=false) to disk FIRST.
//   4. Applies the mutations one by one. Each successful mutation is added
//      to the snapshot which is re-saved.
//   5. Flips applied=true at the end.
//
// If a step fails mid-way the snapshot still records the steps that DID
// succeed, so `restore_runtime` (or `restore_all`) can unwind them.
//
// Contract functions (called from lib.rs at startup):
//   reenforce_hidden_on_startup() — re-applies every applied=true hide so
//     apps that reappeared after reboot/update are suppressed again.
//   spawn_reapply_watcher() — background thread watching Start Menu/Desktop
//     for shortcut reappearance (e.g. after self-updating apps).

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use super::registry::{
    hkcu_rename_run_value, hkcu_set_system_component, hklm_create_app_path, hklm_delete_app_path,
    hklm_read_app_path, hklm_rename_run_value, hklm_set_system_component, read_run_values,
    read_uninstall_entries,
};
use super::state::{
    self, matches_run_value, matches_uninstall, AppPathBackup, HideEntry, KilledProcess, RunRename,
    ScheduledTaskBackup, ShortcutBackup, UninstallHide,
};
use serde::Serialize;

fn hidden_suffix() -> &'static str {
    crate::paths::hidden_marker_suffix()
}

fn is_hidden_run_name(name: &str) -> bool {
    name.ends_with(hidden_suffix()) || name.ends_with("__WC_Hidden")
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HideReport {
    pub key: String,
    pub run_renamed: u32,
    pub uninstall_hidden: u32,
    pub shortcuts_hidden: u32,
    pub errors: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub key: String,
    pub run_restored: u32,
    pub uninstall_restored: u32,
    pub shortcuts_restored: u32,
    pub errors: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BulkReport {
    pub keys: Vec<String>,
    pub reports: Vec<HideReport>,
}

// ─── Start Menu shortcut helpers ─────────────────────────────────────────────

fn walk_lnk(dir: &std::path::Path, stems: &[&str]) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_lnk(&path, stems));
        } else {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            // Match both .lnk (shell shortcuts) and .url (internet shortcuts like
            // "VeraCrypt Website.url") so they are backed up and hidden from search.
            if ext == "lnk" || ext == "url" {
                let name_lower = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if stems.iter().any(|s| name_lower.contains(*s)) {
                    if let Some(s) = path.to_str() {
                        out.push(s.to_string());
                    }
                }
            }
        }
    }
    out
}

/// Build the set of stems to use for shortcut filename matching.
///
/// "activitywatch-app" → ["activitywatch-app", "activitywatch"]
/// "aw-qt"             → ["aw-qt"]  (prefix "aw" < 5 chars, skipped)
/// "everything"        → ["everything"]
///
/// The walk_lnk matcher uses .contains(stem), so "everything" matches
/// "Everything 2.0.lnk", "Everything (1).lnk", "Everything.lnk", etc.
/// The short first-word prefix ("activitywatch") similarly covers versioned
/// variants like "ActivityWatch 0.14.lnk".
fn shortcut_stems(key_stem: &str) -> Vec<String> {
    let mut stems = vec![key_stem.to_string()];
    let prefix = key_stem.split(['-', '_']).next().unwrap_or("");
    if prefix.len() >= 5 && prefix != key_stem {
        stems.push(prefix.to_string());
    }
    stems
}

/// Tell Explorer/Search indexer that the given directory changed so it
/// re-scans immediately rather than waiting for the next polling cycle.
#[cfg(windows)]
fn notify_shell_dir(path: &std::path::Path) {
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_UPDATEDIR, SHCNF_PATHW};
    if let Some(s) = path.to_str() {
        let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            SHChangeNotify(
                SHCNE_UPDATEDIR as i32,
                SHCNF_PATHW,
                wide.as_ptr() as *const _,
                std::ptr::null(),
            );
        }
    }
}

#[cfg(not(windows))]
fn notify_shell_dir(_path: &std::path::Path) {}

/// Flush the shell's icon/association cache so Windows Search picks up
/// shortcut changes immediately rather than waiting for the next crawl.
#[cfg(windows)]
fn flush_shell_cache() {
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

#[cfg(not(windows))]
fn flush_shell_cache() {}

/// Find .lnk/.url files in Start Menu and Desktop locations whose filename
/// stem matches any of the provided stems.
fn find_shell_shortcuts(stems: &[String]) -> Vec<String> {
    let stems_ref: Vec<&str> = stems.iter().map(|s| s.as_str()).collect();
    let mut out = Vec::new();

    // Per-user Start Menu (%APPDATA%\Microsoft\Windows\Start Menu\Programs)
    if let Ok(appdata) = std::env::var("APPDATA") {
        let appdata = std::path::PathBuf::from(appdata);
        let base = appdata.join("Microsoft\\Windows\\Start Menu\\Programs");
        out.extend(walk_lnk(&base, &stems_ref));
        out.extend(walk_lnk(
            &appdata.join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar"),
            &stems_ref,
        ));
        out.extend(walk_lnk(
            &appdata.join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\StartMenu"),
            &stems_ref,
        ));
    }

    // All-users Start Menu (%ProgramData%\Microsoft\Windows\Start Menu\Programs)
    // Covers system-wide installs like Tailscale, Everything, etc.
    if let Ok(programdata) = std::env::var("PROGRAMDATA") {
        let programdata = std::path::PathBuf::from(programdata);
        let base = programdata.join("Microsoft\\Windows\\Start Menu\\Programs");
        out.extend(walk_lnk(&base, &stems_ref));
        out.extend(walk_lnk(&programdata.join("Desktop"), &stems_ref));
    }

    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        out.extend(walk_lnk(
            &std::path::PathBuf::from(userprofile).join("Desktop"),
            &stems_ref,
        ));
    }

    out
}

/// Set FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM on a path so it is
/// invisible to normal Explorer browsing and Start Menu indexing.
/// Silently no-ops on non-Windows or if the call fails (best-effort).
#[cfg(windows)]
fn set_hidden_system_attrs(path: &std::path::Path) {
    use windows_sys::Win32::Storage::FileSystem::{
        SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM,
    };
    if let Some(s) = path.to_str() {
        let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM);
        }
    }
}

#[cfg(not(windows))]
fn set_hidden_system_attrs(_path: &std::path::Path) {}

fn shortcut_backup_dir(key: &str) -> Result<std::path::PathBuf, String> {
    let dir = crate::paths::machine_data_dir()?
        .join("runtime_visibility\\shortcuts")
        .join(key);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir shortcut backup: {}", e))?;
    // KT: hide the backup dir itself so .lnk copies are not visible to Explorer
    // or Start Menu indexers when browsing %ProgramData%.
    set_hidden_system_attrs(&dir);
    Ok(dir)
}

fn shortcut_backup_path(
    backup_dir: &std::path::Path,
    source: &std::path::Path,
) -> std::path::PathBuf {
    let file_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("shortcut.lnk");
    let mut candidate = backup_dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }

    let stem = source
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("shortcut");
    let ext = source.extension().and_then(|n| n.to_str()).unwrap_or("lnk");
    for i in 1..1000 {
        candidate = backup_dir.join(format!("{stem}-{i}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    backup_dir.join(format!(
        "{}-{}.{}",
        stem,
        chrono::Utc::now().timestamp_millis(),
        ext
    ))
}

fn task_matches_key(task: &super::enumerate::ScheduledTaskInfo, key: &str) -> bool {
    let key_lower = key.to_lowercase();
    let stem = key_lower.trim_end_matches(".exe");
    let is_ms_infra = task.task_name.starts_with("\\Microsoft\\Windows\\");

    // When the action exe is available, use it as the primary match signal.
    // An exact basename comparison prevents short stems like "host" or "agent"
    // from matching Windows infrastructure tasks whose paths contain those words.
    if let Some(action) = task.action.as_ref() {
        if let Some(exe) = super::scanner::extract_exe_basename(action) {
            return exe.eq_ignore_ascii_case(&key_lower);
        }
        // Action present but extract_exe_basename returned None (e.g. COM action).
        // Fall through to name-contains only for non-Microsoft-infra tasks.
        if !is_ms_infra && action.to_lowercase().contains(stem) {
            return true;
        }
        // If we have an action but it doesn't match, don't fall through to the
        // name-contains check — that would match infra tasks by accident.
        return false;
    }

    // No action available: fall back to name-contains, but SKIP \Microsoft\Windows\ tasks.
    // KT: schtasks occasionally omits the action for COM-only triggers; the name
    // fallback is kept for user-created tasks (which won't be under \Microsoft\Windows\).
    if is_ms_infra {
        return false;
    }
    task.task_name.to_lowercase().contains(stem)
}

fn set_scheduled_task_enabled(task_name: &str, enabled: bool) -> Result<(), String> {
    let mut cmd = std::process::Command::new("schtasks.exe");
    cmd.args([
        "/Change",
        "/TN",
        task_name,
        if enabled { "/ENABLE" } else { "/DISABLE" },
    ]);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let out = cmd
        .output()
        .map_err(|e| format!("schtasks spawn failed: {}", e))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        Err(format!("{}{}", stdout.trim(), stderr.trim()))
    }
}

// ─────────────────────────────────────────────────────────────────────────────

fn normalize_key(k: &str) -> String {
    let trimmed = k.trim().to_lowercase();
    if trimmed.is_empty() {
        return trimmed;
    }
    if trimmed.ends_with(".exe") {
        trimmed
    } else {
        format!("{}.exe", trimmed)
    }
}

/// Reject keys that don't look like a bare exe basename — prevents path
/// traversal via JS-callable hide_runtime (e.g. "../../Windows/system32/lsass").
fn validate_key(key: &str) -> Result<(), String> {
    // Must be of the form: one or more [a-z0-9._-] chars followed by ".exe".
    // normalize_key already lowercased and appended ".exe" so we just verify.
    if key
        .bytes()
        .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
        && key.ends_with(".exe")
        && key.len() > 4
    {
        Ok(())
    } else {
        Err(format!(
            "invalid key '{}': must be a bare exe basename like 'foo.exe'",
            key
        ))
    }
}

#[tauri::command]
pub fn hide_runtime(key: String) -> Result<HideReport, String> {
    let key = normalize_key(&key);
    if key.is_empty() {
        return Err("empty key".into());
    }
    validate_key(&key)?;

    // Refuse to hide protected Windows infrastructure if someone passes it
    // through. The scanner already filters these from the UI, but the Tauri
    // command is callable from JS so we re-check here.
    if is_protected(&key) {
        return Err(format!("refusing to hide protected runtime '{}'", key));
    }

    let run_values = read_run_values();
    let uninstall = read_uninstall_entries();

    let mut entry = state::load()?
        .entries
        .into_iter()
        .find(|entry| entry.key == key)
        .unwrap_or(HideEntry {
            key: key.clone(),
            hidden_at_unix_ms: chrono::Utc::now().timestamp_millis(),
            applied: false,
            run_value_renames: Vec::new(),
            uninstall_hides: Vec::new(),
            shortcut_backups: Vec::new(),
            app_path_backups: Vec::new(),
            scheduled_task_backups: Vec::new(),
            killed_processes: Vec::new(),
        });

    // Record intent before mutating anything without losing an existing
    // backup manifest during startup re-apply.
    state::upsert(entry.clone())?;

    let mut report = HideReport {
        key: key.clone(),
        run_renamed: 0,
        uninstall_hidden: 0,
        shortcuts_hidden: 0,
        errors: Vec::new(),
    };

    // Step A: rename matching Run values (HKCU and HKLM).
    // Skip values already renamed with either the current marker or the legacy
    // marker so calling
    // hide_runtime twice (e.g. startup re-apply) is safe and idempotent.
    for rv in run_values
        .iter()
        .filter(|rv| matches_run_value(rv, &key) && !is_hidden_run_name(&rv.name))
    {
        let target = format!("{}{}", rv.name, hidden_suffix());
        let result = if rv.hive == "HKLM" {
            hklm_rename_run_value(&rv.subkey, &rv.name, &target)
        } else {
            hkcu_rename_run_value(&rv.subkey, &rv.name, &target)
        };
        match result {
            Ok(()) => {
                if !entry.run_value_renames.iter().any(|r| {
                    r.hive == rv.hive && r.subkey == rv.subkey && r.original_name == rv.name
                }) {
                    entry.run_value_renames.push(RunRename {
                        hive: rv.hive.clone(),
                        subkey: rv.subkey.clone(),
                        original_name: rv.name.clone(),
                        renamed_to: target,
                    });
                }
                report.run_renamed += 1;
                state::upsert(entry.clone())?;
            }
            Err(e) => {
                report
                    .errors
                    .push(format!("Run[{}/{}/{}]: {}", rv.hive, rv.subkey, rv.name, e));
            }
        }
    }

    // Step B: SystemComponent=1 on matching uninstall entries (HKCU and HKLM).
    // Skip entries already hidden so the operation is idempotent.
    for ue in uninstall
        .iter()
        .filter(|u| matches_uninstall(u, &key) && u.system_component != Some(1))
    {
        let previous = ue.system_component;
        let result = if ue.hive == "HKLM" {
            hklm_set_system_component(&ue.subkey, Some(1))
        } else {
            hkcu_set_system_component(&ue.subkey, Some(1))
        };
        match result {
            Ok(()) => {
                if !entry
                    .uninstall_hides
                    .iter()
                    .any(|u| u.hive == ue.hive && u.subkey == ue.subkey)
                {
                    entry.uninstall_hides.push(UninstallHide {
                        hive: ue.hive.clone(),
                        subkey: ue.subkey.clone(),
                        previous_value: previous,
                    });
                }
                report.uninstall_hidden += 1;
                state::upsert(entry.clone())?;
            }
            Err(e) => {
                report
                    .errors
                    .push(format!("Uninstall[{}/{}]: {}", ue.hive, ue.subkey, e));
            }
        }
    }

    // Step C: move matching Start Menu/Desktop shortcuts to a backup dir so
    // the app no longer appears in Windows Search or on the desktop.
    let key_stem = key.trim_end_matches(".exe");
    let stems = shortcut_stems(key_stem);
    let shortcuts = find_shell_shortcuts(&stems);
    let mut notified_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();
    if !shortcuts.is_empty() {
        if let Ok(backup_dir) = shortcut_backup_dir(&key) {
            for src in &shortcuts {
                let src_path = std::path::Path::new(src);
                let dst = shortcut_backup_path(&backup_dir, src_path);
                match std::fs::rename(src, &dst) {
                    Ok(()) => {
                        // Notify Explorer so Windows Search re-indexes immediately.
                        if let Some(parent) = src_path.parent() {
                            let key = parent.to_string_lossy().to_lowercase();
                            if notified_dirs.insert(key) {
                                notify_shell_dir(parent);
                            }
                        }
                        if !entry
                            .shortcut_backups
                            .iter()
                            .any(|s| s.original_path == *src)
                        {
                            entry.shortcut_backups.push(ShortcutBackup {
                                original_path: src.clone(),
                                backup_path: dst.to_string_lossy().to_string(),
                            });
                        }
                        report.shortcuts_hidden += 1;
                        state::upsert(entry.clone())?;
                    }
                    Err(e) => {
                        report.errors.push(format!("Shortcut[{}]: {}", src, e));
                    }
                }
            }
        }
    }

    // Step D: Delete HKLM App Paths key so the app no longer appears as a
    // "Run command" in Windows Search (e.g., tailscale.exe registers there).
    if let Some((default_val, path_val)) = hklm_read_app_path(&key) {
        match hklm_delete_app_path(&key) {
            Ok(()) => {
                if !entry
                    .app_path_backups
                    .iter()
                    .any(|ap| ap.exe_name.eq_ignore_ascii_case(&key))
                {
                    entry.app_path_backups.push(AppPathBackup {
                        exe_name: key.clone(),
                        default_value: default_val,
                        path_value: path_val,
                    });
                }
                state::upsert(entry.clone())?;
            }
            Err(e) => report.errors.push(format!("AppPaths[{}]: {}", key, e)),
        }
    }

    // Step E: disable matching scheduled tasks so hidden tray apps don't come
    // back after the next login/reboot.
    if let Ok(tasks) = super::enumerate::enumerate_scheduled_tasks() {
        for task in tasks.iter().filter(|task| {
            task_matches_key(task, &key) && !task.status.eq_ignore_ascii_case("Disabled")
        }) {
            match set_scheduled_task_enabled(&task.task_name, false) {
                Ok(()) => {
                    if !entry
                        .scheduled_task_backups
                        .iter()
                        .any(|t| t.task_name.eq_ignore_ascii_case(&task.task_name))
                    {
                        entry.scheduled_task_backups.push(ScheduledTaskBackup {
                            task_name: task.task_name.clone(),
                            was_enabled: true,
                        });
                    }
                    state::upsert(entry.clone())?;
                }
                Err(e) => report
                    .errors
                    .push(format!("ScheduledTask[{}]: {}", task.task_name, e)),
            }
        }
    }

    // Step F: Kill the tray/UI process so the tray icon disappears.
    // Record the launch command so the process can be restarted on restore.
    //
    // NO_KILL exemptions: apps where the single process is BOTH the tray AND
    // the essential backend (VPN tunnel, mounted volumes). Killing them would
    // break the feature the user still wants running.  Steps A–D already hide
    // these apps from ARP, Windows Search, and Start Menu.
    // veracrypt.exe: killing it would unmount encrypted volumes mid-session.
    // tailscale.exe: CLI binary, not a running tray process — nothing to kill.
    // tailscale-ipn.exe is intentionally NOT exempted: user wants tray removed.
    // The VPN tunnel may drop (acceptable on user's system without the service).
    const NO_KILL: &[&str] = &["tailscale.exe", "veracrypt.exe"];
    if !NO_KILL.iter().any(|nk| nk.eq_ignore_ascii_case(&key)) {
        let key_stem = key.trim_end_matches(".exe");
        use sysinfo::{ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, false);
        let mut restart_cmd: Vec<String> = Vec::new();
        for process in sys.processes().values() {
            let raw = process.name().to_string_lossy().to_lowercase();
            let norm = if raw.ends_with(".exe") {
                raw.clone()
            } else {
                format!("{}.exe", raw)
            };
            if norm == key || raw == key_stem {
                // process.exe() is the definitive on-disk path — more reliable
                // than cmd()[0] which sysinfo may return empty on Windows when
                // it can't read the process's command line (common for processes
                // launched from the Run key or by the shell).
                let exe_path = process.exe().map(|p| p.to_string_lossy().into_owned());
                let extra_args: Vec<String> = {
                    let c = process.cmd();
                    if c.len() > 1 {
                        c[1..]
                            .iter()
                            .map(|s| s.to_string_lossy().into_owned())
                            .filter(|s| !s.is_empty())
                            .collect()
                    } else {
                        vec![]
                    }
                };
                let full_cmd = if let Some(path) = exe_path {
                    let mut v = vec![path];
                    v.extend(extra_args);
                    v
                } else {
                    // No exe() — fall back to full cmd() as-is
                    process
                        .cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().into_owned())
                        .filter(|s| !s.is_empty())
                        .collect()
                };
                if !full_cmd.is_empty() && restart_cmd.is_empty() {
                    restart_cmd = full_cmd;
                }
            }
        }

        // Tailscale-specific: enable unattended mode BEFORE killing the tray so
        // the Tailscale Windows service keeps the VPN tunnel alive after
        // tailscale-ipn.exe exits. This is a no-op if the service isn't running.
        if key.eq_ignore_ascii_case("tailscale-ipn.exe") {
            let mut ts = std::process::Command::new("tailscale");
            ts.args(["set", "--unattended=true"]);
            #[cfg(windows)]
            ts.creation_flags(0x08000000);
            let _ = ts.output(); // failure is harmless
        }

        let mut kill_cmd = std::process::Command::new("taskkill");
        kill_cmd.args(["/F", "/IM", &key]);
        #[cfg(windows)]
        kill_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        match kill_cmd.output() {
            Ok(out) if out.status.success() => {
                entry
                    .killed_processes
                    .retain(|p| !p.exe_name.eq_ignore_ascii_case(&key));
                entry.killed_processes.push(KilledProcess {
                    exe_name: key.clone(),
                    restart_cmd,
                });
                state::upsert(entry.clone())?;
            }
            Ok(out) => {
                // "not found" means the process wasn't running — not an error.
                let msg = String::from_utf8_lossy(&out.stdout).to_lowercase();
                if !msg.contains("not found") && !msg.is_empty() {
                    report.errors.push(format!("Kill[{}]: {}", key, msg.trim()));
                }
            }
            Err(e) => report.errors.push(format!("Kill[{}]: {}", key, e)),
        }
    }

    entry.applied = true;
    state::upsert(entry)?;
    if report.shortcuts_hidden > 0 {
        flush_shell_cache();
    }
    crate::log::log_message_src(
        "info",
        crate::log::LOG_SRC_CORE,
        &format!(
            "hide_runtime {}: run={} uninstall={} shortcuts={} errors={}",
            key,
            report.run_renamed,
            report.uninstall_hidden,
            report.shortcuts_hidden,
            report.errors.len()
        ),
    );
    Ok(report)
}

#[tauri::command]
pub fn restore_runtime(key: String) -> Result<RestoreReport, String> {
    let key = normalize_key(&key);
    let state = state::load()?;
    let entry = state
        .entries
        .iter()
        .find(|e| e.key == key)
        .cloned()
        .ok_or_else(|| format!("no hide record for '{}'", key))?;
    restore_entry(entry)
}

fn restore_entry(entry: HideEntry) -> Result<RestoreReport, String> {
    let mut report = RestoreReport {
        key: entry.key.clone(),
        run_restored: 0,
        uninstall_restored: 0,
        shortcuts_restored: 0,
        errors: Vec::new(),
    };

    // Reverse step B first (uninstall) then step A (run) — order matters
    // for users observing partial state, since Apps & Features reappearing
    // is the most visible signal that restore is working.
    for u in &entry.uninstall_hides {
        let result = if u.hive == "HKLM" {
            hklm_set_system_component(&u.subkey, u.previous_value)
        } else {
            hkcu_set_system_component(&u.subkey, u.previous_value)
        };
        match result {
            Ok(()) => report.uninstall_restored += 1,
            Err(e) => report
                .errors
                .push(format!("Uninstall[{}/{}]: {}", u.hive, u.subkey, e)),
        }
    }

    for r in &entry.run_value_renames {
        let result = if r.hive == "HKLM" {
            hklm_rename_run_value(&r.subkey, &r.renamed_to, &r.original_name)
        } else {
            hkcu_rename_run_value(&r.subkey, &r.renamed_to, &r.original_name)
        };
        match result {
            Ok(()) => report.run_restored += 1,
            Err(e) => report.errors.push(format!(
                "Run[{}/{}/{}]: {}",
                r.hive, r.subkey, r.original_name, e
            )),
        }
    }

    // Restore Step C: move shortcuts back to their original locations.
    let mut notified_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();
    for s in &entry.shortcut_backups {
        let dst = std::path::Path::new(&s.original_path);
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::rename(&s.backup_path, dst) {
            Ok(()) => {
                // Notify Explorer so Windows Search picks up the restored shortcut.
                if let Some(parent) = dst.parent() {
                    let key = parent.to_string_lossy().to_lowercase();
                    if notified_dirs.insert(key) {
                        notify_shell_dir(parent);
                    }
                }
                report.shortcuts_restored += 1;
            }
            Err(e) => {
                if dst.exists() && !std::path::Path::new(&s.backup_path).exists() {
                    report.shortcuts_restored += 1;
                } else {
                    report
                        .errors
                        .push(format!("ShortcutRestore[{}]: {}", s.original_path, e));
                }
            }
        }
    }

    // Restore Step D: recreate App Paths keys.
    for ap in &entry.app_path_backups {
        if let Err(e) = hklm_create_app_path(
            &ap.exe_name,
            ap.default_value.as_deref(),
            ap.path_value.as_deref(),
        ) {
            report
                .errors
                .push(format!("AppPathsRestore[{}]: {}", ap.exe_name, e));
        }
    }

    // Restore Step E: re-enable scheduled tasks that were enabled before hide.
    for task in &entry.scheduled_task_backups {
        if task.was_enabled {
            if let Err(e) = set_scheduled_task_enabled(&task.task_name, true) {
                report
                    .errors
                    .push(format!("ScheduledTaskRestore[{}]: {}", task.task_name, e));
            }
        }
    }

    // Restore Step F: re-launch killed tray processes.
    for kp in &entry.killed_processes {
        if let Some(exe) = kp.restart_cmd.first() {
            if !exe.is_empty() {
                // Verify the exe path exists before spawning. If it doesn't
                // exist as a path, pass it to Command::new anyway — it may be
                // on PATH (e.g. a bare "aw-qt.exe" without a full path).
                let _ = std::process::Command::new(exe)
                    .args(kp.restart_cmd.get(1..).unwrap_or(&[]))
                    .spawn();
            }
        }
    }

    // Drop the entry from the manifest only if every step succeeded. If
    // restore is partial we leave it so the user can retry.
    if report.errors.is_empty() {
        state::remove(&entry.key)?;
    }

    if report.shortcuts_restored > 0 {
        flush_shell_cache();
    }

    Ok(report)
}

#[tauri::command]
pub fn restore_all_runtimes() -> Result<Vec<RestoreReport>, String> {
    let state = state::load()?;
    let mut reports = Vec::new();
    for entry in state.entries {
        reports.push(restore_entry(entry)?);
    }
    Ok(reports)
}

/// Hide a specific list of exe keys — used by the Identity panel to hide
/// only the known backend apps (VeraCrypt, Tailscale, etc.) without
/// touching user tools like Listary or Proton Drive.
#[tauri::command]
pub fn hide_runtime_list(keys: Vec<String>) -> Result<BulkReport, String> {
    let mut reports = Vec::new();
    let mut reported_keys = Vec::new();
    for key in keys {
        let norm = normalize_key(&key);
        if norm.is_empty() || is_protected(&norm) {
            continue;
        }
        match hide_runtime(key) {
            Ok(r) => {
                reported_keys.push(norm);
                reports.push(r);
            }
            Err(e) => {
                reports.push(HideReport {
                    key: norm.clone(),
                    run_renamed: 0,
                    uninstall_hidden: 0,
                    shortcuts_hidden: 0,
                    errors: vec![e],
                });
                reported_keys.push(norm);
            }
        }
    }
    Ok(BulkReport {
        keys: reported_keys,
        reports,
    })
}

/// "Hide all backend apps" — hides every hideable runtime from the scanner.
/// `restore=true` reverses everything in the manifest. This is the user-
/// facing global toggle.
#[tauri::command]
pub fn set_global_runtime_visibility(hidden: bool) -> Result<BulkReport, String> {
    if !hidden {
        let restored = restore_all_runtimes()?;
        return Ok(BulkReport {
            keys: restored.iter().map(|r| r.key.clone()).collect(),
            reports: Vec::new(),
        });
    }

    let scan = super::scanner::scan_runtimes()?;
    let mut seen = std::collections::HashSet::new();
    let mut reports = Vec::new();
    let mut keys = Vec::new();

    for r in scan.runtimes.iter().filter(|r| r.hideable) {
        let key = normalize_key(&r.name);
        if !seen.insert(key.clone()) {
            continue;
        }
        if is_protected(&key) {
            continue;
        }
        match hide_runtime(key.clone()) {
            Ok(rep) => {
                reports.push(rep);
                keys.push(key);
            }
            Err(e) => {
                reports.push(HideReport {
                    key: key.clone(),
                    run_renamed: 0,
                    uninstall_hidden: 0,
                    shortcuts_hidden: 0,
                    errors: vec![e],
                });
            }
        }
    }

    Ok(BulkReport { keys, reports })
}

/// Called by lib.rs at startup (before the window is shown) to re-suppress
/// any hidden backend apps that reappeared during reboot or after an update.
///
/// Iterates every `applied=true` entry in state.json and calls `hide_runtime`
/// again — which is idempotent (already-renamed Run values and already-hidden
/// uninstall entries are skipped). The process-kill step fires fresh, which
/// removes the tray icon of any process that restarted since last session.
pub fn reenforce_hidden_on_startup() {
    let state = match state::load() {
        Ok(s) => s,
        Err(e) => {
            crate::log::log_message_src(
                "warn",
                crate::log::LOG_SRC_CORE,
                &format!("reenforce_hidden_on_startup: failed to load state: {}", e),
            );
            return;
        }
    };

    let applied: Vec<String> = state
        .entries
        .iter()
        .filter(|e| e.applied)
        .map(|e| e.key.clone())
        .collect();

    if applied.is_empty() {
        return;
    }

    crate::log::log_message_src(
        "info",
        crate::log::LOG_SRC_CORE,
        &format!(
            "reenforce_hidden_on_startup: re-hiding {} entries",
            applied.len()
        ),
    );

    for key in applied {
        match hide_runtime(key.clone()) {
            Ok(r) => {
                crate::log::log_message_src(
                    "info",
                    crate::log::LOG_SRC_CORE,
                    &format!(
                        "reenforce_hidden_on_startup: re-hide {} ok \
                         (run={} uninstall={} shortcuts={} errors={})",
                        key,
                        r.run_renamed,
                        r.uninstall_hidden,
                        r.shortcuts_hidden,
                        r.errors.len()
                    ),
                );
            }
            Err(e) => {
                crate::log::log_message_src(
                    "warn",
                    crate::log::LOG_SRC_CORE,
                    &format!("reenforce_hidden_on_startup: re-hide {} failed: {}", key, e),
                );
            }
        }
    }
}

/// Background watcher that detects when a hidden app's Start Menu / Desktop
/// shortcut REAPPEARS (e.g. because the app self-updated and recreated it)
/// and calls `hide_runtime` again to suppress it.
///
/// Uses the `notify` crate (already a dep for ransomware_monitor) to watch
/// the Start Menu Programs dirs and the Desktop for file-create events.
/// Debounced: a burst of events within DEBOUNCE_MS are merged so a single
/// installer that drops 20 shortcuts triggers one re-hide pass, not 20.
pub fn spawn_reapply_watcher() {
    std::thread::Builder::new()
        .name("rv-reapply-watcher".into())
        .spawn(|| {
            if let Err(e) = reapply_watcher_thread() {
                crate::log::log_message_src(
                    "warn",
                    crate::log::LOG_SRC_CORE,
                    &format!("spawn_reapply_watcher: watcher exited: {}", e),
                );
            }
        })
        .ok();
}

fn reapply_watcher_thread() -> Result<(), String> {
    use notify::{EventKind, RecursiveMode, Watcher};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    // 3-second debounce: events that arrive within this window are merged
    // into one re-hide pass so a burst of installer file drops doesn't
    // trigger dozens of sysinfo scans in quick succession.
    const DEBOUNCE_MS: u64 = 3000;
    // Polling interval when the watched event channel has no new events.
    const POLL_MS: u64 = 500;

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            // Only act on create/modify events — deletions are noise.
            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    let _ = tx.send(event);
                }
                _ => {}
            }
        }
    })
    .map_err(|e| format!("notify::recommended_watcher: {}", e))?;

    // Watch per-user Start Menu, All-users Start Menu, and Desktop dirs.
    let mut watched_paths: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(appdata) = std::env::var("APPDATA") {
        let base =
            std::path::PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs");
        if base.exists() {
            let _ = watcher.watch(&base, RecursiveMode::Recursive);
            watched_paths.push(base);
        }
    }
    if let Ok(programdata) = std::env::var("PROGRAMDATA") {
        let pb = std::path::PathBuf::from(programdata);
        let all_users_sm = pb.join("Microsoft\\Windows\\Start Menu\\Programs");
        if all_users_sm.exists() {
            let _ = watcher.watch(&all_users_sm, RecursiveMode::Recursive);
            watched_paths.push(all_users_sm);
        }
        let pd_desktop = pb.join("Desktop");
        if pd_desktop.exists() {
            let _ = watcher.watch(&pd_desktop, RecursiveMode::Recursive);
            watched_paths.push(pd_desktop);
        }
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let desktop = std::path::PathBuf::from(userprofile).join("Desktop");
        if desktop.exists() {
            let _ = watcher.watch(&desktop, RecursiveMode::Recursive);
            watched_paths.push(desktop);
        }
    }

    if watched_paths.is_empty() {
        return Err("no watchable Start Menu / Desktop dirs found".into());
    }

    crate::log::log_message_src(
        "info",
        crate::log::LOG_SRC_CORE,
        &format!(
            "spawn_reapply_watcher: watching {} dirs for shortcut reappearance",
            watched_paths.len()
        ),
    );

    let mut last_event_at: Option<Instant> = None;
    let mut last_hide_at: std::collections::HashMap<String, std::time::Instant> =
        std::collections::HashMap::new();

    loop {
        // Drain all pending events from the channel, noting the latest one.
        loop {
            match rx.try_recv() {
                Ok(_) => {
                    last_event_at = Some(Instant::now());
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("notify channel disconnected".into());
                }
            }
        }

        // If we collected events and the debounce window has elapsed, check
        // whether any hidden app's shortcut reappeared.
        if let Some(t) = last_event_at {
            if t.elapsed() >= Duration::from_millis(DEBOUNCE_MS) {
                last_event_at = None;
                recheck_hidden_shortcuts_inner(&mut last_hide_at);
            }
        }

        std::thread::sleep(Duration::from_millis(POLL_MS));
    }
}

/// Load state, find `applied=true` entries, and for each check whether any of
/// their shortcut_backups' ORIGINAL paths now exist again (meaning something
/// recreated them). If so, re-run `hide_runtime` to suppress them.
///
/// `last_hide_at` tracks the last time we triggered a re-hide per key.
/// If the last re-hide for a key was less than REHIDE_COOLDOWN_SECS ago we
/// skip it and log a warning — this breaks the tight loop where an aggressive
/// self-updating app recreates its shortcut milliseconds after we remove it.
fn recheck_hidden_shortcuts_inner(
    last_hide_at: &mut std::collections::HashMap<String, std::time::Instant>,
) {
    // KT: 30-second per-key cooldown prevents endless sysinfo/schtasks scans
    // when an app recreates its shortcut faster than we can remove it.
    const REHIDE_COOLDOWN_SECS: u64 = 30;

    let state = match state::load() {
        Ok(s) => s,
        Err(_) => return,
    };

    let mut keys_to_rehide: Vec<String> = Vec::new();

    for entry in state.entries.iter().filter(|e| e.applied) {
        let key_stem = entry.key.trim_end_matches(".exe");
        let stems = shortcut_stems(key_stem);

        // A shortcut "reappeared" if either:
        // (a) a backed-up shortcut's original_path now exists again, OR
        // (b) find_shell_shortcuts discovers a new .lnk for this key's stems
        //     that is NOT one of the known backup sources.
        let backed_up_srcs: std::collections::HashSet<&str> = entry
            .shortcut_backups
            .iter()
            .map(|s| s.original_path.as_str())
            .collect();

        let reappeared = entry
            .shortcut_backups
            .iter()
            .any(|s| std::path::Path::new(&s.original_path).exists());

        let new_shortcuts = find_shell_shortcuts(&stems)
            .into_iter()
            .any(|p| !backed_up_srcs.contains(p.as_str()));

        if reappeared || new_shortcuts {
            keys_to_rehide.push(entry.key.clone());
        }
    }

    for key in keys_to_rehide {
        let now = std::time::Instant::now();
        if let Some(last) = last_hide_at.get(&key) {
            if now.duration_since(*last).as_secs() < REHIDE_COOLDOWN_SECS {
                crate::log::log_message_src(
                    "warn",
                    crate::log::LOG_SRC_CORE,
                    &format!(
                        "spawn_reapply_watcher: '{}' shortcut reappeared again within {}s cooldown — skipping re-hide",
                        key, REHIDE_COOLDOWN_SECS
                    ),
                );
                continue;
            }
        }
        crate::log::log_message_src(
            "info",
            crate::log::LOG_SRC_CORE,
            &format!(
                "spawn_reapply_watcher: shortcut reappeared for '{}', re-hiding",
                key
            ),
        );
        last_hide_at.insert(key.clone(), now);
        if let Err(e) = hide_runtime(key.clone()) {
            crate::log::log_message_src(
                "warn",
                crate::log::LOG_SRC_CORE,
                &format!("spawn_reapply_watcher: re-hide '{}' failed: {}", key, e),
            );
        }
    }
}

fn is_protected(key_lower: &str) -> bool {
    // Kept in sync with scanner::SYSTEM_NAMES. normalize_key appends ".exe" to
    // bare names, so "system" → "system.exe"; include both forms so nothing
    // slips past when called pre- or post-normalize.
    // KT: "system", "registry", "memcompression" appear in SYSTEM_NAMES as bare
    // names — add their .exe forms here so they're blocked after normalize_key.
    const PROTECTED: &[&str] = &[
        "explorer.exe",
        "svchost.exe",
        "lsass.exe",
        "services.exe",
        "winlogon.exe",
        "wininit.exe",
        "csrss.exe",
        "smss.exe",
        "dwm.exe",
        "fontdrvhost.exe",
        "ctfmon.exe",
        "sihost.exe",
        "runtimebroker.exe",
        "taskhostw.exe",
        "searchhost.exe",
        "conhost.exe",
        "applicationframehost.exe",
        "startmenuexperiencehost.exe",
        "shellexperiencehost.exe",
        "systemsettings.exe",
        // Bare-name system processes — normalize_key appends .exe so block both.
        "system.exe",
        "registry.exe",
        "memcompression.exe",
        // Our own binaries — never hide ourselves.
        "wincommander-free.exe",
        "wincommander-pro.exe",
        "commander-free.exe",
        "commander-pro.exe",
    ];
    PROTECTED.contains(&key_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_appends_exe_suffix() {
        assert_eq!(normalize_key("Syncthing"), "syncthing.exe");
        assert_eq!(normalize_key("syncthing.exe"), "syncthing.exe");
        assert_eq!(normalize_key("  FOO  "), "foo.exe");
    }

    #[test]
    fn refuses_to_hide_protected() {
        for k in ["explorer.exe", "svchost.exe", "wincommander-free.exe"] {
            assert!(is_protected(k), "{} should be protected", k);
        }
        assert!(!is_protected("syncthing.exe"));
    }

    #[test]
    fn protected_covers_bare_system_names() {
        // normalize_key converts "system"/"registry"/"memcompression" to .exe forms.
        for k in ["system.exe", "registry.exe", "memcompression.exe"] {
            assert!(is_protected(k), "{} should be protected", k);
        }
    }

    #[test]
    fn validate_key_rejects_traversal() {
        assert!(validate_key("../../lsass.exe").is_err());
        assert!(validate_key("foo\\bar.exe").is_err());
        assert!(validate_key("foo/bar.exe").is_err());
        assert!(validate_key("syncthing.exe").is_ok());
        assert!(validate_key("aw-qt.exe").is_ok());
        assert!(validate_key("my_app.exe").is_ok());
    }

    #[test]
    fn task_matches_key_skips_ms_infra() {
        use super::super::enumerate::ScheduledTaskInfo;
        // \Microsoft\Windows\ task whose name contains "agent" must NOT match "agent.exe"
        let t = ScheduledTaskInfo {
            task_name: "\\Microsoft\\Windows\\UpdateOrchestrator\\Agent".into(),
            status: "Ready".into(),
            next_run: None,
            last_run: None,
            author: None,
            action: None,
        };
        assert!(!task_matches_key(&t, "agent.exe"));

        // Same task but with an action exe that exactly matches — must match.
        let mut t2 = t.clone();
        t2.action = Some("C:\\Windows\\System32\\agent.exe".into());
        assert!(task_matches_key(&t2, "agent.exe"));

        // User task with no action falls through to name-contains.
        let t3 = ScheduledTaskInfo {
            task_name: "\\MyApp\\AgentUpdater".into(),
            status: "Ready".into(),
            next_run: None,
            last_run: None,
            author: None,
            action: None,
        };
        assert!(task_matches_key(&t3, "agent.exe"));
    }
}
