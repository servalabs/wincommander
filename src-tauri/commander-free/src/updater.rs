// src-tauri/commander-free/src/updater.rs
//
// Background self-update scheduler for WinCommander.
//
// Why this lives in Rust (not the old JS setInterval in UpdaterStatus.tsx):
// WinCommander runs minimized in the tray for long stretches, and a hidden
// webview's timers get throttled/suspended by the OS — so a JS interval can
// silently stop checking once the window is minimized, and it only started at
// all when the Dashboard panel mounted. A tokio task runs regardless of window
// state and from process start, fixing both the "never re-checks after booting
// offline" and "stops checking after days of uptime" bugs.
//
// Flow (every 7 days normally, ~5min adaptive retry while offline/erroring):
//   check ─┬─ no update ───────────────→ emit "idle"
//          ├─ update, autoUpdate OFF ───→ emit "available"
//          └─ update, autoUpdate ON ────→ download+verify ─→ stage bytes ─→ emit "staged"
// The frontend RestartPrompt turns "staged" into a "Restart now / Later"
// prompt; `app_install_staged_update` installs the already-downloaded bytes.
//
// Security: host + minisign pubkey stay pinned in tauri.conf.json; `download`
// verifies the signature before returning bytes. Nothing here accepts a URL or
// hash from the frontend. See AGENTS.md security rules.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

#[cfg(feature = "experimental-pq-free-release")]
#[path = "updater/hybrid_free.rs"]
mod hybrid_free;

/// In the experimental build every installation goes through this one gate.
pub(crate) async fn install_update(
    update: &tauri_plugin_updater::Update,
    channel: crate::settings::UpdateChannel,
    staged: Option<Vec<u8>>,
) -> Result<(), String> {
    #[cfg(feature = "experimental-pq-free-release")]
    {
        let verified = hybrid_free::verified_bytes(update, channel, staged).await?;
        require_selected_channel(channel)?;
        let _floor_lock = hybrid_free::record_before_install(&verified)?;
        require_selected_channel(channel)?;
        update
            .install(verified.bytes)
            .map_err(|e| format!("Install failed: {e}"))
    }
    #[cfg(not(feature = "experimental-pq-free-release"))]
    {
        let _ = channel;
        match staged {
            Some(bytes) => update
                .install(bytes)
                .map_err(|e| format!("Install failed: {e}")),
            None => update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(|e| format!("Install failed: {e}")),
        }
    }
}

async fn download_for_staging(
    update: &tauri_plugin_updater::Update,
    channel: crate::settings::UpdateChannel,
) -> Result<Vec<u8>, String> {
    #[cfg(feature = "experimental-pq-free-release")]
    {
        hybrid_free::verified_bytes(update, channel, None)
            .await
            .map(|verified| verified.bytes)
    }
    #[cfg(not(feature = "experimental-pq-free-release"))]
    {
        let _ = channel;
        update
            .download(|_, _| {}, || {})
            .await
            .map_err(|e| e.to_string())
    }
}

const INITIAL_DELAY: Duration = Duration::from_secs(30);
const NORMAL_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60); // 7 days
const RETRY_INTERVAL: Duration = Duration::from_secs(5 * 60); // ~5min when offline
pub(crate) const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
/// A release installer can take longer than an ordinary manifest request, but
/// it must still return control to the UI. This bounds a stalled download,
/// verification, or installer hand-off and lets the user retry instead of
/// leaving the update dialog on an endless spinner.
pub(crate) const INSTALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Holds the verified installer bytes downloaded in the background so the
/// "Restart now" command can install them without re-downloading.
/// `installing` is a shared install lock: both `app_install_staged_update`
/// and `app_install_update_doh` CAS it true before proceeding, preventing
/// concurrent install attempts from a double-click or race.
#[derive(Default)]
pub struct StagedState {
    inner: Mutex<Option<StagedUpdate>>,
    pub installing: AtomicBool,
    /// Last state emitted over `updater://state`, so a late-mounting frontend
    /// (e.g. one still behind the calculator/startup-PIN gate when the Rust
    /// scheduler ran its 30s-post-launch check) can hydrate via
    /// `updater_current_state` instead of missing the fire-and-forget event.
    last_state: Mutex<Option<StatePayload>>,
}

static INSTALLED_PENDING_RESTART_VERSION: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn effective_free_version() -> String {
    INSTALLED_PENDING_RESTART_VERSION
        .lock()
        .ok()
        .and_then(|version| version.clone())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

struct StagedUpdate {
    version: String,
    channel: crate::settings::UpdateChannel,
    bytes: Vec<u8>,
}

/// Consume staged installer bytes only when the newly checked, signed manifest
/// still names the exact same release. A manifest can advance while the app is
/// running; installing older staged bytes through the newer `Update` handle
/// would make the displayed release metadata diverge from the installer.
fn take_staged_for_version(
    staged: &mut Option<StagedUpdate>,
    version: &str,
    channel: crate::settings::UpdateChannel,
) -> Option<Vec<u8>> {
    match staged.take() {
        Some(candidate) if candidate.version == version && candidate.channel == channel => {
            Some(candidate.bytes)
        }
        Some(_) | None => None,
    }
}

#[derive(serde::Serialize, Clone)]
pub struct StatePayload {
    phase: &'static str,
    version: Option<String>,
    current_version: Option<String>,
    body: Option<String>,
    error: Option<String>,
}

fn emit(
    app: &AppHandle,
    phase: &'static str,
    version: Option<String>,
    current_version: Option<String>,
    body: Option<String>,
    error: Option<String>,
) {
    let payload = StatePayload {
        phase,
        version,
        current_version,
        body,
        error,
    };
    // Cache the latest state so `updater_current_state` can replay it to a
    // frontend that attached its listener after this event already fired.
    if let Some(state) = app.try_state::<StagedState>() {
        if let Ok(mut guard) = state.last_state.lock() {
            *guard = Some(payload.clone());
        }
    }
    let _ = app.emit("updater://state", payload);
}

/// Replay the last emitted updater state to a frontend that attached its
/// listener late (behind the calculator/startup-PIN gate) and would otherwise
/// have missed the fire-and-forget `updater://state` event until the next cycle.
#[tauri::command]
pub fn updater_current_state(app: AppHandle) -> Option<StatePayload> {
    app.try_state::<StagedState>()
        .and_then(|s| s.last_state.lock().ok().and_then(|g| g.clone()))
}

/// Emit the terminal "ready" phase — bytes installed to disk, only a relaunch
/// remains. Distinct from "staged" (downloaded, not yet installed) so the UI
/// can show a persistent Restart control. `pub(crate)` so lib.rs's DoH install
/// path can reuse it without widening `emit`/`StatePayload` visibility.
pub(crate) fn emit_ready(
    app: &AppHandle,
    version: Option<String>,
    current_version: Option<String>,
    body: Option<String>,
) {
    if let Some(installed) = version.as_ref() {
        if let Ok(mut pending) = INSTALLED_PENDING_RESTART_VERSION.lock() {
            *pending = Some(installed.clone());
        }
    }
    emit(app, "ready", version, current_version, body, None);
}

/// Self-update preference. Defaults ON when settings can't be read — matches
/// the schema default (`auto_update: default_true`) and the product decision.
fn auto_update_enabled() -> bool {
    crate::settings::read_settings()
        .map(|s| s.app.auto_update)
        .unwrap_or(true)
}

/// Developer kill-switch for the whole update subsystem. When true, the
/// scheduler does nothing and emits nothing. Defaults OFF (updates on) when
/// settings can't be read. Re-read every cycle so toggling it off in Dev
/// Options resumes checks without a restart.
fn updates_disabled() -> bool {
    crate::settings::read_settings()
        .map(|s| s.app.disable_updates)
        .unwrap_or(false)
}

fn preserve_app_until_installer_launch(
    builder: tauri_plugin_updater::UpdaterBuilder,
) -> tauri_plugin_updater::UpdaterBuilder {
    // The plugin's default cleanup runs before ShellExecute can fail or be cancelled.
    // Its successful Windows launch exits the process itself; errors must leave the UI usable.
    builder.on_before_exit(|| {})
}

pub(crate) fn selected_update_channel() -> Result<crate::settings::UpdateChannel, String> {
    Ok(crate::settings::read_settings()?.app.update_channel)
}

pub(crate) fn require_selected_channel(
    channel: crate::settings::UpdateChannel,
) -> Result<(), String> {
    if selected_update_channel()? == channel {
        Ok(())
    } else {
        Err("Update channel changed. Check again before installing.".into())
    }
}

pub(crate) fn clear_staged(app: &AppHandle) {
    if let Some(state) = app.try_state::<StagedState>() {
        if let Ok(mut staged) = state.inner.lock() {
            *staged = None;
        }
        if let Ok(mut last) = state.last_state.lock() {
            *last = None;
        }
    }
    emit(app, "idle", None, None, None, None);
}

pub(crate) fn build_updater_for_channel(
    app: &AppHandle,
    channel: crate::settings::UpdateChannel,
) -> Result<tauri_plugin_updater::Updater, String> {
    let tls = crate::net::required_hybrid_tls_config()?;
    preserve_app_until_installer_launch(app.updater_builder())
        .endpoints(crate::cache_busted_endpoints(app, channel)?)
        .map_err(|e| format!("Updater endpoints failed: {}", e))?
        .configure_client(move |cb| crate::net::configure_required_hybrid_client(cb, tls.clone()))
        .build()
        .map_err(|e| format!("Updater build failed: {}", e))
}

fn already_staged(app: &AppHandle, version: &str, channel: crate::settings::UpdateChannel) -> bool {
    if let Some(state) = app.try_state::<StagedState>() {
        if let Ok(guard) = state.inner.lock() {
            if let Some(staged) = guard.as_ref() {
                return staged.version == version && staged.channel == channel;
            }
        }
    }
    false
}

/// Run one check cycle; returns how long to wait before the next one.
async fn run_cycle(app: &AppHandle) -> Duration {
    // Dev build: the update subsystem is fully off. The dev server must never
    // contact the update endpoint, download an artifact, or surface a restart
    // prompt. `cfg!(debug_assertions)` is true under `tauri dev` and false in
    // `--release`, so production behaviour is unchanged. Kept as a runtime guard
    // (not `#[cfg]`) so the rest of the cycle still compiles — no dead-code noise.
    if cfg!(debug_assertions) {
        return NORMAL_INTERVAL;
    }

    // Dev option: updates fully disabled. Emit nothing (no "checking" flicker,
    // no banner, no prompt) and re-poll the setting on the normal cadence so
    // turning it back off resumes checks without a restart.
    if updates_disabled() {
        return NORMAL_INTERVAL;
    }

    crate::log_message_src("info", "core", "[Updater] cycle start: checking for update");
    emit(app, "checking", None, None, None, None);

    let channel = match selected_update_channel() {
        Ok(channel) => channel,
        Err(error) => {
            emit(app, "error", None, None, None, Some(error));
            return RETRY_INTERVAL;
        }
    };
    let updater = match build_updater_for_channel(app, channel) {
        Ok(u) => u,
        Err(e) => {
            crate::log_message_src(
                "error",
                "core",
                &format!("[Updater] build_updater failed: {}", e),
            );
            emit(app, "error", None, None, None, Some(e));
            return RETRY_INTERVAL;
        }
    };

    crate::log_message_src(
        "info",
        "core",
        "[Updater] manifest fetch: configured endpoint",
    );
    let checked = match tokio::time::timeout(CHECK_TIMEOUT, updater.check()).await {
        // Timed out → treat like offline; retry soon so a reconnect recovers.
        Err(_) => {
            crate::log_message_src("warn", "core", "[Updater] manifest fetch timed out");
            emit(
                app,
                "error",
                None,
                None,
                None,
                Some("Update check timed out".to_string()),
            );
            // No server contact occurred (timeout before response), skip flush.
            return RETRY_INTERVAL;
        }
        // Network/DNS error → retry soon (this is the no-network-at-startup path).
        Ok(Err(e)) => {
            crate::log_message_src(
                "warn",
                "core",
                &format!("[Updater] manifest fetch error: {}", e),
            );
            emit(
                app,
                "error",
                None,
                None,
                None,
                Some(format!("Update check failed: {}", e)),
            );
            return RETRY_INTERVAL;
        }
        Ok(Ok(maybe)) => maybe,
    };
    if require_selected_channel(channel).is_err() {
        return RETRY_INTERVAL;
    }

    let update = match checked {
        None => {
            // Server was contacted and responded: no update available.
            let current = env!("CARGO_PKG_VERSION");
            crate::log_message_src(
                "info",
                "core",
                &format!("[Updater] version compare: current={} → no update", current),
            );
            emit(app, "idle", None, None, None, None);
            return NORMAL_INTERVAL;
        }
        Some(u) => u,
    };

    let version = update.version.clone();
    let current = update.current_version.clone();
    let body = update.body.clone();

    crate::log_message_src(
        "info",
        "core",
        &format!(
            "[Updater] version compare: current={} available={}",
            current, version
        ),
    );

    if !auto_update_enabled() {
        crate::log_message_src(
            "info",
            "core",
            &format!(
                "[Updater] auto-update disabled; emitting available version={}",
                version
            ),
        );
        emit(app, "available", Some(version), Some(current), body, None);
        return NORMAL_INTERVAL;
    }

    // Already staged this exact version? Re-surface the prompt without burning
    // bandwidth re-downloading the same artifact every NORMAL_INTERVAL (7 days).
    if already_staged(app, &version, channel) {
        crate::log_message_src(
            "info",
            "core",
            &format!(
                "[Updater] version={} already staged, re-surfacing prompt",
                version
            ),
        );
        emit(app, "staged", Some(version), Some(current), body, None);
        return NORMAL_INTERVAL;
    }

    crate::log_message_src(
        "info",
        "core",
        &format!("[Updater] downloading version={}", version),
    );
    emit(
        app,
        "downloading",
        Some(version.clone()),
        Some(current.clone()),
        body.clone(),
        None,
    );
    match download_for_staging(&update, channel).await {
        Ok(bytes) => {
            if require_selected_channel(channel).is_err() {
                return RETRY_INTERVAL;
            }
            if let Some(state) = app.try_state::<StagedState>() {
                if let Ok(mut guard) = state.inner.lock() {
                    *guard = Some(StagedUpdate {
                        version: version.clone(),
                        channel,
                        bytes,
                    });
                }
            }
            if require_selected_channel(channel).is_err() {
                clear_staged(app);
                return RETRY_INTERVAL;
            }
            crate::log_message_src(
                "info",
                "core",
                &format!("[Updater] download complete, version={} staged", version),
            );
            emit(app, "staged", Some(version), Some(current), body, None);
            NORMAL_INTERVAL
        }
        Err(e) => {
            crate::log_message_src(
                "error",
                "core",
                &format!("[Updater] download failed: {}", e),
            );
            emit(
                app,
                "error",
                None,
                None,
                None,
                Some(format!("Download failed: {}", e)),
            );
            RETRY_INTERVAL
        }
    }
}

/// Spawn the background scheduler. Call once from `run()`'s setup hook.
pub fn init(app: &AppHandle) {
    init_headless(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(INITIAL_DELAY).await;
        loop {
            let wait = run_cycle(&app).await;
            tokio::time::sleep(wait).await;
        }
    });
}

/// Register only the in-memory updater state required by updater commands.
/// The CLI uses this path so invoking an unrelated command never starts the
/// background update scheduler or performs network I/O as a startup side effect.
pub(crate) fn init_headless(app: &AppHandle) {
    if app.try_state::<StagedState>().is_none() {
        app.manage(StagedState::default());
    }
}

fn update_install_authorized(process_is_elevated: bool) -> Result<(), String> {
    if process_is_elevated {
        Ok(())
    } else {
        Err("Administrator approval is required to install updates. Reopen WinCommander as an administrator and try again.".to_string())
    }
}

pub(crate) fn require_update_administrator() -> Result<(), String> {
    #[cfg(windows)]
    return update_install_authorized(crate::startup_elevation::is_current_process_elevated());
    #[cfg(not(windows))]
    update_install_authorized(true)
}

/// Install the update staged in the background, then the frontend relaunches.
/// Falls back to a fresh download+install if nothing is staged (e.g. the
/// process restarted since the artifact was downloaded). Free-tier command.
///
/// The `installing` AtomicBool on StagedState is a shared install lock used
/// by both this command and `app_install_update_doh`. The CAS prevents two
/// concurrent installs from racing (double-click, two code paths, etc.).
/// Reset to false on error so the user can retry; success → app relaunches.
#[tauri::command]
pub async fn app_install_staged_update(app: AppHandle) -> Result<(), String> {
    require_update_administrator()?;
    // Acquire install lock — synchronous, before any await.
    {
        let state = app
            .try_state::<StagedState>()
            .ok_or_else(|| "Updater state unavailable".to_string())?;
        if state.installing.swap(true, Ordering::AcqRel) {
            return Err("Update install already in progress".to_string());
        }
    }

    // A fresh Update handle is needed to drive install. Keep any staged bytes
    // until this re-check completes: they are reusable only if its manifest
    // names the same version, and retaining them lets a transient re-check
    // failure be retried without a needless re-download.
    let channel = match selected_update_channel() {
        Ok(channel) => channel,
        Err(error) => {
            if let Some(s) = app.try_state::<StagedState>() {
                s.installing.store(false, Ordering::Release);
            }
            return Err(error);
        }
    };
    let updater = build_updater_for_channel(&app, channel);
    let update = match updater {
        Ok(u) => match tokio::time::timeout(CHECK_TIMEOUT, u.check()).await {
            Err(_) => {
                if let Some(s) = app.try_state::<StagedState>() {
                    s.installing.store(false, Ordering::Release);
                }
                return Err("Update re-check timed out".to_string());
            }
            Ok(Ok(Some(up))) => up,
            Ok(Ok(None)) => {
                if let Some(s) = app.try_state::<StagedState>() {
                    s.installing.store(false, Ordering::Release);
                }
                return Err("No update available".to_string());
            }
            Ok(Err(e)) => {
                if let Some(s) = app.try_state::<StagedState>() {
                    s.installing.store(false, Ordering::Release);
                }
                return Err(format!("Update re-check failed: {}", e));
            }
        },
        Err(e) => {
            if let Some(s) = app.try_state::<StagedState>() {
                s.installing.store(false, Ordering::Release);
            }
            return Err(e);
        }
    };
    if let Err(error) = require_selected_channel(channel) {
        if let Some(s) = app.try_state::<StagedState>() {
            s.installing.store(false, Ordering::Release);
        }
        return Err(error);
    }

    // Captured before `install()`/`download_and_install()` consume `update`, so
    // we can emit the terminal "ready" phase (installed, awaiting relaunch).
    let target_version = update.version.clone();
    let target_current = update.current_version.clone();
    let target_body = update.body.clone();

    let staged = app.try_state::<StagedState>().and_then(|s| {
        s.inner
            .lock()
            .ok()
            .and_then(|mut state| take_staged_for_version(&mut state, &target_version, channel))
    });

    let result = tokio::time::timeout(INSTALL_TIMEOUT, install_update(&update, channel, staged))
        .await
        .unwrap_or_else(|_| Err("Update installation timed out".to_string()));

    if result.is_ok() {
        // Bytes are on disk; surface a persistent Restart affordance in case the
        // user dismisses the prompt instead of relaunching immediately.
        emit_ready(
            &app,
            Some(target_version),
            Some(target_current),
            target_body,
        );
    } else if let Some(s) = app.try_state::<StagedState>() {
        s.installing.store(false, Ordering::Release);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{take_staged_for_version, StagedUpdate};
    use crate::settings::UpdateChannel;

    #[test]
    fn update_install_requires_an_actually_elevated_token() {
        assert!(super::update_install_authorized(false)
            .unwrap_err()
            .contains("Administrator approval"));
        assert!(super::update_install_authorized(true).is_ok());
    }

    #[test]
    fn staged_installer_is_reused_only_for_the_manifested_version() {
        let mut matching = Some(StagedUpdate {
            version: "4.2.0".into(),
            channel: UpdateChannel::Stable,
            bytes: vec![1, 2, 3],
        });
        assert_eq!(
            take_staged_for_version(&mut matching, "4.2.0", UpdateChannel::Stable),
            Some(vec![1, 2, 3])
        );
        assert!(matching.is_none());

        let mut stale = Some(StagedUpdate {
            version: "4.2.0".into(),
            channel: UpdateChannel::Beta,
            bytes: vec![4, 5, 6],
        });
        assert_eq!(
            take_staged_for_version(&mut stale, "4.2.0", UpdateChannel::Stable),
            None
        );
        assert!(stale.is_none());
    }
}

#[cfg(all(test, windows))]
#[path = "updater_launch_tests.rs"]
mod launch_tests;
