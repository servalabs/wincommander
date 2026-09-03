//! Destructive-Action Capability — a single-use, args-bound token that a
//! compromised WebView cannot forge. Catastrophic commands require one before
//! running any effect. Minting requires a Rust-initiated native OS dialog, a
//! legitimate Rust-side duress trigger (dead-man / distress-phrase / hotkey),
//! or a renderer-entered PIN that Rust verifies and rate-limits. That PIN is a
//! compatibility and user-intent gate, not an unforgeable factor against a
//! compromised WebView.
//!
//! A frontend confirm() dialog is NOT a control against the assumed-compromised
//! WebView (SECURITY.md threat model) — only these Rust-minted capabilities are.
//! See docs/superpowers/specs/2026-07-07-security-hardening-design.md §4.1.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const TTL: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DestructiveAction {
    SelfDestruct,
    RemoveUsers,
    CryptoErase,
    FleetReenroll,
    DiskDelete,
    DecoyDelete,
    KillSwitch,
}

struct Minted {
    action: DestructiveAction,
    args_hash: [u8; 32],
    at: Instant,
}

static STORE: Lazy<Mutex<HashMap<String, Minted>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static TRUSTED_LOCKDOWN_GATE: TrustedLockdownGate = TrustedLockdownGate::new();
static TRUSTED_LOCKDOWN_EXECUTING: AtomicBool = AtomicBool::new(false);

struct TrustedLockdownGate {
    generation: AtomicU64,
    pending: AtomicBool,
}

impl TrustedLockdownGate {
    const fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            pending: AtomicBool::new(false),
        }
    }

    fn arm(&self) -> Option<u64> {
        if self.pending.swap(true, Ordering::SeqCst) {
            return None;
        }
        Some(self.generation.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn cancel(&self) -> bool {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.pending.swap(false, Ordering::SeqCst)
    }

    fn take_if_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
            && self
                .pending
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
    }
}

fn hash_args(args_canonical: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(args_canonical.as_bytes());
    h.finalize().into()
}

/// Mint a capability for (action, args). Returns the opaque token id.
pub fn mint(action: DestructiveAction, args_canonical: &str) -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let token = raw.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    let mut store = STORE.lock().expect("authz store poisoned");
    // Opportunistic GC of expired tokens.
    store.retain(|_, m| m.at.elapsed() < TTL);
    store.insert(
        token.clone(),
        Minted {
            action,
            args_hash: hash_args(args_canonical),
            at: Instant::now(),
        },
    );
    token
}

/// Consume a capability. Fails closed on any mismatch, expiry, or reuse.
pub fn consume(token: &str, action: DestructiveAction, args_canonical: &str) -> Result<(), String> {
    let mut store = STORE
        .lock()
        .map_err(|_| "confirmation unavailable (authorization store failure)".to_string())?;
    let Some(m) = store.remove(token) else {
        return Err("confirmation required (no valid capability)".into());
    };
    if m.at.elapsed() >= TTL {
        return Err("confirmation expired — please re-confirm".into());
    }
    if m.action != action {
        return Err("confirmation does not match this action".into());
    }
    if m.args_hash != hash_args(args_canonical) {
        return Err("confirmation does not match this request".into());
    }
    Ok(())
}

/// Require and consume a caller-supplied capability at the mutation boundary.
/// `Option` is deliberate: a forged Tauri invoke that omits the argument must
/// reach an explicit fail-closed decision rather than relying on deserialization.
pub fn consume_required(
    token: Option<&str>,
    action: DestructiveAction,
    args_canonical: &str,
) -> Result<(), String> {
    let token = token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "confirmation required (capability missing)".to_string())?;
    consume(token, action, args_canonical)
}

fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort_unstable();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("JSON object key serializes"),
                        canonical_json(&map[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        serde_json::Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => serde_json::to_string(value).expect("JSON scalar serializes"),
    }
}

#[derive(Clone, Serialize)]
pub struct LockdownPlanSnapshot {
    pub self_destruct: crate::settings::SelfDestructSettings,
    pub shred_mft_slack: bool,
}

impl LockdownPlanSnapshot {
    pub fn from_settings(settings: &crate::settings::AppSettings) -> Self {
        Self {
            self_destruct: settings.ideal.privacy.self_destruct.clone(),
            shred_mft_slack: settings
                .ideal
                .tweaks
                .security
                .shred_mft_slack_enabled
                .unwrap_or(false),
        }
    }
}

fn self_destruct_config_args(plan: &LockdownPlanSnapshot) -> String {
    canonical_json(
        &serde_json::to_value(plan).expect("Lockdown plan serializes"),
    )
}

pub fn lockdown_args(
    deactivate_license_first: bool,
    shutdown_system: bool,
    plan: &LockdownPlanSnapshot,
) -> String {
    format!(
        "lockdown|deactivate={deactivate_license_first}|shutdown={shutdown_system}|{}",
        self_destruct_config_args(plan)
    )
}

pub fn full_lockdown_args(plan: &LockdownPlanSnapshot) -> String {
    format!("full_lockdown|{}", self_destruct_config_args(plan))
}

pub fn canonical_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| std::path::PathBuf::from(path))
        .to_string_lossy()
        .into_owned()
}

fn path_bound_args(command: &str, path: &str) -> String {
    let canonical = canonical_path(path);
    let identity = crate::routine_cleaner::file_identity(std::path::Path::new(&canonical));
    serde_json::to_string(&(command, canonical, identity))
        .expect("path-bound arguments serialize")
}

pub fn disk_delete_args(path: &str) -> String {
    path_bound_args("disk_delete_item", path)
}

pub fn decoy_delete_args(path: &str) -> String {
    path_bound_args("delete_decoy", path)
}

pub fn kill_switch_args(enable: bool) -> String {
    serde_json::to_string(&("internet_kill_switch_set", enable))
        .expect("kill-switch arguments serialize")
}

pub fn secure_erase_args(path: &str) -> String {
    path_bound_args("Invoke-7Erase", path)
}

pub fn free_space_erase_args(drive_letter: &str, media_type: &str) -> String {
    serde_json::to_string(&(
        "Invoke-UnallocatedSpaceErase",
        drive_letter,
        media_type,
    ))
    .expect("free-space erase arguments serialize")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendDispatchPolicy {
    Ordinary,
    Capability(DestructiveAction),
    NativeConfirmation,
    InternalOnly,
}

/// Security policy for the renderer-facing generic backend dispatcher.
///
/// Ordinary cleanup commands intentionally remain available. Commands in the
/// lockdown cascade are Rust-owned and must never be callable by feature id
/// from the WebView; the two user-facing erase operations require a native,
/// argument-bound capability at this final dispatch boundary.
pub fn backend_dispatch_policy(command: &str) -> BackendDispatchPolicy {
    match command {
        "Invoke-7Erase" => BackendDispatchPolicy::Capability(DestructiveAction::DiskDelete),
        "Invoke-UnallocatedSpaceErase" => {
            BackendDispatchPolicy::Capability(DestructiveAction::CryptoErase)
        }
        "run_destruct_step"
        | "Remove-LocalUsers"
        | "Destroy-VeraCryptHeader"
        | "Clear-BitLockerKeyProtectors"
        | "Invoke-7Wipe"
        | "Clear-MFTResidentSlack" => BackendDispatchPolicy::InternalOnly,
        _ => match crate::cli::backend_script_risk(command) {
            Some(crate::cli::Risk::ReadOnly | crate::cli::Risk::Mutating) => {
                BackendDispatchPolicy::Ordinary
            }
            Some(crate::cli::Risk::Destructive) => BackendDispatchPolicy::NativeConfirmation,
            None => BackendDispatchPolicy::InternalOnly,
        },
    }
}

pub fn authorize_backend_dispatch(
    command: &str,
    params: &mut HashMap<String, String>,
) -> Result<(), String> {
    match backend_dispatch_policy(command) {
        BackendDispatchPolicy::Ordinary => Ok(()),
        BackendDispatchPolicy::InternalOnly => Err(format!(
            "{command} is available only through the Rust-owned lockdown path"
        )),
        BackendDispatchPolicy::NativeConfirmation => Err(format!(
            "{command} requires interactive native confirmation"
        )),
        BackendDispatchPolicy::Capability(action) => {
            let token = params.remove("CapabilityToken");
            let canonical_args = match command {
                "Invoke-7Erase" => {
                    if params.keys().any(|key| key != "Path" && key != "Type") {
                        return Err("Invoke-7Erase received unsupported parameters".to_string());
                    }
                    match params.get("Type").map(String::as_str).unwrap_or("File") {
                        "File" => {
                            params.insert("Type".to_string(), "File".to_string());
                        }
                        _ => {
                            return Err(
                                "Invoke-7Erase is restricted to file and folder targets"
                                    .to_string(),
                            );
                        }
                    }
                    let path = params
                        .get("Path")
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(|| "Invoke-7Erase requires Path".to_string())?;
                    let canonical = canonical_path(path);
                    let args = secure_erase_args(&canonical);
                    params.insert("Path".to_string(), canonical);
                    args
                }
                "Invoke-UnallocatedSpaceErase" => {
                    if params
                        .keys()
                        .any(|key| key != "DriveLetter" && key != "MediaType")
                    {
                        return Err(
                            "Invoke-UnallocatedSpaceErase received unsupported parameters"
                                .to_string(),
                        );
                    }
                    free_space_erase_args(
                        params.get("DriveLetter").map(String::as_str).unwrap_or("C"),
                        params.get("MediaType").map(String::as_str).unwrap_or("Unknown"),
                    )
                }
                _ => return Err("unsupported destructive backend command".to_string()),
            };
            consume_required(token.as_deref(), action, &canonical_args)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum DestructiveRequest {
    Lockdown {
        #[serde(rename = "deactivateLicenseFirst")]
        deactivate_license_first: bool,
        #[serde(rename = "shutdownSystem")]
        shutdown_system: bool,
    },
    FullLockdown,
    DiskDeleteItem { path: String },
    DeleteDecoy { path: String },
    InternetKillSwitchSet { enable: bool },
    SecureErase { path: String },
    FreeSpaceErase {
        #[serde(rename = "driveLetter")]
        drive_letter: String,
        #[serde(rename = "mediaType")]
        media_type: String,
    },
    SelectiveCryptoErase {
        target: crate::selective_erase::EraseTargetInput,
    },
}

impl DestructiveRequest {
    fn action(&self) -> DestructiveAction {
        match self {
            Self::Lockdown { .. } | Self::FullLockdown => DestructiveAction::SelfDestruct,
            Self::DiskDeleteItem { .. } => DestructiveAction::DiskDelete,
            Self::DeleteDecoy { .. } => DestructiveAction::DecoyDelete,
            Self::InternetKillSwitchSet { .. } => DestructiveAction::KillSwitch,
            Self::SecureErase { .. } => DestructiveAction::DiskDelete,
            Self::FreeSpaceErase { .. } => DestructiveAction::CryptoErase,
            Self::SelectiveCryptoErase { .. } => DestructiveAction::CryptoErase,
        }
    }

    fn canonical_args(&self, plan: Option<&LockdownPlanSnapshot>) -> String {
        match self {
            Self::Lockdown {
                deactivate_license_first,
                shutdown_system,
            } => lockdown_args(
                *deactivate_license_first,
                *shutdown_system,
                plan.expect("Lockdown plan was loaded"),
            ),
            Self::FullLockdown => {
                full_lockdown_args(plan.expect("full Lockdown plan was loaded"))
            }
            Self::DiskDeleteItem { path } => disk_delete_args(path),
            Self::DeleteDecoy { path } => decoy_delete_args(path),
            Self::InternetKillSwitchSet { enable } => kill_switch_args(*enable),
            Self::SecureErase { path } => secure_erase_args(path),
            Self::FreeSpaceErase {
                drive_letter,
                media_type,
            } => free_space_erase_args(drive_letter, media_type),
            Self::SelectiveCryptoErase { target } => {
                crate::selective_erase::canonical_erase_args(target)
            }
        }
    }

    fn confirmation_detail(
        &self,
        plan: Option<&LockdownPlanSnapshot>,
    ) -> String {
        match self {
            Self::Lockdown {
                deactivate_license_first,
                shutdown_system,
            } => lockdown_confirmation(
                plan.expect("Lockdown plan was loaded"),
                *deactivate_license_first,
                *shutdown_system,
            ),
            Self::FullLockdown => {
                full_lockdown_confirmation(plan.expect("full Lockdown plan was loaded"))
            }
            Self::DiskDeleteItem { path } => format!(
                "Permanently delete this disk item?\n\nTarget: {}",
                safe_detail(&canonical_path(path))
            ),
            Self::DeleteDecoy { path } => format!(
                "Permanently delete this decoy file?\n\nTarget: {}",
                safe_detail(&canonical_path(path))
            ),
            Self::InternetKillSwitchSet { enable } => format!(
                "Turn the Internet Kill Switch {} now? This changes machine-wide network access.",
                if *enable { "ON" } else { "OFF" }
            ),
            Self::SecureErase { path } => format!(
                "Securely overwrite and permanently delete this item?\n\nTarget: {}",
                safe_detail(&canonical_path(path))
            ),
            Self::FreeSpaceErase { drive_letter, .. } => format!(
                "Overwrite free space on drive {}? Deleted-data recovery will be intentionally prevented.",
                safe_detail(drive_letter)
            ),
            Self::SelectiveCryptoErase { target } => format!(
                "Permanently crypto-erase this encrypted target?\n\nKind: {}\nPath: {}\nMount: {}\nOS/system target: {}",
                safe_detail(&target.kind),
                target
                    .path
                    .as_deref()
                    .map(canonical_path)
                    .map(|path| safe_detail(&path))
                    .unwrap_or_else(|| "none".to_string()),
                target
                    .mount_point
                    .as_deref()
                    .or(target.mount_letter.as_deref())
                    .map(safe_detail)
                    .unwrap_or_else(|| "none".to_string()),
                yes_no(crate::selective_erase::is_os_target(
                    &target.kind,
                    target.mount_point.as_deref(),
                    target.path.as_deref(),
                    &crate::selective_erase::system_drive(),
                )),
            ),
        }
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn listed(values: Option<&Vec<String>>) -> String {
    match values {
        Some(values) if !values.is_empty() => values
            .iter()
            .map(|value| safe_detail(value))
            .collect::<Vec<_>>()
            .join("; "),
        _ => "none".to_string(),
    }
}

fn lockdown_confirmation(
    plan: &LockdownPlanSnapshot,
    deactivate_license_first: bool,
    shutdown_system: bool,
) -> String {
    let config = &plan.self_destruct;
    format!(
        "Run Lockdown with this fixed plan?\n\nDeactivate licence: {}\nShutdown Windows: {}\nFolder targets: {}\nLocal users: {}\nVeraCrypt containers: {}\nBitLocker drives: {}\nMFT resident-slack wipe: {}\nStep overrides: {}",
        yes_no(deactivate_license_first),
        yes_no(shutdown_system),
        listed(config.shred_folders.as_ref()),
        listed(config.users_to_remove.as_ref()),
        listed(config.crypto_erase_veracrypt_paths.as_ref()),
        listed(config.crypto_erase_bitlocker_drives.as_ref()),
        yes_no(plan.shred_mft_slack),
        config
            .steps
            .as_ref()
            .map(|steps| canonical_json(&serde_json::to_value(steps).expect("steps serialize")))
            .unwrap_or_else(|| "documented defaults".to_string()),
    )
}

fn full_lockdown_confirmation(plan: &LockdownPlanSnapshot) -> String {
    let config = &plan.self_destruct;
    let mut detail = lockdown_confirmation(
        plan,
        config.deactivate_license_first.unwrap_or(false),
        config.shutdown_system.unwrap_or(true),
    );
    let devices = config
        .crypto_erase_veracrypt_devices
        .as_ref()
        .filter(|devices| !devices.is_empty())
        .map(|devices| {
            devices
                .iter()
                .map(|device| {
                    format!(
                        "{} (disk {}, partition {}, id {})",
                        safe_detail(&device.device_path),
                        device.disk_number,
                        device.partition_number,
                        safe_detail(&device.disk_unique_id)
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_else(|| "none".to_string());
    detail.push_str(&format!("\nVeraCrypt device targets: {devices}"));
    detail
}

fn safe_detail(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect()
}

/// Registry of every catastrophic Tauri command and the action it performs.
/// A CI gate (tools/ci/check-destructive-authz.sh) and the unit test
/// `registry_covers_known_catastrophic_commands` enforce that every destructive
/// command appears here, so a new one cannot silently ship without an
/// authorization review.
#[allow(dead_code)]
pub const DESTRUCTIVE_COMMANDS: &[(&str, DestructiveAction)] = &[
    ("lockdown", DestructiveAction::SelfDestruct),
    ("full_lockdown", DestructiveAction::SelfDestruct),
    ("run_destruct_step", DestructiveAction::SelfDestruct),
    ("fleet_connect", DestructiveAction::FleetReenroll),
    ("disk_delete_item", DestructiveAction::DiskDelete),
    ("delete_decoy", DestructiveAction::DecoyDelete),
    ("internet_kill_switch_set", DestructiveAction::KillSwitch),
    ("erase_encrypted_container", DestructiveAction::CryptoErase),
];

#[allow(dead_code)]
pub fn action_for(command: &str) -> Option<DestructiveAction> {
    DESTRUCTIVE_COMMANDS
        .iter()
        .find(|(n, _)| *n == command)
        .map(|(_, a)| *a)
}

/// Verify a PIN against the configured startup-PIN hashes, returning the mode
/// ("real" | "decoy" | "destroy" | "open" | "wrong"). Mirrors
/// startup_auth::verify_startup_pin but is callable synchronously from Rust.
fn verify_pin_local(pin: &str) -> Result<&'static str, String> {
    let s = crate::settings::read_settings().map_err(|e| format!("read settings: {e}"))?;
    let sp = &s.ideal.privacy.startup_pin;
    crate::startup_auth::verify_pin_mode_limited(
        pin,
        sp.real_hash.as_deref(),
        sp.decoy_hash.as_deref(),
        sp.destroy_hash.as_deref(),
    )
}

/// Mint a capability after verifying the requested authorization path.
/// - A renderer-entered PIN is rate-limited and must resolve to the mode required
///   for `action` (`destroy` for the self-destruct/wipe family;
///   `real`/`destroy` otherwise). It is not an unforgeable factor against a
///   compromised WebView.
/// - If no PIN is supplied, fall back to a Rust-initiated native confirm dialog
///   that a WebView script can display but cannot answer.
#[tauri::command]
pub async fn request_destructive_confirmation(
    app: tauri::AppHandle,
    request: DestructiveRequest,
    pin: Option<String>,
) -> Result<String, String> {
    let action = request.action();
    let lockdown_plan = if matches!(
        request,
        DestructiveRequest::Lockdown { .. } | DestructiveRequest::FullLockdown
    ) {
        let settings = crate::settings::read_settings()
            .map_err(|error| format!("read settings: {error}"))?;
        Some(LockdownPlanSnapshot::from_settings(&settings))
    } else {
        None
    };
    let args_canonical = request.canonical_args(lockdown_plan.as_ref());
    let confirmation_detail = request.confirmation_detail(lockdown_plan.as_ref());
    let needs_destroy = matches!(
        action,
        DestructiveAction::SelfDestruct
            | DestructiveAction::RemoveUsers
            | DestructiveAction::CryptoErase
    );
    let authorized = match pin {
        Some(p) if !p.trim().is_empty() => {
            let mode = verify_pin_local(&p)?;
            if needs_destroy {
                mode == "destroy"
            } else {
                mode == "real" || mode == "destroy"
            }
        }
        _ => native_confirm(&app, &confirmation_detail).await,
    };
    if !authorized {
        return Err("confirmation failed".into());
    }
    Ok(mint(action, &args_canonical))
}

fn configured_lockdown_countdown() -> u64 {
    crate::settings::read_settings()
        .map(|settings| u64::from(settings.app.lockdown_timer_sec.clamp(3, 30)))
        .unwrap_or(4)
}

fn emit_lockdown_trigger(app: &tauri::AppHandle, countdown_seconds: u64, cancelled: bool) {
    use tauri::Emitter;
    let _ = app.emit(
        "lockdown-trigger",
        serde_json::json!({
            "countdownSeconds": countdown_seconds,
            "cancelled": cancelled,
        }),
    );
}

pub(crate) fn schedule_trusted_lockdown(app: tauri::AppHandle) {
    let Some(generation) = TRUSTED_LOCKDOWN_GATE.arm() else {
        return;
    };
    let countdown_seconds = configured_lockdown_countdown();
    emit_lockdown_trigger(&app, countdown_seconds, false);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(countdown_seconds)).await;
        if TRUSTED_LOCKDOWN_GATE.take_if_current(generation) {
            execute_trusted_lockdown(app);
        }
    });
}

pub(crate) fn toggle_trusted_lockdown(app: tauri::AppHandle) {
    if TRUSTED_LOCKDOWN_GATE.cancel() {
        emit_lockdown_trigger(&app, 0, true);
    } else {
        schedule_trusted_lockdown(app);
    }
}

pub(crate) fn execute_trusted_lockdown(app: tauri::AppHandle) {
    if TRUSTED_LOCKDOWN_EXECUTING.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::backend::full_lockdown_internal(app).await {
            crate::log_message("error", &format!("[Authz] trusted Lockdown failed: {error}"));
        }
        TRUSTED_LOCKDOWN_EXECUTING.store(false, Ordering::SeqCst);
    });
}

/// Rust-initiated native confirm for a destructive action, exposed for
/// command-level fallback when no capability token is supplied. A WebView
/// script can trigger the dialog but cannot answer it, so it remains a valid
/// control against the assumed-compromised WebView.
pub async fn native_confirm_action(app: &tauri::AppHandle, action: DestructiveAction) -> bool {
    native_confirm(app, &format!("Confirm {action:?}? This action cannot be undone.")).await
}

async fn native_confirm(app: &tauri::AppHandle, detail: &str) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
    // The dialog is a control only because a human answers it. The CLI runtime's
    // one window is built invisible and unfocused, and an unattended run has
    // nobody at the keyboard at all, so this await would never complete — the
    // process would hang forever still holding the CLI execution mutex, which
    // then fails every other mutating CLI run with `cli_busy`. Fail closed: an
    // unanswerable confirmation is a denied confirmation.
    if crate::cli::tauri_runtime_active() {
        crate::log_message(
            "warn",
            "[Authz] refused destructive confirmation: interactive confirmation is unavailable in CLI mode",
        );
        return false;
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(detail)
        .title("WinCommander — confirm destructive action")
        .buttons(MessageDialogButtons::OkCancel)
        .show(move |ok| {
            let _ = tx.send(ok);
        });
    rx.await.unwrap_or(false)
}

pub(crate) async fn confirm_backend_dispatch(
    app: &tauri::AppHandle,
    command: &str,
    params: &HashMap<String, String>,
) -> bool {
    let mut fields: Vec<_> = params.iter().collect();
    fields.sort_unstable_by(|left, right| left.0.cmp(right.0));
    let details = fields
        .into_iter()
        .map(|(key, value)| format!("{key}: {}", safe_detail(value)))
        .collect::<Vec<_>>()
        .join("\n");
    native_confirm(
        app,
        &format!(
            "Run destructive backend command {command}?\n\n{}",
            if details.is_empty() {
                "No parameters".to_string()
            } else {
                details
            }
        ),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn test_plan(self_destruct: crate::settings::SelfDestructSettings) -> LockdownPlanSnapshot {
        LockdownPlanSnapshot {
            self_destruct,
            shred_mft_slack: false,
        }
    }

    #[test]
    fn consume_rejects_wrong_action() {
        let tok = mint(DestructiveAction::SelfDestruct, "hash-A");
        assert!(consume(&tok, DestructiveAction::RemoveUsers, "hash-A").is_err());
    }

    #[test]
    fn consume_rejects_wrong_args() {
        let tok = mint(DestructiveAction::DiskDelete, "path=/a");
        assert!(consume(&tok, DestructiveAction::DiskDelete, "path=/b").is_err());
    }

    #[test]
    fn token_is_single_use() {
        let tok = mint(DestructiveAction::KillSwitch, "on");
        assert!(consume(&tok, DestructiveAction::KillSwitch, "on").is_ok());
        assert!(consume(&tok, DestructiveAction::KillSwitch, "on").is_err());
    }

    #[test]
    fn unknown_token_refused() {
        assert!(consume("not-a-real-token", DestructiveAction::SelfDestruct, "x").is_err());
    }

    #[test]
    fn missing_or_empty_token_is_refused() {
        assert!(consume_required(None, DestructiveAction::DiskDelete, "x").is_err());
        assert!(consume_required(Some(""), DestructiveAction::DiskDelete, "x").is_err());
    }

    #[test]
    fn capability_is_bound_to_command_and_arguments() {
        let plan = test_plan(crate::settings::SelfDestructSettings::default());
        let args = lockdown_args(false, false, &plan);
        let tok = mint(DestructiveAction::SelfDestruct, &args);
        assert!(consume_required(
            Some(&tok),
            DestructiveAction::SelfDestruct,
            &full_lockdown_args(&plan),
        )
        .is_err());

        let args = disk_delete_args("C:\\one");
        let tok = mint(DestructiveAction::DiskDelete, &args);
        assert!(consume_required(
            Some(&tok),
            DestructiveAction::DiskDelete,
            &disk_delete_args("C:\\two"),
        )
        .is_err());
    }

    #[test]
    fn lockdown_capability_is_bound_to_the_complete_configured_plan() {
        let mut confirmed = test_plan(crate::settings::SelfDestructSettings {
            enabled: Some(true),
            shred_folders: Some(vec!["C:\\confirmed".to_string()]),
            ..Default::default()
        });
        let args = full_lockdown_args(&confirmed);
        let token = mint(DestructiveAction::SelfDestruct, &args);
        confirmed.self_destruct.shred_folders = Some(vec!["C:\\substituted".to_string()]);
        assert!(consume_required(
            Some(&token),
            DestructiveAction::SelfDestruct,
            &full_lockdown_args(&confirmed),
        )
        .is_err());
    }

    #[test]
    fn lockdown_plan_canonicalization_is_independent_of_map_insertion_order() {
        let mut first_steps = HashMap::new();
        first_steps.insert("configured_folders".to_string(), true);
        first_steps.insert("remove_users".to_string(), false);
        let mut second_steps = HashMap::new();
        second_steps.insert("remove_users".to_string(), false);
        second_steps.insert("configured_folders".to_string(), true);
        let first = test_plan(crate::settings::SelfDestructSettings {
            steps: Some(first_steps),
            ..Default::default()
        });
        let second = test_plan(crate::settings::SelfDestructSettings {
            steps: Some(second_steps),
            ..Default::default()
        });
        assert_eq!(full_lockdown_args(&first), full_lockdown_args(&second));
    }

    #[test]
    fn lockdown_confirmation_discloses_mft_resident_slack_wipe() {
        let mut plan = test_plan(crate::settings::SelfDestructSettings::default());
        plan.shred_mft_slack = true;
        let detail = DestructiveRequest::FullLockdown.confirmation_detail(Some(&plan));

        assert!(detail.contains("MFT resident-slack wipe: yes"));
    }

    #[cfg(windows)]
    #[test]
    fn path_capability_detects_file_identity_replacement() {
        use std::io::Write;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target.bin");
        let retained = directory.path().join("retained.bin");
        std::fs::File::create(&target)
            .unwrap()
            .write_all(b"first")
            .unwrap();
        let confirmed = disk_delete_args(&target.to_string_lossy());
        std::fs::rename(&target, &retained).unwrap();
        std::fs::File::create(&target)
            .unwrap()
            .write_all(b"replacement")
            .unwrap();
        let replaced = disk_delete_args(&target.to_string_lossy());
        assert_ne!(confirmed, replaced);
    }

    #[test]
    fn required_capability_is_single_use() {
        let args = kill_switch_args(true);
        let tok = mint(DestructiveAction::KillSwitch, &args);
        assert!(consume_required(Some(&tok), DestructiveAction::KillSwitch, &args).is_ok());
        assert!(consume_required(Some(&tok), DestructiveAction::KillSwitch, &args).is_err());
    }

    #[test]
    fn expired_capability_is_refused_and_removed() {
        let token = "expired-test-token".to_string();
        STORE.lock().unwrap().insert(
            token.clone(),
            Minted {
                action: DestructiveAction::DiskDelete,
                args_hash: hash_args("target"),
                at: Instant::now() - TTL,
            },
        );
        assert!(consume(&token, DestructiveAction::DiskDelete, "target").is_err());
        assert!(!STORE.lock().unwrap().contains_key(&token));
    }

    #[test]
    fn concurrent_replay_has_exactly_one_winner() {
        let token = mint(DestructiveAction::DiskDelete, "target");
        let barrier = Arc::new(Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let token = token.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    consume(&token, DestructiveAction::DiskDelete, "target").is_ok()
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|won| *won)
                .count(),
            1
        );
    }

    #[test]
    fn trusted_countdown_gate_is_single_fire_and_cancel_safe() {
        let gate = TrustedLockdownGate::new();
        let first = gate.arm().unwrap();
        assert!(gate.arm().is_none());
        assert!(gate.cancel());
        assert!(!gate.take_if_current(first));
        let second = gate.arm().unwrap();
        assert!(!gate.take_if_current(first));
        assert!(gate.take_if_current(second));
        assert!(!gate.take_if_current(second));
    }

    #[test]
    fn typed_request_owns_action_arguments_and_safe_details() {
        let request = DestructiveRequest::DiskDeleteItem {
            path: "C:\\target\nspoof".to_string(),
        };
        assert_eq!(request.action(), DestructiveAction::DiskDelete);
        assert_eq!(
            request.canonical_args(None),
            disk_delete_args("C:\\target\nspoof")
        );
        let detail = request.confirmation_detail(None);
        assert!(detail.contains("C:\\target�spoof"));
        assert!(!detail.contains("target\nspoof"));
    }

    #[test]
    fn backend_dispatch_keeps_read_only_commands_available() {
        let mut params = HashMap::new();
        assert_eq!(
            backend_dispatch_policy("Get-DnsCacheEntries"),
            BackendDispatchPolicy::Ordinary
        );
        assert!(authorize_backend_dispatch("Get-DnsCacheEntries", &mut params).is_ok());
    }

    #[test]
    fn backend_dispatch_refuses_lockdown_only_feature_ids() {
        for command in [
            "run_destruct_step",
            "Remove-LocalUsers",
            "Destroy-VeraCryptHeader",
            "Clear-BitLockerKeyProtectors",
            "Invoke-7Wipe",
            "Clear-MFTResidentSlack",
            "not-a-registered-command",
        ] {
            let mut params = HashMap::new();
            assert!(
                authorize_backend_dispatch(command, &mut params).is_err(),
                "allowed {command}"
            );
        }
        for command in ["Invoke-CrashDumpErase", "Invoke-PreviousWindowsInstallErase"] {
            assert_eq!(
                backend_dispatch_policy(command),
                BackendDispatchPolicy::NativeConfirmation,
                "did not require native confirmation for {command}"
            );
        }
    }

    #[test]
    fn backend_secure_erase_requires_matching_single_use_capability() {
        let path = "C:\\Users\\test\\target.bin";
        let mut missing = HashMap::from([("Path".to_string(), path.to_string())]);
        assert!(authorize_backend_dispatch("Invoke-7Erase", &mut missing).is_err());

        let token = mint(DestructiveAction::DiskDelete, &secure_erase_args(path));
        let mut authorized = HashMap::from([
            ("Path".to_string(), path.to_string()),
            ("CapabilityToken".to_string(), token.clone()),
        ]);
        assert!(authorize_backend_dispatch("Invoke-7Erase", &mut authorized).is_ok());
        assert!(!authorized.contains_key("CapabilityToken"));

        let mut replay = HashMap::from([
            ("Path".to_string(), path.to_string()),
            ("CapabilityToken".to_string(), token),
        ]);
        assert!(authorize_backend_dispatch("Invoke-7Erase", &mut replay).is_err());

        for (key, value) in [
            ("Name", "DifferentRegistryValue"),
            ("Passes", "99"),
            ("Type", "RegistryProperty"),
        ] {
            let token = mint(DestructiveAction::DiskDelete, &secure_erase_args(path));
            let mut changed = HashMap::from([
                ("Path".to_string(), path.to_string()),
                ("CapabilityToken".to_string(), token),
                (key.to_string(), value.to_string()),
            ]);
            assert!(
                authorize_backend_dispatch("Invoke-7Erase", &mut changed).is_err(),
                "allowed attacker-controlled {key}"
            );
        }
    }

    #[test]
    fn backend_free_space_capability_is_bound_to_drive_and_media() {
        let token = mint(
            DestructiveAction::CryptoErase,
            &free_space_erase_args("D", "SSD"),
        );
        let mut changed_target = HashMap::from([
            ("DriveLetter".to_string(), "E".to_string()),
            ("MediaType".to_string(), "SSD".to_string()),
            ("CapabilityToken".to_string(), token),
        ]);
        assert!(
            authorize_backend_dispatch("Invoke-UnallocatedSpaceErase", &mut changed_target)
                .is_err()
        );
    }

    #[test]
    fn registry_covers_known_catastrophic_commands() {
        for name in [
            "lockdown",
            "full_lockdown",
            "run_destruct_step",
            "fleet_connect",
            "erase_encrypted_container",
        ] {
            assert!(
                action_for(name).is_some(),
                "missing registry entry for {name}"
            );
        }
    }
}
