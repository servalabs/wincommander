// src-tauri/src/sidecar.rs (commander-free crate)
// ═══════════════════════════════════════════════════════════════════════
// Pro-sidecar broker
// ═══════════════════════════════════════════════════════════════════════
//
// Spawns the WinCommander Pro binary on demand, performs the named-pipe
// handshake, and exposes a per-request dispatcher. Phase 7 establishes
// the wire transport; phase 6b/3b migrates the actual paid command
// implementations into Pro and routes requests through here.
//
// Lifecycle (per spawn):
//   1. spawn_pro() generates a random pipe name + 32-byte session token,
//      creates the named-pipe server end, then launches the Pro binary
//      with --core-pipe=<name> --session-token=<token>.
//   2. Pro connects as client, sends its Hello with the echoed token +
//      the SHA-256 hex of itself in `binary_hash`.
//   3. Free verifies token match + (when phase 8 lands) pinned binary
//      hash, then keeps the connection in a ProSession.
//   4. dispatch() writes Requests, awaits Responses keyed by request_id.
//   5. close() sends Bye and waits for the child to exit (with a short
//      kill timeout if Pro misbehaves).
//
// Today this module exposes a single Tauri command, `test_pro_handshake`,
// which spawns Pro, does the handshake, and returns success or the error
// reason. It's a smoke-test surface for phase 7b verification — phase 8
// replaces it with the proper install-aware spawn flow.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
// tokio::process::Command exposes creation_flags() as an inherent method
// on Windows — no `use std::os::windows::process::CommandExt` needed.

use tauri::{AppHandle, Emitter};
use tokio::io::WriteHalf;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Mutex, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use wincmd_shared::{
    hello_from_free, read_envelope, write_envelope, Envelope, ErrorReply, Notification, Request,
    Response, PROTOCOL_VERSION,
};

// ═══════════════════════════════════════════════════════════════════════
// AppHandle for Pro → Free notifications
// ═══════════════════════════════════════════════════════════════════════
//
// Paid Rust modules used to call `app.emit("decoy-accessed", payload)`
// directly because they lived in commander-free and had an AppHandle in
// hand. After phase-6b they live in commander-pro, which has no Tauri
// context. Instead Pro sends an `Envelope::Notification(Notification)`
// over the IPC pipe, and Free's per-session reader task re-emits it
// here. The reader needs an AppHandle to call .emit() on — we keep one
// in a global slot set during Tauri `setup()`.
//
// Notifications received before `set_app_handle()` has fired (a rare
// race during startup) are logged and dropped; the reader doesn't
// queue them, because the dropped notifications would arrive after
// any subscribing UI is already mounted.

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// Install the global AppHandle used by per-session reader tasks to
/// emit Pro-originated notifications. Call once from Tauri's setup().
/// Subsequent calls are no-ops (OnceLock semantics).
pub fn set_app_handle(handle: AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

/// The global AppHandle, if `set_app_handle` has run. Used by handle-less code
/// paths (e.g. the settings write choke point) that need to emit an event or
/// spawn an app-scoped task.
pub fn app_handle() -> Option<AppHandle> {
    APP_HANDLE.get().cloned()
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Per-call timeout: how long Free will wait for one Response from Pro
/// before declaring the dispatch dead and dropping the session. Most
/// paid commands return well under a second; the long tail is
/// Clear-EventLogs (which iterates every Windows event log and can
/// run 60s+ on a busy machine), analysis scans, and evidence-audit
/// clearers.
///
/// Was 30s — too tight for Clear-EventLogs under cascade load, where
/// the user reported "Pro response timeout" mid-cascade and the step
/// silently re-ran on a fresh session, doubling the work. 120s is
/// generous enough to absorb the slowest realistic paid command
/// without retry, while still surfacing a genuinely-stuck Pro
/// (deadlock, blocked PowerShell child, OS swap-storm) as an error
/// within a reasonable wall-clock for the user.
const SESSION_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Mounting a dual volume with a non-default PIM can legitimately spend
/// several minutes deriving keys. Match the backend command ceiling without
/// extending the failure window for unrelated Pro requests.
const MOUNT_ENCRYPTION_VOLUME_TIMEOUT: Duration = Duration::from_secs(20 * 60);

fn request_timeout_for(feature_id: &str) -> Duration {
    if feature_id == "Mount-EncryptionVolume" {
        MOUNT_ENCRYPTION_VOLUME_TIMEOUT
    } else {
        SESSION_REQUEST_TIMEOUT
    }
}

/// How long Free will wait on each handshake stage (pipe.connect, Hello
/// write, Hello read). Pro mirrors this with its own read-side timeout.
///
/// Was 5s — too tight under cascade load. The lockdown cascade fires
/// 25+ paid commands in parallel, and Pro spawn on Windows can be
/// 1–2s under AV scanning. With 4 simultaneous spawns the slowest
/// child commonly exceeds 5s between connecting and exchanging the
/// first frame, surfacing as "Hello write: pipe is being closed
/// (os error 232)" once Pro's read times out and the child exits.
///
/// 30s is generous enough to absorb the worst-case AV-scanned spawn
/// tail without making genuinely-stuck pipes hang the UI for too long.
/// Pair with Pro's HANDSHAKE_TIMEOUT (also 30s) — they must agree.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Windows shows its own modal error when the loader cannot start Pro (for
/// example, a damaged binary or a missing CRT). A burst of background probes
/// must not turn that one failure into a wall of OS dialogs. Serialize fresh
/// launches and suppress new attempts briefly after any failed handshake.
const PRO_SPAWN_FAILURE_COOLDOWN: Duration = Duration::from_secs(60);
static PRO_SPAWN_GATE: OnceLock<Mutex<()>> = OnceLock::new();
static PRO_SPAWN_RETRY_AFTER: OnceLock<std::sync::Mutex<Option<Instant>>> = OnceLock::new();
static PRO_SPAWN_IS_HEALTHY: AtomicBool = AtomicBool::new(false);

fn pro_spawn_gate() -> &'static Mutex<()> {
    PRO_SPAWN_GATE.get_or_init(|| Mutex::new(()))
}

fn pro_spawn_retry_after() -> &'static std::sync::Mutex<Option<Instant>> {
    PRO_SPAWN_RETRY_AFTER.get_or_init(|| std::sync::Mutex::new(None))
}

fn pro_spawn_cooldown_error(now: Instant, retry_after: Option<Instant>) -> Option<String> {
    let retry_after = retry_after?;
    if now >= retry_after {
        return None;
    }
    let remaining = retry_after.duration_since(now);
    Some(format!(
        "Pro could not start. Further launch attempts are paused for {} seconds after the last failure.",
        remaining.as_secs().max(1)
    ))
}

fn clear_pro_spawn_cooldown() {
    if let Ok(mut retry_after) = pro_spawn_retry_after().lock() {
        *retry_after = None;
    }
}

fn record_pro_spawn_failure() {
    if let Ok(mut retry_after) = pro_spawn_retry_after().lock() {
        *retry_after = Some(Instant::now() + PRO_SPAWN_FAILURE_COOLDOWN);
    }
}

fn current_pro_spawn_cooldown_error() -> Option<String> {
    let retry_after = pro_spawn_retry_after().lock().ok().and_then(|value| *value);
    pro_spawn_cooldown_error(Instant::now(), retry_after)
}

// F-3 — Pro binary hash pinning.
//
// Pro hashes itself at startup (commander-pro/src/main.rs) and ships the
// SHA-256 in its Hello ack. Until this landing the value was logged but
// never compared, so even a locally-swapped wincommander-pro.exe was
// trusted as long as it implemented the wire protocol — Pro is the
// privileged side (RDP credentials, lockdown, dismount). The audit
// tracks this as F-3 in ref/security-audit-report.md.
//
// Accepted hashes are injected at build time via env vars:
//   WINCMD_PRO_SHA256_CURRENT   — sha256 of the current release binary
//   WINCMD_PRO_SHA256_PREVIOUS  — kept across one release for graceful upgrade
//
// In release builds the release workflow MUST set CURRENT (PREVIOUS is
// optional). When neither is set, debug builds stay permissive for local
// commander-pro siblings; release builds fail closed so a bad release
// pipeline cannot silently ship with F-3 disabled.
const ACCEPTED_PRO_SHA256_CURRENT: Option<&str> = option_env!("WINCMD_PRO_SHA256_CURRENT");
const ACCEPTED_PRO_SHA256_PREVIOUS: Option<&str> = option_env!("WINCMD_PRO_SHA256_PREVIOUS");

fn pro_hash_is_accepted(actual: &str) -> bool {
    let matches = |expected: Option<&str>| -> bool {
        expected
            .map(|e| !e.trim().is_empty() && e.trim().eq_ignore_ascii_case(actual))
            .unwrap_or(false)
    };
    matches(ACCEPTED_PRO_SHA256_CURRENT) || matches(ACCEPTED_PRO_SHA256_PREVIOUS)
}

fn verify_pro_binary_hash(actual: &str) -> Result<(), String> {
    // Debug builds skip hash pinning entirely — .pro_hash may be stale during local dev.
    if cfg!(debug_assertions) {
        crate::log_message(
            "warn",
            &format!(
                "[Sidecar] Pro binary hash check skipped (debug build). Got hash: {}",
                if actual.is_empty() { "<empty>" } else { actual }
            ),
        );
        return Ok(());
    }

    if actual.is_empty() {
        return Err(
            "Pro did not report a binary hash in Hello ack (handshake refused)".to_string(),
        );
    }

    // Path A: compile-time pinned hash (primary gate — set by tools/hash-pro.ts at build time).
    if pro_hash_is_accepted(actual) {
        return Ok(());
    }

    // Path B: install-metadata acceptance. install_pro_binary() writes the SHA-256 it
    // verified during download to wincommander-pro.json. Accept the reported hash if:
    //   1. The install metadata records this exact hash (proves the install was official
    //      and the hash was verified against the manifest at install time).
    //   2. The managed install-path binary's current on-disk hash also equals `actual`
    //      (proves the binary file hasn't been swapped on disk since install, closing
    //      the metadata-spoof + binary-swap attack vector).
    // This path bridges the gap between a user updating Pro via the UI and the older
    // Free binary that was compiled before that Pro version's hash was known.
    if crate::pro_install::install_metadata_has_hash(actual)
        && crate::pro_install::compute_install_path_sha256()
            .map(|h| h.eq_ignore_ascii_case(actual))
            .unwrap_or(false)
    {
        crate::log_message(
            "info",
            &format!(
                "[Sidecar] Pro accepted via install metadata (hash {})",
                actual
            ),
        );
        return Ok(());
    }

    // Neither compile-time hash nor install metadata matched.
    let configured = ACCEPTED_PRO_SHA256_CURRENT
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
        || ACCEPTED_PRO_SHA256_PREVIOUS
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

    if !configured {
        return Err(
            "Pro binary hash verification is not configured in this release build \
             (WINCMD_PRO_SHA256_CURRENT missing) and no install metadata matched; \
             run Install Pro from settings to register the binary."
                .to_string(),
        );
    }

    // Tamper hook: binary mismatch detected — best-effort, never panic/block.
    {
        let hook_args = serde_json::json!({ "signal": "binary_mismatch" });
        tauri::async_runtime::spawn(async move {
            crate::argus::record_tamper_event_hook(hook_args).await;
        });
    }
    Err(format!(
        "Pro binary hash {} is not in the accepted set — refuse handshake. \
         If you just updated Pro, reinstall it from Settings → Pro.",
        actual
    ))
}

// Monotonic per-process request id. Pro echoes it back in the Response
// so multiple in-flight calls can be demuxed.
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn next_request_id() -> u64 {
    NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
}

// ── Pro session pool (phase 9c — true parallel paid dispatch) ────
//
// Each Pro child speaks a single-stream wire protocol on its own pipe —
// one request, one response, sequentially. To get genuine parallelism
// for things like the lockdown cascade (which fans out 25+ paid
// commands), we keep a pool of N Pro children, each with their own
// pipe. `dispatch_paid_command` checks a session out of the pool, runs
// the round-trip, and returns the session to the pool when done.
// Concurrent dispatches each grab a different session and run in true
// parallel. A semaphore caps total concurrency at POOL_CAPACITY so we
// never spawn more than that many Pro children.
//
// This replaces the previous "single session behind a Mutex" design,
// which serialised every paid call regardless of how many futures the
// orchestrator launched concurrently — the cascade UI's parallel
// overlay was a lie because the IPC bottleneck was real. With the
// pool, the cascade actually runs in parallel up to POOL_CAPACITY
// sessions wide, and the wall-clock matches what the UI shows.
//
// Pool sizing: 4 is the trade-off. Each Pro child costs a few MB of
// RAM and a one-time ~100ms spawn cost. 4 lets the cascade saturate
// most independent surfaces (DNS cache vs USB history vs RDP cache
// don't contend with each other) without spinning up so many children
// that the OS ContextSwitch cost outweighs the parallelism win.
//
// On EOF / error the broken session is dropped; the next dispatch
// transparently respawns into the pool. Bye is sent best-effort to
// every pooled session during process shutdown via `close_pro_session`.

#[cfg(debug_assertions)]
const POOL_CAPACITY: usize = 1;

#[cfg(not(debug_assertions))]
const POOL_CAPACITY: usize = 4;

// Worker processes are deliberately kept briefly so bursty paid actions can
// reuse their verified pipes. They must not become permanent background
// processes after the work ends.
const PRO_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const PRO_POOL_REAP_INTERVAL: Duration = Duration::from_secs(15);
static PRO_POOL_REAPER_STARTED: AtomicBool = AtomicBool::new(false);

/// Per-session table of dispatchers awaiting a Response/Error keyed by
/// `request_id`. The reader task removes an entry when it finds the
/// matching reply and sends down the oneshot. Dispatchers that time
/// out remove their own entry so a late reply finds nothing and is
/// dropped harmlessly. On reader exit the map is cleared, which drops
/// every sender and surfaces a `RecvError` to any pending dispatchers.
type InflightMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>>>;

pub struct ProSession {
    /// Write half of the named pipe — owned by dispatch (only writes).
    write: WriteHalf<NamedPipeServer>,
    child: Child,
    session_token: String,
    /// Shared with the reader task — dispatch inserts a sender keyed by
    /// `request_id` before writing; reader removes + signals when the
    /// matching reply arrives.
    inflight: InflightMap,
    /// Handle to the per-session reader task. Wrapped in `Option` so
    /// `Drop` can `.take()` and abort it without `mem::replace`.
    reader: Option<JoinHandle<()>>,
    /// Tasks draining Pro's stdout/stderr into the unified log (source `pro`).
    /// They self-terminate on EOF when the child dies, but are aborted on Drop
    /// for tidiness. Draining is also what stops Pro blocking on a full pipe.
    log_drains: Vec<JoinHandle<()>>,
}

struct PooledProSession {
    session: ProSession,
    idle_until: Instant,
}

impl Drop for ProSession {
    fn drop(&mut self) {
        // Abort the reader task so it stops polling the read half once
        // the session goes away. Reader normally exits on its own when
        // the pipe closes (Bye + child kill in `close_pro_session`, or
        // child OOM/crash) — Drop's abort is the belt-and-suspenders
        // path for unexpected session disposal (broken-transport branch
        // in dispatch_paid_command, panic unwinding, etc).
        if let Some(handle) = self.reader.take() {
            handle.abort();
        }
        for handle in self.log_drains.drain(..) {
            handle.abort();
        }
    }
}

/// Drain a child stdout/stderr stream line-by-line into the unified log,
/// tagged `source=pro`. Reading the line is what drains the OS pipe buffer —
/// a piped-but-unread stream eventually blocks Pro mid-session — so we read
/// every line and only then (if logging is enabled) persist it. Level is
/// inferred from the text (Approach A). The task ends on EOF / read error.
fn spawn_log_drain<R>(reader: Option<R>) -> Option<JoinHandle<()>>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let reader = reader?;
    Some(tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            crate::log_message_src(crate::infer_pro_level(line), crate::LOG_SRC_PRO, line);
        }
    }))
}

/// Notification sink — what the reader does with a verified
/// `Envelope::Notification`. Production wires this to `AppHandle::emit`;
/// tests wire it to a channel so they can assert what was emitted
/// without needing a Tauri context.
type NotificationSink = Box<dyn Fn(String, serde_json::Value) + Send + Sync>;

/// Per-session reader loop. Reads framed envelopes off the pipe,
/// verifies the HMAC tag with the session token, and routes:
///   - `Notification` → `on_notification(event, payload)`.
///   - `Response(id)` / `Error(id)` → look up the dispatcher's sender
///     in `inflight` and signal it with the result.
///   - `Hello` / `Bye` / `Signed` / `Request` from Pro → log and drop
///     (these are protocol mistakes; we don't want a buggy peer to
///     wedge the reader).
///
/// Exits on EOF / I/O error / Bye. Clears `inflight` on exit so any
/// pending dispatcher's `recv().await` resolves to `RecvError`,
/// triggering the transport-error retry path in
/// `dispatch_paid_command`.
///
/// Generic over the read half so tests can drive the loop with a
/// `tokio::io::DuplexStream` instead of a real named pipe.
async fn reader_loop<R>(
    mut read_half: R,
    inflight: InflightMap,
    session_token: String,
    on_notification: NotificationSink,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    loop {
        let raw = match read_envelope(&mut read_half).await {
            Ok(e) => e,
            Err(e) => {
                crate::log_message(
                    "info",
                    &format!("[ProReader] pipe closed / read error, exiting: {}", e),
                );
                break;
            }
        };

        // Post-handshake Pro→Free traffic is always Signed (matches the
        // dispatch_request guarantee). Reject anything else as a
        // protocol break — refusing it here is safer than guessing.
        let env = match raw.verify_and_unwrap(&session_token) {
            Ok(env) => env,
            Err(reason) => {
                crate::log_message(
                    "warn",
                    &format!("[ProReader] dropping unverifiable frame: {}", reason),
                );
                continue;
            }
        };

        match env {
            Envelope::Response(Response { request_id, result }) => {
                if let Some(tx) = inflight.lock().await.remove(&request_id) {
                    let _ = tx.send(Ok(result));
                } else {
                    // Late reply — dispatcher already timed out. Drop quietly.
                    crate::log_message(
                        "debug",
                        &format!(
                            "[ProReader] late Response for request_id={} (dispatcher gone)",
                            request_id
                        ),
                    );
                }
            }
            Envelope::Error(ErrorReply {
                request_id,
                kind,
                message,
            }) => {
                if let Some(tx) = inflight.lock().await.remove(&request_id) {
                    let _ = tx.send(Err(format!("[pro:{}] {}", kind, message)));
                } else {
                    crate::log_message(
                        "debug",
                        &format!(
                            "[ProReader] late Error for request_id={} kind={} (dispatcher gone)",
                            request_id, kind
                        ),
                    );
                }
            }
            Envelope::Notification(Notification { event, payload }) => {
                on_notification(event, payload);
            }
            Envelope::Bye => {
                crate::log_message("debug", "[ProReader] received Bye from Pro, exiting");
                break;
            }
            Envelope::Hello(_) => {
                crate::log_message(
                    "warn",
                    "[ProReader] unexpected Hello mid-session — ignoring",
                );
            }
            Envelope::Request(_) => {
                // Free is the only side that sends Requests. Receiving
                // one from Pro is a protocol break.
                crate::log_message("warn", "[ProReader] unexpected Request from Pro — ignoring");
            }
            Envelope::Signed(_) => {
                // verify_and_unwrap already produced a non-Signed
                // variant; reaching this arm means double-wrapping,
                // which Envelope::sign explicitly refuses.
                crate::log_message("warn", "[ProReader] dropped double-signed frame");
            }
        }
    }
    // Drain pending dispatchers so they unblock with RecvError instead
    // of waiting until their per-call timeout fires.
    inflight.lock().await.clear();
}

/// Free pool of idle Pro sessions ready for reuse. Sessions are
/// pushed back here when their dispatch completes successfully.
fn pro_session_pool() -> &'static Mutex<Vec<PooledProSession>> {
    static POOL: OnceLock<Mutex<Vec<PooledProSession>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Vec::with_capacity(POOL_CAPACITY)))
}

fn pool_session_is_expired(now: Instant, idle_until: Instant) -> bool {
    now >= idle_until
}

fn pooled_child_is_running(session: &mut ProSession) -> bool {
    match session.child.try_wait() {
        Ok(None) => true,
        Ok(Some(status)) => {
            crate::log_message(
                "info",
                &format!("[ProPool] discarding exited idle worker: {status}"),
            );
            false
        }
        Err(error) => {
            crate::log_message(
                "warn",
                &format!("[ProPool] discarding unreadable idle worker: {error}"),
            );
            false
        }
    }
}

async fn stop_pro_session(mut session: ProSession) {
    // A worker has no durable state. Sending Bye gives a cooperative Pro a
    // chance to close its pipe; kill_on_drop remains the final backstop.
    let _ = timeout(
        Duration::from_secs(2),
        write_envelope(&mut session.write, &Envelope::Bye),
    )
    .await;
    let _ = session.child.start_kill();
    let _ = timeout(Duration::from_secs(2), session.child.wait()).await;
}

async fn stop_pro_sessions(sessions: Vec<ProSession>) {
    for session in sessions {
        stop_pro_session(session).await;
    }
}

fn drain_pro_session_pool(
    pool: &mut Vec<PooledProSession>,
    take_one: bool,
) -> (Option<ProSession>, Vec<ProSession>) {
    let now = Instant::now();
    let mut session = None;
    let mut reusable = Vec::with_capacity(pool.len());
    let mut retired = Vec::new();

    for mut entry in std::mem::take(pool) {
        if pool_session_is_expired(now, entry.idle_until)
            || !pooled_child_is_running(&mut entry.session)
        {
            retired.push(entry.session);
        } else if take_one && session.is_none() {
            session = Some(entry.session);
        } else {
            reusable.push(entry);
        }
    }
    *pool = reusable;
    (session, retired)
}

async fn take_pooled_pro_session() -> Option<ProSession> {
    let (session, retired) = {
        let mut pool = pro_session_pool().lock().await;
        drain_pro_session_pool(&mut pool, true)
    };
    stop_pro_sessions(retired).await;
    session
}

async fn reap_idle_pro_sessions() {
    let retired = {
        let mut pool = pro_session_pool().lock().await;
        let (_, retired) = drain_pro_session_pool(&mut pool, false);
        retired
    };
    stop_pro_sessions(retired).await;
}

fn start_pro_pool_reaper() {
    if PRO_POOL_REAPER_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    tokio::spawn(async {
        loop {
            tokio::time::sleep(PRO_POOL_REAP_INTERVAL).await;
            reap_idle_pro_sessions().await;
        }
    });
}

async fn return_pro_session_to_pool(session: ProSession) {
    let excess = {
        let mut pool = pro_session_pool().lock().await;
        pool.push(PooledProSession {
            session,
            idle_until: Instant::now() + PRO_POOL_IDLE_TIMEOUT,
        });
        if pool.len() > POOL_CAPACITY {
            Some(pool.remove(0).session)
        } else {
            None
        }
    };
    if let Some(session) = excess {
        stop_pro_session(session).await;
    }
    start_pro_pool_reaper();
}

/// Caps the total number of in-flight paid dispatches at POOL_CAPACITY.
/// A dispatch that arrives when all permits are held waits FIFO until
/// another finishes — the same wait the old single-session mutex would
/// have produced, just spread across more parallel slots.
fn pool_semaphore() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(POOL_CAPACITY))
}

/// Compute a Windows named-pipe path from a random suffix. The
/// `\\.\pipe\` prefix is required by the OS; we add a `wincmd-pro-`
/// component so the pipe is greppable in tools like Process Explorer.
fn random_pipe_name() -> String {
    // 16 hex chars = 64 bits of randomness — collision-free for any
    // realistic spawn rate.
    let mut buf = [0u8; 8];
    getrandom_bytes(&mut buf);
    let suffix: String = buf.iter().map(|b| format!("{:02x}", b)).collect();
    format!(r"\\.\pipe\wincmd-pro-{}", suffix)
}

fn random_session_token() -> String {
    let mut buf = [0u8; 32];
    getrandom_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

// CSPRNG-backed: the session token is the HMAC key and the sole secret
// authenticating every frame on the per-session pipe. A time+PID LCG (the old
// v1) was locally predictable, letting a same-machine attacker forge signed
// frames to the privileged Pro peer. OsRng pulls from the OS CSPRNG
// (BCryptGenRandom on Windows) — same source datastore.rs uses for keys/nonces.
fn getrandom_bytes(buf: &mut [u8]) {
    use rand::rngs::OsRng;
    use rand::RngCore;
    OsRng.fill_bytes(buf);
}

/// Result returned to the frontend `test_pro_handshake` command.
#[derive(serde::Serialize)]
pub struct HandshakeResult {
    pub ok: bool,
    pub pipe: Option<String>,
    pub pro_version: Option<String>,
    pub binary_hash: Option<String>,
    pub error: Option<String>,
}

/// Resolve the path to wincommander-pro.exe. Prefers the install path
/// under `%ProgramData%\WinCommander\bin\` (the phase-8 install destination)
/// and falls back to the dev sibling location (`target/debug/...`)
/// when not installed yet. Path-resolution logic lives in pro_install
/// so /ref docs and the get_pro_install_status command see the same
/// truth.
fn resolve_pro_binary() -> Result<PathBuf, String> {
    crate::pro_install::pro_resolve_path().ok_or_else(|| {
        "wincommander-pro.exe not found at %ProgramData%\\WinCommander\\bin\\ or next to current exe"
            .to_string()
    })
}

/// Spawn Pro, perform the handshake, and return the result. Pipe is
/// closed and child is killed before returning — this is the smoke-
/// test entrypoint for phase 7b verification, not the long-lived
/// session that phase 7c will introduce.
pub async fn handshake_pro_once() -> HandshakeResult {
    crate::log_message_src(
        "info",
        "core",
        "[Sidecar] handshake_pro_once: resolving Pro binary",
    );
    let pro_path = match resolve_pro_binary() {
        Ok(p) => p,
        Err(e) => {
            crate::log_message_src(
                "error",
                "core",
                &format!("[Sidecar] handshake_pro_once: binary not found: {}", e),
            );
            return HandshakeResult {
                ok: false,
                pipe: None,
                pro_version: None,
                binary_hash: None,
                error: Some(e),
            };
        }
    };

    let pipe_name = random_pipe_name();
    let token = random_session_token();

    // Create the server side of the pipe BEFORE spawning Pro so Pro's
    // ClientOptions::open() finds it on the first try. ServerOptions
    // creates the pipe; first_pipe_instance(true) prevents another
    // process from squatting on the same name.
    let server: NamedPipeServer = match ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)
    {
        Ok(s) => s,
        Err(e) => {
            return HandshakeResult {
                ok: false,
                pipe: Some(pipe_name),
                pro_version: None,
                binary_hash: None,
                error: Some(format!("pipe create failed: {}", e)),
            };
        }
    };

    // Spawn Pro.
    let mut cmd = Command::new(&pro_path);
    cmd.arg(format!("--core-pipe={}", pipe_name));
    cmd.arg(format!("--session-token={}", token));
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return HandshakeResult {
                ok: false,
                pipe: Some(pipe_name),
                pro_version: None,
                binary_hash: None,
                error: Some(format!("spawn failed: {}", e)),
            };
        }
    };

    // Drain Pro's streams so a handshake-time error/panic is captured and the
    // piped buffers don't block Pro. Detached: they EOF when the child is
    // killed at the end of this smoke test.
    let _ = spawn_log_drain(child.stderr.take());
    let _ = spawn_log_drain(child.stdout.take());

    // Wait for the client to connect, then run the handshake.
    let result: Result<(String, String), String> = (async {
        timeout(HANDSHAKE_TIMEOUT, server.connect())
            .await
            .map_err(|_| "pipe connect timeout".to_string())?
            .map_err(|e| format!("pipe connect error: {}", e))?;

        let mut server = server;

        // Step 1: send Hello to Pro.
        let outbound = Envelope::Hello(hello_from_free(&token));
        timeout(HANDSHAKE_TIMEOUT, write_envelope(&mut server, &outbound))
            .await
            .map_err(|_| "Hello write timeout".to_string())?
            .map_err(|e| format!("Hello write failed: {}", e))?;

        // Step 2: receive Pro's Hello ack.
        let inbound = timeout(HANDSHAKE_TIMEOUT, read_envelope(&mut server))
            .await
            .map_err(|_| "Hello read timeout".to_string())?
            .map_err(|e| format!("Hello read failed: {}", e))?;

        let Envelope::Hello(h) = inbound else {
            return Err("expected Hello ack from Pro, got different envelope".to_string());
        };

        if h.protocol_version != PROTOCOL_VERSION {
            return Err(format!(
                "protocol mismatch: free={}, pro={}",
                PROTOCOL_VERSION, h.protocol_version
            ));
        }

        if h.session_token != token {
            return Err("session token mismatch in Pro's ack".to_string());
        }

        // F-3: refuse handshake if Pro's reported SHA-256 isn't pinned.
        let reported_hash = h.binary_hash.clone().unwrap_or_default();
        verify_pro_binary_hash(&reported_hash)?;

        Ok((h.pro_version.unwrap_or_default(), reported_hash))
    })
    .await;

    // Clean up — kill the child unconditionally; Bye-loop is phase 7c.
    let _ = child.start_kill();
    let _ = child.wait().await;

    match result {
        Ok((ver, hash)) => {
            crate::log_message_src(
                "info",
                "core",
                &format!(
                    "[Sidecar] handshake_pro_once: ok version={} hash={}",
                    ver,
                    if hash.len() >= 8 { &hash[..8] } else { &hash }
                ),
            );
            HandshakeResult {
                ok: true,
                pipe: Some(pipe_name),
                pro_version: Some(ver),
                binary_hash: Some(hash),
                error: None,
            }
        }
        Err(e) => {
            crate::log_message_src(
                "error",
                "core",
                &format!("[Sidecar] handshake_pro_once: failed: {}", e),
            );
            HandshakeResult {
                ok: false,
                pipe: Some(pipe_name),
                pro_version: None,
                binary_hash: None,
                error: Some(e),
            }
        }
    }
}

#[tauri::command]
pub async fn test_pro_handshake() -> Result<serde_json::Value, String> {
    let r = handshake_pro_once().await;
    serde_json::to_value(r).map_err(|e| e.to_string())
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 7c — Long-lived session + paid-command dispatch
// ═══════════════════════════════════════════════════════════════════════

/// Spawn Pro and run the handshake, returning a kept-alive session
/// with a per-session reader task already running. The reader takes
/// ownership of the pipe's read half; dispatch only writes through
/// the write half.
/// Whether a spawned Pro child is a general pool worker or THE dedicated fleet
/// agent. Only the agent gets the FLEET_* env, so only the agent process runs
/// the enroll/heartbeat loop (spawn_if_configured keys off FLEET_URL) — pool
/// workers never race for the fleet-agent singleton. This is what lets us pin
/// the collectors + posture to the same process that drains + sends them
/// (Option G), fixing the per-process telemetry stranding without shrinking the
/// pool the lockdown cascade relies on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionRole {
    Worker,
    FleetAgent,
}

async fn spawn_pro_session() -> Result<ProSession, String> {
    spawn_pro_session_with_role(SessionRole::Worker).await
}

async fn spawn_pro_session_with_role(role: SessionRole) -> Result<ProSession, String> {
    // Once a sidecar launch has completed successfully, preserve the pool's
    // parallel-start behavior for cascade work. The serialized path below is
    // needed only until the first confirmed launch or after a failure.
    if PRO_SPAWN_IS_HEALTHY.load(Ordering::Acquire) {
        let result = spawn_pro_session_unlocked(role).await;
        if result.is_err() {
            PRO_SPAWN_IS_HEALTHY.store(false, Ordering::Release);
            record_pro_spawn_failure();
        }
        return result;
    }

    // Keep the permit through the handshake. `Command::spawn` succeeds even
    // when Windows' native loader immediately rejects the child, so this is
    // the only point where concurrent callers can be coalesced before the OS
    // presents duplicate loader dialogs.
    let _spawn_guard = pro_spawn_gate().lock().await;
    if let Some(error) = current_pro_spawn_cooldown_error() {
        return Err(error);
    }

    let result = spawn_pro_session_unlocked(role).await;
    if result.is_ok() {
        PRO_SPAWN_IS_HEALTHY.store(true, Ordering::Release);
        clear_pro_spawn_cooldown();
    } else {
        PRO_SPAWN_IS_HEALTHY.store(false, Ordering::Release);
        record_pro_spawn_failure();
    }
    result
}

async fn spawn_pro_session_unlocked(role: SessionRole) -> Result<ProSession, String> {
    crate::log_message_src("info", "core", "[Sidecar] spawn_pro_session: start");
    let pro_path = resolve_pro_binary()?;
    let pipe_name = random_pipe_name();
    let token = random_session_token();

    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)
        .map_err(|e| format!("pipe create: {}", e))?;

    let mut cmd = Command::new(&pro_path);
    cmd.arg(format!("--core-pipe={}", pipe_name));
    cmd.arg(format!("--session-token={}", token));
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    // Seed fleet agent env vars from persisted settings so spawn_if_configured()
    // in fleet_push.rs picks them up without requiring a runtime IPC round-trip.
    // ONLY the dedicated FleetAgent session gets these — pool workers stay
    // fleet-blind so exactly one process (this agent) owns the heartbeat AND the
    // collectors, keeping their in-process sample/signal queues co-located with
    // the loop that drains them.
    if role == SessionRole::FleetAgent {
        if let Ok(s) = crate::settings::read_settings() {
            let fleet = &s.app.fleet;
            if fleet.enabled && !fleet.server_url.is_empty() {
                cmd.env("FLEET_URL", &fleet.server_url);
                cmd.env("FLEET_AGENT_ROLE", "1");
                // Stable device id so the env/reboot enroll path reuses the SAME fleet
                // row instead of registering a new device each launch (no duplicates).
                if !s.device_id.is_empty() {
                    cmd.env("FLEET_DEVICE_ID", &s.device_id);
                }
                if fleet.dispatch {
                    cmd.env("FLEET_DISPATCH", "1");
                }
                if !fleet.signing_key_pub.is_empty() {
                    cmd.env("FLEET_SIGNING_KEY_PUB", &fleet.signing_key_pub);
                }
            }
        }
    }

    let mut child = cmd.spawn().map_err(|e| format!("spawn: {}", e))?;

    // Drain Pro's stdout/stderr into the unified log (source `pro`) so its
    // errors/panics are visible in the in-app Error Center — and so the piped
    // streams are actually read (an unread pipe eventually blocks Pro).
    let mut log_drains: Vec<JoinHandle<()>> = Vec::new();
    log_drains.extend(spawn_log_drain(child.stderr.take()));
    log_drains.extend(spawn_log_drain(child.stdout.take()));

    // Handshake runs on the unsplit pipe so both read and write side
    // of read_envelope/write_envelope see a single &mut. Only after
    // the Hello ack succeeds do we io::split() and hand the read half
    // to the reader task.
    timeout(HANDSHAKE_TIMEOUT, server.connect())
        .await
        .map_err(|_| "pipe connect timeout".to_string())?
        .map_err(|e| format!("pipe connect error: {}", e))?;

    let outbound = Envelope::Hello(hello_from_free(&token));
    timeout(HANDSHAKE_TIMEOUT, write_envelope(&mut server, &outbound))
        .await
        .map_err(|_| "Hello write timeout".to_string())?
        .map_err(|e| format!("Hello write: {}", e))?;

    let inbound = timeout(HANDSHAKE_TIMEOUT, read_envelope(&mut server))
        .await
        .map_err(|_| "Hello read timeout".to_string())?
        .map_err(|e| format!("Hello read: {}", e))?;

    match inbound {
        Envelope::Hello(h)
            if h.session_token == token && h.protocol_version == PROTOCOL_VERSION =>
        {
            // F-3: refuse the live session if Pro's reported SHA-256
            // isn't in the pinned-hash set. Same gate as the smoke-test
            // handshake (handshake_pro_once) — defence-in-depth against
            // a locally-swapped wincommander-pro.exe.
            let reported_hash = h.binary_hash.clone().unwrap_or_default();
            let pro_ver = h.pro_version.as_deref().unwrap_or("unknown");
            crate::log_message_src(
                "info",
                "core",
                &format!(
                    "[Sidecar] spawn_pro_session: Hello ack received version={} hash={}",
                    pro_ver,
                    if reported_hash.len() >= 8 {
                        &reported_hash[..8]
                    } else {
                        &reported_hash
                    }
                ),
            );
            verify_pro_binary_hash(&reported_hash).map_err(|e| {
                crate::log_message_src(
                    "error",
                    "core",
                    &format!("[Sidecar] spawn_pro_session: hash verify failed: {}", e),
                );
                e
            })?;
            crate::log_message_src(
                "info",
                "core",
                "[Sidecar] spawn_pro_session: handshake verified, session live",
            );
        }
        Envelope::Hello(_) => {
            crate::log_message_src(
                "error",
                "core",
                "[Sidecar] spawn_pro_session: session/protocol mismatch in Pro Hello ack",
            );
            return Err("session/protocol mismatch in Pro Hello ack".to_string());
        }
        _ => {
            crate::log_message_src(
                "error",
                "core",
                "[Sidecar] spawn_pro_session: expected Hello as first frame from Pro",
            );
            return Err("expected Hello as first frame from Pro".to_string());
        }
    }

    // Handshake ok — split the pipe and spawn the reader. tokio::io::split
    // works for any AsyncRead+AsyncWrite; named-pipe halves serialise at
    // the per-poll level but yield while parked, so a parked read does
    // not block writes.
    let (read_half, write_half) = tokio::io::split(server);
    let inflight: InflightMap = Arc::new(Mutex::new(HashMap::new()));
    // Production notification sink: re-emit via the global AppHandle.
    // If APP_HANDLE hasn't been set yet (would only happen if a paid
    // dispatch fires before setup() ran — currently impossible because
    // set_app_handle is the first thing in setup), the notification is
    // dropped with a warning.
    //
    // Special event: "wc-native-toast" — Pro has no Tauri context, so it sends
    // this event when a paid watcher wants to show an out-of-app alert. Payload
    // shape: { "title": "...", "body": "..." }. Free's reader opens the custom
    // alert window on Pro's behalf. Other notification events flow to the
    // frontend via app.emit() unchanged.
    let on_notification: NotificationSink = Box::new(|event, payload| {
        let Some(app) = APP_HANDLE.get() else {
            crate::log_message(
                "warn",
                &format!(
                    "[ProReader] no AppHandle yet, dropping notification '{}'",
                    event
                ),
            );
            return;
        };
        if event == "wc-native-toast" {
            let title = payload
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("WinCommander");
            let body = payload.get("body").and_then(|v| v.as_str()).unwrap_or("");
            if let Err(e) = crate::native_notify::show_native_notification(app, title, body) {
                crate::log_message(
                    "warn",
                    &format!("[ProReader] wc-native-toast custom alert failed: {}", e),
                );
            }
            return;
        }
        // Evidence bridge: Pro collectors send structured evidence events over
        // the notification channel so they can record to the Free ledger without
        // a direct crate dependency.  Payload shape: { source, severity, summary,
        // detail? }.  Best-effort — a ledger write failure never drops the event
        // from the frontend (we still emit it below so the UI can react).
        if event == "argus-evidence" {
            let source = payload
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("argus")
                .to_string();
            let severity = payload
                .get("severity")
                .and_then(|v| v.as_str())
                .unwrap_or("info")
                .to_string();
            let summary = payload
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // Detail may arrive as a plain string or as a structured JSON value.
            // Single-encode either way so the ledger never stores a double-encoded
            // stringified-JSON literal: pass strings through as-is, serialise any
            // other non-null value once, treat null/absent as None.
            let detail = match payload.get("detail") {
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(serde_json::Value::Null) | None => None,
                Some(v) => serde_json::to_string(v).ok(),
            };
            // best-effort; ignore any write error
            let _ = crate::evidence::evidence_record(source, severity, summary, detail);
            // Do NOT re-emit to the frontend — evidence events are internal-only
            // and the frontend reads the ledger via evidence_read().
            return;
        }

        if let Err(e) = app.emit(&event, payload) {
            crate::log_message(
                "warn",
                &format!("[ProReader] app.emit('{}') failed: {}", event, e),
            );
        }
    });
    let reader = tokio::spawn(reader_loop(
        read_half,
        inflight.clone(),
        token.clone(),
        on_notification,
    ));

    Ok(ProSession {
        write: write_half,
        child,
        session_token: token,
        inflight,
        reader: Some(reader),
        log_drains,
    })
}

/// Send one Request over the live session and await its matching Response
/// via the reader task. Returns Err on transport failure (caller drops
/// the session and retries on the next call) or Pro-side ErrorReply
/// (semantic error — propagate).
///
/// Demuxed reads:
///   - The per-session `reader_loop` owns the read half of the pipe.
///   - Dispatcher inserts a oneshot sender keyed by `request_id` into
///     `session.inflight`, writes the (signed) Request, then awaits
///     the receiver.
///   - Reader looks up the request_id and signals via the oneshot.
///
/// Phase 9b — wire-level integrity:
///   - Outbound Request is signed with the session token via
///     `Envelope::sign(token)` before write_envelope.
///   - Inbound reply is verified by the reader; tampered or replayed
///     frames are dropped and never reach the dispatcher.
///   - Hello / Bye remain unsigned (Hello establishes the key; Bye
///     happens during teardown).
async fn dispatch_request(
    session: &mut ProSession,
    req: Request,
) -> Result<serde_json::Value, String> {
    let request_id = req.request_id;
    let request_timeout = request_timeout_for(&req.feature_id);
    let (tx, rx) = oneshot::channel();

    // Insert before write so a fast reader (already-buffered reply)
    // doesn't miss the dispatcher. Removed on any return path.
    session.inflight.lock().await.insert(request_id, tx);

    let signed = Envelope::Request(req).sign(&session.session_token);
    if let Err(e) = write_envelope(&mut session.write, &signed).await {
        // Take our sender back out — reader would otherwise hold a
        // dangling entry forever for this id (no reply will arrive).
        session.inflight.lock().await.remove(&request_id);
        return Err(format!("write request: {}", e));
    }

    match timeout(request_timeout, rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_recv_err)) => {
            // Sender dropped — either reader exited (pipe closed, EOF,
            // protocol break) or someone else cleared the map. Either
            // way the session is no longer healthy and the caller's
            // transport-error retry path will spawn a fresh one.
            Err("Pro reader exited before responding".to_string())
        }
        Err(_) => {
            // Timed out. Pull our sender so a late reply finds nothing
            // and gets dropped instead of waking a Receiver we no
            // longer care about.
            session.inflight.lock().await.remove(&request_id);
            Err("Pro response timeout".to_string())
        }
    }
}

/// Public entrypoint — used by run_backend_script when the command is paid
/// AND Pro is installed AND the user has a paid entitlement. Multiple
/// concurrent calls can run truly in parallel, each on a different
/// session out of the pool, up to POOL_CAPACITY. On any transport
/// error the broken session is dropped (not returned to the pool); the
/// next dispatch transparently spawns a fresh one.
/// Feature ids that MUST run in the dedicated fleet-agent process — everything
/// touching that process's in-memory state: the enroll/heartbeat/posture/
/// unenroll channel (`fleet_agent_*`) and every Argus collector (whose pending
/// sample/signal queues live in that process's statics and are drained by the
/// heartbeat loop THERE). Routing these to the one agent session co-locates
/// collection with transmission (Option G). Fail-safe by construction: an
/// agent-affine id that slips to the pool only reproduces the pre-fix stranding
/// (no regression), and a non-agent id sent to the agent just serialises one
/// extra light command — neither breaks anything.
fn is_agent_affine(feature_id: &str) -> bool {
    feature_id.starts_with("fleet_agent_")
        || feature_id.contains("argus")
        || feature_id.starts_with("start_session_monitor")
        || feature_id.starts_with("stop_session_monitor")
        || feature_id == "session_monitor_status"
        // Decoy monitoring has a watcher, audit listener, recent-event ring,
        // and Fleet queue that must remain in one durable Pro process. Keep
        // this closed list: a name containing "decoy" is not an entitlement
        // or process-affinity contract by itself.
        || matches!(feature_id,
            "start_decoy_monitor"
                | "stop_decoy_monitor"
                | "decoy_monitor_status"
                | "enroll_decoy"
                | "remove_decoy"
                | "list_decoys"
                | "drop_standard_decoys"
                | "delete_decoy"
                | "get_decoy_recent"
                | "clear_decoy_recent"
                | "set_decoy_read_audit_enabled"
                | "decoy_read_audit_status"
                | "get_last_access_tracking_status"
                | "enable_last_access_tracking"
        )
        // "Access & Session Monitor" (Windows Security-log anomaly detector):
        // its collector state + the "access" Argus signals it enqueues live in
        // the ONE fleet-agent process, so its start/stop/status/recent/clear
        // MUST route there too. Missing this let a local toggle land on any
        // pooled worker and strand its state/signals from the fleet-agent that
        // drains them — the exact multi-process gap the dedicated agent session
        // was created to close (it just wasn't listed here).
        || feature_id.contains("auth_anomaly")
}

/// The single, long-lived fleet-agent Pro session, held OUT of the general pool
/// so agent-affine dispatches always reach the same process and so it is never
/// dropped (dropping the ProSession kills the child via kill_on_drop, which
/// would stop the heartbeat).
fn agent_session_slot() -> &'static tokio::sync::Mutex<Option<ProSession>> {
    static AGENT: OnceLock<tokio::sync::Mutex<Option<ProSession>>> = OnceLock::new();
    AGENT.get_or_init(|| tokio::sync::Mutex::new(None))
}

/// Run an agent-affine command on the dedicated fleet-agent session. Returns
/// `Some(result)` when it ran there; `None` when the agent session couldn't be
/// established, so the caller falls back to the normal pool path (worst case:
/// pre-fix stranding, never a hard break). Does NOT take a pool-semaphore permit
/// — the agent session is outside the pool, so this never steals cascade width.
async fn try_dispatch_via_agent(
    feature_id: &str,
    args: &serde_json::Value,
) -> Option<Result<serde_json::Value, String>> {
    let mut slot = agent_session_slot().lock().await;
    let mut session = match slot.take() {
        Some(s) => s,
        None => match spawn_pro_session_with_role(SessionRole::FleetAgent).await {
            Ok(s) => s,
            Err(e) => {
                crate::log_message(
                    "warn",
                    &format!("[Sidecar] agent session unavailable ({e}); falling back to pool"),
                );
                return None;
            }
        },
    };
    // Same transport-retry contract as the pool path, kept separate so the
    // working pool path is untouched.
    for attempt in 0..2 {
        let req = Request {
            request_id: next_request_id(),
            feature_id: feature_id.to_string(),
            args: args.clone(),
        };
        match dispatch_request(&mut session, req).await {
            Ok(v) => {
                *slot = Some(session); // keep the agent process alive for the heartbeat
                return Some(Ok(v));
            }
            Err(e) if e.starts_with("[pro:") => {
                *slot = Some(session);
                return Some(Err(e));
            }
            Err(transport_err) => {
                if attempt == 0 {
                    crate::log_message(
                        "warn",
                        &format!(
                            "[ProAgent] transport error, respawning agent session: {transport_err}"
                        ),
                    );
                    session = match spawn_pro_session_with_role(SessionRole::FleetAgent).await {
                        Ok(s) => s,
                        Err(e) => {
                            return Some(Err(format!(
                                "agent respawn failed: {e} (initial: {transport_err})"
                            )));
                        }
                    };
                    continue;
                }
                return Some(Err(format!("agent transport error: {transport_err}")));
            }
        }
    }
    None
}

pub async fn dispatch_paid_command(
    feature_id: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    crate::log_message(
        "info",
        &format!(
            "[Sidecar] dispatch_paid_command entry: feature_id='{}'",
            feature_id
        ),
    );

    // ── Investigator-mode Pro refusal (narrowed 2026-05-21) ──────────────
    // Earlier this block refused EVERY paid command in investigator mode,
    // which over-applied — an investigator licence could not even READ
    // the Mesh VPN status, because Get-MeshVPNStatus / Start-MeshVPNLogin are paid-tier but
    // touch nothing related to evidence. The mesh-VPN panel then showed "infinite
    // loading" while the dispatch was rejected silently behind the
    // scenes.
    //
    // New rule mirrors the dispatch-layer prefix gate in backend.rs:
    // refuse only when the Pro command's verb is one of the irreversible
    // families. Read verbs (Get-*) and benign user-flow verbs
    // (Start-MeshVPNLogin, Open-ActivationSettings) pass through. Truly
    // irreversible Pro commands stay refused -- Invoke-MasterPrivacyClean,
    // Invoke-7Erase, Set-OEMInformation, Send-ContingencySignal,
    // Block-Protocol, Hide-BackendApps, Remove-AppxByName, etc.
    //
    // Investigator-mode safety is unchanged in practice: the genuine
    // "clear the seized device" surface is still refused; only the
    // non-irreversible paid features that an examiner might legitimately
    // need (e.g. mesh-VPN status to ferry evidence across the lab) get
    // through.
    if crate::license::is_advanced_mode() {
        let mutating = feature_id.starts_with("Clear-")
            || feature_id.starts_with("Erase-")
            || feature_id.starts_with("Remove-")
            || feature_id.starts_with("Reset-")
            || feature_id.starts_with("Invoke-")
            || feature_id.starts_with("Send-")
            || feature_id.starts_with("Block-")
            || feature_id.starts_with("Hide-")
            || feature_id.starts_with("Set-")
            || feature_id.starts_with("Register-")
            || feature_id.starts_with("Disable-")
            || feature_id.starts_with("Enable-")
            // P2: cascade dispatch — each step is irreversible; the
            // full_lockdown gate above already refuses, but belt-and-braces
            // here catches any direct sidecar calls.
            || feature_id == "run_destruct_step";
        if mutating {
            let msg = format!(
                "Refused: investigator mode does not run state-mutating Pro commands. \
                 '{}' would taint evidence. Logged.",
                feature_id
            );
            crate::log_message("warn", &format!("[Investigator] {}", msg));
            return Err(msg);
        }
    }

    if !crate::pro_install::pro_is_installed() {
        crate::log_message(
            "warn",
            &format!(
                "[Sidecar] '{}' aborted: pro_is_installed()=false",
                feature_id
            ),
        );
        // Special string -- the frontend's executeBackendCommand toast
        // handler watches for this exact prefix and auto-opens the
        // InstallProDialog instead of just showing a toast. Keep it
        // stable; if it changes, update src/hooks/useBackend.ts too.
        return Err(
            "PRO_NOT_INSTALLED:WinCommander Pro is not installed. Click 'Install Pro' to download it."
                .to_string(),
        );
    }
    crate::log_message(
        "info",
        &format!(
            "[Sidecar] '{}' pro_is_installed=true, proceeding to dispatch",
            feature_id
        ),
    );

    // Agent-affine commands (fleet_agent_*, Argus collectors, session monitor)
    // run on the dedicated fleet-agent session so their in-process state stays
    // co-located with the heartbeat loop that drains it. This path is outside
    // the pool (no semaphore permit → doesn't shrink cascade width) and falls
    // back to the pool if the agent session can't be established.
    if is_agent_affine(feature_id) {
        if let Some(result) = try_dispatch_via_agent(feature_id, &args).await {
            return result;
        }
        crate::log_message(
            "warn",
            &format!("[Sidecar] '{feature_id}' agent-affine but agent session down; using pool"),
        );
    }

    // Cap concurrent dispatches at POOL_CAPACITY. The permit lives
    // until function exit; any extra dispatches wait FIFO at acquire().
    let _permit = pool_semaphore()
        .acquire()
        .await
        .map_err(|e| format!("pool semaphore closed: {}", e))?;

    // Try to reuse an idle session; spawn a fresh one if none free.
    // The pool lock is released BEFORE awaiting spawn — otherwise
    // a slow spawn would block other dispatches from claiming idle
    // sessions in parallel.
    let popped = take_pooled_pro_session().await;
    let mut session: ProSession = match popped {
        Some(s) => {
            crate::log_message(
                "info",
                &format!("[Sidecar] '{}' reused pooled session", feature_id),
            );
            s
        }
        None => {
            crate::log_message(
                "info",
                &format!("[Sidecar] '{}' spawning fresh pro session", feature_id),
            );
            match spawn_pro_session().await {
                Ok(s) => {
                    crate::log_message("debug", &format!("[Sidecar] '{}' spawn ok", feature_id));
                    s
                }
                Err(e) => {
                    crate::log_message(
                        "error",
                        &format!("[Sidecar] '{}' spawn failed: {}", feature_id, e),
                    );
                    return Err(format!("Pro spawn failed: {}", e));
                }
            }
        }
    };

    // Auto-retry on transport error: Pro can exit (idle timeout, OS
    // killed it, user updated the binary) while Free still holds a
    // stale pipe handle. The first write returns "pipe is being
    // closed (os error 232)". On that, drop the session, respawn,
    // and dispatch a second time. Only transport errors retry —
    // semantic errors from Pro ([pro:*]) propagate immediately so
    // business-logic failures aren't masked by a "Pro went away" retry.
    for attempt in 0..2 {
        // Build a fresh Request per attempt — ids are session-scoped,
        // so a respawn means we want a new one on the new session.
        let req = Request {
            request_id: next_request_id(),
            feature_id: feature_id.to_string(),
            args: args.clone(),
        };

        match dispatch_request(&mut session, req).await {
            Ok(v) => {
                let preview =
                    serde_json::to_string(&v).unwrap_or_else(|_| "<unserializable>".to_string());
                let preview_short = if preview.len() > 600 {
                    // KT: slice on a char boundary — serde_json emits raw UTF-8,
                    // so Pro results with multi-byte chars (paths, SSIDs, emoji)
                    // straddling byte 600 would panic on a naive `&preview[..600]`.
                    let mut end = 600;
                    while end > 0 && !preview.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!(
                        "{}... [truncated, total len={}]",
                        &preview[..end],
                        preview.len()
                    )
                } else {
                    preview
                };
                crate::log_message(
                    "info",
                    &format!(
                        "[Sidecar] '{}' dispatch ok (attempt {}) result={}",
                        feature_id,
                        attempt + 1,
                        preview_short
                    ),
                );
                // Healthy session → return it to the pool for reuse.
                return_pro_session_to_pool(session).await;
                return Ok(v);
            }
            Err(e) if e.starts_with("[pro:") => {
                crate::log_message(
                    "warn",
                    &format!("[Sidecar] '{}' semantic error: {}", feature_id, e),
                );
                // Semantic error — pipe is still healthy, recycle.
                return_pro_session_to_pool(session).await;
                return Err(e);
            }
            Err(transport_err) => {
                // Transport-level failure — drop the broken session.
                if attempt == 0 {
                    crate::log_message(
                        "warn",
                        &format!(
                            "[ProPool] transport error on attempt 1, respawning: {}",
                            transport_err
                        ),
                    );
                    session = match spawn_pro_session().await {
                        Ok(s) => s,
                        Err(e) => {
                            return Err(format!(
                                "Pro spawn failed after transport error: {} (initial: {})",
                                e, transport_err
                            ));
                        }
                    };
                    continue;
                }
                // Second attempt also failed — surface the most recent
                // error (more relevant than the stale-pipe one).
                crate::log_message(
                    "error",
                    &format!(
                        "[Sidecar] '{}' transport failure both attempts: {}",
                        feature_id, transport_err
                    ),
                );
                return Err(format!("Pro transport error: {}", transport_err));
            }
        }
    }
    // Unreachable: the loop always returns by attempt 2.
    Err("Pro transport error: retry loop exited unexpectedly".to_string())
}

/// QA / dev surface: round-trip one Request through the live session.
/// Compiled out of release builds (C3) — an ungated pass-through to every Pro
/// command must never be reachable from the assumed-compromised WebView. Even
/// in debug it now requires a paid entitlement so it cannot bypass the tier gate.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn test_pro_dispatch(
    feature_id: String,
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    crate::license::require_paid("test_pro_dispatch")?;
    dispatch_paid_command(&feature_id, args.unwrap_or(serde_json::Value::Null)).await
}

/// Best-effort graceful shutdown — drains the pool, sending Bye to
/// every idle session. In-flight dispatches are not interrupted; their
/// sessions are dropped (not returned to the pool) once they finish
/// because the pool will already be torn down. Phase 11 wires this
/// into the Tauri `on_window_event(CloseRequested)` flow.
#[allow(dead_code)]
pub async fn close_pro_session() {
    let mut pool = pro_session_pool().lock().await;
    let sessions = std::mem::take(&mut *pool);
    drop(pool);
    stop_pro_sessions(sessions.into_iter().map(|entry| entry.session).collect()).await;
}

// ═══════════════════════════════════════════════════════════════════════
// Tests — reader_loop demux
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::sync::mpsc;

    #[test]
    fn mount_encryption_volume_has_a_longer_request_timeout() {
        assert_eq!(
            request_timeout_for("Mount-EncryptionVolume"),
            Duration::from_secs(20 * 60)
        );
    }

    #[test]
    fn ordinary_pro_requests_keep_the_standard_timeout() {
        for feature_id in [
            "Create-EncryptionVolume",
            "Mount-EncryptionVolume-Other",
            "Clear-EventLogs",
        ] {
            assert_eq!(
                request_timeout_for(feature_id),
                Duration::from_secs(120),
                "{feature_id} must not receive the mount timeout"
            );
        }
    }

    #[test]
    fn failed_pro_spawn_is_suppressed_until_the_cooldown_expires() {
        let now = Instant::now();
        let retry_after = now + Duration::from_secs(60);

        assert!(pro_spawn_cooldown_error(now, Some(retry_after)).is_some());
        assert!(pro_spawn_cooldown_error(retry_after, Some(retry_after)).is_none());
        assert!(pro_spawn_cooldown_error(now, None).is_none());
    }

    #[test]
    fn pooled_workers_expire_at_the_idle_deadline() {
        let now = Instant::now();
        assert!(!pool_session_is_expired(now, now + PRO_POOL_IDLE_TIMEOUT));
        assert!(pool_session_is_expired(now, now));
        assert!(pool_session_is_expired(now, now - Duration::from_secs(1)));
    }

    #[test]
    fn agent_affine_covers_fleet_and_collectors_only() {
        // Fleet channel + every Argus collector + the session monitor must pin
        // to the agent process.
        for id in [
            "fleet_agent_configure",
            "fleet_agent_set_posture",
            "fleet_agent_request_unenroll",
            "start_argus_app_usage",
            "stop_argus_dlp",
            "argus_tamper_status",
            "get_argus_print_usb_recent",
            "record_argus_tamper_event",
            "argus_monitoring_mirror",
            "start_session_monitor",
            "stop_session_monitor",
            "session_monitor_status",
            // "Access & Session Monitor" (auth-anomaly) — its collector state +
            // the "access" Argus signals it enqueues must co-locate with the
            // fleet-agent that drains them (regression pin for the stranding fix).
            "start_auth_anomaly_monitor",
            "stop_auth_anomaly_monitor",
            "auth_anomaly_status",
            "get_auth_anomaly_recent",
            "clear_auth_anomaly_recent",
            "set_auth_anomaly_config",
            "start_decoy_monitor",
            "stop_decoy_monitor",
            "decoy_monitor_status",
            "enroll_decoy",
            "set_decoy_read_audit_enabled",
        ] {
            assert!(is_agent_affine(id), "{id} should be agent-affine");
        }
        // Ordinary paid commands (incl. the parallel lockdown-cascade surfaces)
        // must NOT pin to the agent — they stay on the pool for parallelism.
        for id in [
            "Clear-DnsCache",
            "run_destruct_step",
            "start_network_honeypot",
            "vm_launch",
            "Set-AppCapabilityAccess",
            "start_screen_capture_watch",
        ] {
            assert!(!is_agent_affine(id), "{id} should NOT be agent-affine");
        }
    }

    /// Drive `reader_loop` over an in-memory duplex pair. The test plays
    /// the role of Pro: it writes signed Responses and Notifications
    /// onto its end of the pipe; reader_loop reads them off the other
    /// end and routes Response → inflight oneshot, Notification → sink.
    ///
    /// Proves the core A-1 invariant: a Notification mid-stream does
    /// NOT block a concurrent Request/Response and does NOT consume the
    /// dispatcher's reply.
    #[tokio::test]
    async fn reader_demuxes_notification_and_response() {
        let token = "test-session-token";
        let (mut pro_side, free_side) = tokio::io::duplex(8192);

        // Inflight map with one pending dispatcher waiting for request_id = 7.
        let inflight: InflightMap = Arc::new(Mutex::new(HashMap::new()));
        let (resp_tx, resp_rx) = oneshot::channel();
        inflight.lock().await.insert(7, resp_tx);

        // Notification sink → mpsc so the test can assert what was emitted.
        let (notif_tx, mut notif_rx) = mpsc::unbounded_channel::<(String, serde_json::Value)>();
        let sink: NotificationSink = Box::new(move |event, payload| {
            // Ignore send error — the receiver outlives this closure in
            // the test, so this never fails in practice.
            let _ = notif_tx.send((event, payload));
        });

        // Spawn the reader, taking the free side of the duplex.
        let reader_inflight = inflight.clone();
        let reader_token = token.to_string();
        let reader_task = tokio::spawn(async move {
            reader_loop(free_side, reader_inflight, reader_token, sink).await;
        });

        // Pro writes (in order):
        //   1. Notification — must hit the sink without touching inflight.
        //   2. Response(id=7) — must signal the dispatcher's oneshot.
        //   3. Notification — must hit the sink again, post-response.
        //   4. Bye — clean exit.
        let n1 = Envelope::Notification(Notification {
            event: "decoy-accessed".to_string(),
            payload: serde_json::json!({"path": "C:\\bait.docx"}),
        })
        .sign(token);
        write_envelope(&mut pro_side, &n1).await.unwrap();

        let resp = Envelope::Response(Response {
            request_id: 7,
            result: serde_json::json!({"ok": true, "code": 42}),
        })
        .sign(token);
        write_envelope(&mut pro_side, &resp).await.unwrap();

        let n2 = Envelope::Notification(Notification {
            event: "voice-lockdown-fired".to_string(),
            payload: serde_json::json!({"ts": 1234567890u64}),
        })
        .sign(token);
        write_envelope(&mut pro_side, &n2).await.unwrap();

        write_envelope(&mut pro_side, &Envelope::Bye).await.unwrap();
        pro_side.shutdown().await.unwrap();

        // Assert the dispatcher's oneshot fired with the right result.
        let received = resp_rx
            .await
            .expect("reader should have signalled the oneshot");
        assert_eq!(received.unwrap()["code"], 42);

        // Wait for reader to finish processing (Bye triggers exit).
        reader_task.await.unwrap();

        // Both notifications should have landed in the sink, in order.
        let first = notif_rx.recv().await.expect("first notification missing");
        assert_eq!(first.0, "decoy-accessed");
        assert_eq!(first.1["path"], "C:\\bait.docx");

        let second = notif_rx.recv().await.expect("second notification missing");
        assert_eq!(second.0, "voice-lockdown-fired");

        // Inflight map should be empty (cleared on reader exit).
        assert!(inflight.lock().await.is_empty());
    }

    /// When the reader exits (Bye / EOF / pipe close), it drains the
    /// inflight map so pending dispatchers see RecvError immediately
    /// instead of waiting until their per-call timeout fires.
    #[tokio::test]
    async fn reader_exit_unblocks_pending_dispatchers() {
        let token = "tk";
        let (pro_side, free_side) = tokio::io::duplex(1024);

        let inflight: InflightMap = Arc::new(Mutex::new(HashMap::new()));
        let (tx_a, rx_a) = oneshot::channel::<Result<serde_json::Value, String>>();
        let (tx_b, rx_b) = oneshot::channel::<Result<serde_json::Value, String>>();
        inflight.lock().await.insert(1, tx_a);
        inflight.lock().await.insert(2, tx_b);

        let sink: NotificationSink = Box::new(|_, _| {});

        let reader_inflight = inflight.clone();
        let reader_token = token.to_string();
        let reader_task = tokio::spawn(async move {
            reader_loop(free_side, reader_inflight, reader_token, sink).await;
        });

        // Close Pro's side → reader hits EOF and exits.
        drop(pro_side);
        reader_task.await.unwrap();

        // Both pending dispatchers see RecvError (Err on the recv side).
        assert!(rx_a.await.is_err());
        assert!(rx_b.await.is_err());
        assert!(inflight.lock().await.is_empty());
    }

    /// A frame with a bad HMAC tag (or unsigned post-handshake) must be
    /// dropped without signalling any dispatcher or sink. Reader keeps
    /// going so a single tampered frame doesn't kill the session.
    #[tokio::test]
    async fn reader_drops_unsigned_and_continues() {
        let token = "tk";
        let (mut pro_side, free_side) = tokio::io::duplex(2048);

        let inflight: InflightMap = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = oneshot::channel::<Result<serde_json::Value, String>>();
        inflight.lock().await.insert(5, tx);

        let (notif_tx, mut notif_rx) = mpsc::unbounded_channel::<(String, serde_json::Value)>();
        let sink: NotificationSink = Box::new(move |e, p| {
            let _ = notif_tx.send((e, p));
        });

        let reader_inflight = inflight.clone();
        let reader_token = token.to_string();
        let reader_task = tokio::spawn(async move {
            reader_loop(free_side, reader_inflight, reader_token, sink).await;
        });

        // First frame: unsigned Response — reader must drop and keep reading.
        let bad = Envelope::Response(Response {
            request_id: 5,
            result: serde_json::json!("should-not-see-this"),
        });
        write_envelope(&mut pro_side, &bad).await.unwrap();

        // Second frame: properly signed Response with same id — this one
        // must succeed, proving the reader didn't wedge on the bad frame.
        let good = Envelope::Response(Response {
            request_id: 5,
            result: serde_json::json!("see-this"),
        })
        .sign(token);
        write_envelope(&mut pro_side, &good).await.unwrap();

        write_envelope(&mut pro_side, &Envelope::Bye).await.unwrap();
        pro_side.shutdown().await.unwrap();

        let got = rx.await.unwrap().unwrap();
        assert_eq!(got, serde_json::json!("see-this"));
        assert!(notif_rx.try_recv().is_err()); // no notifications emitted
        reader_task.await.unwrap();
    }
}
