//! Starts a locally installed ActivityWatch whenever Productivity is enabled.
//!
//! This deliberately lives outside the frontend and the paid-command router:
//! An enabled tracker should be available even when the Productivity panel is
//! hidden or the webview has not painted yet. Disabled installations do no
//! process scan, executable discovery, retry, or child-process work.

#[cfg(windows)]
use std::{
    net::{SocketAddr, TcpStream},
    path::Path,
    process::Command,
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use sysinfo::{Pid, ProcessesToUpdate, System};

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
    if matches!(
        error,
        "ActivityWatch is not installed" | "activitywatch_not_installed"
    ) {
        ActivityWatchSupervisorCode::NotInstalled
    } else if error == "activitywatch_local_api_unreachable"
        || error.contains("API is unreachable")
        || error.contains("did not become ready")
    {
        ActivityWatchSupervisorCode::LocalApiUnreachable
    } else if error == "activitywatch_watchers_unhealthy"
        || error.contains("watchers did not become healthy")
    {
        ActivityWatchSupervisorCode::WatchersUnhealthy
    } else if error == "activitywatch_disabled"
        || error.contains("disabled in Productivity settings")
    {
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
                        &format!(
                            "[ActivityWatch] auto-start attempt {attempt}/3 skipped: {}",
                            activity_watch_supervisor_code(&error).as_str()
                        ),
                    );
                    // A missing installation cannot heal during this launch.
                    if error == "activitywatch_not_installed" || attempt == 3 {
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
#[path = "activity_watch_installation.rs"]
mod installation;

#[cfg(windows)]
use installation::{discover_binaries, Binaries};

#[cfg(windows)]
#[derive(Clone, Copy)]
enum ActivityWatchProcess {
    Server,
    Tauri,
    AfkWatcher,
    WindowWatcher,
}

#[cfg(windows)]
#[derive(Default, Debug, PartialEq, Eq)]
struct SessionProcesses {
    server: usize,
    tauri_server: usize,
    afk_watcher: usize,
    window_watcher: usize,
    unknown_session: usize,
}

#[cfg(windows)]
impl SessionProcesses {
    fn add(&mut self, process: ActivityWatchProcess) {
        match process {
            ActivityWatchProcess::Server => self.server += 1,
            ActivityWatchProcess::Tauri => {
                self.server += 1;
                self.tauri_server += 1;
            }
            ActivityWatchProcess::AfkWatcher => self.afk_watcher += 1,
            ActivityWatchProcess::WindowWatcher => self.window_watcher += 1,
        }
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.unknown_session > 0 {
            return Err("activitywatch_session_unavailable");
        }
        if self.server > 1 {
            return Err("activitywatch_duplicate_server");
        }
        if self.afk_watcher > 1 {
            return Err("activitywatch_duplicate_afk_watcher");
        }
        if self.window_watcher > 1 {
            return Err("activitywatch_duplicate_window_watcher");
        }
        Ok(())
    }

    fn watcher_pair_running(&self) -> bool {
        self.afk_watcher == 1 && self.window_watcher == 1
    }
}

#[cfg(windows)]
fn ensure_started() -> Result<(), String> {
    static START_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = START_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "activitywatch_lock_unavailable".to_string())?;
    let running = running_processes().map_err(str::to_string)?;
    running.validate().map_err(str::to_string)?;
    let server_healthy = server_ready();

    // A healthy existing pair wins. We wait for watchers after launching them
    // below, so a second panel open cannot mistake a still-starting watcher
    // for a missing one and add another tray process.
    if server_healthy && running.server == 1 && running.watcher_pair_running() {
        crate::log_message(
            "info",
            "[ActivityWatch] existing server and watchers are healthy",
        );
        return Ok(());
    }

    if server_healthy && running.server == 0 {
        // Port 5600 is owned by a server outside this Windows session. Do not
        // attach this user's watchers to it or start a competing server.
        return Err("activitywatch_server_owned_elsewhere".to_string());
    }

    if running.tauri_server == 1 {
        return wait_for_watcher_pair();
    }

    let binaries = discover_binaries().ok_or_else(|| "activitywatch_not_installed".to_string())?;
    let Binaries::Classic {
        server,
        afk,
        window,
    } = binaries
    else {
        if running.server != 0 {
            return Err("activitywatch_local_api_unreachable".to_string());
        }
        if running.afk_watcher != 0 || running.window_watcher != 0 {
            return Err("activitywatch_watchers_unhealthy".to_string());
        }
        if let Binaries::Tauri(launcher) = binaries {
            start(&launcher)?;
        }
        if !server_ready() {
            return Err("activitywatch_local_api_unreachable".to_string());
        }
        return wait_for_watcher_pair();
    };
    if !server_healthy {
        if running.server == 1 {
            return Err("activitywatch_local_api_unreachable".to_string());
        }
        start(&server)?;
        // Watchers need the REST API to be accepting connections; starting
        // them before the server is ready can leave them disconnected.
        if !server_ready() {
            return Err("activitywatch_local_api_unreachable".to_string());
        }
    }

    // Re-scan after server readiness and then wait for the started processes.
    // The lock serializes callers in this app; the re-scan closes the gap with
    // the external ActivityWatch launcher without terminating its processes.
    let running = running_processes().map_err(str::to_string)?;
    running.validate().map_err(str::to_string)?;
    if !server_ready() || running.server != 1 {
        return Err("activitywatch_local_api_unreachable".to_string());
    }
    if running.tauri_server == 1 {
        return wait_for_watcher_pair();
    }
    if running.afk_watcher == 0 {
        start(&afk)?;
    }
    if running.window_watcher == 0 {
        start(&window)?;
    }

    wait_for_watcher_pair()?;

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
            return Err("activitywatch_disabled".to_string());
        }
        ensure_started()
    }
    #[cfg(not(windows))]
    {
        Err("activitywatch_windows_only".to_string())
    }
}

#[cfg(windows)]
fn running_processes() -> Result<SessionProcesses, &'static str> {
    let mut system = System::new().map_err(|_| "activitywatch_process_list_unavailable")?;
    system.refresh_processes(ProcessesToUpdate::All, false);
    let current_session = system
        .process(Pid::from_u32(std::process::id()))
        .and_then(|process| process.session_id())
        .ok_or("activitywatch_session_unavailable")?;
    let mut result = SessionProcesses::default();
    system
        .processes()
        .values()
        .filter_map(|process| {
            let process_kind = activity_watch_process(process.name().to_string_lossy().as_ref())?;
            Some((process_kind, process.session_id()))
        })
        .for_each(|(process_kind, session)| match session {
            Some(session) if session == current_session => result.add(process_kind),
            Some(_) => {}
            None => result.unknown_session += 1,
        });
    Ok(result)
}

#[cfg(windows)]
fn activity_watch_process(name: &str) -> Option<ActivityWatchProcess> {
    let name = name.to_ascii_lowercase();
    match name.strip_suffix(".exe").unwrap_or(&name) {
        "aw-server" | "aw-server-rust" => Some(ActivityWatchProcess::Server),
        "aw-tauri" => Some(ActivityWatchProcess::Tauri),
        "aw-watcher-afk" => Some(ActivityWatchProcess::AfkWatcher),
        "aw-watcher-window" => Some(ActivityWatchProcess::WindowWatcher),
        _ => None,
    }
}

#[cfg(windows)]
fn wait_for_watcher_pair() -> Result<(), String> {
    for _ in 0..20 {
        let running = running_processes().map_err(str::to_string)?;
        running.validate().map_err(str::to_string)?;
        if server_accepting_connections() && running.server == 1 && running.watcher_pair_running() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err("activitywatch_watchers_unhealthy".to_string())
}

/// Reads ActivityWatch's loopback API from Rust so the WebView is not blocked
/// by ActivityWatch's intentionally header-less local HTTP server. The path is
/// constrained to its API namespace; this is never a general-purpose proxy.
#[tauri::command]
pub async fn activity_watch_request(path: String) -> Result<serde_json::Value, String> {
    http::request(&path).await
}

#[path = "activity_watch_http.rs"]
mod http;

#[cfg(windows)]
fn server_ready() -> bool {
    for _ in 0..40 {
        if server_accepting_connections() {
            return true;
        }
        thread::sleep(Duration::from_millis(250));
    }
    false
}

#[cfg(windows)]
fn server_accepting_connections() -> bool {
    let address: SocketAddr = "127.0.0.1:5600".parse().expect("constant socket address");
    TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok()
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
        .map_err(|_| "activitywatch_start_failed".to_string())
}

#[cfg(all(test, windows))]
mod tests {
    use super::{activity_watch_process, ActivityWatchProcess, SessionProcesses};

    #[test]
    fn classifies_only_the_supported_activitywatch_components() {
        assert!(matches!(
            activity_watch_process("aw-tauri.exe"),
            Some(ActivityWatchProcess::Tauri)
        ));
        assert!(matches!(
            activity_watch_process("aw-server.exe"),
            Some(ActivityWatchProcess::Server)
        ));
        assert!(matches!(
            activity_watch_process("AW-WATCHER-AFK"),
            Some(ActivityWatchProcess::AfkWatcher)
        ));
        assert!(matches!(
            activity_watch_process("aw-watcher-window.exe"),
            Some(ActivityWatchProcess::WindowWatcher)
        ));
        assert!(activity_watch_process("aw-watcher-web").is_none());
        assert!(activity_watch_process("aw-tauri.exe.exe").is_none());
    }

    #[test]
    fn tauri_counts_as_one_server_and_conflicts_with_a_second_server() {
        let mut state = SessionProcesses::default();
        state.add(ActivityWatchProcess::Tauri);
        state.add(ActivityWatchProcess::AfkWatcher);
        state.add(ActivityWatchProcess::WindowWatcher);
        assert_eq!(state.validate(), Ok(()));
        assert_eq!(state.server, 1);
        assert_eq!(state.tauri_server, 1);
        assert!(state.watcher_pair_running());
        state.add(ActivityWatchProcess::Server);
        assert_eq!(state.validate(), Err("activitywatch_duplicate_server"));
    }

    #[test]
    fn rejects_duplicate_watchers_without_terminating_them() {
        let state = SessionProcesses {
            server: 1,
            tauri_server: 0,
            afk_watcher: 2,
            window_watcher: 1,
            unknown_session: 0,
        };
        assert_eq!(state.validate(), Err("activitywatch_duplicate_afk_watcher"));
    }

    #[test]
    fn accepts_exactly_one_server_and_watcher_pair() {
        let state = SessionProcesses {
            server: 1,
            tauri_server: 0,
            afk_watcher: 1,
            window_watcher: 1,
            unknown_session: 0,
        };
        assert_eq!(state.validate(), Ok(()));
        assert!(state.watcher_pair_running());
    }
}
