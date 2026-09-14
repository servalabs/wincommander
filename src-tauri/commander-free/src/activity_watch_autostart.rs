//! Starts a locally installed ActivityWatch whenever Productivity is enabled.
//!
//! This deliberately lives outside the frontend and the paid-command router:
//! An enabled tracker should be available even when the Productivity panel is
//! hidden or the webview has not painted yet. Disabled installations do no
//! process scan, executable discovery, retry, or child-process work.

#[cfg(windows)]
use std::{
    env,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use sysinfo::{ProcessesToUpdate, System};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Bounded, content-free local diagnosis that the Fleet reporter can carry
/// without exposing an ActivityWatch bucket, window title, URL, file path, or
/// the operating system's process error text. The reporting bridge owns the
/// timestamped delivery receipt; this supervisor owns only process/API facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityWatchSupervisorCode {
    NotInstalled,
    LocalApiUnreachable,
    WatchersUnhealthy,
    StartupDisabled,
    StartupFailed,
}

impl ActivityWatchSupervisorCode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::LocalApiUnreachable => "local_api_unreachable",
            Self::WatchersUnhealthy => "watchers_unhealthy",
            Self::StartupDisabled => "startup_disabled",
            Self::StartupFailed => "startup_failed",
        }
    }
}

/// Translate internal or OS-originated failures to the small public-safe
/// reason vocabulary. Keep this mapping string-only and side-effect free so
/// it can be used by a check-in producer without re-running supervision.
pub(crate) fn activity_watch_supervisor_code(error: &str) -> ActivityWatchSupervisorCode {
    if error == "ActivityWatch is not installed" {
        ActivityWatchSupervisorCode::NotInstalled
    } else if error.contains("API is unreachable") || error.contains("did not become ready") {
        ActivityWatchSupervisorCode::LocalApiUnreachable
    } else if error.contains("watchers did not become healthy") {
        ActivityWatchSupervisorCode::WatchersUnhealthy
    } else if error.contains("disabled in Productivity settings") {
        ActivityWatchSupervisorCode::StartupDisabled
    } else {
        ActivityWatchSupervisorCode::StartupFailed
    }
}

/// Begin the one-shot ActivityWatch supervisor without delaying app startup.
pub fn init() {
    #[cfg(windows)]
    if !is_configured() {
        return;
    }

    #[cfg(windows)]
    thread::spawn(|| {
        // Let the single-instance guard, tray, and WebView initialize first.
        thread::sleep(Duration::from_secs(3));
        for attempt in 1..=3 {
            match ensure_started() {
                Ok(()) => return,
                Err(error) => {
                    crate::log_message(
                        "warn",
                        &format!("[ActivityWatch] auto-start attempt {attempt}/3 skipped: {error}"),
                    );
                    // A missing installation cannot heal during this launch.
                    if matches!(
                        activity_watch_supervisor_code(&error),
                        ActivityWatchSupervisorCode::NotInstalled
                    ) || attempt == 3
                    {
                        return;
                    }
                    thread::sleep(Duration::from_secs(attempt as u64 * 5));
                }
            }
        }
    });
}

#[cfg(windows)]
fn is_configured() -> bool {
    crate::settings::read_settings().is_ok_and(|settings| {
        settings
            .app
            .modules
            .get("productivity")
            .copied()
            .unwrap_or(false)
    })
}

#[cfg(windows)]
#[derive(Debug)]
struct Binaries {
    server: PathBuf,
    afk: PathBuf,
    window: PathBuf,
}

#[cfg(windows)]
fn ensure_started() -> Result<(), String> {
    static START_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = START_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "ActivityWatch start lock unavailable".to_string())?;
    let running = running_processes();
    let server_running = running
        .iter()
        .any(|name| name == "aw-server" || name == "aw-server-rust");
    let server_healthy = server_ready();
    let afk_running = running.iter().any(|name| name == "aw-watcher-afk");
    let window_running = running.iter().any(|name| name == "aw-watcher-window");

    // A healthy existing instance wins. Do not require its original installer
    // path just to use it, and never start a second server process.
    if server_healthy && afk_running && window_running {
        crate::log_message(
            "info",
            "[ActivityWatch] existing server and watchers are healthy",
        );
        return Ok(());
    }

    let binaries =
        discover_binaries().ok_or_else(|| "ActivityWatch is not installed".to_string())?;
    if !server_healthy {
        if server_running {
            return Err(
                "ActivityWatch server process is running but its API is unreachable".to_string(),
            );
        }
        start(&binaries.server)?;
        // Watchers need the REST API to be accepting connections; starting
        // them before the server is ready can leave them disconnected.
        if !server_ready() {
            return Err("ActivityWatch server did not become ready on port 5600".to_string());
        }
    }

    // Re-scan after server readiness so a concurrent launcher cannot cause us
    // to spawn a duplicate watcher between the initial health check and here.
    let running = running_processes();
    if !running.iter().any(|name| name == "aw-watcher-afk") {
        start(&binaries.afk)?;
    }
    if !running.iter().any(|name| name == "aw-watcher-window") {
        start(&binaries.window)?;
    }

    // Process spawn only tells us Windows accepted the launch request. Wait a
    // bounded amount for BOTH required watchers before declaring a healthy
    // instance; otherwise the collector could silently read a server with no
    // fresh window/AFK activity. Subsequent retries re-scan first, so this
    // cannot spawn a duplicate watcher while one is still starting.
    if !watchers_ready() {
        return Err("ActivityWatch watchers did not become healthy".to_string());
    }

    crate::log_message(
        "info",
        "[ActivityWatch] server and watchers ensured at startup",
    );
    Ok(())
}

/// Verify or start ActivityWatch through the single native supervisor. The
/// Productivity panel calls this on open, which covers a tracker stopped after
/// Commander started without racing a browser-side process launcher.
#[tauri::command]
pub async fn activity_watch_ensure_started() -> Result<(), String> {
    #[cfg(windows)]
    {
        if !is_configured() {
            return Err("ActivityWatch startup is disabled in Productivity settings".to_string());
        }
        ensure_started()
    }
    #[cfg(not(windows))]
    {
        Err("ActivityWatch supervision is available only on Windows".to_string())
    }
}

#[cfg(windows)]
fn running_processes() -> Vec<String> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, false);
    system
        .processes()
        .values()
        .map(|process| {
            process
                .name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .trim_end_matches(".exe")
                .to_string()
        })
        .collect()
}

/// Reads ActivityWatch's loopback API from Rust so the WebView is not blocked
/// by ActivityWatch's intentionally header-less local HTTP server. The path is
/// constrained to its API namespace; this is never a general-purpose proxy.
#[tauri::command]
pub async fn activity_watch_request(path: String) -> Result<serde_json::Value, String> {
    if !path.starts_with("/api/0/")
        || path.len() > 4_096
        || path.contains(['\\', '#', '@'])
        || path.contains("//")
    {
        return Err("Invalid ActivityWatch API path".to_string());
    }

    let url = format!("http://127.0.0.1:5600{path}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|error| format!("ActivityWatch client setup failed: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "ActivityWatch is not running.".to_string())?;
    if !response.status().is_success() {
        return Err(format!("ActivityWatch returned HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > 64 * 1024 * 1024)
    {
        return Err("ActivityWatch returned an oversized response.".to_string());
    }
    response
        .json::<serde_json::Value>()
        .await
        .map_err(|_| "ActivityWatch returned invalid JSON.".to_string())
}

#[cfg(windows)]
fn server_ready() -> bool {
    let address: SocketAddr = "127.0.0.1:5600".parse().expect("constant socket address");
    for _ in 0..40 {
        if TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(250));
    }
    false
}

#[cfg(windows)]
fn watchers_ready() -> bool {
    for _ in 0..20 {
        let running = running_processes();
        let afk = running.iter().any(|name| name == "aw-watcher-afk");
        let window = running.iter().any(|name| name == "aw-watcher-window");
        if afk && window {
            return true;
        }
        thread::sleep(Duration::from_millis(250));
    }
    false
}

#[cfg(windows)]
fn start(executable: &Path) -> Result<(), String> {
    let working_directory = executable
        .parent()
        .ok_or_else(|| format!("invalid executable path: {}", executable.display()))?;
    Command::new(executable)
        .current_dir(working_directory)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        // The raw OS error can include a user-specific installation path.
        // A local caller only needs the safe category; detailed diagnostics
        // remain in the local process log, never in Fleet delivery metadata.
        .map_err(|_| "ActivityWatch process could not be started".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervisor_errors_map_to_bounded_safe_codes() {
        assert_eq!(
            activity_watch_supervisor_code("ActivityWatch is not installed").as_str(),
            "not_installed"
        );
        assert_eq!(
            activity_watch_supervisor_code(
                "ActivityWatch server process is running but its API is unreachable"
            )
            .as_str(),
            "local_api_unreachable"
        );
        assert_eq!(
            activity_watch_supervisor_code("ActivityWatch watchers did not become healthy")
                .as_str(),
            "watchers_unhealthy"
        );
        assert_eq!(
            activity_watch_supervisor_code("start C:\\Users\\example: access denied").as_str(),
            "startup_failed"
        );
    }
}

#[cfg(windows)]
fn discover_binaries() -> Option<Binaries> {
    let mut roots = Vec::new();
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let local_app_data = PathBuf::from(local_app_data);
        roots.push(local_app_data.join("Programs").join("ActivityWatch"));
        let winget_packages = local_app_data
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");
        if let Ok(entries) = std::fs::read_dir(winget_packages) {
            roots.extend(
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| {
                                name.to_ascii_lowercase().starts_with("activitywatch")
                            })
                    }),
            );
        }
    }
    if let Some(program_files) = env::var_os("ProgramFiles") {
        roots.push(PathBuf::from(program_files).join("ActivityWatch"));
    }
    if let Some(program_files_x86) = env::var_os("ProgramFiles(x86)") {
        roots.push(PathBuf::from(program_files_x86).join("ActivityWatch"));
    }

    roots.into_iter().find_map(|root| {
        let server = [
            "aw-server\\aw-server.exe",
            "aw-server-rust\\aw-server-rust.exe",
            "aw-server-rust\\aw-server.exe",
            "aw-server.exe",
        ]
        .into_iter()
        .map(|relative| root.join(relative))
        .find(|path| path.is_file())?;
        let afk = root.join("aw-watcher-afk\\aw-watcher-afk.exe");
        let window = root.join("aw-watcher-window\\aw-watcher-window.exe");
        (afk.is_file() && window.is_file()).then_some(Binaries {
            server,
            afk,
            window,
        })
    })
}
