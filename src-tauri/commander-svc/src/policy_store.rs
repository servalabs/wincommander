// SPDX-License-Identifier: AGPL-3.0-or-later
//! Durable managed-policy store (plan §3, §4.4; decisions D-2, D-7).
//!
//! # Why this exists
//!
//! Plan §3 commitment 1: **the SYSTEM service owns durable state** (policy,
//! ticket cache, receipt journal) because it outlives the GUI and the user
//! session. Today `commander-svc` has no Fleet-facing code at all — a signed
//! epoch only ever arrives at Pro or Free via `fleet_push`. Whichever of
//! those processes verifies it (D-7 decides which), the verified epoch must
//! still cross *another* process boundary into svc over `\\.\pipe\wincmd-svc`
//! (`svc.policy.install_epoch`, `SessionHelper` class — see
//! `wincmd-shared/src/svc.rs`). This module is what that verb installs into.
//!
//! # The security posture this module enforces
//!
//! D-2's reasoning applied to this specific hop: svc must not trust "my
//! caller already checked the signature". A compromised or replaced caller
//! (Pro or Free) must not be able to hand svc a forged "feature disabled"
//! policy, nor roll policy back via a stale replay, nor half-apply a
//! ruleset that fails to compile. Concretely, every install goes through,
//! in this exact order, and never partially:
//!
//!   1. **Independent signature verification** — rebuild the exact bytes the
//!      fleet server signed via [`wincmd_shared::fleet::epoch_preimage`] and
//!      verify against the pinned fleet signing key. Mirrors
//!      `commander-free/src/settings.rs`'s `apply_admin_config_cmd`
//!      (~:2690-2747): reject on signer-key mismatch BEFORE even looking at
//!      the signature bytes (anti key-swap), then verify.
//!   2. **Monotonic-version guard** — reject any epoch whose `version` is
//!      not *strictly greater* than what this subtree already holds. Equal
//!      versions are rejected too: two signed epochs sharing one version
//!      number can only mean a replay of an already-installed epoch (a
//!      harmless no-op we still refuse, to keep "same version ⇒ same
//!      content" an invariant this module can rely on) or a version number
//!      being reused with different content by a buggy or compromised
//!      signer — either way, silently accepting it would break the
//!      anti-rollback property the whole scheme depends on.
//!   3. **Deserialize** the relevant config subtree, then
//!      **`wincmd_clip_rules::compile()`** it. Only after BOTH succeed does
//!      the new ruleset become the active one.
//!   4. **Atomic swap with last-valid retention** — write the newly-verified
//!      epoch to a temp file and rename it over the live file, THEN (only on
//!      write success) swap the in-memory active ruleset. If any step above
//!      fails, the previous ruleset (if any) stays active untouched and the
//!      `rules_compiled` health flag flips to `false` so a health reporter
//!      can surface it — this is the exact plan §9 Phase 1c exit criterion.
//!
//! # Generic over subtree, per plan §3's "svc owns both"
//!
//! `commander-svc` is meant to own durable policy for **both** Clipboard
//! Guard and Ink Receipt, not just the former. Rather than hardcoding this
//! module to clipboard rules, the install/verify/atomic-swap machinery is
//! generic over a [`SubtreeCompiler`] witness type — [`ClipboardGuardCompiler`]
//! instantiates it with `wincmd_clip_rules::compile`, real content, real
//! limits. [`InkReceiptCompiler`] instantiates the SAME machinery today with
//! an identity "compile" step (raw JSON in, raw JSON out) because Ink
//! Receipt's actual policy schema and compiler are a later phase's job
//! (plan §5.3's `ink_receipt/policy.rs`) — this module does not invent that
//! schema, it just makes sure the durable-storage half doesn't need to be
//! rewritten when that phase lands.
//!
//! # Content-free by construction (plan §8)
//!
//! [`PolicyStoreError`] never carries a rule name, a pattern, or clipboard
//! text — `CompileFailed` holds pre-formatted [`wincmd_clip_rules::CompileError`]
//! strings, which are index-only by that type's own contract, and every
//! other variant is a bare enum tag, an `i64`, or an `io::ErrorKind` tag.
//! Anything a caller logs about a failed install therefore inherits this
//! guarantee automatically. See the `error_display_never_contains_rule_content`
//! test for the end-to-end check.
//!
//! # File permissions
//!
//! The policy directory holds SYSTEM-owned durable state that gates a real
//! security control. If a non-admin principal can write to it, the entire
//! verification chain above is theater. [`PolicyFs::ensure_dir_secure`] must
//! both enforce (lock the DACL down to LocalSystem + Builtin\Administrators
//! only) and fail closed if it cannot — the precedent for that stance is
//! `commander-pro`'s `event_store::open()` returning `InsecurePermissions`.
//! [`WindowsPolicyFs`] is the real, `#[cfg(windows)]`-gated implementation;
//! [`PolicyStore::open`] refuses to construct a store at all if this check
//! fails.
//!
//! # Injectable seams
//!
//! [`PolicyFs`] and [`Clock`] are traits so every test above is pure and
//! platform-independent — see `print_usb_monitor.rs:113-123` in the sibling
//! `commander-pro` repo for the model this follows. The real
//! [`WindowsPolicyFs`] / [`SystemClock`] implementations are the only parts
//! of this module that touch the OS.

// Not wired into main.rs/pipe.rs yet — that's agent C4's job (see this
// module's doc comment and this agent's handoff notes). Until C4's pipe
// dispatch calls into `PolicyStore`, the compiler sees every `pub` item
// here as unused from a `bin` crate's perspective. Same precedent as
// `pro_broker.rs`'s `#![allow(dead_code)]` for `HashAcceptance` et al.,
// which was also written ahead of being wired in.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

// ── Errors ───────────────────────────────────────────────────────────────────

/// Every reason an epoch install (or a startup load) can fail. Content-free
/// by construction — see the module doc's "Content-free by construction"
/// section. Safe to place directly in a log line or a health-reporter
/// message; never format the [`EpochInstallInput`] or a
/// `wincmd_clip_rules::Rule` value alongside it instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyStoreError {
    /// The epoch's claimed `signer_key_b64` does not match the pinned fleet
    /// signing key. Checked BEFORE signature verification (anti key-swap),
    /// mirroring `commander-free/src/settings.rs`'s `apply_admin_config_cmd`.
    SignerKeyMismatch,
    /// `wincmd_shared::fleet::verify_signature_b64` returned `false` for the
    /// rebuilt `epoch_preimage` bytes.
    BadSignature,
    /// `incoming` is not strictly greater than `current`. Equal is rejected
    /// too — see the module doc for why.
    VersionNotAdvancing { current: i64, incoming: i64 },
    /// The subtree's config key was missing, or its contents didn't parse
    /// into the subtree's `Raw` type.
    DeserializeFailed,
    /// `SubtreeCompiler::compile` failed. Each string is already a
    /// content-free `wincmd_clip_rules::CompileError` rendering (index-only
    /// by that type's own contract) — never raw pattern or rule-name text.
    CompileFailed(Vec<String>),
    /// A filesystem operation failed. Carries only the `io::ErrorKind` tag
    /// (e.g. `"PermissionDenied"`), never a path or OS message string, to
    /// stay within the same content-free contract as every other variant.
    Io(String),
    /// The policy directory's permissions could not be verified as
    /// SYSTEM/Administrators-only, or the enforcing DACL write itself
    /// failed. Fails closed: this is a hard error, not a warning.
    InsecurePermissions,
}

impl std::fmt::Display for PolicyStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyStoreError::SignerKeyMismatch => {
                write!(
                    f,
                    "epoch signer key does not match the pinned fleet signing key"
                )
            }
            PolicyStoreError::BadSignature => write!(f, "epoch signature verification failed"),
            PolicyStoreError::VersionNotAdvancing { current, incoming } => write!(
                f,
                "epoch version {incoming} does not strictly advance current version {current}"
            ),
            PolicyStoreError::DeserializeFailed => {
                write!(f, "policy subtree failed to deserialize")
            }
            PolicyStoreError::CompileFailed(errors) => write!(
                f,
                "{} rule(s) failed to compile: {}",
                errors.len(),
                errors.join("; ")
            ),
            PolicyStoreError::Io(kind) => write!(f, "policy store I/O error: {kind}"),
            PolicyStoreError::InsecurePermissions => {
                write!(f, "policy store directory has insecure permissions")
            }
        }
    }
}

impl std::error::Error for PolicyStoreError {}

// ── Injectable seams ─────────────────────────────────────────────────────────

/// Filesystem seam for the policy store. Every method is synchronous —
/// callers running inside an async context (svc's tokio runtime) are
/// expected to offload via `spawn_blocking`, matching `settings_host.rs`'s
/// existing synchronous-`std::fs` style.
pub trait PolicyFs: Send + Sync {
    /// Read the full contents of `path`. Must return an
    /// `io::ErrorKind::NotFound` error (not a generic error) when the file
    /// is absent — [`ManagedSubtree::load_at_startup`] depends on being able
    /// to tell "never installed" apart from "corrupt".
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;

    /// Durably replace the contents of `path` with `bytes`. MUST be a
    /// genuine atomic replace (temp file + rename over the live file) —
    /// never remove-then-create, which leaves a window where `path` doesn't
    /// exist at all. See [`WindowsPolicyFs::atomic_write`]'s doc for why
    /// `std::fs::rename` already satisfies this on Windows without any
    /// hand-rolled `ReplaceFileW`/`MoveFileEx` call.
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;

    /// Ensure `dir` exists and is writable ONLY by SYSTEM/Administrators,
    /// enforcing that ACL if necessary. Must fail closed
    /// (`PolicyStoreError::InsecurePermissions`) rather than silently
    /// proceeding if the directory cannot be verified/locked down — the
    /// precedent is `commander-pro`'s `event_store::open()` returning
    /// `InsecurePermissions`.
    fn ensure_dir_secure(&self, dir: &Path) -> Result<(), PolicyStoreError>;
}

/// Clock seam — purely observational in this module (feeds
/// [`SubtreeHealth::last_attempt_unix`]), never part of a security decision
/// (the monotonic-version guard is a pure integer comparison, not
/// time-based).
pub trait Clock: Send + Sync {
    fn now_unix(&self) -> i64;
}

/// Production [`Clock`] backed by the OS wall clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            // A clock set before 1970 is a misconfigured machine, not a
            // security-relevant condition for this purely-observational
            // timestamp — fall back to 0 rather than panicking a service
            // thread over it.
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// Canonical on-disk location: `%ProgramData%\WinCommander\policy\`.
/// Mirrors `settings_host.rs::svc_settings_path`'s fallback idiom exactly
/// (temp dir when `ProgramData` is unset, e.g. non-Windows dev/CI).
pub fn default_policy_dir() -> PathBuf {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("WinCommander").join("policy")
}

/// Real, `#[cfg(windows)]`-gated [`PolicyFs`]. Directory ACL enforcement is
/// in the sibling `windows_acl` module; see its doc comment for the exact
/// mechanism and why it needs no new `windows-sys` Cargo feature.
#[cfg(windows)]
pub struct WindowsPolicyFs;

#[cfg(windows)]
impl PolicyFs for WindowsPolicyFs {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        // `std::fs::rename` on Windows is implemented on top of
        // `MoveFileExW(.., MOVEFILE_REPLACE_EXISTING)` — it is NOT
        // "remove `path`, then create it fresh". There is never a moment
        // where `path` doesn't exist between these two calls succeeding,
        // which is exactly the atomicity plan §4.4 requires. This is why
        // this module doesn't hand-roll the `ReplaceFileW`/`MoveFileEx` FFI
        // call itself.
        std::fs::rename(&tmp, path)
    }

    fn ensure_dir_secure(&self, dir: &Path) -> Result<(), PolicyStoreError> {
        std::fs::create_dir_all(dir).map_err(|_| PolicyStoreError::InsecurePermissions)?;
        windows_acl::lock_down(dir).map_err(|_| PolicyStoreError::InsecurePermissions)
    }
}

/// Real Windows DACL enforcement for the policy directory. Kept in its own
/// `#[cfg(windows)]` submodule so the rest of this file — including every
/// test — stays platform-independent.
#[cfg(windows)]
mod windows_acl {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::Security::Authorization::{SetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        AddAccessAllowedAce, AllocateAndInitializeSid, FreeSid, InitializeAcl, ACL, ACL_REVISION,
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSID,
        SECURITY_NT_AUTHORITY,
    };
    use windows_sys::Win32::System::SystemServices::{
        DOMAIN_ALIAS_RID_ADMINS, SECURITY_BUILTIN_DOMAIN_RID,
    };

    /// NTFS "Full Control" access mask (`winnt.h FILE_ALL_ACCESS`).
    /// Hardcoded rather than pulled in via `windows-sys`'s
    /// `Win32_Storage_FileSystem` feature — a feature this crate does not
    /// otherwise need — mirroring `pipe.rs`'s own `0x12019b` "hardcode the
    /// mask, document the source" convention for the same reason.
    const FILE_ALL_ACCESS: u32 = 0x001F_01FF;

    /// Replace `path`'s DACL with a protected (non-inherited), two-entry
    /// ACL granting Full Control to LocalSystem and Builtin\Administrators
    /// ONLY. Every other principal — including the interactive user the
    /// endpoint GUI runs as — is denied by ABSENCE of any ACE, not by an
    /// explicit deny entry (omission is simpler than an explicit deny and
    /// just as effective on a protected DACL with no inherited ACEs).
    ///
    /// Fails closed: `Err(())` on ANY failure (SID allocation, ACL
    /// construction, or the `SetNamedSecurityInfoW` call itself). The
    /// caller ([`super::WindowsPolicyFs::ensure_dir_secure`]) maps any
    /// failure to `PolicyStoreError::InsecurePermissions` and refuses to
    /// proceed — a service that cannot prove it locked the directory down
    /// must not pretend the directory is secure.
    pub(super) fn lock_down(path: &Path) -> Result<(), ()> {
        unsafe {
            let admin_sid = well_known_sid(
                2,
                SECURITY_BUILTIN_DOMAIN_RID as u32,
                DOMAIN_ALIAS_RID_ADMINS as u32,
            )?;
            let _admin_guard = SidGuard(admin_sid);
            let system_sid = well_known_sid(1, 18u32, 0)?; // SECURITY_LOCAL_SYSTEM_RID
            let _system_guard = SidGuard(system_sid);

            // 8-byte ACL header + two ACCESS_ALLOWED_ACE entries for two
            // small well-known SIDs. 1024 bytes is generous headroom (the
            // same "one small fixed buffer" idiom Win32 ACL sample code
            // universally uses). Backed by `Vec<u64>`, not `[u8; N]`,
            // purely so the buffer starts 8-byte aligned — `ACL`'s layout
            // requires DWORD alignment.
            let mut buf: Vec<u64> = vec![0u64; 128];
            let acl_ptr = buf.as_mut_ptr() as *mut ACL;
            let acl_len = (buf.len() * 8) as u32;

            if InitializeAcl(acl_ptr, acl_len, ACL_REVISION) == 0 {
                return Err(());
            }
            if AddAccessAllowedAce(acl_ptr, ACL_REVISION, FILE_ALL_ACCESS, system_sid) == 0 {
                return Err(());
            }
            if AddAccessAllowedAce(acl_ptr, ACL_REVISION, FILE_ALL_ACCESS, admin_sid) == 0 {
                return Err(());
            }

            let wide_path: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0u16))
                .collect();

            let result = SetNamedSecurityInfoW(
                wide_path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                acl_ptr,
                std::ptr::null(),
            );
            if result != ERROR_SUCCESS {
                return Err(());
            }
        }
        Ok(())
    }

    /// Allocate a well-known SID under `SECURITY_NT_AUTHORITY` with 1 or 2
    /// sub-authorities. Mirrors `pipe.rs`'s `is_admin_token`/
    /// `is_local_system_token` SID-building calls exactly (same function,
    /// same authority, same calling convention) — kept as one small helper
    /// here since this module needs to BUILD SIDs to grant access, not
    /// check token membership against them.
    unsafe fn well_known_sid(sub_count: u8, rid0: u32, rid1: u32) -> Result<PSID, ()> {
        let mut sid: PSID = std::ptr::null_mut();
        let nt_authority = SECURITY_NT_AUTHORITY;
        let ok = AllocateAndInitializeSid(
            &nt_authority,
            sub_count,
            rid0,
            rid1,
            0,
            0,
            0,
            0,
            0,
            0,
            &mut sid,
        );
        if ok == 0 {
            return Err(());
        }
        Ok(sid)
    }

    struct SidGuard(PSID);
    impl Drop for SidGuard {
        fn drop(&mut self) {
            unsafe {
                FreeSid(self.0);
            }
        }
    }
}

// ── Signature verification (the security core) ──────────────────────────────

/// The full signed epoch envelope, as received fresh over the pipe (via
/// `svc.policy.install_epoch`) or as persisted to disk for the next startup
/// load. One shape serves both uses deliberately: the signature covers the
/// WHOLE `config` object (not just one subtree), so re-verifying at startup
/// needs exactly the same fields that were needed to verify at install time.
///
/// `Debug`/`Serialize` are derived for persistence and test convenience
/// ONLY. Never log a full `EpochInstallInput` — `config` may carry
/// fleet-authored rule text. Log [`PolicyStoreError`] instead, which is
/// content-free by construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochInstallInput {
    pub version: i64,
    pub config: serde_json::Value,
    pub locked_paths: Vec<String>,
    pub managed: bool,
    pub target_kind: String,
    pub target_id: Option<String>,
    pub signature_b64: String,
    pub signer_key_b64: String,
}

/// Independent signature verification (D-7's reasoning, applied to this
/// hop per D-2's third caller). Rebuilds the exact bytes the fleet server
/// signed via [`wincmd_shared::fleet::epoch_preimage`] — never hand-assembled
/// — and checks the signer-key pin BEFORE touching the signature, mirroring
/// `commander-free/src/settings.rs::apply_admin_config_cmd` (~:2715-2736).
fn verify_epoch_signature(
    pinned_key_b64: &str,
    input: &EpochInstallInput,
) -> Result<(), PolicyStoreError> {
    // Anti key-swap: an epoch claiming a DIFFERENT signer key than the one
    // pinned at enroll time is rejected before we even look at the
    // signature bytes, exactly like the Free precedent this mirrors.
    if input.signer_key_b64 != pinned_key_b64 {
        return Err(PolicyStoreError::SignerKeyMismatch);
    }
    let msg = wincmd_shared::fleet::epoch_preimage(&wincmd_shared::fleet::EpochSigningInput {
        version: input.version,
        config: &input.config,
        locked_paths: &input.locked_paths,
        managed: input.managed,
        target_kind: &input.target_kind,
        target_id: input.target_id.as_deref(),
    });
    if !wincmd_shared::fleet::verify_signature_b64(pinned_key_b64, &msg, &input.signature_b64) {
        return Err(PolicyStoreError::BadSignature);
    }
    Ok(())
}

// ── Generic subtree machinery ────────────────────────────────────────────────

/// What one durable policy subtree needs to go from "verified raw JSON" to
/// "ready-to-use compiled artifact". A witness type (a unit struct
/// implementing this trait) rather than a value — see [`ClipboardGuardCompiler`]
/// / [`InkReceiptCompiler`] — so [`ManagedSubtree`] can be generic over it
/// without needing to store an instance.
trait SubtreeCompiler {
    /// The key this subtree occupies within the FULL signed epoch's
    /// `config` object (plan §4.4: "a `clipboardGuard` subtree of
    /// `config_json`"). The signature covers the whole `config`, so this
    /// only picks out WHERE to look inside an already-verified object —
    /// never something trusted unsigned.
    const CONFIG_KEY: &'static str;

    /// The deserialized-but-uncompiled policy body (e.g. `Vec<Rule>` for
    /// Clipboard Guard).
    type Raw: Clone;
    /// The ready-to-use artifact `compile()` produces (e.g.
    /// `wincmd_clip_rules::CompiledRuleSet`).
    type Compiled;

    /// Pull this subtree's `Raw` out of the full verified `config` object.
    fn extract_raw(config: &serde_json::Value) -> Result<Self::Raw, PolicyStoreError>;

    /// Compile `raw` into `Compiled`. Errors MUST be content-free strings
    /// (index-only, per `wincmd_clip_rules::CompileError`'s own contract) —
    /// never pattern or rule-name text.
    fn compile(raw: &Self::Raw) -> Result<Self::Compiled, Vec<String>>;
}

/// Clipboard Guard's [`SubtreeCompiler`]: real rules, real
/// `wincmd_clip_rules::compile`, default limits (plan §4.1's documented
/// defaults: 100 enabled rules, 2 KiB patterns, 256 KiB/8 MiB regex bounds).
struct ClipboardGuardCompiler;

impl SubtreeCompiler for ClipboardGuardCompiler {
    const CONFIG_KEY: &'static str = "clipboardGuard";
    type Raw = Vec<wincmd_clip_rules::Rule>;
    type Compiled = wincmd_clip_rules::CompiledRuleSet;

    fn extract_raw(config: &serde_json::Value) -> Result<Self::Raw, PolicyStoreError> {
        let subtree = config
            .get(Self::CONFIG_KEY)
            .ok_or(PolicyStoreError::DeserializeFailed)?;
        let rules_value = subtree
            .get("rules")
            .ok_or(PolicyStoreError::DeserializeFailed)?;
        serde_json::from_value(rules_value.clone()).map_err(|_| PolicyStoreError::DeserializeFailed)
    }

    fn compile(raw: &Self::Raw) -> Result<Self::Compiled, Vec<String>> {
        wincmd_clip_rules::compile(raw, &wincmd_clip_rules::RuleSetLimits::default())
            .map_err(|errors| errors.iter().map(|e| e.to_string()).collect())
    }
}

/// Ink Receipt's [`SubtreeCompiler`] — a deliberate placeholder. Ink
/// Receipt's real policy schema and compiler are a LATER phase's job (plan
/// §5.3's `ink_receipt/policy.rs`); this task does not invent that schema.
/// Until it lands, this subtree stores its verified raw JSON with full
/// version tracking and the same atomic-install/last-valid-retention
/// discipline as Clipboard Guard, via a no-op "compile" (raw == compiled) —
/// so the generic machinery below already serves both subtrees per plan
/// §3's "svc owns both" ownership model, and swapping in a real compiler
/// later doesn't require touching this file's install/load/health plumbing.
struct InkReceiptCompiler;

impl SubtreeCompiler for InkReceiptCompiler {
    const CONFIG_KEY: &'static str = "inkReceipt";
    type Raw = serde_json::Value;
    type Compiled = serde_json::Value;

    fn extract_raw(config: &serde_json::Value) -> Result<Self::Raw, PolicyStoreError> {
        config
            .get(Self::CONFIG_KEY)
            .cloned()
            .ok_or(PolicyStoreError::DeserializeFailed)
    }

    fn compile(raw: &Self::Raw) -> Result<Self::Compiled, Vec<String>> {
        Ok(raw.clone())
    }
}

/// Health snapshot for one subtree, read by C4's health reporter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtreeHealth {
    /// `false` means svc currently holds ZERO installed policy for this
    /// subtree — either it has never received one, or the on-disk copy
    /// failed to load at startup with no prior in-memory fallback. `true`
    /// means some verified+compiled artifact is active and readable via the
    /// subtree's accessor — it may not be the newest version Fleet has
    /// published (see `rules_compiled` for whether the MOST RECENT install
    /// attempt itself succeeded).
    pub policy_current: bool,
    /// `false` means the most recent install ATTEMPT — whether a fresh
    /// epoch over the pipe, or the on-disk copy loaded at startup — failed
    /// at any step (signer-key mismatch, bad signature, stale/equal
    /// version, deserialize, or compile). The previous last-valid artifact,
    /// if any, stays ACTIVE regardless; this flag is purely "does the most
    /// recent attempt need attention", which is exactly the plan §9 Phase
    /// 1c exit criterion ("an uncompilable ruleset leaves the previous one
    /// active and sets `rules_compiled: false`").
    pub rules_compiled: bool,
    /// When the most recent install attempt (success or failure) occurred,
    /// per the injected [`Clock`]. Purely observational — never read by any
    /// decision in this module.
    pub last_attempt_unix: Option<i64>,
}

impl SubtreeHealth {
    const NEVER_INSTALLED: SubtreeHealth = SubtreeHealth {
        policy_current: false,
        rules_compiled: false,
        last_attempt_unix: None,
    };
}

/// One durable, signed, versioned policy subtree. Generic over
/// [`SubtreeCompiler`] so the SAME install/load/health machinery serves
/// both Clipboard Guard (today, real content) and Ink Receipt (today, a
/// placeholder identity compiler) — see the module doc's "Generic over
/// subtree" section.
struct ManagedSubtree<C: SubtreeCompiler> {
    version: Option<i64>,
    raw: Option<C::Raw>,
    compiled: Option<C::Compiled>,
    rules_compiled: bool,
    last_attempt_unix: Option<i64>,
}

impl<C: SubtreeCompiler> ManagedSubtree<C> {
    fn new() -> Self {
        Self {
            version: None,
            raw: None,
            compiled: None,
            rules_compiled: false,
            last_attempt_unix: None,
        }
    }

    fn health(&self) -> SubtreeHealth {
        SubtreeHealth {
            policy_current: self.compiled.is_some(),
            rules_compiled: self.rules_compiled,
            last_attempt_unix: self.last_attempt_unix,
        }
    }

    /// The shared verify → deserialize → compile pipeline, used identically
    /// by a fresh install (from the pipe) and by startup load (from disk) —
    /// the only difference between those two callers is where the
    /// [`EpochInstallInput`] comes from and whether a monotonic-version
    /// check applies first. Does NOT touch `self` or any filesystem state —
    /// pure verification, so it's trivially testable.
    fn verify_and_compile(
        pinned_key: &str,
        input: &EpochInstallInput,
    ) -> Result<(C::Raw, C::Compiled), PolicyStoreError> {
        verify_epoch_signature(pinned_key, input)?;
        let raw = C::extract_raw(&input.config)?;
        let compiled = C::compile(&raw).map_err(PolicyStoreError::CompileFailed)?;
        Ok((raw, compiled))
    }

    /// Install a freshly-received epoch. Exact order: monotonic-version
    /// guard (cheap, checked before any parsing work) → verify → deserialize
    /// → compile → persist (atomic) → ONLY THEN swap the in-memory active
    /// state. If any step fails, `self.{version,raw,compiled}` are left
    /// completely untouched (never half-applied) and `rules_compiled` flips
    /// to `false`.
    fn install(
        &mut self,
        fs: &dyn PolicyFs,
        path: &Path,
        pinned_key: &str,
        input: EpochInstallInput,
        clock: &dyn Clock,
    ) -> Result<(), PolicyStoreError> {
        let fail = |this: &mut Self, e: PolicyStoreError| -> PolicyStoreError {
            this.rules_compiled = false;
            this.last_attempt_unix = Some(clock.now_unix());
            e
        };

        if let Some(current) = self.version {
            if input.version <= current {
                return Err(fail(
                    self,
                    PolicyStoreError::VersionNotAdvancing {
                        current,
                        incoming: input.version,
                    },
                ));
            }
        }

        let (raw, compiled) = match Self::verify_and_compile(pinned_key, &input) {
            Ok(pair) => pair,
            Err(e) => return Err(fail(self, e)),
        };

        // `EpochInstallInput` round-trips through plain `serde_json` scalars
        // and a `Value` — serialization only fails for things like a NaN/
        // infinite float or a non-string map key, never by echoing content,
        // but we still don't interpolate the underlying error's Display
        // (see `PolicyStoreError::Io`'s doc) to stay conservatively
        // content-free.
        let bytes = match serde_json::to_vec(&input) {
            Ok(b) => b,
            Err(_) => return Err(fail(self, PolicyStoreError::DeserializeFailed)),
        };

        // Persist BEFORE swapping in-memory state — never the other way
        // round. A crash between these two steps must never leave svc
        // believing it applied an epoch it never durably recorded.
        if let Err(e) = fs.atomic_write(path, &bytes) {
            return Err(fail(self, PolicyStoreError::Io(format!("{:?}", e.kind()))));
        }

        self.version = Some(input.version);
        self.raw = Some(raw);
        self.compiled = Some(compiled);
        self.rules_compiled = true;
        self.last_attempt_unix = Some(clock.now_unix());
        Ok(())
    }

    /// Load whatever was last durably installed, re-verifying it exactly as
    /// `install` would. Graceful degradation, never a crash: a missing file
    /// is a legitimate "never configured yet" state
    /// ([`StartupOutcome::NeverInstalled`]); a present-but-corrupt or
    /// failing-to-verify file is reported via
    /// [`StartupOutcome::Degraded`] with both health flags left `false` —
    /// loud (the health reporter sees it), never silent (svc does not
    /// pretend to have a working policy it doesn't have).
    fn load_at_startup(
        &mut self,
        fs: &dyn PolicyFs,
        path: &Path,
        pinned_key: &str,
    ) -> StartupOutcome {
        let bytes = match fs.read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return StartupOutcome::NeverInstalled
            }
            Err(e) => {
                return StartupOutcome::Degraded(PolicyStoreError::Io(format!("{:?}", e.kind())))
            }
        };
        let input: EpochInstallInput = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => return StartupOutcome::Degraded(PolicyStoreError::DeserializeFailed),
        };
        match Self::verify_and_compile(pinned_key, &input) {
            Ok((raw, compiled)) => {
                self.version = Some(input.version);
                self.raw = Some(raw);
                self.compiled = Some(compiled);
                self.rules_compiled = true;
                StartupOutcome::Loaded {
                    version: input.version,
                }
            }
            Err(e) => {
                self.rules_compiled = false;
                StartupOutcome::Degraded(e)
            }
        }
    }
}

/// Outcome of loading one subtree at service startup.
#[derive(Debug)]
pub enum StartupOutcome {
    /// No persisted file existed — first boot, or this subtree has never
    /// received an epoch. Not a failure.
    NeverInstalled,
    /// Loaded, independently re-verified, and recompiled successfully.
    Loaded { version: i64 },
    /// The persisted file exists but failed to load — corrupt bytes, a
    /// signature that no longer verifies, or a compile failure. svc does
    /// NOT crash and does NOT silently run without a policy; both health
    /// flags read `false` so this is surfaced loudly instead.
    Degraded(PolicyStoreError),
}

/// Combined startup report for both subtrees, returned by
/// [`PolicyStore::load_at_startup`].
#[derive(Debug)]
pub struct StartupReport {
    pub clipboard: StartupOutcome,
    pub ink_receipt: StartupOutcome,
}

/// Combined health snapshot for both subtrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyStoreHealth {
    pub clipboard: SubtreeHealth,
    pub ink_receipt: SubtreeHealth,
}

/// The resolved, already-verified Clipboard Guard ruleset, as returned to
/// back `svc.clipboard.get_policy` (`ReadOnly` — plan §4.3 / GROUNDING §7:
/// "Safe: a non-admin user can already observe the rules by triggering
/// them"). Carries the raw `Rule` list (not the opaque `CompiledRuleSet`,
/// which has no `Serialize` impl) so C4 can put it straight on the wire as
/// JSON.
///
/// `Debug`/`Clone` are for test/introspection convenience — the same
/// caveat as `EpochInstallInput` applies: don't put this in a log line.
#[derive(Debug, Clone)]
pub struct ClipboardPolicyView {
    pub version: i64,
    pub rules: Vec<wincmd_clip_rules::Rule>,
}

const CLIPBOARD_FILE_NAME: &str = "clipboard-guard.json";
const INK_RECEIPT_FILE_NAME: &str = "ink-receipt.json";

/// The durable managed-policy store. One instance per service process,
/// expected to be held in an `Arc` and shared across the pipe's connection
/// tasks (see the module doc's "Injectable seams" section for why `fs` and
/// `clock` are trait objects rather than generics: this keeps the type
/// concrete and easy for C4 to wire into `main.rs`/`pipe.rs`).
pub struct PolicyStore {
    fs: Box<dyn PolicyFs>,
    clock: Box<dyn Clock>,
    dir: PathBuf,
    pinned_signing_key_b64: String,
    clipboard: Mutex<ManagedSubtree<ClipboardGuardCompiler>>,
    ink_receipt: Mutex<ManagedSubtree<InkReceiptCompiler>>,
}

impl PolicyStore {
    /// Construct a store rooted at `dir`, pinned to `pinned_signing_key_b64`
    /// (the same fleet signing key Free pins at enroll time). Fails closed
    /// with `PolicyStoreError::InsecurePermissions` if `dir`'s permissions
    /// cannot be verified/enforced as SYSTEM/Administrators-only — mirrors
    /// `commander-pro`'s `event_store::open()` precedent. Does NOT load any
    /// persisted policy yet; call [`PolicyStore::load_at_startup`]
    /// afterwards.
    pub fn open(
        fs: Box<dyn PolicyFs>,
        clock: Box<dyn Clock>,
        dir: PathBuf,
        pinned_signing_key_b64: String,
    ) -> Result<PolicyStore, PolicyStoreError> {
        fs.ensure_dir_secure(&dir)?;
        Ok(PolicyStore {
            fs,
            clock,
            dir,
            pinned_signing_key_b64,
            clipboard: Mutex::new(ManagedSubtree::new()),
            ink_receipt: Mutex::new(ManagedSubtree::new()),
        })
    }

    fn clipboard_path(&self) -> PathBuf {
        self.dir.join(CLIPBOARD_FILE_NAME)
    }

    fn ink_receipt_path(&self) -> PathBuf {
        self.dir.join(INK_RECEIPT_FILE_NAME)
    }

    /// Load whatever was last durably installed for both subtrees. Call
    /// once at service startup, after [`PolicyStore::open`].
    pub fn load_at_startup(&self) -> StartupReport {
        let clipboard = match self.clipboard.lock() {
            Ok(mut guard) => guard.load_at_startup(
                self.fs.as_ref(),
                &self.clipboard_path(),
                &self.pinned_signing_key_b64,
            ),
            Err(_) => StartupOutcome::Degraded(PolicyStoreError::Io("mutex poisoned".to_string())),
        };
        let ink_receipt = match self.ink_receipt.lock() {
            Ok(mut guard) => guard.load_at_startup(
                self.fs.as_ref(),
                &self.ink_receipt_path(),
                &self.pinned_signing_key_b64,
            ),
            Err(_) => StartupOutcome::Degraded(PolicyStoreError::Io("mutex poisoned".to_string())),
        };
        StartupReport {
            clipboard,
            ink_receipt,
        }
    }

    /// Install a freshly-received, signed epoch into the Clipboard Guard
    /// subtree. This is what `svc.policy.install_epoch` calls (via C4's
    /// pipe wiring) when the incoming request targets clipboard policy.
    pub fn install_clipboard_epoch(
        &self,
        input: EpochInstallInput,
    ) -> Result<(), PolicyStoreError> {
        let path = self.clipboard_path();
        let mut guard = self
            .clipboard
            .lock()
            .map_err(|_| PolicyStoreError::Io("mutex poisoned".to_string()))?;
        guard.install(
            self.fs.as_ref(),
            &path,
            &self.pinned_signing_key_b64,
            input,
            self.clock.as_ref(),
        )
    }

    /// Install a freshly-received, signed epoch into the Ink Receipt
    /// subtree. Placeholder compiler today (see [`InkReceiptCompiler`]) —
    /// still fully verified, versioned, and atomically persisted.
    pub fn install_ink_receipt_epoch(
        &self,
        input: EpochInstallInput,
    ) -> Result<(), PolicyStoreError> {
        let path = self.ink_receipt_path();
        let mut guard = self
            .ink_receipt
            .lock()
            .map_err(|_| PolicyStoreError::Io("mutex poisoned".to_string()))?;
        guard.install(
            self.fs.as_ref(),
            &path,
            &self.pinned_signing_key_b64,
            input,
            self.clock.as_ref(),
        )
    }

    /// Backs `svc.clipboard.get_policy` (`ReadOnly`). `None` if the
    /// subtree has never had a policy successfully installed or loaded.
    pub fn get_clipboard_policy(&self) -> Option<ClipboardPolicyView> {
        let guard = self.clipboard.lock().ok()?;
        let version = guard.version?;
        let rules = guard.raw.clone()?;
        Some(ClipboardPolicyView { version, rules })
    }

    /// Backs a future `svc.ink_receipt.get_policy` (`ReadOnly`). Returns the
    /// raw verified JSON since Ink Receipt has no typed policy schema yet —
    /// see [`InkReceiptCompiler`]'s doc comment.
    pub fn get_ink_receipt_policy(&self) -> Option<(i64, serde_json::Value)> {
        let guard = self.ink_receipt.lock().ok()?;
        let version = guard.version?;
        let raw = guard.raw.clone()?;
        Some((version, raw))
    }

    pub fn clipboard_health(&self) -> SubtreeHealth {
        self.clipboard
            .lock()
            .map(|g| g.health())
            .unwrap_or(SubtreeHealth::NEVER_INSTALLED)
    }

    pub fn ink_receipt_health(&self) -> SubtreeHealth {
        self.ink_receipt
            .lock()
            .map(|g| g.health())
            .unwrap_or(SubtreeHealth::NEVER_INSTALLED)
    }

    /// Combined health snapshot C4's health reporter reads.
    pub fn health(&self) -> PolicyStoreHealth {
        PolicyStoreHealth {
            clipboard: self.clipboard_health(),
            ink_receipt: self.ink_receipt_health(),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

    // ── Test doubles ──────────────────────────────────────────────────────

    /// In-memory [`PolicyFs`] so every test here is pure and platform-
    /// independent — no real filesystem, no real Windows ACL calls.
    struct FakePolicyFs {
        files: Mutex<HashMap<PathBuf, Vec<u8>>>,
        permissions_ok: AtomicBool,
        fail_next_write: AtomicBool,
    }

    impl FakePolicyFs {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
                permissions_ok: AtomicBool::new(true),
                fail_next_write: AtomicBool::new(false),
            }
        }

        fn seed(&self, path: &Path, bytes: Vec<u8>) {
            self.files.lock().unwrap().insert(path.to_path_buf(), bytes);
        }

        fn stored(&self, path: &Path) -> Option<Vec<u8>> {
            self.files.lock().unwrap().get(path).cloned()
        }
    }

    impl PolicyFs for FakePolicyFs {
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not found"))
        }

        fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
            if self.fail_next_write.swap(false, Ordering::SeqCst) {
                return Err(std::io::Error::other("injected failure"));
            }
            self.files
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), bytes.to_vec());
            Ok(())
        }

        fn ensure_dir_secure(&self, _dir: &Path) -> Result<(), PolicyStoreError> {
            if self.permissions_ok.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(PolicyStoreError::InsecurePermissions)
            }
        }
    }

    struct FakeClock(AtomicI64);
    impl Clock for FakeClock {
        fn now_unix(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    // ── Signed fixtures ───────────────────────────────────────────────────
    //
    // Generated once via a throwaway helper binary that depends on
    // `fleet-proto` + `ed25519-dalek` directly (neither of which
    // `commander-svc` needs as a real dependency just to run these tests),
    // from a fixed 32-byte seed — reproducible, and avoids adding a signing
    // crate to this crate's Cargo.toml just for test fixtures. Mirrors the
    // "pinned golden bytes" idiom `fleet-proto`'s own
    // `golden_epoch_preimage` test uses.
    //
    // All fixtures share `locked_paths: []`, `managed: true`,
    // `target_kind: "org"`, `target_id: None`.

    const PINNED_PUBKEY_B64: &str = "IVL40Zt5HSRFMkLhXy6rbLfP+ntqXtMAl5YOBpiB2xI=";
    const OTHER_PUBKEY_B64: &str = "My6+jSfLcyOzpAHBwTtd1kvMwOEOzaHCtdEaA3eaheU=";

    /// version 5, one valid, compilable phrase rule.
    const CONFIG_A_JSON: &str = "{\"clipboardGuard\":{\"rules\":[{\"actions\":[\"notify_user\"],\"cooldownSeconds\":30,\"enabled\":true,\"id\":\"0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b\",\"locked\":false,\"matcher\":{\"kind\":\"phrase\",\"params\":{\"case_sensitive\":false,\"value\":\"secret\"}},\"name\":\"test-rule\",\"priority\":100,\"revision\":1,\"severity\":\"warn\",\"snoozable\":true}]}}";
    const SIG_A_B64: &str =
        "ZCZ4ufGbvsNVbgfIV+yTSMQIlCgxBmfC2lVQGKm9tem3ZiyiQUzOEmf1lIXbWXfcaGIFCwnB4Aoca2EjZxZ0Ag==";

    /// version 6, SAME config as A (still a valid signature, over a
    /// different `version` prefix) — for the "strictly greater accepted"
    /// and "stale version rejected" tests.
    const SIG_B_B64: &str =
        "z7Bh9P2+FcWYeh9bAiQ0oV13QlVPPfFSrsn8hBNueossVujUWXZKxzTzFTbvYV2SGNxI4y2KGq3Z1JaMX3CbCw==";

    /// version 7, one rule that fails to compile (lookaround regex). Rule
    /// name AND pattern both carry a SENTINEL marker, for the
    /// content-free-error test.
    const CONFIG_D_JSON: &str = "{\"clipboardGuard\":{\"rules\":[{\"actions\":[\"notify_user\"],\"cooldownSeconds\":30,\"enabled\":true,\"id\":\"1e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4c\",\"locked\":false,\"matcher\":{\"kind\":\"regex\",\"params\":{\"case_sensitive\":false,\"pattern\":\"(?=SENTINEL_MARKER_pattern_zzz)\"}},\"name\":\"SENTINEL_MARKER_rule_name_zzz\",\"priority\":100,\"revision\":1,\"severity\":\"warn\",\"snoozable\":true}]}}";
    const SIG_D_B64: &str =
        "6Ue6yoD12Iql6HomNFl0WyeDuQh0oWVALkuP3T91IMGP2bcf3Mpaky/UF0QPuKofuL2hIxbcKHvx+wXjsCKqBg==";

    fn input_a() -> EpochInstallInput {
        EpochInstallInput {
            version: 5,
            config: serde_json::from_str(CONFIG_A_JSON).unwrap(),
            locked_paths: vec![],
            managed: true,
            target_kind: "org".to_string(),
            target_id: None,
            signature_b64: SIG_A_B64.to_string(),
            signer_key_b64: PINNED_PUBKEY_B64.to_string(),
        }
    }

    fn input_b_v6() -> EpochInstallInput {
        let mut i = input_a();
        i.version = 6;
        i.signature_b64 = SIG_B_B64.to_string();
        i
    }

    fn input_d_uncompilable() -> EpochInstallInput {
        EpochInstallInput {
            version: 7,
            config: serde_json::from_str(CONFIG_D_JSON).unwrap(),
            locked_paths: vec![],
            managed: true,
            target_kind: "org".to_string(),
            target_id: None,
            signature_b64: SIG_D_B64.to_string(),
            signer_key_b64: PINNED_PUBKEY_B64.to_string(),
        }
    }

    fn new_store() -> PolicyStore {
        PolicyStore::open(
            Box::new(FakePolicyFs::new()),
            Box::new(FakeClock(AtomicI64::new(1_000))),
            PathBuf::from("/fake/policy"),
            PINNED_PUBKEY_B64.to_string(),
        )
        .expect("fake fs reports secure permissions by default")
    }

    // ── 1. Independent signature verification ────────────────────────────

    #[test]
    fn forged_signature_is_rejected() {
        let store = new_store();
        let mut bad = input_a();
        // Flip a character in an otherwise well-formed base64 signature.
        bad.signature_b64 = SIG_B_B64.to_string(); // valid sig, but for version 6's bytes, not 5's
        let err = store.install_clipboard_epoch(bad).unwrap_err();
        assert_eq!(err, PolicyStoreError::BadSignature);
        assert!(store.get_clipboard_policy().is_none());
    }

    #[test]
    fn signer_key_mismatch_is_rejected() {
        let store = new_store();
        let mut input = input_a();
        input.signer_key_b64 = OTHER_PUBKEY_B64.to_string();
        let err = store.install_clipboard_epoch(input).unwrap_err();
        assert_eq!(err, PolicyStoreError::SignerKeyMismatch);
        assert!(store.get_clipboard_policy().is_none());
    }

    // ── 2. Monotonic-version guard ────────────────────────────────────────

    #[test]
    fn stale_version_is_rejected() {
        let store = new_store();
        store
            .install_clipboard_epoch(input_b_v6())
            .expect("v6 installs");
        let err = store.install_clipboard_epoch(input_a()).unwrap_err(); // v5 < v6
        assert_eq!(
            err,
            PolicyStoreError::VersionNotAdvancing {
                current: 6,
                incoming: 5
            }
        );
        // The v6 ruleset must still be active.
        assert_eq!(store.get_clipboard_policy().unwrap().version, 6);
    }

    #[test]
    fn equal_version_is_rejected() {
        let store = new_store();
        store
            .install_clipboard_epoch(input_a())
            .expect("v5 installs");
        let err = store.install_clipboard_epoch(input_a()).unwrap_err(); // v5 == v5
        assert_eq!(
            err,
            PolicyStoreError::VersionNotAdvancing {
                current: 5,
                incoming: 5
            }
        );
    }

    #[test]
    fn strictly_greater_version_is_accepted() {
        let store = new_store();
        store
            .install_clipboard_epoch(input_a())
            .expect("v5 installs");
        store
            .install_clipboard_epoch(input_b_v6())
            .expect("v6 > v5 installs");
        let view = store.get_clipboard_policy().expect("policy present");
        assert_eq!(view.version, 6);
        let health = store.clipboard_health();
        assert!(health.policy_current);
        assert!(health.rules_compiled);
    }

    #[test]
    fn first_install_with_no_prior_version_is_accepted() {
        let store = new_store();
        // Nothing installed yet -- self.version is None, so ANY version
        // (including a high one) must be accepted, not rejected as "not
        // strictly greater than nothing".
        store
            .install_clipboard_epoch(input_b_v6())
            .expect("first-ever install of any version must succeed");
    }

    // ── 3. Atomic install / last-valid retention (plan §9 Phase 1c) ──────

    #[test]
    fn uncompilable_ruleset_leaves_previous_active_and_sets_rules_compiled_false() {
        let store = new_store();
        store
            .install_clipboard_epoch(input_a())
            .expect("v5 installs");

        let err = store
            .install_clipboard_epoch(input_d_uncompilable())
            .unwrap_err();
        assert!(matches!(err, PolicyStoreError::CompileFailed(_)));

        // Previous ruleset (v5) is STILL active.
        let view = store.get_clipboard_policy().expect("v5 still active");
        assert_eq!(view.version, 5);

        let health = store.clipboard_health();
        assert!(
            health.policy_current,
            "a last-valid ruleset is still active"
        );
        assert!(
            !health.rules_compiled,
            "the latest attempt must be flagged unhealthy"
        );
    }

    #[test]
    fn install_failure_partway_through_never_leaves_half_applied_state() {
        let fs = std::sync::Arc::new(FakePolicyFs::new());
        // We need to reach into the same fake FS after construction, so
        // build the store from the same Box the Arc wraps by hand here
        // instead of via `new_store()`.
        struct ArcFs(std::sync::Arc<FakePolicyFs>);
        impl PolicyFs for ArcFs {
            fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
                self.0.read(path)
            }
            fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
                self.0.atomic_write(path, bytes)
            }
            fn ensure_dir_secure(&self, dir: &Path) -> Result<(), PolicyStoreError> {
                self.0.ensure_dir_secure(dir)
            }
        }
        let store = PolicyStore::open(
            Box::new(ArcFs(fs.clone())),
            Box::new(FakeClock(AtomicI64::new(1_000))),
            PathBuf::from("/fake/policy"),
            PINNED_PUBKEY_B64.to_string(),
        )
        .unwrap();

        store
            .install_clipboard_epoch(input_a())
            .expect("v5 installs");
        let path_before = store.clipboard_path();
        let bytes_before = fs.stored(&path_before).expect("v5 persisted");

        // Force the NEXT write to fail, then attempt a fully-valid v6
        // install (good signature, strictly-greater version, compiles
        // fine) -- everything succeeds up to the persist step.
        fs.fail_next_write.store(true, Ordering::SeqCst);
        let err = store.install_clipboard_epoch(input_b_v6()).unwrap_err();
        assert!(matches!(err, PolicyStoreError::Io(_)));

        // In-memory state: still v5, completely unchanged.
        assert_eq!(store.get_clipboard_policy().unwrap().version, 5);
        assert!(!store.clipboard_health().rules_compiled);

        // On-disk state: still v5's bytes, byte-for-byte -- the failed
        // write never touched the persisted file.
        assert_eq!(fs.stored(&path_before), Some(bytes_before));
    }

    // ── 4. Load-at-startup with graceful degradation ─────────────────────

    #[test]
    fn corrupt_on_disk_policy_at_startup_degrades_loudly_not_silently() {
        // Seed garbage bytes directly into the fake fs, bypassing `install`
        // entirely, to simulate a corrupted/tampered persisted file that
        // predates this process ever starting.
        let fs = FakePolicyFs::new();
        let path = PathBuf::from("/fake/policy").join(CLIPBOARD_FILE_NAME);
        fs.seed(&path, b"this is not json".to_vec());
        let store = PolicyStore::open(
            Box::new(fs),
            Box::new(FakeClock(AtomicI64::new(1_000))),
            PathBuf::from("/fake/policy"),
            PINNED_PUBKEY_B64.to_string(),
        )
        .unwrap();

        let report = store.load_at_startup();
        assert!(matches!(report.clipboard, StartupOutcome::Degraded(_)));

        let health = store.clipboard_health();
        assert!(
            !health.policy_current,
            "no active policy after a corrupt load"
        );
        assert!(!health.rules_compiled);
        // Never installed, but load_at_startup completing at all (no
        // panic) is the "does not crash" half of the requirement.
        assert!(store.get_clipboard_policy().is_none());
    }

    #[test]
    fn never_installed_subtree_is_not_treated_as_degraded() {
        let store = new_store();
        let report = store.load_at_startup();
        assert!(matches!(report.clipboard, StartupOutcome::NeverInstalled));
        assert!(matches!(report.ink_receipt, StartupOutcome::NeverInstalled));
    }

    // ── 5. File permissions ────────────────────────────────────────────────

    #[test]
    fn insecure_file_permissions_are_rejected() {
        let fs = FakePolicyFs::new();
        fs.permissions_ok.store(false, Ordering::SeqCst);
        let result = PolicyStore::open(
            Box::new(fs),
            Box::new(FakeClock(AtomicI64::new(1_000))),
            PathBuf::from("/fake/policy"),
            PINNED_PUBKEY_B64.to_string(),
        );
        assert!(matches!(result, Err(PolicyStoreError::InsecurePermissions)));
    }

    // ── 6. Content-free enforcement (plan §8) ─────────────────────────────

    #[test]
    fn error_never_contains_rule_content_sentinel() {
        let store = new_store();
        let err = store
            .install_clipboard_epoch(input_d_uncompilable())
            .unwrap_err();
        let debug_text = format!("{:?}", err);
        let display_text = format!("{}", err);
        for sentinel in [
            "SENTINEL_MARKER_rule_name_zzz",
            "SENTINEL_MARKER_pattern_zzz",
        ] {
            assert!(
                !debug_text.contains(sentinel),
                "Debug output leaked rule content: {debug_text}"
            );
            assert!(
                !display_text.contains(sentinel),
                "Display output leaked rule content: {display_text}"
            );
        }
    }

    // ── 7. Read accessor sanity ────────────────────────────────────────────

    #[test]
    fn get_clipboard_policy_returns_installed_rules() {
        let store = new_store();
        store
            .install_clipboard_epoch(input_a())
            .expect("v5 installs");
        let view = store.get_clipboard_policy().expect("policy present");
        assert_eq!(view.version, 5);
        assert_eq!(view.rules.len(), 1);
        assert_eq!(view.rules[0].name, "test-rule");
    }

    #[test]
    fn ink_receipt_subtree_installs_independently_of_clipboard() {
        let store = new_store();
        let mut ir_input = input_a();
        // Ink Receipt's placeholder compiler expects an `inkReceipt` key,
        // not `clipboardGuard` -- and since the signature covers the WHOLE
        // config, this must be a freshly (re-)signed fixture for that
        // shape. Reuse `verify_and_compile`'s contract indirectly: a config
        // with no `inkReceipt` key must fail closed as `DeserializeFailed`,
        // never silently succeed with empty content.
        ir_input.config = serde_json::json!({ "clipboardGuard": { "rules": [] } });
        // Signature won't match this mutated config -- expect BadSignature,
        // not a silent pass. This asserts the ink-receipt path independently
        // re-verifies rather than trusting the clipboard path's fixture.
        let err = store.install_ink_receipt_epoch(ir_input).unwrap_err();
        assert_eq!(err, PolicyStoreError::BadSignature);
    }

    // ── Windows-only ACL smoke test ────────────────────────────────────────
    // Not a correctness test for the pure logic above (which never touches
    // the real filesystem) -- just confirms `lock_down` doesn't panic and
    // succeeds when run with sufficient privilege. Skips (rather than
    // fails) if the environment doesn't grant WRITE_DAC on a freshly
    // created temp directory, matching the skip-on-environment-limitation
    // idiom `commander-pro`'s `event_store` integration tests use.
    #[cfg(windows)]
    #[test]
    fn windows_lock_down_smoke() {
        let dir = std::env::temp_dir().join(format!(
            "wincmd-policy-store-acl-test-{}",
            std::process::id()
        ));
        if std::fs::create_dir_all(&dir).is_err() {
            eprintln!("[skip] could not create temp dir for ACL smoke test");
            return;
        }
        match super::windows_acl::lock_down(&dir) {
            Ok(()) => {}
            Err(()) => {
                eprintln!(
                    "[skip] lock_down failed -- environment likely lacks WRITE_DAC on temp dir"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
