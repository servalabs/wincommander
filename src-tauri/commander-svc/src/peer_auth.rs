// SPDX-License-Identifier: AGPL-3.0-or-later
//! Peer authentication for `CapabilityClass::SessionHelper` (D-2).
//!
//! `wincmd-shared/src/svc.rs`'s `classify_verb` sorts every `svc.*` verb into
//! `ReadOnly` / `SessionHelper` / `Privileged`, but only *classifies* --
//! confirming that the connected pipe peer actually deserves the
//! `SessionHelper` bucket is this module's job. Three real callers need it:
//! the per-user Clipboard Guard helper (`svc.clipboard.report_event`), the
//! Ink Receipt bridge (`svc.ink_receipt.reserve_ticket` /
//! `.report_receipt`), and the Pro/Free -> svc policy-epoch hop
//! (`svc.policy.install_epoch`). None of them is admin or LocalSystem, so
//! `pipe.rs::authorize`'s existing SID check can't attest them; without a
//! real peer check here, `SessionHelper` would be indistinguishable from
//! "any process in the interactive user's session" -- exactly the hole D-2
//! exists to close.
//!
//! # The gate
//!
//! [`SessionHelperGate::authorize`] grants [`TrustOrigin::SessionHelperPinned`]
//! only when **every** one of these holds for the connecting peer process:
//!
//! 1. **Interactive-session membership** -- the peer's token session id
//!    equals the session Windows currently treats as "the" interactive one
//!    (`WTSGetActiveConsoleSessionId`), not a disconnected/other session.
//! 2. **Binary-path pinning** -- the peer's fully canonicalized image path
//!    sits directly under the resolved install directory and its filename
//!    is on [`ALLOWED_SESSION_HELPER_FILENAMES`].
//! 3. **Rate limiting** -- a fixed-window counter keyed on
//!    `(session id, canonical path, verb)` bounds how often even a fully
//!    pinned peer may call in (see [`RATE_LIMIT_MAX_CALLS`] /
//!    [`RATE_LIMIT_WINDOW`]), so a compromised-but-pinned helper cannot
//!    flood the receipt journal, the ticket ledger, or policy installs.
//!    Exceeding the limit is a deny that still counts the attempt -- it is
//!    never silently dropped.
//!
//! Every one of these is fail-closed: a probe that cannot determine a fact
//! (can't open the process, can't resolve the path, can't tell which session
//! is active) denies exactly like
//! a fact that was determined and failed. [`SessionHelperGate::authorize_at`]
//! is written so the **only** `Ok` return is the single line at the very
//! end, after every check has run via `?` or an explicit early `return Err`
//! -- a future fifth check slots in the same way and cannot accidentally
//! introduce an early-allow path.
//!
//! # TOCTOU -- residual risk, stated honestly
//!
//! A PID can be recycled between "we learned this PID from the pipe" and
//! "we act on the peer's identity". [`WinPeerAuthProbe::identity`] narrows
//! this: it opens the process **once** and reads both the image path and
//! the token session id from that **same** handle, so the whole identity
//! snapshot is internally consistent with "whoever held this PID at the
//! moment we opened it" even if the PID is reused a moment later. It does
//! **not** eliminate the window before that first `OpenProcess` call (the
//! caller must pass a PID that was *just* derived from the live pipe, which
//! is `pipe.rs`'s responsibility, not this module's), and it does not cover
//! the risk that a process able to replace an executable inside the protected
//! install directory can inherit that filename's authorization. Packaging
//! must therefore keep the service directory administrator/SYSTEM-writable
//! only. This gate deliberately does not require Authenticode so unsigned and
//! self-signed Free builds remain functional on managed client machines.
//!
//! # Wiring (for the caller that connects this to `pipe.rs`)
//!
//! This module is not wired into `main.rs` / `pipe.rs` yet -- deliberately,
//! per this phase's scope. To wire it up:
//!
//! * Add `#[cfg(windows)] mod peer_auth;` to `main.rs`.
//! * Construct one `SessionHelperGate::new(Arc::new(WinPeerAuthProbe))` for
//!   the service's lifetime (e.g. behind a `OnceLock`/`Arc` held alongside
//!   the pipe server), not one per connection -- the rate limiter's state
//!   must outlive individual connections to mean anything.
//! * In `pipe.rs::authorize`'s `CapabilityClass::SessionHelper` arm, call
//!   `gate.authorize(pid, verb)` where `pid` is the **same** value already
//!   obtained from `GetNamedPipeClientProcessId` for the existing
//!   `caller_is_privileged` check -- do not re-derive it from the pipe
//!   handle a second time here.
//! * [`WinPeerAuthProbe`]'s methods do blocking Win32 calls. Call
//!   `gate.authorize(..)` from inside
//!   `tokio::task::spawn_blocking`, exactly as `print_usb_monitor.rs`'s
//!   `PrintProbe` contract requires for its own blocking probes.
//! * On the `Ok(trust_origin)` path, whatever handler persists the
//!   resulting clipboard event / receipt / installed epoch must store
//!   `trust_origin` alongside the record (D-2's "trust-origin marker").
//!
//! # A known placeholder
//!
//! [`ALLOWED_SESSION_HELPER_FILENAMES`] lists the per-user Clipboard Guard
//! helper and Ink Receipt bridge under working-name filenames -- neither
//! binary exists yet (both are later-phase deliverables per the plan).
//! Whoever names and ships those binaries for real must reconcile this list
//! with the actual filenames before `SessionHelper` traffic can ever reach
//! them; until then every peer claiming to be one of them is correctly
//! denied by [`PeerAuthError::PathNotAllowed`], which is the fail-closed
//! default this module always prefers over guessing.
//!
//! # Cargo.toml dependency note
//!
//! [`WinPeerAuthProbe::active_interactive_session`] needs
//! `windows_sys::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId`,
//! which requires the `Win32_System_RemoteDesktop` feature on the
//! `windows-sys` dependency. That feature is not yet enabled in
//! `commander-svc/Cargo.toml` as of this module's introduction -- see the
//! handoff note for the exact one-line diff. Every other Win32 call this
//! module uses (`OpenProcess`, `OpenProcessToken`, `GetTokenInformation`,
//! `QueryFullProcessImageNameW`) is already covered by features that crate
//! already enables. Path canonicalization uses `std::fs::canonicalize`
//! (stdlib, no windows-sys feature), so it does not need
//! `Win32_Storage_FileSystem`.

#![cfg(windows)]
// Not wired into `main.rs`/`pipe.rs` yet (see the "Wiring" section above) --
// nothing constructs a `SessionHelperGate` or calls its public API, so every
// item here is dead code until that lands. Same precedent as
// `pro_broker.rs`'s identical `#![allow(dead_code)]` for the same reason.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{GetTokenInformation, TokenSessionId, TOKEN_QUERY};
use windows_sys::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

// ── Policy constants ─────────────────────────────────────────────────────

/// Leaf filenames allowed to call in as a `SessionHelper`, all expected
/// directly under the resolved install directory (no subdirectories --
/// keeping the allow-list flat avoids a second traversal surface). See the
/// module-level "known placeholder" note: the helper/bridge names below are
/// working names pending the binaries that will actually carry them.
pub const ALLOWED_SESSION_HELPER_FILENAMES: &[&str] = &[
    "wincommander-free.exe",
    "wincommander-pro.exe",
    "wincmd-clip-helper.exe",
    "wincmd-ink-receipt-bridge.exe",
];

/// Maximum SessionHelper calls counted per `(session, canonical path, verb)`
/// inside [`RATE_LIMIT_WINDOW`]. Sized well above the busiest legitimate
/// caller -- `svc.clipboard.report_event` fires at most once per clipboard
/// change, and no human copies faster than a few times a second -- while
/// still bounding how hard a compromised-but-pinned helper can hammer the
/// receipt journal, the ticket ledger, or a policy install.
pub const RATE_LIMIT_MAX_CALLS: u32 = 30;

/// Fixed window [`RATE_LIMIT_MAX_CALLS`] is counted over.
pub const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10);

// ── Trust origin ─────────────────────────────────────────────────────────

/// Marks *how* a submission got past the `SessionHelper` gate. D-2 requires
/// every stored clipboard event / receipt / installed epoch to carry this
/// alongside the record, so a later forged-record investigation can tell
/// which trust path produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustOrigin {
    /// Passed interactive-session membership, canonical binary-path
    /// pinning, and rate limiting. The serialized label is retained for
    /// compatibility with existing receipts even though no certificate is
    /// required.
    SessionHelperPinned,
}

// ── Errors ────────────────────────────────────────────────────────────────

/// Why a `SessionHelper` authorization attempt was denied.
///
/// Every variant's [`std::fmt::Display`] text is a fixed, stable string --
/// none of them ever interpolates the peer's resolved path, pid, or verb
/// name. Plan §8 keeps exactly this kind of detail out of error messages
/// and log lines; callers may put this `Display` output directly into an
/// `ErrorReply.message` without a second redaction pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerAuthError {
    /// Could not determine the peer's identity at all (process exited,
    /// `OpenProcess`/`OpenProcessToken`/`QueryFullProcessImageNameW`/
    /// `GetTokenInformation` failed, or the resolved path could not be
    /// canonicalized). Fail-closed: treated exactly like every other deny.
    IdentityUnavailable,
    /// Could not determine which session is currently the interactive one.
    NoInteractiveSession,
    /// The peer is not running in the session identified as the current
    /// interactive session (e.g. a disconnected or other RDP session).
    WrongSession,
    /// The peer's canonical image path is not on the binary-path allow-list.
    PathNotAllowed,
    /// The per-(session, path, verb) rate limit was exceeded.
    RateLimited,
}

impl std::fmt::Display for PeerAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            PeerAuthError::IdentityUnavailable => {
                "session-helper peer identity could not be determined"
            }
            PeerAuthError::NoInteractiveSession => "no interactive session is currently active",
            PeerAuthError::WrongSession => {
                "session-helper peer is not in the active interactive session"
            }
            PeerAuthError::PathNotAllowed => {
                "session-helper peer binary path is not on the allow-list"
            }
            PeerAuthError::RateLimited => "session-helper call rate limit exceeded",
        };
        f.write_str(text)
    }
}

impl std::error::Error for PeerAuthError {}

// ── Probe abstraction ────────────────────────────────────────────────────

/// Facts about a peer process gathered from a single opened handle/token,
/// so the whole snapshot is internally consistent even if the PID is
/// recycled a moment later (see the module-level TOCTOU note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentitySnapshot {
    /// Session id from the peer process's own token (`TokenSessionId`).
    pub session_id: u32,
    /// Fully resolved, absolute image path -- reparse points followed,
    /// `.`/`..` collapsed, any 8.3 short-name component expanded.
    pub canonical_image_path: PathBuf,
}

/// Abstraction over every Win32/OS fact [`SessionHelperGate`] needs about a
/// peer. The real implementation ([`WinPeerAuthProbe`]) makes the actual
/// syscalls; tests inject a fake so the decision logic in
/// [`SessionHelperGate::authorize_at`] is fully exercised without any Win32
/// dependency.
pub trait PeerAuthProbe: Send + Sync {
    /// Resolve session id + canonical image path for `pid`. Implementations
    /// MUST open the process once and answer both facts from that same
    /// handle/token rather than re-opening by PID a second time.
    fn identity(&self, pid: u32) -> Result<PeerIdentitySnapshot, PeerAuthError>;

    /// The session id Windows currently treats as "the" interactive
    /// session -- the physical console, not a disconnected/other session.
    fn active_interactive_session(&self) -> Result<u32, PeerAuthError>;
}

// ── Path canonicalization ────────────────────────────────────────────────

/// Fully resolve `raw_path`: reparse points (symlinks/junctions) followed,
/// `.`/`..` segments collapsed, any 8.3 short-name component expanded to
/// its long form. Backed by `std::fs::canonicalize`, which on Windows opens
/// the target and calls `GetFinalPathNameByHandleW` -- the mechanism that
/// defeats all three tricks D-2 names ("a path check that can be bypassed
/// by a junction is not a check").
///
/// Exposed standalone (not folded into [`WinPeerAuthProbe::identity`]) so
/// this behaviour can be unit tested against a real temp-directory
/// filesystem without any Win32 peer/process APIs at all.
pub fn canonicalize_peer_path(raw_path: &Path) -> Result<PathBuf, PeerAuthError> {
    std::fs::canonicalize(raw_path).map_err(|_| PeerAuthError::IdentityUnavailable)
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    // Windows paths are case-insensitive but case-preserving; both sides
    // here have already been through `canonicalize_peer_path`/
    // `resolve_install_root`, so a case-insensitive string compare (not a
    // full Unicode case-fold) is sufficient for the drive-letter/ASCII
    // paths this allow-list deals in.
    a.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
}

/// Resolve the installed program directory from this service binary's own
/// location (`current_exe()`'s parent, canonicalized). Returns `None` if
/// that can't be determined for any reason -- callers must treat `None` as
/// "no path is ever allowed", never as "skip the path check".
fn resolve_install_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    std::fs::canonicalize(dir).ok()
}

// ── The gate ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RateLimitKey {
    session_id: u32,
    canonical_path_lower: String,
    verb: String,
}

struct WindowState {
    window_start: Instant,
    count: u32,
}

/// Enforces the `CapabilityClass::SessionHelper` peer gate (D-2): session
/// membership, binary-path pinning, and
/// per-(session, path, verb) rate limiting. See the module doc for the
/// full design and wiring notes.
pub struct SessionHelperGate {
    probe: Arc<dyn PeerAuthProbe>,
    allowed_root: Option<PathBuf>,
    windows: Mutex<HashMap<RateLimitKey, WindowState>>,
}

impl SessionHelperGate {
    /// Production constructor. Resolves the binary-path allow-list root
    /// from this service's own install directory. If that can't be
    /// resolved, every call is denied with [`PeerAuthError::PathNotAllowed`]
    /// -- refusing a legitimate helper is the correct default here, not a
    /// bug, when we can't positively identify "the" install directory.
    pub fn new(probe: Arc<dyn PeerAuthProbe>) -> Self {
        Self::with_allowed_root(probe, resolve_install_root())
    }

    /// Constructor that pins the allow-list root explicitly instead of
    /// deriving it from `current_exe()`. Used by tests; also available to
    /// an integration harness that wants to point the gate at a controlled
    /// staging directory. Production wiring should call [`Self::new`].
    pub fn with_allowed_root(probe: Arc<dyn PeerAuthProbe>, allowed_root: Option<PathBuf>) -> Self {
        Self {
            probe,
            allowed_root,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Decide whether the process at `pid` may act as a `SessionHelper` for
    /// `verb` right now. Blocking: see the module doc's wiring note --
    /// call this from `tokio::task::spawn_blocking`, not directly from an
    /// async fn.
    pub fn authorize(&self, pid: u32, verb: &str) -> Result<TrustOrigin, PeerAuthError> {
        self.authorize_at(pid, verb, Instant::now())
    }

    /// [`Self::authorize`] with an injectable clock, so the rate limiter's
    /// window logic is deterministically testable.
    fn authorize_at(
        &self,
        pid: u32,
        verb: &str,
        now: Instant,
    ) -> Result<TrustOrigin, PeerAuthError> {
        // Every prior line either returns early via `?`/`return Err` or
        // falls through; the ONLY `Ok` is the final line below. A future
        // sixth check slots in the same shape and cannot accidentally
        // create an early-allow path.
        let identity = self.probe.identity(pid)?;
        let active_session = self.probe.active_interactive_session()?;

        if identity.session_id != active_session {
            return Err(PeerAuthError::WrongSession);
        }

        if !self.is_allowed_helper_path(&identity.canonical_image_path) {
            return Err(PeerAuthError::PathNotAllowed);
        }

        self.check_rate_limit(&identity, verb, now)?;

        Ok(TrustOrigin::SessionHelperPinned)
    }

    fn is_allowed_helper_path(&self, canonical: &Path) -> bool {
        let Some(root) = &self.allowed_root else {
            return false;
        };
        let Some(parent) = canonical.parent() else {
            return false;
        };
        if !paths_equal(parent, root) {
            return false;
        }
        let Some(name) = canonical.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        ALLOWED_SESSION_HELPER_FILENAMES
            .iter()
            .any(|allowed| name.eq_ignore_ascii_case(allowed))
    }

    fn check_rate_limit(
        &self,
        identity: &PeerIdentitySnapshot,
        verb: &str,
        now: Instant,
    ) -> Result<(), PeerAuthError> {
        let key = RateLimitKey {
            session_id: identity.session_id,
            canonical_path_lower: identity
                .canonical_image_path
                .to_string_lossy()
                .to_lowercase(),
            verb: verb.to_string(),
        };

        // Mutex poisoning would only happen if some other call panicked
        // while holding this lock; recovering the guard rather than
        // unwrapping keeps this path panic-free even then.
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let state = windows.entry(key).or_insert_with(|| WindowState {
            window_start: now,
            count: 0,
        });

        if now.duration_since(state.window_start) >= RATE_LIMIT_WINDOW {
            state.window_start = now;
            state.count = 0;
        }

        // Increment even past the limit -- an exceeded limit is a deny
        // that still counts, never a silent drop, so a sustained flood
        // stays observable in `state.count` rather than vanishing.
        state.count = state.count.saturating_add(1);

        if state.count > RATE_LIMIT_MAX_CALLS {
            return Err(PeerAuthError::RateLimited);
        }
        Ok(())
    }
}

// ── Production probe (real Win32) ──────────────────────────────────────

/// Real [`PeerAuthProbe`] backed by Win32 syscalls.
///
/// # Blocking
/// Every method does synchronous, blocking I/O. See the module doc's
/// wiring note: callers on a tokio runtime MUST run
/// [`SessionHelperGate::authorize`] inside `tokio::task::spawn_blocking`.
pub struct WinPeerAuthProbe;

struct HandleGuard(HANDLE);
impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

impl PeerAuthProbe for WinPeerAuthProbe {
    fn identity(&self, pid: u32) -> Result<PeerIdentitySnapshot, PeerAuthError> {
        unsafe {
            // Open the process ONCE; answer every fact that needs a handle
            // (image path, token session id) from this same handle/token
            // rather than re-opening by PID per fact -- see the
            // module-level TOCTOU note for what this does and doesn't buy.
            let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if proc.is_null() {
                return Err(PeerAuthError::IdentityUnavailable);
            }
            let _proc_guard = HandleGuard(proc);

            // 32768 = Windows' extended-length path ceiling (32767 chars +
            // NUL); QueryFullProcessImageNameW reports the used length back
            // through `size`, excluding the terminator.
            let mut buf = [0u16; 32768];
            let mut size = buf.len() as u32;
            if QueryFullProcessImageNameW(proc, 0, buf.as_mut_ptr(), &mut size) == 0 {
                return Err(PeerAuthError::IdentityUnavailable);
            }
            let raw_path = String::from_utf16_lossy(&buf[..size as usize]);
            let canonical_image_path = canonicalize_peer_path(Path::new(&raw_path))?;

            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(proc, TOKEN_QUERY, &mut token) == 0 {
                return Err(PeerAuthError::IdentityUnavailable);
            }
            let _token_guard = HandleGuard(token);

            let mut session_id: u32 = 0;
            let mut returned = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenSessionId,
                &mut session_id as *mut u32 as *mut core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
                &mut returned,
            );
            if ok == 0 {
                return Err(PeerAuthError::IdentityUnavailable);
            }

            Ok(PeerIdentitySnapshot {
                session_id,
                canonical_image_path,
            })
        }
    }

    fn active_interactive_session(&self) -> Result<u32, PeerAuthError> {
        // 0xFFFFFFFF is the documented sentinel for "no session is
        // currently attached to the console" (e.g. a locked console with
        // no one signed in) -- treat that as "there is no interactive
        // session to match against right now", not as a session id.
        let session_id = unsafe { WTSGetActiveConsoleSessionId() };
        if session_id == 0xFFFF_FFFF {
            return Err(PeerAuthError::NoInteractiveSession);
        }
        Ok(session_id)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Fake probe ──────────────────────────────────────────────────────

    struct FakeProbe {
        identity_fn: Box<dyn Fn(u32) -> Result<PeerIdentitySnapshot, PeerAuthError> + Send + Sync>,
        active_session: Result<u32, PeerAuthError>,
    }

    impl PeerAuthProbe for FakeProbe {
        fn identity(&self, pid: u32) -> Result<PeerIdentitySnapshot, PeerAuthError> {
            (self.identity_fn)(pid)
        }
        fn active_interactive_session(&self) -> Result<u32, PeerAuthError> {
            self.active_session
        }
    }

    const TEST_ROOT: &str = r"C:\Program Files\WinCommander";
    const TEST_VERB: &str = "svc.clipboard.report_event";

    fn ok_identity() -> PeerIdentitySnapshot {
        PeerIdentitySnapshot {
            session_id: 7,
            canonical_image_path: PathBuf::from(TEST_ROOT).join("wincommander-free.exe"),
        }
    }

    fn fixed_probe(
        identity: Result<PeerIdentitySnapshot, PeerAuthError>,
        active_session: Result<u32, PeerAuthError>,
    ) -> FakeProbe {
        FakeProbe {
            identity_fn: Box::new(move |_pid| identity.clone()),
            active_session,
        }
    }

    fn gate_with(probe: FakeProbe) -> SessionHelperGate {
        SessionHelperGate::with_allowed_root(Arc::new(probe), Some(PathBuf::from(TEST_ROOT)))
    }

    // ── Happy path ────────────────────────────────────────────────────

    #[test]
    fn allow_unsigned_peer_when_session_and_path_pass() {
        let gate = gate_with(fixed_probe(Ok(ok_identity()), Ok(7)));
        assert_eq!(
            gate.authorize_at(4242, TEST_VERB, Instant::now()),
            Ok(TrustOrigin::SessionHelperPinned)
        );
    }

    // ── Deny on each individual failure ─────────────────────────────────

    #[test]
    fn deny_on_session_mismatch() {
        let gate = gate_with(fixed_probe(Ok(ok_identity()), Ok(99)));
        assert_eq!(
            gate.authorize_at(1, TEST_VERB, Instant::now()),
            Err(PeerAuthError::WrongSession)
        );
    }

    #[test]
    fn deny_on_path_not_under_install_root() {
        let mut identity = ok_identity();
        identity.canonical_image_path = PathBuf::from(r"C:\Users\attacker\evil.exe");
        let gate = gate_with(fixed_probe(Ok(identity), Ok(7)));
        assert_eq!(
            gate.authorize_at(1, TEST_VERB, Instant::now()),
            Err(PeerAuthError::PathNotAllowed)
        );
    }

    #[test]
    fn deny_on_filename_not_on_allow_list_even_under_correct_root() {
        let mut identity = ok_identity();
        identity.canonical_image_path = PathBuf::from(TEST_ROOT).join("cmd.exe");
        let gate = gate_with(fixed_probe(Ok(identity), Ok(7)));
        assert_eq!(
            gate.authorize_at(1, TEST_VERB, Instant::now()),
            Err(PeerAuthError::PathNotAllowed)
        );
    }

    #[test]
    fn deny_when_allowed_root_is_unresolved() {
        // Fail-closed default: if the install root can't be determined,
        // NOTHING is allowed, even a peer that would otherwise pass.
        let probe = fixed_probe(Ok(ok_identity()), Ok(7));
        let gate = SessionHelperGate::with_allowed_root(Arc::new(probe), None);
        assert_eq!(
            gate.authorize_at(1, TEST_VERB, Instant::now()),
            Err(PeerAuthError::PathNotAllowed)
        );
    }

    // ── Deny when any probe errors (fail-closed) ────────────────────────

    #[test]
    fn deny_when_identity_probe_errors() {
        let gate = gate_with(fixed_probe(Err(PeerAuthError::IdentityUnavailable), Ok(7)));
        assert_eq!(
            gate.authorize_at(1, TEST_VERB, Instant::now()),
            Err(PeerAuthError::IdentityUnavailable)
        );
    }

    #[test]
    fn deny_when_active_session_probe_errors() {
        let gate = gate_with(fixed_probe(
            Ok(ok_identity()),
            Err(PeerAuthError::NoInteractiveSession),
        ));
        assert_eq!(
            gate.authorize_at(1, TEST_VERB, Instant::now()),
            Err(PeerAuthError::NoInteractiveSession)
        );
    }

    // ── Rate limiting ─────────────────────────────────────────────────

    #[test]
    fn rate_limiter_permits_up_to_limit_then_denies() {
        let gate = gate_with(fixed_probe(Ok(ok_identity()), Ok(7)));
        let now = Instant::now();
        for i in 0..RATE_LIMIT_MAX_CALLS {
            assert_eq!(
                gate.authorize_at(1, TEST_VERB, now),
                Ok(TrustOrigin::SessionHelperPinned),
                "call {i} should be within the limit"
            );
        }
        assert_eq!(
            gate.authorize_at(1, TEST_VERB, now),
            Err(PeerAuthError::RateLimited)
        );
        // A further call in the same window keeps being denied, not
        // silently ignored.
        assert_eq!(
            gate.authorize_at(1, TEST_VERB, now),
            Err(PeerAuthError::RateLimited)
        );
    }

    #[test]
    fn rate_limiter_resets_after_window_elapses() {
        let gate = gate_with(fixed_probe(Ok(ok_identity()), Ok(7)));
        let start = Instant::now();
        for _ in 0..RATE_LIMIT_MAX_CALLS {
            gate.authorize_at(1, TEST_VERB, start).unwrap();
        }
        assert_eq!(
            gate.authorize_at(1, TEST_VERB, start),
            Err(PeerAuthError::RateLimited)
        );

        let later = start + RATE_LIMIT_WINDOW + Duration::from_millis(1);
        assert_eq!(
            gate.authorize_at(1, TEST_VERB, later),
            Ok(TrustOrigin::SessionHelperPinned)
        );
    }

    #[test]
    fn rate_limiter_is_scoped_per_verb() {
        let gate = gate_with(fixed_probe(Ok(ok_identity()), Ok(7)));
        let now = Instant::now();
        for _ in 0..RATE_LIMIT_MAX_CALLS {
            gate.authorize_at(1, "svc.clipboard.report_event", now)
                .unwrap();
        }
        assert_eq!(
            gate.authorize_at(1, "svc.clipboard.report_event", now),
            Err(PeerAuthError::RateLimited)
        );
        // A different verb from the SAME pinned peer has its own bucket --
        // flooding one verb must not lock out an unrelated one.
        assert_eq!(
            gate.authorize_at(1, "svc.policy.install_epoch", now),
            Ok(TrustOrigin::SessionHelperPinned)
        );
    }

    #[test]
    fn rate_limit_key_distinguishes_session_path_and_verb_independently() {
        let base = RateLimitKey {
            session_id: 7,
            canonical_path_lower: r"c:\program files\wincommander\wincommander-free.exe".into(),
            verb: TEST_VERB.into(),
        };
        let different_session = RateLimitKey {
            session_id: 8,
            ..base.clone()
        };
        let different_path = RateLimitKey {
            canonical_path_lower: r"c:\program files\wincommander\other.exe".into(),
            ..base.clone()
        };
        let different_verb = RateLimitKey {
            verb: "svc.policy.install_epoch".into(),
            ..base.clone()
        };
        // Each axis is independently part of the key -- changing only one
        // field must change bucket identity, so one user's/verb's/path's
        // flood cannot borrow another's quota.
        assert_ne!(base, different_session);
        assert_ne!(base, different_path);
        assert_ne!(base, different_verb);
        assert_eq!(base.clone(), base);
    }

    // ── Trust origin round-trips ─────────────────────────────────────

    #[test]
    fn trust_origin_serializes_as_stable_snake_case() {
        let json = serde_json::to_string(&TrustOrigin::SessionHelperPinned).unwrap();
        assert_eq!(json, "\"session_helper_pinned\"");
    }

    // ── Error messages never leak a path ────────────────────────────

    #[test]
    fn error_display_never_looks_like_a_path() {
        for err in [
            PeerAuthError::IdentityUnavailable,
            PeerAuthError::NoInteractiveSession,
            PeerAuthError::WrongSession,
            PeerAuthError::PathNotAllowed,
            PeerAuthError::RateLimited,
        ] {
            let text = err.to_string();
            assert!(
                !text.contains('\\') && !text.contains('/'),
                "error text must never look like it embeds a path: {text:?}"
            );
        }
    }

    // ── canonicalize_peer_path: real-filesystem defeat tests ─────────

    fn unique_temp_dir(label: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "wincmd-peer-auth-test-{label}-{}-{}",
            std::process::id(),
            label.len() // trivial extra entropy to dodge same-label reuse across parallel test runs
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn canonicalize_peer_path_defeats_dotdot_segments() {
        let base = unique_temp_dir("dotdot");
        let target = base.join("target.exe");
        std::fs::write(&target, b"x").unwrap();
        let messy = base.join("nonexistent").join("..").join("target.exe");

        let direct = canonicalize_peer_path(&target).unwrap();
        let via_dotdot = canonicalize_peer_path(&messy).unwrap();
        assert_eq!(direct, via_dotdot);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn canonicalize_peer_path_defeats_junction_redirection() {
        let base = unique_temp_dir("junction");
        let real_dir = base.join("real");
        std::fs::create_dir_all(&real_dir).unwrap();
        let target = real_dir.join("target.exe");
        std::fs::write(&target, b"x").unwrap();
        let link_dir = base.join("link");

        // Directory junctions need no special privilege (unlike symlinks),
        // so this should succeed in any normal environment; if it doesn't,
        // skip rather than fail the whole suite on an environment quirk.
        let status = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&link_dir)
            .arg(&real_dir)
            .output();

        match status {
            Ok(out) if out.status.success() => {
                let via_junction = canonicalize_peer_path(&link_dir.join("target.exe")).unwrap();
                let direct = canonicalize_peer_path(&target).unwrap();
                assert_eq!(
                    direct, via_junction,
                    "a junction must resolve to the real target's canonical path"
                );
            }
            _ => {
                eprintln!("skipping junction test: `mklink /J` unavailable in this environment");
            }
        }

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn canonicalize_peer_path_defeats_short_names() {
        let base = unique_temp_dir("shortname");
        let long_dir = base.join("VeryLongDirectoryNameForShortNameDefeatTest");
        std::fs::create_dir_all(&long_dir).unwrap();
        let target = long_dir.join("target.exe");
        std::fs::write(&target, b"x").unwrap();

        if let Some(short) = short_path_for_test(&target) {
            if short != target {
                let via_short = canonicalize_peer_path(&short).unwrap();
                let direct = canonicalize_peer_path(&target).unwrap();
                assert_eq!(
                    direct, via_short,
                    "an 8.3 short name must resolve to the same canonical path as the long name"
                );
                let _ = std::fs::remove_dir_all(&base);
                return;
            }
        }
        eprintln!(
            "skipping short-name test: 8.3 name generation unavailable/disabled on this volume"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Test-only: ask `cmd.exe` for the 8.3 short form of `path`. Not used
    /// by production code -- the real probe never needs to CONSTRUCT a
    /// short name, only to have `canonicalize_peer_path` correctly UNDO one
    /// if a caller supplies it.
    ///
    /// Deliberately writes a throwaway `.bat` file and invokes it with
    /// `path` as a normal argument, rather than passing an inline
    /// `for %I in ("...") do ...` string to `cmd /C`: `cmd /C "<script>"`
    /// only strips one pair of outer quotes when the script contains no
    /// OTHER quotes, and this script's own `("...")` quoting defeats that
    /// heuristic, corrupting the path. A `.bat` file sidesteps it entirely
    /// -- the path is passed as an ordinary, normally-quoted argument.
    fn short_path_for_test(path: &Path) -> Option<PathBuf> {
        let bat_path =
            std::env::temp_dir().join(format!("wincmd-shortname-probe-{}.bat", std::process::id()));
        std::fs::write(
            &bat_path,
            "@echo off\r\nfor %%I in (\"%~1\") do echo %%~sI\r\n",
        )
        .ok()?;
        let output = std::process::Command::new(&bat_path)
            .arg(path)
            .output()
            .ok();
        let _ = std::fs::remove_file(&bat_path);
        let output = output?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            None
        } else {
            Some(PathBuf::from(text))
        }
    }

    // ── resolve_install_root sanity ──────────────────────────────────

    #[test]
    fn resolve_install_root_points_at_an_existing_directory() {
        // In any normal test environment `current_exe()` succeeds, so this
        // should resolve to the test binary's own (existing) directory.
        if let Some(root) = resolve_install_root() {
            assert!(root.is_dir());
        }
    }
}
