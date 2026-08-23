//! Starts a locally installed ActivityWatch when its module and tracker are enabled.
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
    thread,
    time::Duration,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use sysinfo::{ProcessesToUpdate, System};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
                    if error == "ActivityWatch is not installed" || attempt == 3 {
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
            && settings.ideal.productivity.tracker_enabled.unwrap_or(false)
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
    let binaries =
        discover_binaries().ok_or_else(|| "ActivityWatch is not installed".to_string())?;
    let running = running_processes();

    if !running
        .iter()
        .any(|name| name == "aw-server" || name == "aw-server-rust")
    {
        start(&binaries.server)?;
    }

    // Watchers need the REST API to be accepting connections; starting them
    // before the server is ready can leave them silently disconnected.
    if !server_ready() {
        return Err("ActivityWatch server did not become ready on port 5600".to_string());
    }

    if !running.iter().any(|name| name == "aw-watcher-afk") {
        start(&binaries.afk)?;
    }
    if !running.iter().any(|name| name == "aw-watcher-window") {
        start(&binaries.window)?;
    }

    crate::log_message(
        "info",
        "[ActivityWatch] server and watchers ensured at startup",
    );
    Ok(())
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
fn start(executable: &Path) -> Result<(), String> {
    let working_directory = executable
        .parent()
        .ok_or_else(|| format!("invalid executable path: {}", executable.display()))?;
    Command::new(executable)
        .current_dir(working_directory)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("start {}: {error}", executable.display()))
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
