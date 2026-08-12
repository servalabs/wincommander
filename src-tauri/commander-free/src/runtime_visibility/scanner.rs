// Process + window + autostart scanner. Read-only.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use sysinfo::{ProcessesToUpdate, System};

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub enum RuntimeType {
    TypeA, // service + UI split (headless + spawns children)
    TypeB, // tray-backed (autostart + no window)
    TypeC, // headless daemon (name hint + no window)
    TypeD, // hidden-mode single exe (no window, no other signal)
    TypeE, // visible GUI
    Unknown,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DetectedRuntime {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub exe_path: Option<String>,
    pub has_visible_window: bool,
    pub starts_at_logon: bool,
    pub kind: RuntimeType,
    pub hideable: bool,
    pub tags: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub runtimes: Vec<DetectedRuntime>,
    pub scanned_at_unix_ms: i64,
    pub total_processes: usize,
}

#[cfg(windows)]
pub(super) fn pids_with_visible_windows() -> HashSet<u32> {
    use std::cell::RefCell;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    thread_local! {
        static COLLECTED: RefCell<HashSet<u32>> = RefCell::new(HashSet::new());
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, _: isize) -> i32 {
        if IsWindowVisible(hwnd) != 0 {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != 0 {
                COLLECTED.with(|c| {
                    c.borrow_mut().insert(pid);
                });
            }
        }
        1
    }

    COLLECTED.with(|c| c.borrow_mut().clear());
    unsafe {
        EnumWindows(Some(enum_proc), 0);
    }
    COLLECTED.with(|c| c.borrow().clone())
}

#[cfg(not(windows))]
pub(super) fn pids_with_visible_windows() -> HashSet<u32> {
    HashSet::new()
}

/// Read both HKCU and HKLM Run/RunOnce — returns lowercase exe basenames.
#[cfg(windows)]
pub(super) fn autostart_executables() -> HashSet<String> {
    use crate::runtime_visibility::registry::read_run_values;
    let mut out = HashSet::new();
    for entry in read_run_values() {
        if let Some(exe) = extract_exe_basename(&entry.command) {
            out.insert(exe.to_lowercase());
        }
    }
    out
}

#[cfg(not(windows))]
pub(super) fn autostart_executables() -> HashSet<String> {
    HashSet::new()
}

/// Extract the bare executable filename ("syncthing.exe") from a Run-style
/// command line that may be quoted, have args, or use forward/back slashes.
pub(super) fn extract_exe_basename(cmd: &str) -> Option<String> {
    let trimmed = cmd.trim();
    let first = if let Some(stripped) = trimmed.strip_prefix('"') {
        stripped.split('"').next()?.to_string()
    } else {
        trimmed.split_whitespace().next()?.to_string()
    };
    PathBuf::from(first)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

const HEADLESS_NAME_HINTS: &[&str] = &[
    "service",
    "svc",
    "agent",
    "daemon",
    "host",
    "helper",
    "updater",
    "sync",
    "tunnel",
    "broker",
    "watcher",
    "indexer",
    "scheduler",
    "syncthing",
    "dropbox",
    "onedrive",
    "rclone",
    "wsl",
    "tailscaled",
    "nordvpnd",
    "expressvpnd",
    "wireguard-service",
];

const SYSTEM_NAMES: &[&str] = &[
    "system",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "services.exe",
    "lsass.exe",
    "winlogon.exe",
    "svchost.exe",
    "dwm.exe",
    "explorer.exe",
    "fontdrvhost.exe",
    "ctfmon.exe",
    "taskhostw.exe",
    "runtimebroker.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "systemsettings.exe",
    "applicationframehost.exe",
    "sihost.exe",
    "conhost.exe",
    "memcompression",
];

fn is_system(name_lower: &str) -> bool {
    SYSTEM_NAMES.contains(&name_lower)
}

fn looks_headless(name_lower: &str) -> bool {
    HEADLESS_NAME_HINTS.iter().any(|h| name_lower.contains(h))
}

fn classify(
    name_lower: &str,
    has_window: bool,
    has_child_processes: bool,
    starts_at_logon: bool,
) -> (RuntimeType, bool, Vec<String>) {
    let mut tags = Vec::new();
    if starts_at_logon {
        tags.push("autostart".into());
    }
    if !has_window {
        tags.push("headless".into());
    }
    if has_child_processes {
        tags.push("has-children".into());
    }
    if looks_headless(name_lower) {
        tags.push("backend-name".into());
    }

    if is_system(name_lower) {
        return (RuntimeType::Unknown, false, tags);
    }

    let kind = if has_window {
        RuntimeType::TypeE
    } else if looks_headless(name_lower) && has_child_processes {
        RuntimeType::TypeA
    } else if !has_window && looks_headless(name_lower) {
        RuntimeType::TypeC
    } else if !has_window && starts_at_logon {
        RuntimeType::TypeB
    } else if !has_window {
        RuntimeType::TypeD
    } else {
        RuntimeType::Unknown
    };

    let hideable = matches!(
        kind,
        RuntimeType::TypeA | RuntimeType::TypeB | RuntimeType::TypeC | RuntimeType::TypeD
    );

    (kind, hideable, tags)
}

#[tauri::command]
pub fn scan_runtimes() -> Result<ScanResult, String> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let visible_pids = pids_with_visible_windows();
    let autostart = autostart_executables();

    let mut child_counts: HashMap<u32, u32> = HashMap::new();
    for proc in sys.processes().values() {
        if let Some(parent) = proc.parent() {
            *child_counts.entry(parent.as_u32()).or_insert(0) += 1;
        }
    }

    let mut runtimes = Vec::with_capacity(sys.processes().len());
    for (pid, proc) in sys.processes() {
        let pid_u32 = pid.as_u32();
        let name = proc.name().to_string_lossy().to_string();
        let name_lower = name.to_lowercase();
        let exe_path = proc.exe().map(|p| p.to_string_lossy().to_string());

        let has_window = visible_pids.contains(&pid_u32);
        let starts_at_logon = autostart.contains(&name_lower);
        let has_children = child_counts.get(&pid_u32).copied().unwrap_or(0) > 0;

        let (kind, hideable, tags) =
            classify(&name_lower, has_window, has_children, starts_at_logon);

        if matches!(kind, RuntimeType::Unknown) && is_system(&name_lower) {
            continue;
        }

        runtimes.push(DetectedRuntime {
            pid: pid_u32,
            parent_pid: proc.parent().map(|p| p.as_u32()),
            name,
            exe_path,
            has_visible_window: has_window,
            starts_at_logon,
            kind,
            hideable,
            tags,
        });
    }

    runtimes.sort_by(|a, b| {
        b.hideable
            .cmp(&a.hideable)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    let total_processes = sys.processes().len();
    Ok(ScanResult {
        runtimes,
        scanned_at_unix_ms: chrono::Utc::now().timestamp_millis(),
        total_processes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_quoted_paths_with_args() {
        assert_eq!(
            extract_exe_basename(r#""C:\Program Files\Syncthing\syncthing.exe" --no-console"#),
            Some("syncthing.exe".to_string())
        );
    }

    #[test]
    fn basename_handles_unquoted_simple_path() {
        assert_eq!(
            extract_exe_basename(r"C:\Windows\System32\foo.exe /silent"),
            Some("foo.exe".to_string())
        );
    }

    #[test]
    fn classifier_marks_visible_window_as_type_e() {
        let (kind, hideable, _) = classify("notepad.exe", true, false, false);
        assert_eq!(kind, RuntimeType::TypeE);
        assert!(!hideable);
    }

    #[test]
    fn classifier_marks_headless_with_children_as_type_a() {
        let (kind, hideable, _) = classify("syncthing-service", false, true, true);
        assert_eq!(kind, RuntimeType::TypeA);
        assert!(hideable);
    }

    #[test]
    fn classifier_marks_autostart_no_window_no_hint_as_type_b() {
        let (kind, hideable, _) = classify("randomtray.exe", false, false, true);
        assert_eq!(kind, RuntimeType::TypeB);
        assert!(hideable);
    }

    #[test]
    fn classifier_skips_windows_infrastructure() {
        let (kind, hideable, _) = classify("svchost.exe", false, true, false);
        assert_eq!(kind, RuntimeType::Unknown);
        assert!(!hideable);
    }
}
