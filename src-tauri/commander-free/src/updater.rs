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

const INITIAL_DELAY: Duration = Duration::from_secs(30);
const NORMAL_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60); // 7 days
const RETRY_INTERVAL: Duration = Duration::from_secs(5 * 60); // ~5min when offline
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);

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

struct StagedUpdate {
    version: String,
    bytes: Vec<u8>,
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

fn build_updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    app.updater_builder()
        .endpoints(crate::cache_busted_endpoints(app)?)
        .map_err(|e| format!("Updater endpoints failed: {}", e))?
        .configure_client(|cb| cb.dns_resolver(crate::net::doh_resolver()))
        .build()
        .map_err(|e| format!("Updater build failed: {}", e))
}

fn already_staged(app: &AppHandle, version: &str) -> bool {
    if let Some(state) = app.try_state::<StagedState>() {
        if let Ok(guard) = state.inner.lock() {
            if let Some(staged) = guard.as_ref() {
                return staged.version == version;
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

    let updater = match build_updater(app) {
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
    if already_staged(app, &version) {
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
    match update.download(|_, _| {}, || {}).await {
        Ok(bytes) => {
            if let Some(state) = app.try_state::<StagedState>() {
                if let Ok(mut guard) = state.inner.lock() {
                    *guard = Some(StagedUpdate {
                        version: version.clone(),
                        bytes,
                    });
                }
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
    // Acquire install lock — synchronous, before any await.
    {
        let state = app
            .try_state::<StagedState>()
            .ok_or_else(|| "Updater state unavailable".to_string())?;
        if state.installing.swap(true, Ordering::AcqRel) {
            return Err("Update install already in progress".to_string());
        }
    }

    // Take staged bytes out of the Mutex before await points.
    let staged = app
        .try_state::<StagedState>()
        .and_then(|s| s.inner.lock().ok().and_then(|mut g| g.take()));

    // A fresh Update handle is needed to drive install(); reusing already-
    // verified staged bytes avoids a re-download while staying correct.
    let updater = build_updater(&app);
    let update = match updater {
        Ok(u) => match u.check().await {
            Ok(Some(up)) => up,
            Ok(None) => {
                if let Some(s) = app.try_state::<StagedState>() {
                    s.installing.store(false, Ordering::Release);
                }
                return Err("No update available".to_string());
            }
            Err(e) => {
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

    // Captured before `install()`/`download_and_install()` consume `update`, so
    // we can emit the terminal "ready" phase (installed, awaiting relaunch).
    let target_version = update.version.clone();
    let target_current = update.current_version.clone();
    let target_body = update.body.clone();

    let result = match staged {
        Some(st) => update
            .install(st.bytes)
            .map_err(|e| format!("Install failed: {}", e)),
        None => update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| format!("Install failed: {}", e)),
    };

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
