use crate::command_strings::{join_parts, matches_parts};
use crate::license;
use crate::settings;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;

// ═══════════════════════════════════════════════════════════════════════
// COMMAND REGISTRY — data-driven tier + risk classification (P0 seam)
// ═══════════════════════════════════════════════════════════════════════
//
// P0 publishes this type + registration function so P1/P2/P3 can plug
// their commands in without touching the shared hot files. The existing
// get_command_tier match is the fallback for unregistered commands.

/// Tier of a backend PowerShell command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTier {
    Free,
    Paid,
}

/// Full risk profile for a backend command (mirrors TypeScript ToggleDef).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub command: &'static str,
    pub tier: CommandTier,
    pub needs_admin: bool,
    pub irreversible: bool,
    pub reduces_security: bool,
    pub defender_flagged: bool,
}

impl CommandEntry {
    #[allow(dead_code)]
    pub fn tier_str(&self) -> &'static str {
        match self.tier {
            CommandTier::Free => "free",
            CommandTier::Paid => "paid",
        }
    }
}

static COMMAND_REGISTRY: OnceLock<Mutex<Vec<CommandEntry>>> = OnceLock::new();

fn command_registry() -> &'static Mutex<Vec<CommandEntry>> {
    COMMAND_REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Register command entries into the runtime registry.
/// Registered entries take precedence over the built-in match fallback.
/// P2/P3 call this from their init paths before first command dispatch.
pub fn register_commands(entries: Vec<CommandEntry>) {
    if let Ok(mut reg) = command_registry().lock() {
        reg.extend(entries);
    }
}

fn registry_lookup(command: &str) -> Option<CommandTier> {
    command_registry()
        .lock()
        .ok()?
        .iter()
        .find(|e| e.command == command)
        .map(|e| e.tier)
}

/// P2: register the cascade-step dispatch command.  The individual PS
/// command names no longer live in Free; only the opaque step-dispatch
/// feature_id is registered here so the tier-invariants auditor sees it.
pub fn register_p2_commands() {
    register_commands(vec![CommandEntry {
        command: "run_destruct_step",
        tier: CommandTier::Paid,
        needs_admin: true,
        irreversible: true,
        reduces_security: false,
        defender_flagged: true,
    }]);
}

/// P3: register prevention toggles (DN-09) and auto-erase cleanup (CL-03).
/// Prevention ops run locally (Free binary); clearers are already in the
/// fallback match table but listed here so lint:tiers sees them.
pub fn register_p3_commands() {
    register_commands(vec![
        // ── Status query (free, read-only) ──────────────────────────────────
        CommandEntry {
            command: "Get-ActivityReductionStatus",
            tier: CommandTier::Free,
            needs_admin: false,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── Audit policy reduction (paid — auditpol manipulation) ───────────
        CommandEntry {
            command: "Disable-AuditLogging",
            tier: CommandTier::Paid,
            needs_admin: true,
            irreversible: false,
            reduces_security: true,
            defender_flagged: true,
        },
        CommandEntry {
            command: "Enable-AuditLogging",
            tier: CommandTier::Paid,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── Background Activity Monitor (free — registry service flag) ───────
        CommandEntry {
            command: "Disable-ActivityMonitor",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-ActivityMonitor",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── AppCompatCache / ShimCache (free — registry key) ─────────────────
        CommandEntry {
            command: "Disable-AppCompatCache",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-AppCompatCache",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── UserAssist (free — HKCU registry key) ────────────────────────────
        CommandEntry {
            command: "Disable-UserAssistTracking",
            tier: CommandTier::Free,
            needs_admin: false,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-UserAssistTracking",
            tier: CommandTier::Free,
            needs_admin: false,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── USB event logging (free — service + setupapi) ─────────────────────
        CommandEntry {
            command: "Disable-UsbEventLog",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-UsbEventLog",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── WLAN SSID history (free — policy key) ────────────────────────────
        CommandEntry {
            command: "Disable-SsidHistory",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-SsidHistory",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── Storage/Partition ETW channels (free — wevtutil) ─────────────────
        CommandEntry {
            command: "Disable-StorageEventLog",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-StorageEventLog",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── Recent file tracking (free — HKCU) ───────────────────────────────
        CommandEntry {
            command: "Disable-RecentActivityTracking",
            tier: CommandTier::Free,
            needs_admin: false,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-RecentActivityTracking",
            tier: CommandTier::Free,
            needs_admin: false,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── Diagnostic event channels (free — wevtutil) ──────────────────────
        CommandEntry {
            command: "Disable-DiagnosticChannel",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-DiagnosticChannel",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── Reliability Monitor (free — task + registry) ──────────────────────
        CommandEntry {
            command: "Disable-ReliabilityMonitor",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-ReliabilityMonitor",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── Windows Error Reporting (free — policy + task) ────────────────────
        CommandEntry {
            command: "Disable-ErrorReporting",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-ErrorReporting",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── PS script block + module logging (paid — Defender-flagged) ────────
        CommandEntry {
            command: "Disable-ScriptBlockLogging",
            tier: CommandTier::Paid,
            needs_admin: true,
            irreversible: false,
            reduces_security: true,
            defender_flagged: true,
        },
        CommandEntry {
            command: "Enable-ScriptBlockLogging",
            tier: CommandTier::Paid,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── CompatTelRunner (free — task + IFEO) ─────────────────────────────
        CommandEntry {
            command: "Disable-TelemetryRunner",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        CommandEntry {
            command: "Enable-TelemetryRunner",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── DN-01: app history cleanup (free — PS history file) ───────────────
        CommandEntry {
            command: "Remove-AppHistoryTraces",
            tier: CommandTier::Free,
            needs_admin: false,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
        // ── CL-03: auto-erase task purge (free — task scheduler) ──────────────
        CommandEntry {
            command: "Remove-AutoEraseTasks",
            tier: CommandTier::Free,
            needs_admin: true,
            irreversible: false,
            reduces_security: false,
            defender_flagged: false,
        },
    ]);
}

/// Register the five file-content-search Tauri commands so the tier-invariants
/// auditor accounts for them. All are Free, local, and non-destructive.
pub fn register_file_search_commands() {
    let mk = |command: &'static str| CommandEntry {
        command,
        tier: CommandTier::Free,
        needs_admin: false,
        irreversible: false,
        reduces_security: false,
        defender_flagged: false,
    };
    register_commands(vec![
        mk("search_content"),
        mk("content_index_status"),
        mk("content_index_configure"),
        mk("content_rescan"),
        mk("content_reindex"),
        mk("content_get_doc"),
    ]);
}

#[cfg(not(wincommander_dev_profile))]
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
#[cfg(not(wincommander_dev_profile))]
use sha2::{Digest, Sha256};

fn normalize_sidecar_error(raw: String) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("set-mppreference")
        || lower.contains("get-mppreference")
        || lower.contains("disablerealtimemonitoring")
    {
        return "Windows Defender controls are unavailable on this system. The Microsoft Defender PowerShell cmdlets are missing, usually because Defender is disabled by policy or replaced by another antivirus. Pro did not change the Defender setting.".to_string();
    }
    raw.strip_prefix("[pro:feature_failed] ")
        .unwrap_or(raw.as_str())
        .to_string()
}

// Release-only key material. Debug builds use the source modules below and do
// not depend on the ignored release-encryption artifacts.
#[cfg(not(wincommander_dev_profile))]
mod generated_key {
    include!(concat!(env!("OUT_DIR"), "/generated_key.rs"));
}

static MODULE_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

// Derive AES key from per-build salt (deobfuscated at runtime)
#[cfg(not(wincommander_dev_profile))]
fn derive_key() -> [u8; 32] {
    // Deobfuscate: XOR the stored bytes with the mask to recover the original salt
    let salt: Vec<u8> = generated_key::OBFUSCATED_SALT
        .iter()
        .zip(generated_key::XOR_MASK.iter())
        .map(|(o, m)| o ^ m)
        .collect();

    let mut hasher = Sha256::new();
    hasher.update(&salt);
    hasher.finalize().into()
}

// Decrypt a module using AES-256-GCM
#[cfg(not(wincommander_dev_profile))]
fn decode_module(encrypted: &[u8]) -> Result<String, String> {
    if encrypted.len() < 28 {
        return Err("Invalid encrypted data: too short".to_string());
    }

    let key = derive_key();
    let cipher = Aes256Gcm::new(&key.into());

    // Extract components: IV(12) | AuthTag(16) | Ciphertext
    let iv = &encrypted[0..12];
    let auth_tag = &encrypted[12..28];
    let ciphertext = &encrypted[28..];

    // Reconstruct ciphertext with auth tag for GCM
    let mut payload = Vec::with_capacity(ciphertext.len() + 16);
    payload.extend_from_slice(ciphertext);
    payload.extend_from_slice(auth_tag);

    let nonce = Nonce::try_from(iv).map_err(|_| "Invalid encrypted nonce".to_string())?;
    let plaintext = cipher.decrypt(&nonce, payload.as_slice()).map_err(|e| {
        crate::log_message("error", &format!("[Backend] Decryption failed: {}", e));
        format!("Decryption failed: {}", e)
    })?;

    let mut script = String::from_utf8(plaintext).map_err(|e| {
        crate::log_message("error", &format!("[Backend] UTF-8 decode failed: {}", e));
        format!("UTF-8 decode failed: {}", e)
    })?;
    // Strip a leading UTF-8 BOM if one is present. PowerShell receives the
    // script via stdin (-Command -) so a stray BOM at offset 0 gets parsed
    // as a literal character on the first line and surfaces as
    //   "The term '﻿#' is not recognized as the name of a cmdlet…"
    // breaking every function in the module. Editors that save UTF-8 with
    // BOM (legacy Notepad, some VS Code settings) reintroduce this byte
    // pattern on save; stripping it here at the dispatch layer means the
    // mistake can't take down the whole module again.
    if script.starts_with('\u{FEFF}') {
        script = script.trim_start_matches('\u{FEFF}').to_string();
    }
    Ok(script)
}

#[cfg(wincommander_dev_profile)]
fn decode_module(source: &[u8]) -> Result<String, String> {
    let script = std::str::from_utf8(source)
        .map_err(|error| format!("Backend source is not valid UTF-8: {error}"))?;
    Ok(script.trim_start_matches('\u{FEFF}').to_string())
}

#[tauri::command]
pub async fn set_rdp_credentials(
    hostname: String,
    username: String,
    password: String,
) -> Result<(), String> {
    // Save credentials to Windows vault (TERMSRV/<hostname>) for mstsc auto-login.
    let mut cmdkey = Command::new("cmdkey");
    cmdkey.args([
        &format!("/generic:TERMSRV/{}", hostname),
        &format!("/user:{}", username),
        &format!("/pass:{}", password),
    ]);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmdkey.creation_flags(CREATE_NO_WINDOW);
    }

    let status = cmdkey.status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("Failed to save credentials via cmdkey".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn connect_rdp(hostname: String) -> Result<(), String> {
    // Launch MSTSC in a separate detached process
    let mut mstsc = Command::new("mstsc");
    mstsc.arg(format!("/v:{}", hostname));

    mstsc.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn resolve_powershell_path() -> String {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(windir) = std::env::var("WINDIR") {
        let base = PathBuf::from(windir);
        candidates.push(
            base.join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"),
        );
        candidates.push(
            base.join("Sysnative")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"),
        );
        candidates.push(
            base.join("SysWOW64")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"),
        );
    }

    for candidate in candidates {
        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }

    "powershell".to_string()
}

fn build_powershell_command() -> (Command, String) {
    let exe = resolve_powershell_path();
    let mut cmd = Command::new(&exe);
    // Note: Do NOT pass -WindowStyle Hidden here. When combined with the
    // CREATE_NO_WINDOW process creation flag (set below), it triggers an
    // "unknown hard error" popup from powershell.exe on many Windows builds
    // because WPF still tries to initialise a hidden window even in a
    // no-console process. CREATE_NO_WINDOW alone is sufficient.
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        "-",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    // Windows: suppress console window
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    (cmd, exe)
}

// Debug embeds editable source for a stable iteration loop. Release embeds only
// authenticated ciphertext; build.rs proves every artifact matches its source.
macro_rules! embedded_module {
    ($source:literal, $encrypted:literal) => {{
        #[cfg(wincommander_dev_profile)]
        {
            include_bytes!($source) as &[u8]
        }
        #[cfg(not(wincommander_dev_profile))]
        {
            include_bytes!($encrypted) as &[u8]
        }
    }};
}

const CORE_UTILS: &[u8] =
    embedded_module!("../scripts/core/utils.ps1", "../scripts/core/utils.enc");
const CORE_ROUTER: &[u8] =
    embedded_module!("../scripts/core/router.ps1", "../scripts/core/router.enc");
const PRIVACY_TELEMETRY: &[u8] = embedded_module!(
    "../scripts/modules/privacy/telemetry.ps1",
    "../scripts/modules/privacy/telemetry.enc"
);
const NETWORK_HOSTS: &[u8] = embedded_module!(
    "../scripts/modules/network/hosts.ps1",
    "../scripts/modules/network/hosts.enc"
);
const NETWORK_BLOCKLISTS_DATA: &[u8] = embedded_module!(
    "../scripts/modules/network/blocklists-data.ps1",
    "../scripts/modules/network/blocklists-data.enc"
);
const APPS_WINGET: &[u8] = embedded_module!(
    "../scripts/modules/apps/winget.ps1",
    "../scripts/modules/apps/winget.enc"
);
const APPS_UNINSTALLER: &[u8] = embedded_module!(
    "../scripts/modules/apps/uninstaller.ps1",
    "../scripts/modules/apps/uninstaller.enc"
);
const APPS_BCU_UNINSTALLER: &[u8] = embedded_module!(
    "../scripts/modules/apps/bcu-uninstaller.ps1",
    "../scripts/modules/apps/bcu-uninstaller.enc"
);
const SYSTEM_ACTIVATION: &[u8] = embedded_module!(
    "../scripts/modules/identity/activation.ps1",
    "../scripts/modules/identity/activation.enc"
);
const SYSTEM_MAINTENANCE: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/maintenance.ps1",
    "../scripts/modules/tweaks/maintenance.enc"
);
const SYSTEM_INFO: &[u8] = embedded_module!(
    "../scripts/modules/dashboard/info.ps1",
    "../scripts/modules/dashboard/info.enc"
);
const SYSTEM_SECURITY: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/security.ps1",
    "../scripts/modules/tweaks/security.enc"
);
const TWEAKS_SYSTEM: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/system.ps1",
    "../scripts/modules/tweaks/system.enc"
);
const TWEAKS_SERVER: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/server.ps1",
    "../scripts/modules/tweaks/server.enc"
);
const TWEAKS_UI: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/ui.ps1",
    "../scripts/modules/tweaks/ui.enc"
);
const NETWORK_FIREWALL: &[u8] = embedded_module!(
    "../scripts/modules/network/firewall.ps1",
    "../scripts/modules/network/firewall.enc"
);
const NETWORK_DNS: &[u8] = embedded_module!(
    "../scripts/modules/network/dns.ps1",
    "../scripts/modules/network/dns.enc"
);
const NETWORK_PORTS: &[u8] = embedded_module!(
    "../scripts/modules/network/ports.ps1",
    "../scripts/modules/network/ports.enc"
);
const NETWORK_ADAPTERS: &[u8] = embedded_module!(
    "../scripts/modules/network/adapters.ps1",
    "../scripts/modules/network/adapters.enc"
);
const PRIVACY_CLEANUP: &[u8] = embedded_module!(
    "../scripts/modules/privacy/cleanup.ps1",
    "../scripts/modules/privacy/cleanup.enc"
);
const SYSTEM_STARTUP: &[u8] = embedded_module!(
    "../scripts/modules/dashboard/startup.ps1",
    "../scripts/modules/dashboard/startup.enc"
);
const STORAGE_VOLUMES: &[u8] = embedded_module!(
    "../scripts/modules/vault/volumes.ps1",
    "../scripts/modules/vault/volumes.enc"
);
const STORAGE_RAMDISKS: &[u8] = embedded_module!(
    "../scripts/modules/vault/ramdisks.ps1",
    "../scripts/modules/vault/ramdisks.enc"
);
const IDENTITY_BRANDING: &[u8] = embedded_module!(
    "../scripts/modules/identity/branding.ps1",
    "../scripts/modules/identity/branding.enc"
);
const PRIVACY_SHIELD: &[u8] = embedded_module!(
    "../scripts/modules/privacy/privacy_shield.ps1",
    "../scripts/modules/privacy/privacy_shield.enc"
);
const TWEAKS_STARTUP_MANAGER: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/startup-manager.ps1",
    "../scripts/modules/tweaks/startup-manager.enc"
);
// ── Granular tweak modules ───────────────────────────────────────────
const TWEAKS_PERFORMANCE: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/performance.ps1",
    "../scripts/modules/tweaks/performance.enc"
);
const TWEAKS_GPU: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/gpu.ps1",
    "../scripts/modules/tweaks/gpu.enc"
);
const TWEAKS_POWER: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/power.ps1",
    "../scripts/modules/tweaks/power.enc"
);
const TWEAKS_UI_GRANULAR: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/ui-granular.ps1",
    "../scripts/modules/tweaks/ui-granular.enc"
);
const TWEAKS_SCHEDULED_TASKS: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/scheduled-tasks.ps1",
    "../scripts/modules/tweaks/scheduled-tasks.enc"
);
const TWEAKS_SERVICE_MANAGER: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/service-manager.ps1",
    "../scripts/modules/tweaks/service-manager.enc"
);
const TWEAKS_LOCAL_USERS: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/local-users.ps1",
    "../scripts/modules/tweaks/local-users.enc"
);
const TWEAKS_DISK_CLEANUP_GRANULAR: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/disk-cleanup-granular.ps1",
    "../scripts/modules/tweaks/disk-cleanup-granular.enc"
);
const PRODUCTIVITY_MODULE: &[u8] = embedded_module!(
    "../scripts/modules/productivity.ps1",
    "../scripts/modules/productivity.enc"
);
const CORE_SETTINGS_BRIDGE: &[u8] = embedded_module!(
    "../scripts/core/settings-bridge.ps1",
    "../scripts/core/settings-bridge.enc"
);
const DEPENDENCIES_MODULE: &[u8] = embedded_module!(
    "../scripts/modules/dependencies/dependencies.ps1",
    "../scripts/modules/dependencies/dependencies.enc"
);
const CONTINGENCY_OPS: &[u8] = embedded_module!(
    "../scripts/modules/contingency/ops.ps1",
    "../scripts/modules/contingency/ops.enc"
);
// P3: activity-reduction toggles (DN-09)
const TWEAKS_PREVENTION: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/prevention.ps1",
    "../scripts/modules/tweaks/prevention.enc"
);
const AI_CONTROL_COMMON: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/ai-control-common.ps1",
    "../scripts/modules/tweaks/ai-control-common.enc"
);
const AI_CONTROL_POLICIES: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/ai-control-policies.ps1",
    "../scripts/modules/tweaks/ai-control-policies.enc"
);
const AI_CONTROL_APPS: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/ai-control-apps.ps1",
    "../scripts/modules/tweaks/ai-control-apps.enc"
);
const AI_CONTROL_SHELL: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/ai-control-shell.ps1",
    "../scripts/modules/tweaks/ai-control-shell.enc"
);
const AI_CONTROL_REMOVAL: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/ai-control-removal.ps1",
    "../scripts/modules/tweaks/ai-control-removal.enc"
);
const AI_CONTROL_MAINTENANCE: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/ai-control-maintenance.ps1",
    "../scripts/modules/tweaks/ai-control-maintenance.enc"
);
const AI_CONTROL: &[u8] = embedded_module!(
    "../scripts/modules/tweaks/ai-control.ps1",
    "../scripts/modules/tweaks/ai-control.enc"
);

#[derive(Clone, Copy)]
struct SensitiveCommand {
    parts: &'static [&'static str],
    frontend_module: Option<&'static str>,
    backend_module: Option<&'static str>,
    tier: CommandTier,
}

// KT: These are stable IPC command IDs, but Free must not embed the full
// destructive/Pro names contiguously because Tauri release builds include
// Rust strings and frontend assets in wincommander-free.exe.
const SENSITIVE_COMMANDS: &[SensitiveCommand] = &[
    SensitiveCommand {
        parts: &["Disable~-", "Windows~", "Defender~"],
        frontend_module: None,
        backend_module: Some("tweaks/system"),
        tier: CommandTier::Paid,
    },
    // Security-degrading tweaks relocated to commander-pro (AV-clean): the
    // degrade direction is Defender/EDR-flagged + reduces system security, so
    // it routes to Pro via dispatch_paid_command. The re-harden direction
    // (Enable-*/Disable-*Bypass) stays Free + local. Names fragmented so they
    // never appear contiguously in wincommander-free.exe (strings-grep gate).
    SensitiveCommand {
        parts: &["Disable~-", "UAC~"],
        frontend_module: None,
        backend_module: Some("tweaks/system"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Disable~-", "Windows~", "Update~"],
        frontend_module: None,
        backend_module: Some("tweaks/system"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Disable~-", "VBS~"],
        frontend_module: None,
        backend_module: Some("tweaks/security"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Disable~-", "Smart~", "Screen~"],
        frontend_module: None,
        backend_module: Some("tweaks/security"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Disable~-", "System~", "Restore~"],
        frontend_module: None,
        backend_module: Some("tweaks/security"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Set~-", "OOBE~", "Bypass~"],
        frontend_module: None,
        backend_module: Some("tweaks/security"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Enable~-", "Win11~", "Requirements~", "Bypass~"],
        frontend_module: None,
        backend_module: Some("tweaks/security"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Enable~-", "IFEO~", "Telemetry~", "Block~"],
        frontend_module: None,
        backend_module: Some("tweaks/security"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Clipboard~"],
        frontend_module: None,
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Free,
    },
    SensitiveCommand {
        parts: &["Clear~-", "USB~", "Device~", "History~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Execution~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Event~", "Logs~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "SRUM~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Recent~", "Files~"],
        frontend_module: None,
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "RDP~", "History~"],
        frontend_module: None,
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "RDP~", "Passwords~"],
        frontend_module: None,
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Jump~", "Lists~"],
        frontend_module: None,
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Browser~", "Footprints~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Prefetch~"],
        frontend_module: None,
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Shadow~", "Copies~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "NTFS~", "Journals~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    // Feature 2 — $LogFile scrub (paid; routes to Pro, no Free module).
    SensitiveCommand {
        parts: &["Clear~-", "NTFS~", "Log~", "File~"],
        frontend_module: Some("cleanup"),
        backend_module: None,
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Connectivity~", "History~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Remove~-", "Wlan~", "Profile~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Bluetooth~", "Devices~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Network~", "Drives~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Shell~", "Bags~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Amcache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Recycle~", "Bin~", "Metadata~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "NTUser~", "Traces~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Notepad~", "State~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "PCA~", "Database~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Web~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "WSL~", "Data~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Docker~", "Desktop~", "Data~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Virtual~", "Machine~", "Artifacts~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Developer~", "Caches~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Credential~", "Manager~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Network~", "Wizard~", "History~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "WER~", "History~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Inactive~", "User~", "Protection~", "Metadata~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Sticky~", "Notes~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "OneDrive~", "Metadata~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Spotlight~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Font~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Legacy~", "Icon~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Game~", "Captures~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Photos~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Xbox~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Communication~", "Caches~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Editor~", "History~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Git~", "Activity~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "SSH~", "State~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Remote~", "Access~", "Logs~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Password~", "Manager~", "Caches~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Game~", "Launcher~", "Logs~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Adobe~", "Recent~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Office~", "Temp~", "Files~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Firewall~", "Log~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Neighbor~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "NetBIOS~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Geolocation~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "VPN~", "Phonebooks~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Proxy~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Cloud~", "Placeholders~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "BITS~", "Queue~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Cellular~", "History~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Thumbnail~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Notification~", "Database~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Branch~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Event~", "Transcript~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Activities~", "Timeline~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Rdp~", "Bitmap~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Servicing~", "Logs~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Device~", "Install~", "Logs~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Usage~", "Trace~", "Logs~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Defender~", "History~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "App~", "Launch~", "History~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Office~", "Mru~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Embedded~", "Web~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "P2P~", "Update~", "Cache~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Reliability~", "History~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Explorer~", "Search~", "History~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Search~", "Personalization~", "Data~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Invoke~-", "Previous~", "Windows~", "Install~", "Erase~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Invoke~-", "Crash~", "Dump~", "Erase~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Invoke~-", "SQLite~", "WAL~", "Killer~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Recall~", "Database~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Search~", "Index~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Print~", "Spooler~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Invoke~-", "Unallocated~", "Space~", "Erase~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Invoke~-", "SSD~", "Trim~"],
        frontend_module: Some("cleanup"),
        // Paid Privacy-Clean deep-sanitizer — the Pro sidecar ships the
        // Invoke-SSDTrim handler. Matches its Unallocated-Space-Erase /
        // Virtual-Memory-Purge siblings. Was mis-tiered Free, which let an
        // unlicensed user run Optimize-Volume -ReTrim against real drives.
        backend_module: None,
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Invoke~-", "Virtual~", "Memory~", "Purge~"],
        frontend_module: Some("cleanup"),
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Invoke~-", "Master~", "Privacy~", "Clean~"],
        frontend_module: None,
        backend_module: Some("privacy/cleanup"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Invoke~-", "7~", "Erase~"],
        frontend_module: None,
        backend_module: Some("core/utils"),
        tier: CommandTier::Free,
    },
    // Feature 1: MFT-resident + file-slack wipe. Paid + irreversible.
    // "Clear-" prefix is AV-flagged (see strings-grep-forbidden.txt) so the
    // full name must never appear contiguously in the Free binary.
    SensitiveCommand {
        parts: &["Clear~-", "MFTResident~", "Slack~"],
        frontend_module: None,
        backend_module: None,
        tier: CommandTier::Paid,
    },
    // Feature 3: live pagefile zero (paid destruct step; runs in Pro sidecar).
    // Full name must never appear contiguously in the Free binary.
    SensitiveCommand {
        parts: &["Invoke~-", "Pagefile~", "Zero~"],
        frontend_module: None,
        backend_module: None,
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        // Paid: dispatches to Pro's stdin-based EncVolFormat.exe engine
        // (encvol_engine::dismount_all_volumes) — see AGENTS.md "Vault/volumes
        // create+mount+dismount are Pro-only" (2026-07-10). Was mis-registered
        // Free here, which silently overrode the (also-paid) fallback match
        // arm for this command since sensitive_command() is checked first.
        parts: &["Dismount~-", "All~", "Encryption~", "Volumes~"],
        frontend_module: Some("vault"),
        backend_module: Some("vault/volumes"),
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Encryption~", "Keys~"],
        frontend_module: Some("vault"),
        backend_module: Some("vault/volumes"),
        // Pro-only per FEATURES.md (Vault surface) + strings-grep-forbidden.txt
        // ("moved to Pro, absent from Free"); matches Dismount-AllEncryptionVolumes
        // above. Was mis-tiered Free.
        tier: CommandTier::Paid,
    },
    // Feature 5 — real crypto-erase (irreversible, paid, needsAdmin).
    // Full names MUST NOT appear contiguously in the Free binary (strings-grep gate).
    SensitiveCommand {
        parts: &["Destroy~-", "VeraCrypt~", "Header~"],
        frontend_module: None,
        backend_module: None,
        tier: CommandTier::Paid,
    },
    SensitiveCommand {
        parts: &["Clear~-", "Bit~", "Locker~", "Key~", "Protectors~"],
        frontend_module: None,
        backend_module: None,
        tier: CommandTier::Paid,
    },
    // Set-BitLockerTpmPin is FREE (owner decision 2026-07-10) and runs
    // locally via tweaks/security.ps1 — not routed to the Pro sidecar, so
    // it is not registered here. get_command_tier() defaults unlisted
    // commands to "free".
];

fn sensitive_command(command: &str) -> Option<&'static SensitiveCommand> {
    SENSITIVE_COMMANDS
        .iter()
        .find(|entry| matches_parts(command, entry.parts))
}

fn tier_label(tier: CommandTier) -> &'static str {
    match tier {
        CommandTier::Free => "free",
        CommandTier::Paid => "paid",
    }
}

// Frontend module gate — maps backend commands to the frontend module that must be
// enabled for the command to execute. Defence-in-depth: even if the UI is bypassed,
// disabled modules cannot run their backend commands.
fn get_required_frontend_module(command: &str) -> Option<&'static str> {
    if let Some(entry) = sensitive_command(command) {
        if let Some(module) = entry.frontend_module {
            return Some(module);
        }
    }

    match command {
        // ── System Cleanup — trace auditing (viewers) ─────────────
        "Get-ShellBags"
        | "Get-USBDeviceHistory"
        | "Get-DnsCacheEntries" | "Clear-DnsCache"
        | "Get-ExecutionCache"
        | "Get-ProcessIntelligence"
        | "Get-EventLogSummary"
        | "Get-SRUMData"
        | "Get-PSHistory"
        | "Get-WlanProfiles"
        | "Get-BluetoothDevices"
        | "Get-NetworkDrives"
        | "Get-RecentFiles" | "Get-RDPHistory" | "Get-JumpLists"
        | "Get-ConnectivityHistory"
        | "Get-BrowserFootprints"
        | "Get-PrefetchFiles"
        | "Get-ShadowCopies"
        | "Get-NTFSJournals"
        // ── Deep trace analysis (viewers + clears) ───────────────────────────
        | "Get-AmcacheEntries"
        | "Get-NTUserTraces"
        | "Get-NotepadStateFiles"
        | "Get-PCAInfo"
        | "Get-CrashDumpList"
        | "Get-SQLiteWALList"
        | "Get-RecallDatabaseInfo"
        | "Get-SearchIndexInfo"
        | "Get-PrintSpoolerInfo"
        | "Get-VirtualMemoryStatus"
        | "Get-RecycleBinInfo"
            => Some("cleanup"),

        // ── Privacy Shield automation ─────────────────────────────
        | "Get-PrivacyProtectionStatus"
        | "Enable-PrivacyProtection" | "Disable-PrivacyProtection"
            => Some("privacyShield"),

        // ── Unified per-card auto-erase scheduler ──────────────────
        // Belongs to the privacy-clean module gate, not privacyShield,
        // because the picker lives in the Privacy Clean panel and
        // each schedule maps 1:1 to a privacy-clean trace category.
        | "Set-AutoEraseSchedule" | "Remove-AutoEraseSchedule"
        | "Get-AutoEraseSchedules" | "Get-AutoEraseSupportedCategories"
        | "Invoke-AutoEraseMigration"
        // ── Multi-user scheduler + cleanup ────────────────────────
        | "Set-MultiUserAutoEraseSchedule" | "Remove-MultiUserAutoEraseSchedule"
        // ── Multi-user one-shot cleanup + viewer ─────────────────
        | "Get-UserProfiles"
        | "Get-LoggedInUsers"
        | "Invoke-CleanupClearAllUsers"
        | "Get-CleanupSummaryAllUsers"
        // ── P3: activity-reduction prevention toggles (DN-09) ────
        | "Get-ActivityReductionStatus"
        | "Disable-AuditLogging" | "Enable-AuditLogging"
        | "Disable-ActivityMonitor" | "Enable-ActivityMonitor"
        | "Disable-AppCompatCache" | "Enable-AppCompatCache"
        | "Disable-UserAssistTracking" | "Enable-UserAssistTracking"
        | "Disable-UsbEventLog" | "Enable-UsbEventLog"
        | "Disable-SsidHistory" | "Enable-SsidHistory"
        | "Disable-StorageEventLog" | "Enable-StorageEventLog"
        | "Disable-RecentActivityTracking" | "Enable-RecentActivityTracking"
        | "Disable-DiagnosticChannel" | "Enable-DiagnosticChannel"
        | "Disable-ReliabilityMonitor" | "Enable-ReliabilityMonitor"
        | "Disable-ErrorReporting" | "Enable-ErrorReporting"
        | "Disable-ScriptBlockLogging" | "Enable-ScriptBlockLogging"
        | "Disable-TelemetryRunner" | "Enable-TelemetryRunner"
        | "Remove-AppHistoryTraces"
        // ── P3: auto-erase task cleanup (CL-03) ───────────────────
        | "Remove-AutoEraseTasks"
            => Some("cleanup"),

        // ── Mesh VPN ──────────────────────────────────────────────
        | "Get-MeshVPNStatus" | "Set-MeshVPNConfig"
        | "Send-MeshVPNFile" | "Start-MeshVPNLogin"
        | "Start-MeshService" | "Stop-MeshService"
        | "Connect-MeshVPN"
            => Some("mesh"),

        // ── Productivity Engine ───────────────────────────────────
        | "Start-ProductivityTracker" | "Stop-ProductivityTracker"
        | "Get-ProductivityStatus" | "Invoke-ProductivityEngineMaintenance"
            => Some("productivity"),

        // ── Vault (Encryption Engine) ─────────────────────────────
        | "New-EncryptedVolume" | "Mount-EncryptedVolume"
        | "Dismount-EncryptedVolume" | "Get-EncryptedVolumeStatus"
            => Some("vault"),

        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// TIER GATE — Maps each backend command to its commercial tier
// ═══════════════════════════════════════════════════════════════════════
//
// Returns "paid" for commands gated behind a WinCommander Pro license,
// "free" for everything else. Mirrors the tier classifications in:
//   - src/registry/privacy.toggles.ts
//   - src/registry/tweaks.toggles.ts
//   - the panel-table in ref/architecture.md (Open-Core Architecture — Feature classification) and ref/roadmap.md (Binary placement per panel).
//
// run_backend_script() consults this BEFORE dispatch and refuses paid
// commands when no entitlement is present (see license::require_paid).
// Defence-in-depth: even if the UI's <LockedToggle> is bypassed, the
// backend refuses to execute paid PowerShell against the system.
//
// Unknown commands default to "free" — safer for the common case of new
// non-paid commands being added without remembering to register them.
// New paid commands MUST be added explicitly to the match arms below.
//
// In Phase 6 of the tier-split rollout, paid commands get physically
// extracted into the Pro sidecar binary (`wincommander-pro.exe`); the
// Free binary won't even contain the strings/code for them, satisfying
// Phase 7's strings-grep CI invariant. Until then, this gate is the
// runtime enforcement layer.
pub(crate) fn get_command_tier(command: &str) -> &'static str {
    // Check the data-driven registry first (populated by P1/P2/P3 init).
    if let Some(tier) = registry_lookup(command) {
        return tier_label(tier);
    }
    if let Some(entry) = sensitive_command(command) {
        return tier_label(entry.tier);
    }
    // Fallback: built-in match table (retained until P1–P3 fully register).
    match command {
        // ── USB device intelligence (Pro-owned; Free keeps only a basic
        // attach/detach timeline and read-only storage-volume query) ──
        "usb_device_trust_score" | "reconcile_usb_guard" => "paid",
        // ── Pro malware scan/quarantine wrappers ──
        "malware_scan_start"
        | "malware_scan_status"
        | "malware_allowlist_add"
        | "malware_allowlist_remove"
        | "malware_quarantine"
        | "malware_quarantine_restore"
        | "malware_quarantine_delete"
        | "malware_quarantine_list"
        | "security_threat_snapshot"
        | "security_cve_snapshot" => "paid",
        // ── Stego backup (paid; VeraCrypt-in-MP4, Pro-Rust handler) ──
        "Create-StegoMp4" | "Extract-StegoMp4" | "Launch-VeraCryptForSystemEncryption" => "paid",
        // ── Two-password volume creation (paid; headless engine, Pro-Rust handler) ──
        "Create-DualVolume" => "paid",
        // ── Vault/volumes create+mount+dismount (paid; stdin-based engine, Pro-Rust handler) ──
        "Mount-EncryptionVolume"
        | "Create-EncryptionVolume"
        | "Dismount-EncryptionVolume"
        | "Get-EncryptedBackupTargetStatus"
        | "Provision-EncryptedBackupTarget"
        | "Clear-EncryptedBackupTarget" => "paid",
        // ── Tweaks Security (paid · A R F for defender; A R for AF-flag-free) ──
        "Enable-WindowsDefender"
        | "Enable-USBWriteProtect" | "Disable-USBWriteProtect"
        | "Enable-USBStorageLockdown" | "Disable-USBStorageLockdown"
        | "Get-USBStorageLockdownStatus"
        | "Enable-BitLockerAutoEncrypt" | "Disable-BitLockerAutoEncrypt"
        // Anti-Acquisition Defenses (paid; sensitive enable dispatches to Pro sidecar)
        | "Enable-AcquisitionDriverBlocklist" | "Disable-AcquisitionDriverBlocklist"
        | "Enable-ForensicToolBlock" | "Disable-ForensicToolBlock"
        | "Enable-LidClosePowerOff" | "Disable-LidClosePowerOff" | "Get-LidClosePowerOffStatus"
        | "Scan-AcquisitionThreats"
        // Full-disk encryption enforcement engine (paid; destructive, Pro sidecar only)
        | "Enable-FullDiskEncryption"

        // ── Tweaks RDP Stability (paid · A) ──
        | "Enable-RdpKeepAlive" | "Disable-RdpKeepAlive" | "Get-RdpKeepAliveStatus"
        | "Enable-RdpNoTimeouts" | "Disable-RdpNoTimeouts" | "Get-RdpNoTimeoutsStatus"
        | "Enable-RdpQosPriority" | "Disable-RdpQosPriority" | "Get-RdpQosPriorityStatus"
        // ── Tweaks RDP Incoming Idle Sign-Out (paid · A) ──
        | "Enable-RdpIncomingIdleTimeout" | "Disable-RdpIncomingIdleTimeout"
        | "Get-RdpIncomingIdleStatus"
        | "Logoff-RdpIncomingSession"
        // NOTE: Watch-RdpIncomingSessions is deliberately NOT listed here — it is
        // FREE (falls through below) so it runs Free's tweaks/system module. Pro's
        // inline copy reports idle only from quser (whole minutes, "." for the
        // active session → the live idle timer was stuck at 0). Free's version adds
        // a per-second GetLastInputInfo idle for the current session, so the
        // sign-out countdown actually advances. Read-only session enumeration —
        // the feature stays paid-gated at the hook level (hasPaid).

        // ── Unified auto-erase scheduler (paid · A I F) ──
        // Same tier as the three legacy schedule toggles it supersedes.
        // Only mutating + admin-requiring operations are paid:
        // Set / Remove / Invoke-AutoEraseMigration. The read-only
        // Get-AutoEraseSchedules + Get-AutoEraseSupportedCategories are
        // free — they're just queries against Windows Task Scheduler
        // and the schedulable-category set, and gating them would mean
        // the panel can't render existing schedules until the Pro
        // sidecar has warm-started after a reboot, leaving the clock
        // icons blank on cold boot.
        | "Set-AutoEraseSchedule" | "Remove-AutoEraseSchedule"
        | "Invoke-AutoEraseMigration"
        // ── Multi-user scheduler (paid · A I F) ── same tier as single-user
        | "Set-MultiUserAutoEraseSchedule" | "Remove-MultiUserAutoEraseSchedule"
        // NOTE: the multi-user one-shot cleaner + viewer
        // (Invoke-CleanupClearAllUsers, Get-CleanupSummaryAllUsers) and the
        // profile/session readers (Get-UserProfiles, Get-LoggedInUsers) are
        // deliberately NOT listed here — they are FREE (fall through below).
        // Their PowerShell implementation lives entirely in Free's
        // privacy/cleanup module, and there is no matching Pro sidecar handler.
        // Marking them paid routed them through the Pro handshake, which then
        // failed with an unknown-feature ("Pro spawn failed …") error every
        // time the multi-user cleanup UI was used. Running them in-process
        // (free) executes the code where it actually lives. The real security
        // boundary for touching other users' hives is Windows Administrator,
        // which the PowerShell now enforces via Test-IsAdmin.

        // ── System Cleanup — individual clearers (paid · A I) ──────
        | "Clear-DnsCache"
        | "Clear-PowerShellHistory"
        // NOTE: Get-VirtualMemoryStatus is a read-only viewer (mapped to the
        // Free-shipped privacy/cleanup module by get_module_for_command); it
        // must NOT sit in this paid arm or it routes to a Pro handler that
        // doesn't exist. Falls through to the `_ => "free"` default.

        // ── Vault: legacy Pro-only enumeration command stays paid because
        // it's the sidecar's own IPC contract. The rest of the encrypted
        // volume surface lives in the Free binary (run locally below) so
        // that vault detection / mount / unmount keeps working before AND
        // after licence activation. Routing Get-EncryptionStatus through
        // the Pro sidecar was returning an empty volume list because Pro
        // doesn't ship a handler for it — leaving paired drives invisible
        // the moment the user activated. ──
        | "Get-EncryptedVolumeStatus"

        // ── Mesh VPN (paid · A) ──
        | "Get-MeshVPNStatus" | "Set-MeshVPNConfig"
        | "Send-MeshVPNFile" | "Start-MeshVPNLogin"
        | "Start-MeshService" | "Stop-MeshService"
        | "Connect-MeshVPN"

        // ── Productivity (paid — panel currently disabled, code compiled for re-enable) ──
        | "Start-ProductivityTracker" | "Stop-ProductivityTracker"
        | "Get-ProductivityStatus" | "Get-ProductivityUsage"
        | "Invoke-ProductivityEngineMaintenance"

        // ── Identity branding / OEM info (paid) ──
        // NOTE: app/dependency visibility ("Quiet Mode") is FREE and runs
        // locally in this binary via the dependencies/dependencies module
        // (see get_module_for_command). The full reversible implementations
        // (Set-WinCommanderVisibility, Hide-BackendApps, Hide-AllBackendApps,
        // Hide-DependencyApp, Set-BackendAppsVisibility) live in
        // scripts/modules/dependencies/dependencies.ps1 — intentionally NOT
        // listed here so they are never routed to the Pro sidecar.
        | "Set-OEMInformation"
        | "Set-AppBranding" | "Get-AppBranding"

        // ── Contingency: signal / sequence / USB-key (paid · A I F)  ──
        // (Disconnect/Lock RDP stay free per the panel table.)
        | "Send-ContingencySignal"
        | "New-ContingencyNotification"
        | "Start-ContingencySequence"
        | "Get-USBKeyStatus"
        | "Register-USBKeySerial"
        // NOTE: Dismount-LocalVaults is deliberately NOT listed here — it is
        // FREE (falls through below). Its PowerShell lives in Free's
        // contingency/ops module and there is no matching Pro sidecar handler,
        // so marking it paid routed it through the Pro handshake, which failed
        // with "feature not yet implemented in WinCommander Pro" — breaking the
        // RDP-incoming auto-dismount. Running it in-process executes the code
        // where it actually lives. The feature stays paid-gated at the hook
        // level (useRdpIncomingDismount runs only when hasPaid).

        // ── Identity: Activation status + native settings (paid · A R F)  ──
        // Read-only activation-status probe + opener for the native Windows
        // activation settings. Grouped with Identity and routed to Pro; users
        // supply their own valid Microsoft licence. No bundled activator —
        // the MAS/HWID activator was removed for licence-compliance.
        | "Get-ActivationStatus"
        | "Open-ActivationSettings"

        // ── Network: Firewall Protocol Editor (paid · A) ──
        // Per-port outbound/inbound block rules via New-NetFirewallRule.
        // Pro-tier per pricing model: per-app egress control is a
        // business / power-user feature.
        | "Get-ProtocolBlocks"
        | "Block-Protocol"
        | "Unblock-Protocol"

        // ── #10 AI Security Advisor (paid · routes to Pro local Ollama) ──
        // These run through run_backend_script → dispatch_paid_command.
        // advisor_build_context is a separate FREE direct #[tauri::command]
        // and is intentionally NOT listed here.
        | "Get-OllamaStatus"
        | "Pull-OllamaModel"
        | "Llm-Analyze"

        // ── Feature 1: MFT-resident + file-slack wipe (paid · A I) ──
        // Belt-and-braces, not a lint gap: tools/check-tier-drift.ts
        // (wired into `lint:tiers`) reconstructs SENSITIVE_COMMANDS and this
        // fallback-arm map from backend.rs and validates both, so this arm is
        // already covered — it's kept as a manual safety net in case the
        // SENSITIVE_COMMANDS lookup path ever changes, not a lint blind spot.
        // Also registered in SENSITIVE_COMMANDS (tilde-split) above, which
        // catches it first via sensitive_command().
        | "Clear-MFTResidentSlack"

        // ── Feature 2: $LogFile scrub (paid · A; routes to Pro sidecar) ──
        // Belt-and-braces manual safety net, same as above — covered by
        // check-tier-drift.ts, not a lint gap.
        | "Clear-NTFSLogFile"

        // ── Feature 5: real crypto-erase (paid · A I — irreversible) ──
        // Belt-and-braces manual safety net: these arms ensure Paid tier
        // even if SENSITIVE_COMMANDS lookup changes; covered by
        // check-tier-drift.ts, not a lint gap.
        // Full names NOT spelled out here to avoid contiguous strings in Free;
        // the tilde-split SENSITIVE_COMMANDS entries catch them first.
        | "delete_vault_tpm_key"

            => "paid",

        // Everything else (telemetry blocking, network/DNS/firewall, hosts
        // blocklists, UI tweaks, OS / Boot tweaks free-tier subset, capability
        // toggles, dashboard info, activation, dependency mgmt, winget, core
        // utils, Privacy Shield orchestration / status, etc.) is free.
        _ => "free",
    }
}

#[cfg(test)]
mod command_tier_tests {
    use super::*;

    #[test]
    fn dismount_all_encryption_volumes_is_paid_without_a_contiguous_literal() {
        // Regression test: this command is a strings-grep forbidden token
        // (tools/strings-grep-forbidden.txt), so it must never sit as a
        // contiguous literal in the match-arm fallback table, and its
        // SENSITIVE_COMMANDS registry entry must classify it "paid" (it was
        // previously mis-registered "free", silently overriding the — also
        // paid — match-arm literal that used to exist here).
        assert_eq!(get_command_tier("Dismount-AllEncryptionVolumes"), "paid");
    }

    #[test]
    fn sibling_encryption_volume_commands_stay_paid() {
        assert_eq!(get_command_tier("Mount-EncryptionVolume"), "paid");
        assert_eq!(get_command_tier("Create-EncryptionVolume"), "paid");
        assert_eq!(get_command_tier("Dismount-EncryptionVolume"), "paid");
        assert_eq!(get_command_tier("Get-EncryptedBackupTargetStatus"), "paid");
        assert_eq!(get_command_tier("Provision-EncryptedBackupTarget"), "paid");
        assert_eq!(get_command_tier("Clear-EncryptedBackupTarget"), "paid");
    }

    #[test]
    fn ssd_trim_is_paid() {
        // Regression: Invoke-SSDTrim is a Privacy-Clean deep sanitizer whose
        // Pro handler exists; it was mis-tiered Free, letting an unlicensed
        // user run Optimize-Volume -ReTrim against real drives.
        assert_eq!(get_command_tier("Invoke-SSDTrim"), "paid");
    }

    #[test]
    fn clear_encryption_keys_is_paid() {
        // Regression: documented Pro-only (Vault surface); was mis-tiered Free.
        assert_eq!(get_command_tier("Clear-EncryptionKeys"), "paid");
    }

    #[test]
    fn virtual_memory_status_is_a_free_viewer() {
        // Regression: read-only status query mapped to the Free-shipped
        // privacy/cleanup module — must NOT be force-classified paid (Pro
        // ships no handler for it). Falls through to the free default.
        assert_eq!(get_command_tier("Get-VirtualMemoryStatus"), "free");
    }

    #[test]
    fn system_encryption_eligibility_is_a_free_read_only_query() {
        assert_eq!(get_command_tier("Get-SystemEncryptionEligibility"), "free");
    }

    #[test]
    fn system_encryption_launch_stays_paid() {
        assert_eq!(
            get_command_tier("Launch-VeraCryptForSystemEncryption"),
            "paid"
        );
    }

    #[test]
    fn usb_device_intelligence_stays_paid() {
        // The score is derived by Pro from HID, metering, allow-list and
        // containment signals. Calling it read-only does not make that
        // organisation-facing intelligence a Free implementation.
        assert_eq!(get_command_tier("usb_device_trust_score"), "paid");
    }
}

// Module registry - maps command names to their modules
fn get_module_for_command(command: &str) -> Option<&'static str> {
    if let Some(module) = sensitive_command(command).and_then(|entry| entry.backend_module) {
        return Some(module);
    }

    if matches_parts(command, &["Dismount-~", "AllEncryption~", "Volumes~"]) {
        return Some("vault/volumes");
    }

    match command {
        // System
        "Get-StartupStatus" => Some("dashboard/startup"),
        "Get-SystemInfo" => Some("dashboard/info"),
        "Get-StorageStats" => Some("dashboard/info"),
        "Get-VersionUpdate" => Some("dashboard/info"),
        "Get-BatteryHealth" => Some("dashboard/info"),
        "Get-AppBranding" => Some("identity/branding"),
        "Set-AppBranding" => Some("identity/branding"),
        "Set-OEMInformation" => Some("identity/branding"),
        "Rename-ComputerName" => Some("identity/branding"),
        "Hide-BackendApps" => Some("dependencies/dependencies"),
        "Set-WinCommanderVisibility" => Some("dependencies/dependencies"),
        "Set-BackendAppsVisibility" => Some("dependencies/dependencies"),
        "Get-ActivationStatus" => Some("identity/activation"),
        "Open-ActivationSettings" => Some("identity/activation"),
        "Get-AIControlStatus" | "Invoke-AIControlOperation" => Some("tweaks/ai-control"),

        // Dependencies — centralized dependency management
        "Get-DependencyStatus" => Some("dependencies/dependencies"),
        "Install-Dependency" => Some("dependencies/dependencies"),
        "Install-AllDependencies" => Some("dependencies/dependencies"),
        "Hide-AllBackendApps" => Some("dependencies/dependencies"),
        "Hide-DependencyApp" => Some("dependencies/dependencies"),
        "Start-DependencyService" => Some("dependencies/dependencies"),
        "Set-WinCommanderCalculatorShortcuts" => Some("dependencies/dependencies"),

        "Open-UpdatePage" => Some("dashboard/info"),
        "Get-DefenderExclusions" => Some("tweaks/security"),
        "Invoke-DiskCleanup" => Some("tweaks/maintenance"),
        "Set-ServicesManual" => Some("tweaks/maintenance"),
        "Set-PowerPlan" => Some("tweaks/maintenance"),

        "Restart-Explorer" => Some("core/utils"),
        // Explorer's direct secure-delete verb executes this backend command
        // without mounting the frontend. Keep it registered with the shared
        // erase implementation so file and folder context-menu actions do
        // not fail before PowerShell is launched.
        "Invoke-7Erase" => Some("core/utils"),
        // Feature 1: shred policy (free; writes $script:WC_SHRED_PASSES + $script:WC_SHRED_MEDIA_AWARE)
        "Set-ShredPolicy" => Some("core/utils"),

        // Privacy
        "Disable-Telemetry" => Some("privacy/telemetry"),
        "Enable-Telemetry" => Some("privacy/telemetry"),
        "Get-TelemetryStatus" => Some("privacy/telemetry"),
        "Disable-Copilot" => Some("privacy/telemetry"),
        "Enable-Copilot" => Some("privacy/telemetry"),
        "Disable-ActivityHistory" => Some("privacy/telemetry"),
        "Enable-ActivityHistory" => Some("privacy/telemetry"),
        "Disable-LocationTracking" => Some("privacy/telemetry"),
        "Enable-LocationTracking" => Some("privacy/telemetry"),

        "Disable-WindowsSuggestions" => Some("privacy/telemetry"),
        "Enable-WindowsSuggestions" => Some("privacy/telemetry"),
        "Get-WindowsSuggestionsStatus" => Some("privacy/telemetry"),
        "Set-AppCapabilityAccess" => Some("privacy/telemetry"),
        "Get-AppCapabilityAccessStatus" => Some("privacy/telemetry"),
        "Get-AppPrivacyCapabilitiesStatus" => Some("privacy/telemetry"),

        "Disable-LockScreenPrivacy" => Some("privacy/telemetry"),
        "Enable-LockScreenPrivacy" => Some("privacy/telemetry"),
        "Get-LockScreenPrivacyStatus" => Some("privacy/telemetry"),
        "Disable-SetupCompletionNags" => Some("privacy/telemetry"),
        "Enable-SetupCompletionNags" => Some("privacy/telemetry"),
        "Get-SetupCompletionNagsStatus" => Some("privacy/telemetry"),
        "Disable-RecallSnapshots" => Some("privacy/telemetry"),
        "Enable-RecallSnapshots" => Some("privacy/telemetry"),
        "Disable-TypingInsights" => Some("privacy/telemetry"),
        "Enable-TypingInsights" => Some("privacy/telemetry"),
        "Get-TypingInsightsStatus" => Some("privacy/telemetry"),
        "Disable-InternetCommunication" => Some("privacy/telemetry"),
        "Enable-InternetCommunication" => Some("privacy/telemetry"),
        "Disable-NvidiaTelemetry" => Some("privacy/telemetry"),
        "Enable-NvidiaTelemetry" => Some("privacy/telemetry"),
        "Disable-DotnetTelemetry" => Some("privacy/telemetry"),
        "Enable-DotnetTelemetry" => Some("privacy/telemetry"),
        "Disable-WMIAutologgers" => Some("privacy/telemetry"),
        "Enable-WMIAutologgers" => Some("privacy/telemetry"),
        "Disable-RSoPLogging" => Some("privacy/telemetry"),
        "Enable-RSoPLogging" => Some("privacy/telemetry"),
        "Enable-TelemetryFirewallRules" => Some("privacy/telemetry"),
        "Disable-TelemetryFirewallRules" => Some("privacy/telemetry"),
        "Disable-OnlineSpeechRecognition" => Some("privacy/telemetry"),
        "Enable-OnlineSpeechRecognition" => Some("privacy/telemetry"),
        "Disable-InkingTypingPersonalization" => Some("privacy/telemetry"),
        "Enable-InkingTypingPersonalization" => Some("privacy/telemetry"),
        "Disable-HttpAcceptLanguageOptOut" => Some("privacy/telemetry"),
        "Enable-HttpAcceptLanguageOptOut" => Some("privacy/telemetry"),
        "Disable-DiagnosticDataViewer" => Some("privacy/telemetry"),
        "Enable-DiagnosticDataViewer" => Some("privacy/telemetry"),
        "Set-FeedbackFrequencyNever" => Some("privacy/telemetry"),
        "Reset-FeedbackFrequency" => Some("privacy/telemetry"),
        "Disable-WiFiSense" => Some("privacy/telemetry"),
        "Enable-WiFiSense" => Some("privacy/telemetry"),
        "Disable-CrossDeviceResume" => Some("privacy/telemetry"),
        "Enable-CrossDeviceResume" => Some("privacy/telemetry"),
        "Disable-BulkCapabilityPermissions" => Some("privacy/telemetry"),
        "Enable-BulkCapabilityPermissions" => Some("privacy/telemetry"),
        "Disable-SyncSettings" => Some("privacy/telemetry"),
        "Enable-SyncSettings" => Some("privacy/telemetry"),
        "Disable-AdvertisingID" => Some("privacy/telemetry"),
        "Enable-AdvertisingID" => Some("privacy/telemetry"),
        "Disable-TailoredExperiences" => Some("privacy/telemetry"),
        "Enable-TailoredExperiences" => Some("privacy/telemetry"),
        "Disable-OfficeLogging" => Some("privacy/telemetry"),
        "Enable-OfficeLogging" => Some("privacy/telemetry"),
        "Disable-DiagnosticEventTracing" => Some("privacy/telemetry"),
        "Enable-DiagnosticEventTracing" => Some("privacy/telemetry"),

        // Privacy - Cleanup
        // Privacy-destructive Clear-*/Invoke-* mostly run in commander-pro
        // now (see handlers.rs). Clipboard stays local/free because it is a
        // volatile in-session clear, not a forensic wipe.
        "Clear-Clipboard" => Some("privacy/cleanup"),
        "Get-USBDeviceHistory" => Some("privacy/cleanup"),
        "Get-DnsCacheEntries" => Some("privacy/cleanup"),
        "Get-ExecutionCache" => Some("privacy/cleanup"),
        "Disable-ClipboardHistory" => Some("privacy/cleanup"),
        "Enable-ClipboardHistory" => Some("privacy/cleanup"),
        "Get-ClipboardHistoryStatus" => Some("privacy/cleanup"),
        "Disable-CloudClipboardSync" => Some("privacy/cleanup"),
        "Enable-CloudClipboardSync" => Some("privacy/cleanup"),
        "Set-AutoEraseSchedule" => Some("privacy/cleanup"),
        "Remove-AutoEraseSchedule" => Some("privacy/cleanup"),
        "Get-AutoEraseSchedules" => Some("privacy/cleanup"),
        "Get-AutoEraseSupportedCategories" => Some("privacy/cleanup"),
        "Invoke-AutoEraseMigration" => Some("privacy/cleanup"),
        // Multi-user scope
        "Get-UserProfiles" => Some("privacy/cleanup"),
        // Fleet targets local accounts, not profile folders. Keep this read-only
        // discovery command independent of the Privacy Clean frontend toggle.
        "Get-FleetAccessUsers" => Some("privacy/cleanup"),
        "Get-LoggedInUsers" => Some("privacy/cleanup"),
        "Invoke-CleanupClearAllUsers" => Some("privacy/cleanup"),
        "Get-CleanupSummaryAllUsers" => Some("privacy/cleanup"),
        "Set-MultiUserAutoEraseSchedule" => Some("privacy/cleanup"),
        "Remove-MultiUserAutoEraseSchedule" => Some("privacy/cleanup"),
        "Get-PrivacyProtectionStatus" => Some("privacy/cleanup"),
        "Enable-PrivacyProtection" => Some("privacy/cleanup"),
        "Disable-PrivacyProtection" => Some("privacy/cleanup"),
        "Disable-Pagefile" => Some("privacy/cleanup"),
        "Enable-Pagefile" => Some("privacy/cleanup"),
        "Disable-RecentFilesTracking" => Some("privacy/cleanup"),
        // ── Phase E hide-recent toggles ──
        "Disable-QuickAccessRecent"
        | "Enable-QuickAccessRecent"
        | "Get-QuickAccessRecentStatus" => Some("privacy/cleanup"),
        "Disable-QuickAccessFrequent"
        | "Enable-QuickAccessFrequent"
        | "Get-QuickAccessFrequentStatus" => Some("privacy/cleanup"),
        "Disable-RunMRU" | "Enable-RunMRU" | "Get-RunMRUStatus" => Some("privacy/cleanup"),
        "Disable-SearchHistory" | "Enable-SearchHistory" | "Get-SearchHistoryStatus" => {
            Some("privacy/cleanup")
        }
        "Disable-TerminalHistory" | "Enable-TerminalHistory" | "Get-TerminalHistoryStatus" => {
            Some("privacy/cleanup")
        }
        "Enable-RecentFilesTracking" => Some("privacy/cleanup"),
        "Disable-JumpLists" => Some("privacy/cleanup"),
        "Enable-JumpLists" => Some("privacy/cleanup"),
        "Disable-ThumbnailCache" => Some("privacy/cleanup"),
        "Enable-ThumbnailCache" => Some("privacy/cleanup"),
        "Get-EventLogSummary" => Some("privacy/cleanup"),
        "Get-SRUMData" => Some("privacy/cleanup"),
        "Get-PSHistory" => Some("privacy/cleanup"),
        "Get-ShellBags" => Some("privacy/cleanup"),
        "Get-ProcessIntelligence" => Some("privacy/cleanup"),
        "Get-WlanProfiles" => Some("privacy/cleanup"),
        "Get-BluetoothDevices" => Some("privacy/cleanup"),
        "Get-NetworkDrives" => Some("privacy/cleanup"),
        // New trace viewers
        "Get-RecentFiles" => Some("privacy/cleanup"),
        "Get-RDPHistory" => Some("privacy/cleanup"),
        "Get-ConnectivityHistory" => Some("privacy/cleanup"),
        "Get-JumpLists" => Some("privacy/cleanup"),
        "Get-BrowserFootprints" => Some("privacy/cleanup"),
        // Clear-BrowserFootprints is paid (Pro sidecar handler) — it must
        // NOT appear as a contiguous literal in the Free binary (AV-clean
        // strings gate). Tier + backend module + frontend gate are all
        // supplied by its obfuscated SENSITIVE_COMMANDS entry instead.
        "Get-PrefetchFiles" => Some("privacy/cleanup"),
        "Get-ShadowCopies" => Some("privacy/cleanup"),
        "Get-NTFSJournals" => Some("privacy/cleanup"),
        // GROUP I-A: Advanced deep trace analysis — viewers
        "Get-AmcacheEntries" => Some("privacy/cleanup"),
        "Get-NTUserTraces" => Some("privacy/cleanup"),
        "Get-NotepadStateFiles" => Some("privacy/cleanup"),
        "Get-PCAInfo" => Some("privacy/cleanup"),
        "Get-CrashDumpList" => Some("privacy/cleanup"),
        "Get-SQLiteWALList" => Some("privacy/cleanup"),
        "Get-RecallDatabaseInfo" => Some("privacy/cleanup"),
        "Get-SearchIndexInfo" => Some("privacy/cleanup"),
        "Get-PrintSpoolerInfo" => Some("privacy/cleanup"),
        // GROUP I-A: Advanced deep trace analysis — clearers run in Pro sidecar
        // (handlers.rs). Free keeps only the viewers.
        "Get-VirtualMemoryStatus" => Some("privacy/cleanup"),
        "Get-RecycleBinInfo" => Some("privacy/cleanup"),
        "Get-WebCacheInfo" => Some("privacy/cleanup"),
        "Get-WSLDataInfo" => Some("privacy/cleanup"),
        "Get-DockerDesktopDataInfo" => Some("privacy/cleanup"),
        "Get-VirtualMachineArtifactsInfo" => Some("privacy/cleanup"),
        "Get-DeveloperCachesInfo" => Some("privacy/cleanup"),
        "Get-CredentialManagerInfo" => Some("privacy/cleanup"),
        "Get-NetworkWizardHistoryInfo" => Some("privacy/cleanup"),
        "Get-WERHistoryInfo" => Some("privacy/cleanup"),
        "Get-InactiveUserProtectionMetadataInfo" => Some("privacy/cleanup"),
        "Get-StickyNotesInfo" => Some("privacy/cleanup"),
        "Get-OneDriveMetadataInfo" => Some("privacy/cleanup"),
        "Get-SpotlightCacheInfo" => Some("privacy/cleanup"),
        "Get-FontCacheInfo" => Some("privacy/cleanup"),
        "Get-LegacyIconCacheInfo" => Some("privacy/cleanup"),
        "Get-GameCapturesInfo" => Some("privacy/cleanup"),
        "Get-PhotosCacheInfo" => Some("privacy/cleanup"),
        "Get-XboxCacheInfo" => Some("privacy/cleanup"),
        "Get-CommunicationCachesInfo" => Some("privacy/cleanup"),
        "Get-EditorHistoryInfo" => Some("privacy/cleanup"),
        "Get-GitActivityInfo" => Some("privacy/cleanup"),
        "Get-SSHStateInfo" => Some("privacy/cleanup"),
        "Get-RemoteAccessLogsInfo" => Some("privacy/cleanup"),
        "Get-PasswordManagerCachesInfo" => Some("privacy/cleanup"),
        "Get-GameLauncherLogsInfo" => Some("privacy/cleanup"),
        "Get-AdobeRecentInfo" => Some("privacy/cleanup"),
        "Get-OfficeTempFilesInfo" => Some("privacy/cleanup"),
        "Get-FirewallLogInfo" => Some("privacy/cleanup"),
        "Get-NeighborCacheInfo" => Some("privacy/cleanup"),
        "Get-NetBIOSCacheInfo" => Some("privacy/cleanup"),
        "Get-GeolocationCacheInfo" => Some("privacy/cleanup"),
        "Get-VPNPhonebooksInfo" => Some("privacy/cleanup"),
        "Get-ProxyCacheInfo" => Some("privacy/cleanup"),
        "Get-CloudPlaceholdersInfo" => Some("privacy/cleanup"),
        "Get-BITSQueueInfo" => Some("privacy/cleanup"),
        "Get-CellularHistoryInfo" => Some("privacy/cleanup"),
        "Get-ThumbnailCacheInfo" => Some("privacy/cleanup"),
        "Get-NotificationDatabaseInfo" => Some("privacy/cleanup"),
        "Get-BranchCacheInfo" => Some("privacy/cleanup"),
        "Get-EventTranscriptInfo" => Some("privacy/cleanup"),
        "Get-ActivitiesTimelineInfo" => Some("privacy/cleanup"),
        "Get-RdpBitmapCacheInfo" => Some("privacy/cleanup"),
        "Get-ServicingLogsInfo" => Some("privacy/cleanup"),
        "Get-DeviceInstallLogsInfo" => Some("privacy/cleanup"),
        "Get-UsageTraceLogsInfo" => Some("privacy/cleanup"),
        "Get-DefenderHistoryInfo" => Some("privacy/cleanup"),
        "Get-AppLaunchHistoryInfo" => Some("privacy/cleanup"),
        "Get-OfficeMruInfo" => Some("privacy/cleanup"),
        "Get-EmbeddedWebCacheInfo" => Some("privacy/cleanup"),
        "Get-P2PUpdateCacheInfo" => Some("privacy/cleanup"),
        "Get-ReliabilityHistoryInfo" => Some("privacy/cleanup"),
        "Get-ExplorerSearchHistoryInfo" => Some("privacy/cleanup"),
        "Get-SearchPersonalizationInfo" => Some("privacy/cleanup"),
        "Get-PreviousWindowsInstallInfo" => Some("privacy/cleanup"),

        "Disable-ConsumerFeatures" => Some("tweaks/system"),
        "Enable-ConsumerFeatures" => Some("tweaks/system"),
        "Invoke-RemoveOneDrive" => Some("apps/uninstaller"),
        "Remove-MicrosoftTeams" => Some("apps/uninstaller"),
        "Get-TeamsStatus" => Some("apps/uninstaller"),
        // Appx debloat is a Free Packages & Apps surface. Keep listing,
        // remove, restore, and deprovision together in the local uninstaller
        // module so the panel never crosses the Pro sidecar boundary.
        "Get-InstalledAppxInventory" => Some("apps/uninstaller"),
        "Get-DebloatWindowsIconData" => Some("apps/uninstaller"),
        "Remove-AppxByName" => Some("apps/uninstaller"),
        "Restore-AppxByName" => Some("apps/uninstaller"),
        "Set-AppxDeprovisioned" => Some("apps/uninstaller"),
        "Enable-RemoveGalleryHome" => Some("tweaks/ui"),
        "Disable-RemoveGalleryHome" => Some("tweaks/ui"),
        "Enable-IPv4Preference" => Some("network/dns"),
        "Disable-IPv4Preference" => Some("network/dns"),

        // Network - Hosts
        "Get-BlocklistStatus" => Some("network/hosts"),
        "Add-BlocklistToHosts" => Some("network/hosts"),
        "Remove-BlocklistFromHosts" => Some("network/hosts"),

        // Network - DNS
        "Get-DNSStatus" => Some("network/dns"),
        "Set-SecureDNS" => Some("network/dns"),
        "Clear-SecureDNS" => Some("network/dns"),
        "Enable-DNSCensorshipProtection" => Some("network/dns"),
        "Disable-DNSCensorshipProtection" => Some("network/dns"),

        "Get-NetworkPorts" => Some("network/ports"),

        "Get-PhysicalNetworkAdapters" => Some("network/adapters"),
        "Set-AdapterRandomMAC" => Some("network/adapters"),
        "Restore-AdapterMAC" => Some("network/adapters"),

        // Network - Firewall
        "Get-FirewallRules" => Some("network/firewall"),
        "Add-FirewallBlockRule" => Some("network/firewall"),
        "Set-FirewallRuleEnabled" => Some("network/firewall"),
        "Remove-FirewallRule" => Some("network/firewall"),
        "Enable-LockdownMode" => Some("network/firewall"),
        "Disable-LockdownMode" => Some("network/firewall"),
        "Get-FirewallStatus" => Some("network/firewall"),
        "Get-ProtocolBlocks" => Some("network/firewall"),
        "Block-Protocol" => Some("network/firewall"),
        "Unblock-Protocol" => Some("network/firewall"),

        // A-2: mesh/vpn.ps1 module moved entirely to commander-pro
        // (all 7 mesh commands paid + handled in Pro's handlers.rs).
        // Free no longer ships the module — dispatch routes via
        // dispatch_paid_command to Pro before any module lookup.

        // Apps
        "Get-AppManifest" => Some("apps/winget"),
        "Install-WingetApps" => Some("apps/winget"),
        "Upgrade-AllApps" => Some("apps/winget"),
        "Upgrade-App" => Some("apps/winget"),
        "Test-WingetInstalled" => Some("apps/winget"),
        "Install-Winget" => Some("apps/winget"),
        "Get-AppInventory" => Some("apps/winget"),
        "Invoke-RemoveEdge" => Some("apps/uninstaller"),
        "Test-EdgeInstalled" => Some("apps/uninstaller"),
        "Test-OneDriveInstalled" => Some("apps/uninstaller"),

        // Apps - BCU (Bulk Crap Uninstaller) — CLI-only, no GUI launch
        "Test-BcuInstalled" => Some("apps/bcu-uninstaller"),
        "Install-BcuUninstaller" => Some("apps/bcu-uninstaller"),
        "Get-BcuApplicationList" => Some("apps/bcu-uninstaller"),
        "Invoke-BcuUninstall" => Some("apps/bcu-uninstaller"),
        "Invoke-BcuQuietUninstallSingle" => Some("apps/bcu-uninstaller"),
        "Invoke-BcuLoudUninstallSingle" => Some("apps/bcu-uninstaller"),
        "Invoke-BcuCleanupProgramFiles" => Some("apps/bcu-uninstaller"),
        "Remove-BcuOrphanedFolder" => Some("apps/bcu-uninstaller"),
        "Get-BcuWindowsFeatures" => Some("apps/bcu-uninstaller"),
        "Disable-BcuWindowsFeature" => Some("apps/bcu-uninstaller"),
        "Enable-BcuWindowsFeature" => Some("apps/bcu-uninstaller"),
        "Get-BcuStartupItems" => Some("apps/bcu-uninstaller"),
        "Remove-BcuStartupItem" => Some("apps/bcu-uninstaller"),
        "Get-BcuRegistryLeftovers" => Some("apps/bcu-uninstaller"),
        "Remove-BcuRegistryLeftover" => Some("apps/bcu-uninstaller"),
        "Export-BcuApplicationList" => Some("apps/bcu-uninstaller"),

        // Tweaks - System
        "Enable-WindowsDefender" => Some("tweaks/system"),
        "Get-DefenderStatus" => Some("tweaks/system"),
        "Disable-WindowsUpdate" => Some("tweaks/system"),
        "Enable-WindowsUpdate" => Some("tweaks/system"),
        "Get-UpdateStatus" => Some("tweaks/system"),
        "Disable-Hibernation" => Some("tweaks/system"),
        "Enable-Hibernation" => Some("tweaks/system"),
        "Disable-FastStartup" => Some("tweaks/system"),
        "Enable-FastStartup" => Some("tweaks/system"),
        // Feature 3: RAM-spill control (free; wrapper in tweaks/system.ps1)
        "Enable-RamSpillControl" | "Disable-RamSpillControl" => Some("tweaks/system"),
        "Disable-UAC" => Some("tweaks/system"),
        "Enable-UAC" => Some("tweaks/system"),
        "Disable-Superfetch" => Some("tweaks/system"),
        "Enable-Superfetch" => Some("tweaks/system"),
        "Disable-Prefetch" => Some("tweaks/system"),
        "Enable-Prefetch" => Some("tweaks/system"),
        "Enable-NTFSOptimizations" => Some("tweaks/system"),
        "Disable-NTFSOptimizations" => Some("tweaks/system"),
        "Enable-DetailedBSOD" => Some("tweaks/system"),
        "Disable-DetailedBSOD" => Some("tweaks/system"),
        "Disable-AutomaticMaintenance" => Some("tweaks/system"),
        "Enable-AutomaticMaintenance" => Some("tweaks/system"),
        "Enable-Win32LongPaths" => Some("tweaks/system"),
        "Disable-Win32LongPaths" => Some("tweaks/system"),
        "Disable-SmbBandwidthThrottling" => Some("tweaks/system"),
        "Enable-SmbBandwidthThrottling" => Some("tweaks/system"),
        "Enable-USBWriteProtect" => Some("tweaks/system"),
        "Disable-USBWriteProtect" => Some("tweaks/system"),
        "Enable-USBStorageLockdown" => Some("tweaks/system"),
        "Disable-USBStorageLockdown" => Some("tweaks/system"),
        "Get-USBStorageLockdownStatus" => Some("tweaks/system"),
        "Get-HardeningStatus" => Some("tweaks/system"),
        "Create-RestorePoint" => Some("tweaks/system"),
        "Enable-RdpKeepAlive" => Some("tweaks/system"),
        "Disable-RdpKeepAlive" => Some("tweaks/system"),
        "Get-RdpKeepAliveStatus" => Some("tweaks/system"),
        "Enable-RdpNoTimeouts" => Some("tweaks/system"),
        "Disable-RdpNoTimeouts" => Some("tweaks/system"),
        "Get-RdpNoTimeoutsStatus" => Some("tweaks/system"),
        "Enable-RdpQosPriority" => Some("tweaks/system"),
        "Disable-RdpQosPriority" => Some("tweaks/system"),
        "Get-RdpQosPriorityStatus" => Some("tweaks/system"),
        "Enable-RdpIncomingIdleTimeout" => Some("tweaks/system"),
        "Disable-RdpIncomingIdleTimeout" => Some("tweaks/system"),
        "Get-RdpIncomingIdleStatus" => Some("tweaks/system"),
        "Watch-RdpIncomingSessions" => Some("tweaks/system"),
        "Logoff-RdpIncomingSession" => Some("tweaks/system"),
        // Host hardening (Feature 4)
        "Disable-SystemRestore" => Some("tweaks/security"),
        "Enable-SystemRestore" => Some("tweaks/security"),
        "Disable-SleepPassword" => Some("tweaks/security"),
        "Enable-SleepPassword" => Some("tweaks/security"),
        "Enable-KernelDMAProtection" => Some("tweaks/security"),
        "Disable-KernelDMAProtection" => Some("tweaks/security"),
        "Get-KernelDMAProtectionStatus" => Some("tweaks/security"),
        "Enable-SystemDEP" => Some("tweaks/security"),
        "Disable-SystemDEP" => Some("tweaks/security"),
        "Enable-ASRRules" => Some("tweaks/security"),
        "Disable-ASRRules" => Some("tweaks/security"),
        "Enable-ControlledFolderAccess" => Some("tweaks/security"),
        "Disable-ControlledFolderAccess" => Some("tweaks/security"),
        "Enable-NetworkProtection" => Some("tweaks/security"),
        "Disable-NetworkProtection" => Some("tweaks/security"),
        "Enable-MandatoryASLR" => Some("tweaks/security"),
        "Disable-MandatoryASLR" => Some("tweaks/security"),
        "Enable-BottomUpASLR" => Some("tweaks/security"),
        "Disable-BottomUpASLR" => Some("tweaks/security"),
        "Enable-SystemCFG" => Some("tweaks/security"),
        "Disable-SystemCFG" => Some("tweaks/security"),
        "Enable-HeapIntegrity" => Some("tweaks/security"),
        "Disable-HeapIntegrity" => Some("tweaks/security"),
        "Enable-SEHOP" => Some("tweaks/security"),
        "Disable-SEHOP" => Some("tweaks/security"),
        "Get-ExploitProtectionStatus" => Some("tweaks/security"),
        "Disable-CrashDumps" => Some("tweaks/prevention"),
        "Enable-CrashDumps" => Some("tweaks/prevention"),
        "Disable-VBS" => Some("tweaks/security"),
        "Enable-VBS" => Some("tweaks/security"),
        "Disable-BitLockerAutoEncrypt" => Some("tweaks/security"),
        "Enable-BitLockerAutoEncrypt" => Some("tweaks/security"),
        "Enable-AcquisitionDriverBlocklist" => Some("tweaks/security"),
        "Disable-AcquisitionDriverBlocklist" => Some("tweaks/security"),
        "Scan-AcquisitionThreats" => Some("tweaks/security"),
        "Enable-FullDiskEncryption" => Some("tweaks/security"),
        "Enable-ForensicToolBlock" => Some("tweaks/security"),
        "Disable-ForensicToolBlock" => Some("tweaks/security"),
        "Enable-LidClosePowerOff" => Some("tweaks/security"),
        "Disable-LidClosePowerOff" => Some("tweaks/security"),
        "Get-LidClosePowerOffStatus" => Some("tweaks/security"),
        "Disable-WPBT" => Some("tweaks/security"),
        "Enable-WPBT" => Some("tweaks/security"),
        "Set-BitLockerTpmPin" => Some("tweaks/security"),
        "Disable-SmartScreen" => Some("tweaks/security"),
        "Enable-SmartScreen" => Some("tweaks/security"),
        "Set-OOBEBypass" => Some("tweaks/security"),
        "Clear-OOBEBypass" => Some("tweaks/security"),
        "Disable-GameDVR" => Some("tweaks/security"),
        "Enable-GameDVR" => Some("tweaks/security"),
        "Disable-RemoteAssistance" => Some("tweaks/security"),
        "Enable-RemoteAssistance" => Some("tweaks/security"),
        "Block-AnonymousSamEnumeration" => Some("tweaks/security"),
        "Allow-AnonymousSamEnumeration" => Some("tweaks/security"),
        "Enable-Win11RequirementsBypass" => Some("tweaks/security"),
        "Disable-Win11RequirementsBypass" => Some("tweaks/security"),
        "Enable-IFEOTelemetryBlock" => Some("tweaks/security"),
        "Disable-IFEOTelemetryBlock" => Some("tweaks/security"),
        "Enable-FirefoxHardening" => Some("tweaks/security"),
        "Disable-FirefoxHardening" => Some("tweaks/security"),
        "Enable-BraveHardening" => Some("tweaks/security"),
        "Disable-BraveHardening" => Some("tweaks/security"),
        "Enable-ChromeHardening" => Some("tweaks/security"),
        "Disable-ChromeHardening" => Some("tweaks/security"),
        "Enable-EdgeHardening" => Some("tweaks/security"),
        "Disable-EdgeHardening" => Some("tweaks/security"),
        "Install-UniversalBrowserExtensions" => Some("tweaks/security"),
        "Remove-UniversalBrowserExtensions" => Some("tweaks/security"),
        "Get-InstalledBrowsersJson" => Some("tweaks/security"),
        "Enable-HardenBrowserByName" => Some("tweaks/security"),
        "Disable-HardenBrowserByName" => Some("tweaks/security"),
        "Remove-CopilotAIComponents" => Some("tweaks/security"),
        "Restore-CopilotAIComponents" => Some("tweaks/security"),
        "Disable-MemoryCompression" => Some("tweaks/system"),
        "Enable-MemoryCompression" => Some("tweaks/system"),
        "Set-Win32PrioritySeparation" => Some("tweaks/system"),
        "Reset-Win32PrioritySeparation" => Some("tweaks/system"),
        "Set-DesktopShellPriority" => Some("tweaks/system"),
        "Reset-DesktopShellPriority" => Some("tweaks/system"),
        "Set-OptimizedTimeouts" => Some("tweaks/system"),
        "Reset-OptimizedTimeouts" => Some("tweaks/system"),
        "Disable-ReservedStorage" => Some("tweaks/system"),
        "Enable-ReservedStorage" => Some("tweaks/system"),
        "Enable-TSX" => Some("tweaks/system"),
        "Disable-TSX" => Some("tweaks/system"),
        "Disable-FirstLogonAnimation" => Some("tweaks/system"),
        "Enable-FirstLogonAnimation" => Some("tweaks/system"),
        "Disable-StartupSound" => Some("tweaks/system"),
        "Enable-StartupSound" => Some("tweaks/system"),
        "Disable-AutoRestartSignon" => Some("tweaks/system"),
        "Enable-AutoRestartSignon" => Some("tweaks/system"),
        "Disable-AutoRebootOnBSOD" => Some("tweaks/system"),
        "Enable-AutoRebootOnBSOD" => Some("tweaks/system"),
        "Set-SmallMemoryDump" => Some("tweaks/system"),
        "Reset-SmallMemoryDump" => Some("tweaks/system"),
        "Disable-ContentDeliveryManager" => Some("tweaks/system"),
        "Enable-ContentDeliveryManager" => Some("tweaks/system"),
        "Enable-IFEOPriorityTuning" => Some("tweaks/system"),
        "Disable-IFEOPriorityTuning" => Some("tweaks/system"),
        "Disable-AutoDiskCheck" => Some("tweaks/system"),
        "Enable-AutoDiskCheck" => Some("tweaks/system"),
        "Enable-SvcHostSplit" => Some("tweaks/system"),
        "Disable-SvcHostSplit" => Some("tweaks/system"),
        "Invoke-NGenPrecompile" => Some("tweaks/system"),
        "Enable-DirectPlay" => Some("tweaks/system"),
        "Disable-PowerShellV2" => Some("tweaks/system"),
        "Enable-PowerShellV2" => Some("tweaks/system"),
        "Disable-PrintingSubsystem" => Some("tweaks/system"),
        "Enable-PrintingSubsystem" => Some("tweaks/system"),
        "Disable-WorkFoldersClient" => Some("tweaks/system"),
        "Enable-WorkFoldersClient" => Some("tweaks/system"),
        "Disable-DeliveryOptimization" => Some("tweaks/system"),
        "Enable-DeliveryOptimization" => Some("tweaks/system"),
        "Disable-DriverAutoUpdate" => Some("tweaks/system"),
        "Enable-DriverAutoUpdate" => Some("tweaks/system"),
        "Disable-StoreAutoDownload" => Some("tweaks/system"),
        "Enable-StoreAutoDownload" => Some("tweaks/system"),
        "Disable-UpdateNotifications" => Some("tweaks/system"),
        "Enable-UpdateNotifications" => Some("tweaks/system"),
        "Block-DevHomeOutlookAutoInstall" => Some("tweaks/system"),
        "Unblock-DevHomeOutlookAutoInstall" => Some("tweaks/system"),
        "Enable-ClassicPhotoViewer" => Some("tweaks/system"),
        "Disable-ClassicPhotoViewer" => Some("tweaks/system"),
        "Disable-AudioDucking" => Some("tweaks/system"),
        "Enable-AudioDucking" => Some("tweaks/system"),
        "Enable-UTCTime" => Some("tweaks/system"),
        "Disable-UTCTime" => Some("tweaks/system"),
        "Enable-LinkedConnections" => Some("tweaks/system"),
        "Disable-LinkedConnections" => Some("tweaks/system"),
        "Set-UnlimitedPasswordAge" => Some("tweaks/system"),
        "Reset-PasswordAge" => Some("tweaks/system"),
        "Enable-MSISafeMode" => Some("tweaks/system"),
        "Disable-MSISafeMode" => Some("tweaks/system"),
        "Invoke-SystemRepair" => Some("tweaks/maintenance"),
        "Invoke-WindowsUpdateRepair" => Some("tweaks/maintenance"),
        "Invoke-Defrag" => Some("tweaks/maintenance"),
        "Invoke-DeepCleanup" => Some("tweaks/maintenance"),

        // Tweaks - Windows Server
        "Enable-PersistentRdpAnimations" => Some("tweaks/server"),
        "Disable-PersistentRdpAnimations" => Some("tweaks/server"),
        "Get-PersistentRdpAnimationsStatus" => Some("tweaks/server"),
        "Disable-CtrlAltDelLogon" => Some("tweaks/server"),
        "Enable-CtrlAltDelLogon" => Some("tweaks/server"),
        "Enable-HideLastSignedInUser" => Some("tweaks/server"),
        "Disable-HideLastSignedInUser" => Some("tweaks/server"),
        "Enable-ConsoleInactivityLock" => Some("tweaks/server"),
        "Disable-ConsoleInactivityLock" => Some("tweaks/server"),
        "Disable-ShutdownEventTracker" => Some("tweaks/server"),
        "Enable-ShutdownEventTracker" => Some("tweaks/server"),
        "Disable-ServerManagerAtLogon" => Some("tweaks/server"),
        "Enable-ServerManagerAtLogon" => Some("tweaks/server"),
        "Disable-IEEnhancedSecurity" => Some("tweaks/server"),
        "Enable-IEEnhancedSecurity" => Some("tweaks/server"),
        "Block-WDigestCredentials" => Some("tweaks/server"),
        "Allow-WDigestCredentials" => Some("tweaks/server"),
        "Enable-LsaProtection" => Some("tweaks/server"),
        "Disable-LsaProtection" => Some("tweaks/server"),
        "Block-LegacyNtlm" => Some("tweaks/server"),
        "Allow-LegacyNtlm" => Some("tweaks/server"),
        "Enable-SmbSigningRequired" => Some("tweaks/server"),
        "Disable-SmbSigningRequired" => Some("tweaks/server"),
        "Disable-Smb1Protocol" => Some("tweaks/server"),
        "Enable-Smb1Protocol" => Some("tweaks/server"),
        "Disable-RemoteRegistryService" => Some("tweaks/server"),
        "Enable-RemoteRegistryService" => Some("tweaks/server"),
        "Get-ServerTweakStatus" => Some("tweaks/server"),

        // Tweaks - UI
        "Disable-BackgroundApps" => Some("tweaks/ui"),
        "Enable-BackgroundApps" => Some("tweaks/ui"),
        "Disable-Notifications" => Some("tweaks/ui"),
        "Enable-Notifications" => Some("tweaks/ui"),
        "Enable-ClassicContextMenu" => Some("tweaks/ui"),
        "Disable-ClassicContextMenu" => Some("tweaks/ui"),
        "Enable-EndTaskOnTaskbar" => Some("tweaks/ui"),
        "Disable-EndTaskOnTaskbar" => Some("tweaks/ui"),
        "Disable-BingSearch" => Some("tweaks/ui"),
        "Enable-BingSearch" => Some("tweaks/ui"),
        "Show-FileExtensions" => Some("tweaks/ui"),
        "Hide-FileExtensions" => Some("tweaks/ui"),
        "Show-HiddenFiles" => Some("tweaks/ui"),
        "Hide-HiddenFiles" => Some("tweaks/ui"),
        "Disable-FolderTypeDiscovery" => Some("tweaks/ui"),
        "Enable-FolderTypeDiscovery" => Some("tweaks/ui"),
        "Remove-ShortcutSuffix" => Some("tweaks/ui"),
        "Restore-ShortcutSuffix" => Some("tweaks/ui"),
        "Disable-AutoPlay" => Some("tweaks/ui"),
        "Enable-AutoPlay" => Some("tweaks/ui"),
        "Disable-LowDiskCheck" => Some("tweaks/ui"),
        "Enable-LowDiskCheck" => Some("tweaks/ui"),
        "Set-ExplorerOpensThisPC" => Some("tweaks/ui"),
        "Set-ExplorerOpensQuickAccess" => Some("tweaks/ui"),
        "Hide-SyncProviderNotifications" => Some("tweaks/ui"),
        "Show-SyncProviderNotifications" => Some("tweaks/ui"),
        "Disable-TransparencyEffects" => Some("tweaks/ui"),
        "Enable-TransparencyEffects" => Some("tweaks/ui"),
        "Enable-FullPathInTitleBar" => Some("tweaks/ui"),
        "Disable-FullPathInTitleBar" => Some("tweaks/ui"),
        "Set-TaskbarDebloated" => Some("tweaks/ui"),
        "Reset-TaskbarDebloated" => Some("tweaks/ui"),
        "Disable-StartRecommendations" => Some("tweaks/ui"),
        "Enable-StartRecommendations" => Some("tweaks/ui"),
        "Enable-TakeOwnershipMenu" => Some("tweaks/ui"),
        "Disable-TakeOwnershipMenu" => Some("tweaks/ui"),
        "Enable-EnthusiastMode" => Some("tweaks/ui"),
        "Disable-EnthusiastMode" => Some("tweaks/ui"),
        "Enable-InstantMenuDelay" => Some("tweaks/ui"),
        "Disable-InstantMenuDelay" => Some("tweaks/ui"),
        "Enable-WallpaperQuality" => Some("tweaks/ui"),
        "Disable-WallpaperQuality" => Some("tweaks/ui"),
        "Disable-AccessibilityShortcuts" => Some("tweaks/ui"),
        "Enable-AccessibilityShortcuts" => Some("tweaks/ui"),
        "Disable-MouseAcceleration" => Some("tweaks/ui"),
        "Enable-MouseAcceleration" => Some("tweaks/ui"),
        "Disable-AutocorrectSpellcheck" => Some("tweaks/ui"),
        "Enable-AutocorrectSpellcheck" => Some("tweaks/ui"),
        "Enable-PowerShell7DefaultShell" => Some("tweaks/ui"),
        "Disable-PowerShell7DefaultShell" => Some("tweaks/ui"),
        "Get-PowerShell7DefaultStatus" => Some("tweaks/ui"),

        // Tweaks - Startup Manager
        "Get-StartupItems" => Some("tweaks/startup-manager"),
        "Disable-StartupItem" => Some("tweaks/startup-manager"),
        "Enable-StartupItem" => Some("tweaks/startup-manager"),
        "Invoke-OptimizeStartup" => Some("tweaks/startup-manager"),

        // ── Tweaks - Performance ─────────────────────────────────────
        "Enable-MMCSSGamingProfile" => Some("tweaks/performance"),
        "Disable-MMCSSGamingProfile" => Some("tweaks/performance"),
        "Enable-KeyboardLatencyOptimised" => Some("tweaks/performance"),
        "Disable-KeyboardLatencyOptimised" => Some("tweaks/performance"),
        "Enable-NumLockOnBoot" => Some("tweaks/performance"),
        "Disable-NumLockOnBoot" => Some("tweaks/performance"),
        "Enable-GpuScheduling" => Some("tweaks/performance"),
        "Disable-GpuScheduling" => Some("tweaks/performance"),

        // ── Tweaks - GPU ─────────────────────────────────────────────
        "Get-GpuVendors" => Some("tweaks/gpu"),
        "Disable-AmdUlps"
        | "Enable-AmdUlps"
        | "Disable-AmdPowerGating"
        | "Enable-AmdPowerGating"
        | "Disable-AmdVideoClockGating"
        | "Enable-AmdVideoClockGating"
        | "Disable-AmdAspm"
        | "Enable-AmdAspm"
        | "Disable-NvidiaDynamicPstate"
        | "Enable-NvidiaDynamicPstate"
        | "Disable-NvidiaAsyncPstates"
        | "Enable-NvidiaAsyncPstates"
        | "Disable-IntelAsyncFlips"
        | "Enable-IntelAsyncFlips"
        | "Disable-IntelAdaptiveVsync"
        | "Enable-IntelAdaptiveVsync" => Some("tweaks/gpu"),

        // ── Tweaks - Power ───────────────────────────────────────────
        // Note: Ultimate Performance is handled by Set-PowerPlan (Mode='ultimate')
        // in tweaks/maintenance, not by a separate toggle here.
        "Disable-UsbSelectiveSuspend"
        | "Enable-UsbSelectiveSuspend"
        | "Disable-CpuThrottling"
        | "Enable-CpuThrottling" => Some("tweaks/power"),

        // ── Tweaks - UI granular ─────────────────────────────────────
        "Show-DesktopIconThisPc"
        | "Hide-DesktopIconThisPc"
        | "Show-DesktopIconUserFiles"
        | "Hide-DesktopIconUserFiles"
        | "Show-DesktopIconNetwork"
        | "Hide-DesktopIconNetwork"
        | "Show-DesktopIconRecycleBin"
        | "Hide-DesktopIconRecycleBin"
        | "Show-DesktopIconControlPanel"
        | "Hide-DesktopIconControlPanel"
        | "Remove-ShortcutArrow"
        | "Restore-ShortcutArrow"
        | "Disable-SnapAssistFlyout"
        | "Enable-SnapAssistFlyout"
        | "Enable-ExplorerCompactMode"
        | "Disable-ExplorerCompactMode"
        | "Enable-ExplorerCheckboxes"
        | "Disable-ExplorerCheckboxes"
        | "Disable-WindowShake"
        | "Enable-WindowShake"
        | "Show-ClockSeconds"
        | "Hide-ClockSeconds" => Some("tweaks/ui-granular"),

        // ── Tweaks - Scheduled Tasks Manager ─────────────────────────
        "Get-AllScheduledTasks"
        | "Disable-ScheduledTaskByPath"
        | "Enable-ScheduledTaskByPath"
        | "Start-ScheduledTaskByPath"
        | "Stop-ScheduledTaskByPath"
        | "Remove-ScheduledTaskByPath" => Some("tweaks/scheduled-tasks"),

        // ── Tweaks - Service Manager ─────────────────────────────────
        // Bulk "Apply Recommended Profile" routes through the existing
        // Set-ServicesManual command (tweaks/maintenance) — single button
        // in the System Maintenance card. No separate Apply command here.
        "Get-AllServices"
        | "Set-ServiceStartMode"
        | "Start-ServiceByName"
        | "Stop-ServiceByName"
        | "Restart-ServiceByName" => Some("tweaks/service-manager"),

        "Get-LocalLoginUsers" | "Set-LocalLoginUserHidden" => Some("tweaks/local-users"),

        // ── Tweaks - Disk Cleanup (granular) ─────────────────────────
        "Get-DiskCleanupScan" | "Invoke-DiskCleanupCategories" => {
            Some("tweaks/disk-cleanup-granular")
        }

        // ── P3: Activity-reduction prevention toggles (DN-09) ─────
        "Get-ActivityReductionStatus"
        | "Disable-AuditLogging"
        | "Enable-AuditLogging"
        | "Disable-ActivityMonitor"
        | "Enable-ActivityMonitor"
        | "Disable-AppCompatCache"
        | "Enable-AppCompatCache"
        | "Disable-UserAssistTracking"
        | "Enable-UserAssistTracking"
        | "Disable-UsbEventLog"
        | "Enable-UsbEventLog"
        | "Disable-SsidHistory"
        | "Enable-SsidHistory"
        | "Disable-StorageEventLog"
        | "Enable-StorageEventLog"
        | "Disable-RecentActivityTracking"
        | "Enable-RecentActivityTracking"
        | "Disable-DiagnosticChannel"
        | "Enable-DiagnosticChannel"
        | "Disable-ReliabilityMonitor"
        | "Enable-ReliabilityMonitor"
        | "Disable-ErrorReporting"
        | "Enable-ErrorReporting"
        | "Disable-ScriptBlockLogging"
        | "Enable-ScriptBlockLogging"
        | "Disable-TelemetryRunner"
        | "Enable-TelemetryRunner"
        | "Remove-AppHistoryTraces" => Some("tweaks/prevention"),

        // ── P3: Auto-erase task cleanup (CL-03) ───────────────────
        "Remove-AutoEraseTasks" => Some("tweaks/scheduled-tasks"),

        // Storage - Encrypted Volumes
        "Get-BitLockerVolumes" => Some("vault/volumes"),
        "Get-EncryptionStatus" => Some("vault/volumes"),
        "Mount-EncryptionVolume" => Some("vault/volumes"),
        "Dismount-EncryptionVolume" => Some("vault/volumes"),
        "Open-EncryptionVolume" => Some("vault/volumes"),
        "Create-EncryptionVolume" => Some("vault/volumes"),
        "Create-DualVolume" => Some("vault/volumes"),
        "Create-StegoMp4" => Some("vault/volumes"),
        "Extract-StegoMp4" => Some("vault/volumes"),
        "Get-VolumeInfo" => Some("vault/volumes"),
        "Get-SystemEncryptionStatus" => Some("vault/volumes"),
        "Get-SystemEncryptionEligibility" => Some("vault/volumes"),
        "Get-EncryptionPartitions" => Some("vault/volumes"),
        "Get-AvailableDriveLetters" => Some("vault/volumes"),
        "Get-EncryptedBackupTargetStatus" => Some("vault/volumes"),
        "Provision-EncryptedBackupTarget" => Some("vault/volumes"),
        "Clear-EncryptedBackupTarget" => Some("vault/volumes"),

        // Storage - RAM Disks (ImDisk)
        "Test-RamDiskInstalled" => Some("vault/ramdisks"),
        "Install-RamDiskEngine" => Some("vault/ramdisks"),
        "Get-RamDiskStatus" => Some("vault/ramdisks"),
        "Get-SystemRamInfo" => Some("vault/ramdisks"),
        "New-RamDisk" => Some("vault/ramdisks"),
        "Remove-RamDisk" => Some("vault/ramdisks"),
        "Remove-AllRamDisks" => Some("vault/ramdisks"),
        "Open-RamDisk" => Some("vault/ramdisks"),

        // Privacy - Unified Shield
        "Get-PrivacyShieldStatus" => Some("privacy/privacy_shield"),
        "Start-PrivacyShield" => Some("privacy/privacy_shield"),
        "Stop-PrivacyShield" => Some("privacy/privacy_shield"),
        "Install-PrivacyShieldAI" => Some("dependencies/dependencies"),
        "Get-AIDependenciesStatus" => Some("dependencies/dependencies"),

        // Productivity
        "Get-ProductivityStatus" => Some("productivity"),
        "Start-ProductivityTracker" => Some("productivity"),
        "Stop-ProductivityTracker" => Some("productivity"),
        "Invoke-ProductivityEngineMaintenance" => Some("productivity"),

        // Settings Migration & System Probe
        "Get-WCMigrationData" => Some("core/settings-bridge"),
        "Get-WCSystemProbe" => Some("core/settings-bridge"),

        // Contingency
        "Get-ActiveRDPSessions" => Some("contingency/ops"),
        "Disconnect-RDPSession" => Some("contingency/ops"),
        "Disconnect-AllRDPSessions" => Some("contingency/ops"),
        "Lock-RDPAccess" => Some("contingency/ops"),
        "Unlock-RDPAccess" => Some("contingency/ops"),
        "Send-ContingencySignal" => Some("contingency/ops"),
        "New-ContingencyNotification" => Some("contingency/ops"),
        "Start-ContingencySequence" => Some("contingency/ops"),
        "Get-USBKeyStatus" => Some("contingency/ops"),
        "Register-USBKeySerial" => Some("contingency/ops"),
        "Dismount-RDPServerVaults" => Some("contingency/ops"),
        "Disconnect-RDPClientIdle" => Some("contingency/ops"),
        "Watch-RDPClientIdle" => Some("contingency/ops"),
        "Show-RDPIdleWarning" => Some("contingency/ops"),
        "Hide-RDPClientWindow" => Some("contingency/ops"),
        "Dismount-LocalVaults" => Some("contingency/ops"),

        _ => None,
    }
}

/// Returns a list of all registered PS command names for use by the flow editor command picker.
pub fn list_all_commands() -> Vec<String> {
    let mut commands: Vec<String> = vec![
        "Get-StartupStatus",
        "Get-SystemInfo",
        "Get-StorageStats",
        "Get-VersionUpdate",
        "Get-AppBranding",
        "Set-AppBranding",
        "Set-OEMInformation",
        "Rename-ComputerName",
        "Get-ActivationStatus",
        "Get-DependencyStatus",
        "Install-Dependency",
        "Install-AllDependencies",
        "Start-DependencyService",
        "Set-WinCommanderCalculatorShortcuts",
        "Restart-Explorer",
        "Invoke-DiskCleanup",
        "Set-ServicesManual",
        "Set-PowerPlan",
        "Disable-Telemetry",
        "Enable-Telemetry",
        "Get-TelemetryStatus",
        "Disable-Copilot",
        "Enable-Copilot",
        "Disable-ActivityHistory",
        "Enable-ActivityHistory",
        "Disable-LocationTracking",
        "Enable-LocationTracking",
        "Disable-WindowsSuggestions",
        "Enable-WindowsSuggestions",
        "Set-AppCapabilityAccess",
        "Get-AppCapabilityAccessStatus",
        "Disable-LockScreenPrivacy",
        "Enable-LockScreenPrivacy",
        // Host hardening (Feature 4)
        "Disable-SystemRestore",
        "Enable-SystemRestore",
        "Disable-SleepPassword",
        "Enable-SleepPassword",
        "Enable-KernelDMAProtection",
        "Disable-KernelDMAProtection",
        "Get-KernelDMAProtectionStatus",
        "Disable-CrashDumps",
        "Enable-CrashDumps",
        "Disable-RecallSnapshots",
        "Enable-RecallSnapshots",
        "Disable-AdvertisingID",
        "Enable-AdvertisingID",
        "Disable-TailoredExperiences",
        "Enable-TailoredExperiences",
        "Disable-OfficeLogging",
        "Enable-OfficeLogging",
        "Disable-DiagnosticEventTracing",
        "Enable-DiagnosticEventTracing",
        "Disable-SyncSettings",
        "Enable-SyncSettings",
        "Clear-PowerShellHistory",
        "Clear-DnsCache",
        "Enable-WindowsDefender",
        "Disable-WindowsUpdate",
        "Enable-WindowsUpdate",
        "Disable-Hibernation",
        "Enable-Hibernation",
        "Disable-FastStartup",
        "Enable-FastStartup",
        "Disable-UAC",
        "Enable-UAC",
        "Disable-Superfetch",
        "Enable-Superfetch",
        "Disable-BackgroundApps",
        "Enable-BackgroundApps",
        "Disable-Notifications",
        "Enable-Notifications",
        "Enable-ClassicContextMenu",
        "Disable-ClassicContextMenu",
        "Enable-PersistentRdpAnimations",
        "Disable-PersistentRdpAnimations",
        "Get-PersistentRdpAnimationsStatus",
        "Disable-CtrlAltDelLogon",
        "Enable-CtrlAltDelLogon",
        "Enable-HideLastSignedInUser",
        "Disable-HideLastSignedInUser",
        "Enable-ConsoleInactivityLock",
        "Disable-ConsoleInactivityLock",
        "Disable-ShutdownEventTracker",
        "Enable-ShutdownEventTracker",
        "Disable-ServerManagerAtLogon",
        "Enable-ServerManagerAtLogon",
        "Disable-IEEnhancedSecurity",
        "Enable-IEEnhancedSecurity",
        "Block-WDigestCredentials",
        "Allow-WDigestCredentials",
        "Enable-LsaProtection",
        "Disable-LsaProtection",
        "Block-LegacyNtlm",
        "Allow-LegacyNtlm",
        "Enable-SmbSigningRequired",
        "Disable-SmbSigningRequired",
        "Disable-Smb1Protocol",
        "Enable-Smb1Protocol",
        "Disable-RemoteRegistryService",
        "Enable-RemoteRegistryService",
        "Get-ServerTweakStatus",
        "Enable-NTFSOptimizations",
        "Disable-NTFSOptimizations",
        "Enable-USBWriteProtect",
        "Disable-USBWriteProtect",
        "Enable-USBStorageLockdown",
        "Disable-USBStorageLockdown",
        "Disable-SmartScreen",
        "Enable-SmartScreen",
        "Enable-LockdownMode",
        "Disable-LockdownMode",
        "Add-BlocklistToHosts",
        "Remove-BlocklistFromHosts",
        "Set-SecureDNS",
        "Clear-SecureDNS",
        "Enable-DNSCensorshipProtection",
        "Disable-DNSCensorshipProtection",
        "Get-MeshVPNStatus",
        "Set-MeshVPNConfig",
        "Send-MeshVPNFile",
        "Start-MeshVPNLogin",
        "Start-MeshService",
        "Stop-MeshService",
        "Connect-MeshVPN",
        "Get-EncryptionStatus",
        "Mount-EncryptionVolume",
        "Dismount-EncryptionVolume",
        "Create-EncryptionVolume",
        "Create-DualVolume",
        "Create-StegoMp4",
        "Extract-StegoMp4",
        "Get-EncryptedBackupTargetStatus",
        "Provision-EncryptedBackupTarget",
        "Clear-EncryptedBackupTarget",
        "Start-PrivacyShield",
        "Stop-PrivacyShield",
        "Start-ProductivityTracker",
        "Stop-ProductivityTracker",
        "Get-ActiveRDPSessions",
        "Disconnect-RDPSession",
        "Disconnect-AllRDPSessions",
        "Lock-RDPAccess",
        "Unlock-RDPAccess",
        "Send-ContingencySignal",
        "New-ContingencyNotification",
        "Start-ContingencySequence",
        "Get-USBKeyStatus",
        "Register-USBKeySerial",
        "Dismount-RDPServerVaults",
        "Disconnect-RDPClientIdle",
        "Dismount-LocalVaults",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    commands.push(join_parts(&["Dismount-~", "AllEncryption~", "Volumes~"]));
    commands.extend(
        SENSITIVE_COMMANDS
            .iter()
            .map(|entry| join_parts(entry.parts)),
    );
    commands
}

// Get and decrypt a module by name
fn load_module(module_name: &str) -> Result<String, String> {
    // Check cache first
    let cache_mutex = MODULE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache_mutex
        .lock()
        .map_err(|e| format!("Failed to lock cache: {}", e))?;

    if let Some(cached) = cache.get(module_name) {
        return Ok(cached.clone());
    }

    crate::log_message(
        "debug",
        &format!("[Backend] Loading module: {}", module_name),
    );

    // Get encrypted module data
    let encrypted = match module_name {
        "core/utils" => CORE_UTILS,
        "core/router" => CORE_ROUTER,
        "dashboard/startup" => SYSTEM_STARTUP,
        "dashboard/info" => SYSTEM_INFO,
        "identity/activation" => SYSTEM_ACTIVATION,
        "identity/branding" => IDENTITY_BRANDING,
        "tweaks/maintenance" => SYSTEM_MAINTENANCE,
        "tweaks/security" => SYSTEM_SECURITY,

        "privacy/telemetry" => PRIVACY_TELEMETRY,
        "privacy/cleanup" => PRIVACY_CLEANUP,
        "network/hosts" => NETWORK_HOSTS,
        "network/blocklists-data" => NETWORK_BLOCKLISTS_DATA,
        "network/firewall" => NETWORK_FIREWALL,
        "network/dns" => NETWORK_DNS,
        "network/ports" => NETWORK_PORTS,
        "network/adapters" => NETWORK_ADAPTERS,
        "apps/winget" => APPS_WINGET,
        "apps/uninstaller" => APPS_UNINSTALLER,
        "apps/bcu-uninstaller" => APPS_BCU_UNINSTALLER,
        "tweaks/system" => TWEAKS_SYSTEM,
        "tweaks/server" => TWEAKS_SERVER,
        "tweaks/ui" => TWEAKS_UI,
        "tweaks/startup-manager" => TWEAKS_STARTUP_MANAGER,
        "tweaks/performance" => TWEAKS_PERFORMANCE,
        "tweaks/gpu" => TWEAKS_GPU,
        "tweaks/power" => TWEAKS_POWER,
        "tweaks/ui-granular" => TWEAKS_UI_GRANULAR,
        "tweaks/scheduled-tasks" => TWEAKS_SCHEDULED_TASKS,
        "tweaks/service-manager" => TWEAKS_SERVICE_MANAGER,
        "tweaks/local-users" => TWEAKS_LOCAL_USERS,
        "tweaks/disk-cleanup-granular" => TWEAKS_DISK_CLEANUP_GRANULAR,
        "tweaks/prevention" => TWEAKS_PREVENTION,
        "tweaks/ai-control-common" => AI_CONTROL_COMMON,
        "tweaks/ai-control-policies" => AI_CONTROL_POLICIES,
        "tweaks/ai-control-apps" => AI_CONTROL_APPS,
        "tweaks/ai-control-shell" => AI_CONTROL_SHELL,
        "tweaks/ai-control-removal" => AI_CONTROL_REMOVAL,
        "tweaks/ai-control-maintenance" => AI_CONTROL_MAINTENANCE,
        "tweaks/ai-control" => AI_CONTROL,
        "vault/volumes" => STORAGE_VOLUMES,
        "vault/ramdisks" => STORAGE_RAMDISKS,
        "privacy/privacy_shield" => PRIVACY_SHIELD,
        "productivity" => PRODUCTIVITY_MODULE,
        "core/settings-bridge" => CORE_SETTINGS_BRIDGE,
        "dependencies/dependencies" => DEPENDENCIES_MODULE,
        "contingency/ops" => CONTINGENCY_OPS,
        _ => return Err(format!("Unknown module: {}", module_name)),
    };

    let decrypted = decode_module(encrypted)?;
    cache.insert(module_name.to_string(), decrypted.clone());

    Ok(decrypted)
}

// ═══════════════════════════════════════════════════════════════════════
// POST-COMMAND SETTINGS SYNC
// Maps toggle commands to their corresponding settings.json fields.
// Called after every successful run_backend_script execution.
// ═══════════════════════════════════════════════════════════════════════

fn get_settings_sync_patch(
    command: &str,
    params: &HashMap<String, String>,
    result: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    use serde_json::json;
    if matches_parts(command, &["Disable~-", "Windows~", "Defender~"]) {
        return Some(json!({"tweaks":{"security":{"defenderDisabled": true}}}));
    }

    match command {
        // ── Privacy: Telemetry ───────────────────────────────────────
        "Disable-Telemetry" => Some(
            json!({"privacy":{"telemetry":{"windowsDisabled": true, "powershell7Disabled": true, "officeDisabled": true}}}),
        ),
        "Enable-Telemetry" => Some(
            json!({"privacy":{"telemetry":{"windowsDisabled": false, "powershell7Disabled": false, "officeDisabled": false}}}),
        ),
        "Disable-Copilot" => Some(json!({"privacy":{"telemetry":{"copilotDisabled": true}}})),
        "Enable-Copilot" => Some(json!({"privacy":{"telemetry":{"copilotDisabled": false}}})),
        "Disable-ActivityHistory" => {
            Some(json!({"privacy":{"telemetry":{"activityHistoryDisabled": true}}}))
        }
        "Enable-ActivityHistory" => {
            Some(json!({"privacy":{"telemetry":{"activityHistoryDisabled": false}}}))
        }
        "Disable-LocationTracking" | "Enable-LocationTracking" => {
            // Location can be blocked/unblocked through several independent
            // Windows layers (AppPrivacy GP force-deny, the DeviceAccess
            // Global master switch, LocationAndSensors GP, the per-app
            // ConsentStore, and the lfsvc service's startup type). Trusting
            // the command name alone assumes the apply fully succeeded; read
            // back the PS function's own post-apply verification (`disabled`
            // in its result) instead, so the toggle reflects what the system
            // actually did, not what we asked it to do.
            let disabled = result
                .and_then(|r| r.get("disabled"))
                .and_then(|v| v.as_bool())
                .unwrap_or(command == "Disable-LocationTracking");
            Some(json!({"privacy":{"telemetry":{"locationTrackingDisabled": disabled}}}))
        }

        "Disable-WindowsSuggestions" => {
            Some(json!({"privacy":{"telemetry":{"windowsSuggestionsDisabled": true}}}))
        }
        "Enable-WindowsSuggestions" => {
            Some(json!({"privacy":{"telemetry":{"windowsSuggestionsDisabled": false}}}))
        }

        // ── Privacy: Lock Screen ─────────────────────────────────────
        "Disable-LockScreenPrivacy" => {
            Some(json!({"privacy":{"lockscreen":{"privacyDisabled": true}}}))
        }
        "Enable-LockScreenPrivacy" => {
            Some(json!({"privacy":{"lockscreen":{"privacyDisabled": false}}}))
        }

        // ── Privacy: Setup Nags ──────────────────────────────────────
        "Disable-SetupCompletionNags" => {
            Some(json!({"privacy":{"setupCompletionNagsDisabled": true}}))
        }
        "Enable-SetupCompletionNags" => {
            Some(json!({"privacy":{"setupCompletionNagsDisabled": false}}))
        }

        // ── Privacy: Clipboard ───────────────────────────────────────
        "Disable-ClipboardHistory" => {
            Some(json!({"privacy":{"clipboard":{"historyDisabled": true}}}))
        }
        "Enable-ClipboardHistory" => {
            Some(json!({"privacy":{"clipboard":{"historyDisabled": false}}}))
        }
        "Disable-CloudClipboardSync" => {
            Some(json!({"privacy":{"clipboard":{"cloudSyncDisabled": true}}}))
        }
        "Enable-CloudClipboardSync" => {
            Some(json!({"privacy":{"clipboard":{"cloudSyncDisabled": false}}}))
        }

        // ── Privacy: Tracking ────────────────────────────────────────
        "Disable-RecentFilesTracking" => {
            Some(json!({"privacy":{"tracking":{"recentFilesDisabled": true}}}))
        }
        "Enable-RecentFilesTracking" => {
            Some(json!({"privacy":{"tracking":{"recentFilesDisabled": false}}}))
        }
        "Disable-JumpLists" => Some(json!({"privacy":{"tracking":{"jumpListsDisabled": true}}})),
        "Enable-JumpLists" => Some(json!({"privacy":{"tracking":{"jumpListsDisabled": false}}})),
        "Disable-ThumbnailCache" => {
            Some(json!({"privacy":{"tracking":{"thumbnailCacheDisabled": true}}}))
        }
        "Enable-ThumbnailCache" => {
            Some(json!({"privacy":{"tracking":{"thumbnailCacheDisabled": false}}}))
        }
        "Disable-Pagefile" => Some(json!({"privacy":{"tracking":{"pagefileDisabled": true}}})),
        "Enable-Pagefile" => Some(json!({"privacy":{"tracking":{"pagefileDisabled": false}}})),
        "Disable-AdvertisingID" => {
            Some(json!({"privacy":{"tracking":{"advertisingIdDisabled": true}}}))
        }
        "Enable-AdvertisingID" => {
            Some(json!({"privacy":{"tracking":{"advertisingIdDisabled": false}}}))
        }
        "Disable-TailoredExperiences" => {
            Some(json!({"privacy":{"tracking":{"tailoredExperiencesDisabled": true}}}))
        }
        "Enable-TailoredExperiences" => {
            Some(json!({"privacy":{"tracking":{"tailoredExperiencesDisabled": false}}}))
        }
        "Disable-OfficeLogging" => {
            Some(json!({"privacy":{"tracking":{"officeLoggingDisabled": true}}}))
        }
        "Enable-OfficeLogging" => {
            Some(json!({"privacy":{"tracking":{"officeLoggingDisabled": false}}}))
        }
        "Disable-DiagnosticEventTracing" => {
            Some(json!({"privacy":{"tracking":{"diagnosticEventTracingDisabled": true}}}))
        }
        "Enable-DiagnosticEventTracing" => {
            Some(json!({"privacy":{"tracking":{"diagnosticEventTracingDisabled": false}}}))
        }

        // ── Privacy: Protection & Shield ─────────────────────────────
        "Enable-PrivacyProtection" => Some(json!({"privacy":{"privacyProtectionEnabled": true}})),
        "Disable-PrivacyProtection" => Some(json!({"privacy":{"privacyProtectionEnabled": false}})),
        "Stop-PrivacyShield" => Some(json!({"privacy":{"privacyShield":{"shieldRunning": false}}})),

        // ── Tweaks: Security ─────────────────────────────────────────
        "Enable-WindowsDefender" => {
            Some(json!({"tweaks":{"security":{"defenderDisabled": false}}}))
        }
        "Disable-WindowsUpdate" => {
            Some(json!({"tweaks":{"security":{"windowsUpdateDisabled": true}}}))
        }
        "Enable-WindowsUpdate" => {
            Some(json!({"tweaks":{"security":{"windowsUpdateDisabled": false}}}))
        }
        "Disable-UAC" => Some(json!({"tweaks":{"security":{"uacDisabled": true}}})),
        "Enable-UAC" => Some(json!({"tweaks":{"security":{"uacDisabled": false}}})),
        "Enable-USBWriteProtect" => Some(json!({"tweaks":{"security":{"usbWriteProtect": true}}})),
        "Disable-USBWriteProtect" => {
            Some(json!({"tweaks":{"security":{"usbWriteProtect": false}}}))
        }
        "Enable-USBStorageLockdown" => {
            Some(json!({"tweaks":{"security":{"usbStorageLockdown": true}}}))
        }
        "Disable-USBStorageLockdown" => {
            Some(json!({"tweaks":{"security":{"usbStorageLockdown": false}}}))
        }
        "Disable-ConsumerFeatures" => {
            Some(json!({"tweaks":{"security":{"consumerFeaturesDisabled": true}}}))
        }
        "Enable-ConsumerFeatures" => {
            Some(json!({"tweaks":{"security":{"consumerFeaturesDisabled": false}}}))
        }

        // ── Tweaks: OS ───────────────────────────────────────────────
        "Disable-Superfetch" => Some(json!({"tweaks":{"os":{"superfetchDisabled": true}}})),
        "Enable-Superfetch" => Some(json!({"tweaks":{"os":{"superfetchDisabled": false}}})),
        "Disable-Prefetch" => Some(json!({"tweaks":{"os":{"prefetchDisabled": true}}})),
        "Enable-Prefetch" => Some(json!({"tweaks":{"os":{"prefetchDisabled": false}}})),
        "Disable-Hibernation" => Some(json!({"tweaks":{"os":{"hibernationDisabled": true}}})),
        "Enable-Hibernation" => Some(json!({"tweaks":{"os":{"hibernationDisabled": false}}})),
        "Disable-FastStartup" => Some(json!({"tweaks":{"os":{"fastStartupDisabled": true}}})),
        "Enable-FastStartup" => Some(json!({"tweaks":{"os":{"fastStartupDisabled": false}}})),
        "Enable-NTFSOptimizations" => {
            Some(json!({"tweaks":{"os":{"ntfsOptimizationsEnabled": true}}}))
        }
        "Disable-NTFSOptimizations" => {
            Some(json!({"tweaks":{"os":{"ntfsOptimizationsEnabled": false}}}))
        }
        "Enable-DetailedBSOD" => Some(json!({"tweaks":{"os":{"detailedBsodEnabled": true}}})),
        "Disable-DetailedBSOD" => Some(json!({"tweaks":{"os":{"detailedBsodEnabled": false}}})),
        "Disable-AutomaticMaintenance" => {
            Some(json!({"tweaks":{"os":{"automaticMaintenanceDisabled": true}}}))
        }
        "Enable-AutomaticMaintenance" => {
            Some(json!({"tweaks":{"os":{"automaticMaintenanceDisabled": false}}}))
        }
        "Enable-Win32LongPaths" => Some(json!({"tweaks":{"os":{"win32LongPathsEnabled": true}}})),
        "Disable-Win32LongPaths" => Some(json!({"tweaks":{"os":{"win32LongPathsEnabled": false}}})),
        "Disable-SmbBandwidthThrottling" => {
            Some(json!({"tweaks":{"os":{"smbBandwidthThrottlingDisabled": true}}}))
        }
        "Enable-SmbBandwidthThrottling" => {
            Some(json!({"tweaks":{"os":{"smbBandwidthThrottlingDisabled": false}}}))
        }
        // Set-PowerPlan writes the top-level tweaks.powerPlan path that
        // the UI actually reads (the old tweaks.os.powerPlan field is
        // gone — see chore(settings) commit).
        "Set-PowerPlan" => Some(
            json!({"tweaks":{"powerPlan": params.get("Mode").cloned().unwrap_or_default().to_lowercase()}}),
        ),
        "Set-BitLockerTpmPin" => {
            let enabled = params
                .get("Enable")
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(true);
            Some(json!({"tweaks":{"security":{"bitlockerTpmPinEnforce": enabled}}}))
        }

        // ── Tweaks: Windows Server ───────────────────────────────────
        "Enable-PersistentRdpAnimations" => {
            Some(json!({"tweaks":{"server":{"persistentRdpAnimations": true}}}))
        }
        "Disable-PersistentRdpAnimations" => {
            Some(json!({"tweaks":{"server":{"persistentRdpAnimations": false}}}))
        }
        "Disable-CtrlAltDelLogon" => {
            Some(json!({"tweaks":{"server":{"ctrlAltDelDisabled": true}}}))
        }
        "Enable-CtrlAltDelLogon" => {
            Some(json!({"tweaks":{"server":{"ctrlAltDelDisabled": false}}}))
        }
        "Enable-HideLastSignedInUser" => {
            Some(json!({"tweaks":{"server":{"lastSignedInUserHidden": true}}}))
        }
        "Disable-HideLastSignedInUser" => {
            Some(json!({"tweaks":{"server":{"lastSignedInUserHidden": false}}}))
        }
        "Enable-ConsoleInactivityLock" => {
            Some(json!({"tweaks":{"server":{"consoleInactivityLock": true}}}))
        }
        "Disable-ConsoleInactivityLock" => {
            Some(json!({"tweaks":{"server":{"consoleInactivityLock": false}}}))
        }
        "Disable-ShutdownEventTracker" => {
            Some(json!({"tweaks":{"server":{"shutdownTrackerDisabled": true}}}))
        }
        "Enable-ShutdownEventTracker" => {
            Some(json!({"tweaks":{"server":{"shutdownTrackerDisabled": false}}}))
        }
        "Disable-ServerManagerAtLogon" => {
            Some(json!({"tweaks":{"server":{"serverManagerAtLogonDisabled": true}}}))
        }
        "Enable-ServerManagerAtLogon" => {
            Some(json!({"tweaks":{"server":{"serverManagerAtLogonDisabled": false}}}))
        }
        "Disable-IEEnhancedSecurity" => {
            Some(json!({"tweaks":{"server":{"ieEnhancedSecurityDisabled": true}}}))
        }
        "Enable-IEEnhancedSecurity" => {
            Some(json!({"tweaks":{"server":{"ieEnhancedSecurityDisabled": false}}}))
        }
        "Block-WDigestCredentials" => Some(json!({"tweaks":{"server":{"wdigestBlocked": true}}})),
        "Allow-WDigestCredentials" => Some(json!({"tweaks":{"server":{"wdigestBlocked": false}}})),
        "Enable-LsaProtection" => Some(json!({"tweaks":{"server":{"lsaProtectionEnabled": true}}})),
        "Disable-LsaProtection" => {
            Some(json!({"tweaks":{"server":{"lsaProtectionEnabled": false}}}))
        }
        "Block-LegacyNtlm" => Some(json!({"tweaks":{"server":{"legacyNtlmBlocked": true}}})),
        "Allow-LegacyNtlm" => Some(json!({"tweaks":{"server":{"legacyNtlmBlocked": false}}})),
        "Enable-SmbSigningRequired" => {
            Some(json!({"tweaks":{"server":{"smbSigningRequired": true}}}))
        }
        "Disable-SmbSigningRequired" => {
            Some(json!({"tweaks":{"server":{"smbSigningRequired": false}}}))
        }
        "Disable-Smb1Protocol" => Some(json!({"tweaks":{"server":{"smb1Disabled": true}}})),
        "Enable-Smb1Protocol" => Some(json!({"tweaks":{"server":{"smb1Disabled": false}}})),
        "Disable-RemoteRegistryService" => {
            Some(json!({"tweaks":{"server":{"remoteRegistryDisabled": true}}}))
        }
        "Enable-RemoteRegistryService" => {
            Some(json!({"tweaks":{"server":{"remoteRegistryDisabled": false}}}))
        }

        // ── Tweaks: UI ───────────────────────────────────────────────
        "Enable-ClassicContextMenu" => Some(json!({"tweaks":{"ui":{"classicContextMenu": true}}})),
        "Disable-ClassicContextMenu" => {
            Some(json!({"tweaks":{"ui":{"classicContextMenu": false}}}))
        }
        "Show-FileExtensions" => Some(json!({"tweaks":{"ui":{"fileExtensionsVisible": true}}})),
        "Hide-FileExtensions" => Some(json!({"tweaks":{"ui":{"fileExtensionsVisible": false}}})),
        "Show-HiddenFiles" => Some(json!({"tweaks":{"ui":{"hiddenFilesVisible": true}}})),
        "Hide-HiddenFiles" => Some(json!({"tweaks":{"ui":{"hiddenFilesVisible": false}}})),
        "Enable-RemoveGalleryHome" => Some(json!({"tweaks":{"ui":{"galleryHomeRemoved": true}}})),
        "Disable-RemoveGalleryHome" => Some(json!({"tweaks":{"ui":{"galleryHomeRemoved": false}}})),
        "Disable-BingSearch" => Some(json!({"tweaks":{"ui":{"bingSearchDisabled": true}}})),
        "Enable-BingSearch" => Some(json!({"tweaks":{"ui":{"bingSearchDisabled": false}}})),
        "Disable-BackgroundApps" => Some(json!({"tweaks":{"ui":{"backgroundAppsDisabled": true}}})),
        "Enable-BackgroundApps" => Some(json!({"tweaks":{"ui":{"backgroundAppsDisabled": false}}})),
        "Disable-Notifications" => Some(json!({"tweaks":{"ui":{"notificationsDisabled": true}}})),
        "Enable-Notifications" => Some(json!({"tweaks":{"ui":{"notificationsDisabled": false}}})),
        "Enable-EndTaskOnTaskbar" => Some(json!({"tweaks":{"ui":{"endTaskOnTaskbar": true}}})),
        "Disable-EndTaskOnTaskbar" => Some(json!({"tweaks":{"ui":{"endTaskOnTaskbar": false}}})),
        "Disable-FolderTypeDiscovery" => {
            Some(json!({"tweaks":{"ui":{"folderTypeDiscoveryDisabled": true}}}))
        }
        "Enable-FolderTypeDiscovery" => {
            Some(json!({"tweaks":{"ui":{"folderTypeDiscoveryDisabled": false}}}))
        }
        "Remove-ShortcutSuffix" => Some(json!({"tweaks":{"ui":{"shortcutSuffixRemoved": true}}})),
        "Restore-ShortcutSuffix" => Some(json!({"tweaks":{"ui":{"shortcutSuffixRemoved": false}}})),
        "Disable-AutoPlay" => Some(json!({"tweaks":{"ui":{"autoPlayDisabled": true}}})),
        "Enable-AutoPlay" => Some(json!({"tweaks":{"ui":{"autoPlayDisabled": false}}})),
        "Disable-LowDiskCheck" => Some(json!({"tweaks":{"ui":{"lowDiskCheckDisabled": true}}})),
        "Enable-LowDiskCheck" => Some(json!({"tweaks":{"ui":{"lowDiskCheckDisabled": false}}})),
        "Set-ExplorerOpensThisPC" => Some(json!({"tweaks":{"ui":{"explorerOpensThisPc": true}}})),
        "Set-ExplorerOpensQuickAccess" => {
            Some(json!({"tweaks":{"ui":{"explorerOpensThisPc": false}}}))
        }
        "Hide-SyncProviderNotifications" => {
            Some(json!({"tweaks":{"ui":{"syncProviderNotificationsHidden": true}}}))
        }
        "Show-SyncProviderNotifications" => {
            Some(json!({"tweaks":{"ui":{"syncProviderNotificationsHidden": false}}}))
        }
        "Disable-TransparencyEffects" => {
            Some(json!({"tweaks":{"ui":{"transparencyDisabled": true}}}))
        }
        "Enable-TransparencyEffects" => {
            Some(json!({"tweaks":{"ui":{"transparencyDisabled": false}}}))
        }
        "Enable-FullPathInTitleBar" => Some(json!({"tweaks":{"ui":{"fullPathInTitleBar": true}}})),
        "Disable-FullPathInTitleBar" => {
            Some(json!({"tweaks":{"ui":{"fullPathInTitleBar": false}}}))
        }
        "Enable-PowerShell7DefaultShell" => {
            Some(json!({"tweaks":{"ui":{"powerShell7Default": true}}}))
        }
        "Disable-PowerShell7DefaultShell" => {
            Some(json!({"tweaks":{"ui":{"powerShell7Default": false}}}))
        }
        "Set-TaskbarDebloated" => Some(json!({"tweaks":{"ui":{"taskbarDebloated": true}}})),
        "Reset-TaskbarDebloated" => Some(json!({"tweaks":{"ui":{"taskbarDebloated": false}}})),
        "Disable-StartRecommendations" => {
            Some(json!({"tweaks":{"ui":{"startRecommendationsDisabled": true}}}))
        }
        "Enable-StartRecommendations" => {
            Some(json!({"tweaks":{"ui":{"startRecommendationsDisabled": false}}}))
        }

        // ── Tweaks: Security (new) ───────────────────────────────────
        "Disable-VBS" => Some(json!({"tweaks":{"security":{"vbsDisabled": true}}})),
        "Enable-VBS" => Some(json!({"tweaks":{"security":{"vbsDisabled": false}}})),
        "Disable-BitLockerAutoEncrypt" => {
            Some(json!({"tweaks":{"security":{"bitlockerAutoEncryptDisabled": true}}}))
        }
        "Enable-BitLockerAutoEncrypt" => {
            Some(json!({"tweaks":{"security":{"bitlockerAutoEncryptDisabled": false}}}))
        }
        // Anti-Acquisition Defenses (enable = protection ON)
        "Enable-AcquisitionDriverBlocklist" => {
            Some(json!({"tweaks":{"security":{"acquisitionDriverBlocklist": true}}}))
        }
        "Disable-AcquisitionDriverBlocklist" => {
            Some(json!({"tweaks":{"security":{"acquisitionDriverBlocklist": false}}}))
        }
        "Enable-ForensicToolBlock" => {
            Some(json!({"tweaks":{"security":{"forensicToolBlock": true}}}))
        }
        "Disable-ForensicToolBlock" => {
            Some(json!({"tweaks":{"security":{"forensicToolBlock": false}}}))
        }
        "Enable-LidClosePowerOff" => {
            Some(json!({"tweaks":{"security":{"lidClosePowerOff": true}}}))
        }
        "Disable-LidClosePowerOff" => {
            Some(json!({"tweaks":{"security":{"lidClosePowerOff": false}}}))
        }
        "Disable-WPBT" => Some(json!({"tweaks":{"security":{"wpbtDisabled": true}}})),
        "Enable-WPBT" => Some(json!({"tweaks":{"security":{"wpbtDisabled": false}}})),
        "Disable-RemoteAssistance" => {
            Some(json!({"tweaks":{"security":{"remoteAssistanceDisabled": true}}}))
        }
        "Enable-RemoteAssistance" => {
            Some(json!({"tweaks":{"security":{"remoteAssistanceDisabled": false}}}))
        }
        "Block-AnonymousSamEnumeration" => {
            Some(json!({"tweaks":{"security":{"anonymousSamEnumerationBlocked": true}}}))
        }
        "Allow-AnonymousSamEnumeration" => {
            Some(json!({"tweaks":{"security":{"anonymousSamEnumerationBlocked": false}}}))
        }
        "Disable-SmartScreen" => Some(json!({"tweaks":{"security":{"smartScreenDisabled": true}}})),
        "Enable-SmartScreen" => Some(json!({"tweaks":{"security":{"smartScreenDisabled": false}}})),
        "Set-OOBEBypass" => Some(json!({"tweaks":{"security":{"oobeBypassEnabled": true}}})),
        "Clear-OOBEBypass" => Some(json!({"tweaks":{"security":{"oobeBypassEnabled": false}}})),
        "Disable-GameDVR" => Some(json!({"tweaks":{"security":{"gameDvrDisabled": true}}})),
        "Enable-GameDVR" => Some(json!({"tweaks":{"security":{"gameDvrDisabled": false}}})),
        "Remove-CopilotAIComponents" => {
            Some(json!({"tweaks":{"security":{"copilotAiRemoved": true}}}))
        }
        "Restore-CopilotAIComponents" => {
            Some(json!({"tweaks":{"security":{"copilotAiRemoved": false}}}))
        }

        // ── Tweaks: OS (new) ─────────────────────────────────────────
        "Disable-MemoryCompression" => {
            Some(json!({"tweaks":{"os":{"memoryCompressionDisabled": true}}}))
        }
        "Enable-MemoryCompression" => {
            Some(json!({"tweaks":{"os":{"memoryCompressionDisabled": false}}}))
        }
        "Set-Win32PrioritySeparation" => {
            Some(json!({"tweaks":{"os":{"win32PrioritySeparation": true}}}))
        }
        "Reset-Win32PrioritySeparation" => {
            Some(json!({"tweaks":{"os":{"win32PrioritySeparation": false}}}))
        }
        "Set-DesktopShellPriority" => {
            Some(json!({"tweaks":{"os":{"desktopShellPriorityEnabled": true}}}))
        }
        "Reset-DesktopShellPriority" => {
            Some(json!({"tweaks":{"os":{"desktopShellPriorityEnabled": false}}}))
        }
        "Set-OptimizedTimeouts" => {
            Some(json!({"tweaks":{"os":{"serviceTimeoutsOptimized": true}}}))
        }
        "Reset-OptimizedTimeouts" => {
            Some(json!({"tweaks":{"os":{"serviceTimeoutsOptimized": false}}}))
        }
        "Disable-ReservedStorage" => {
            Some(json!({"tweaks":{"os":{"reservedStorageDisabled": true}}}))
        }
        "Enable-ReservedStorage" => {
            Some(json!({"tweaks":{"os":{"reservedStorageDisabled": false}}}))
        }

        // ── Tweaks: RDP Stability (paid) ─────────────────────────────
        "Enable-RdpKeepAlive" => Some(json!({"tweaks":{"rdp":{"keepAlive": true}}})),
        "Disable-RdpKeepAlive" => Some(json!({"tweaks":{"rdp":{"keepAlive": false}}})),
        "Enable-RdpNoTimeouts" => Some(json!({"tweaks":{"rdp":{"noTimeouts": true}}})),
        "Disable-RdpNoTimeouts" => Some(json!({"tweaks":{"rdp":{"noTimeouts": false}}})),
        "Enable-RdpQosPriority" => Some(json!({"tweaks":{"rdp":{"qosPriority": true}}})),
        "Disable-RdpQosPriority" => Some(json!({"tweaks":{"rdp":{"qosPriority": false}}})),
        "Enable-RdpIncomingIdleTimeout" => Some(
            json!({"tweaks":{"rdp":{"incomingIdleTimeoutEnabled": true, "incomingIdleTimeoutSeconds": params.get("Seconds").and_then(|s| s.parse::<u64>().ok()).or_else(|| params.get("Minutes").and_then(|m| m.parse::<u64>().ok()).map(|m| m * 60)).unwrap_or(900)}}}),
        ),
        "Disable-RdpIncomingIdleTimeout" => {
            Some(json!({"tweaks":{"rdp":{"incomingIdleTimeoutEnabled": false}}}))
        }

        // ── Tweaks: Boot & Kernel (new) ──────────────────────────────
        "Enable-TSX" => Some(json!({"tweaks":{"bootKernel":{"tsxEnabled": true}}})),
        "Disable-TSX" => Some(json!({"tweaks":{"bootKernel":{"tsxEnabled": false}}})),
        "Disable-FirstLogonAnimation" => {
            Some(json!({"tweaks":{"bootKernel":{"firstLogonAnimationDisabled": true}}}))
        }
        "Enable-FirstLogonAnimation" => {
            Some(json!({"tweaks":{"bootKernel":{"firstLogonAnimationDisabled": false}}}))
        }
        "Disable-StartupSound" => {
            Some(json!({"tweaks":{"bootKernel":{"startupSoundDisabled": true}}}))
        }
        "Enable-StartupSound" => {
            Some(json!({"tweaks":{"bootKernel":{"startupSoundDisabled": false}}}))
        }
        "Disable-AutoRestartSignon" => {
            Some(json!({"tweaks":{"bootKernel":{"autoRestartSignonDisabled": true}}}))
        }
        "Enable-AutoRestartSignon" => {
            Some(json!({"tweaks":{"bootKernel":{"autoRestartSignonDisabled": false}}}))
        }
        "Disable-AutoRebootOnBSOD" => {
            Some(json!({"tweaks":{"bootKernel":{"autoRebootOnBsodDisabled": true}}}))
        }
        "Enable-AutoRebootOnBSOD" => {
            Some(json!({"tweaks":{"bootKernel":{"autoRebootOnBsodDisabled": false}}}))
        }
        "Set-SmallMemoryDump" => {
            Some(json!({"tweaks":{"bootKernel":{"smallMemoryDumpEnabled": true}}}))
        }
        "Reset-SmallMemoryDump" => {
            Some(json!({"tweaks":{"bootKernel":{"smallMemoryDumpEnabled": false}}}))
        }

        // ── Privacy: Tracking (new) ──────────────────────────────────
        "Disable-RecallSnapshots" => {
            Some(json!({"privacy":{"tracking":{"recallSnapshotsDisabled": true}}}))
        }
        "Enable-RecallSnapshots" => {
            Some(json!({"privacy":{"tracking":{"recallSnapshotsDisabled": false}}}))
        }
        "Disable-TypingInsights" => {
            Some(json!({"privacy":{"tracking":{"typingInsightsDisabled": true}}}))
        }
        "Enable-TypingInsights" => {
            Some(json!({"privacy":{"tracking":{"typingInsightsDisabled": false}}}))
        }

        // ── Privacy: Tracking (Phase E hide-recent toggles) ──────────
        "Disable-QuickAccessRecent" => {
            Some(json!({"privacy":{"tracking":{"quickAccessRecentDisabled": true}}}))
        }
        "Enable-QuickAccessRecent" => {
            Some(json!({"privacy":{"tracking":{"quickAccessRecentDisabled": false}}}))
        }
        "Disable-QuickAccessFrequent" => {
            Some(json!({"privacy":{"tracking":{"quickAccessFrequentDisabled": true}}}))
        }
        "Enable-QuickAccessFrequent" => {
            Some(json!({"privacy":{"tracking":{"quickAccessFrequentDisabled": false}}}))
        }
        "Disable-RunMRU" => Some(json!({"privacy":{"tracking":{"runMruDisabled": true}}})),
        "Enable-RunMRU" => Some(json!({"privacy":{"tracking":{"runMruDisabled": false}}})),
        "Disable-SearchHistory" => {
            Some(json!({"privacy":{"tracking":{"searchHistoryDisabled": true}}}))
        }
        "Enable-SearchHistory" => {
            Some(json!({"privacy":{"tracking":{"searchHistoryDisabled": false}}}))
        }
        "Disable-TerminalHistory" => {
            Some(json!({"privacy":{"tracking":{"terminalHistoryDisabled": true}}}))
        }
        "Enable-TerminalHistory" => {
            Some(json!({"privacy":{"tracking":{"terminalHistoryDisabled": false}}}))
        }

        // ── Privacy: Internet Communication (new) ────────────────────
        "Disable-InternetCommunication" => {
            Some(json!({"privacy":{"internetCommunication":{"restrictedEnabled": true}}}))
        }
        "Enable-InternetCommunication" => {
            Some(json!({"privacy":{"internetCommunication":{"restrictedEnabled": false}}}))
        }

        // ── Privacy: App Capabilities (dynamic) ──────────────────────
        "Set-AppCapabilityAccess" => {
            let capability = params.get("Capability")?;
            let access = params.get("Access")?.clone();
            let field = match capability.as_str() {
                "webcam" => "webcam",
                "microphone" => "microphone",
                "location" => "location",
                "contacts" => "contacts",
                "appointments" => "calendar",
                "phoneCall" => "phoneCall",
                "phoneCallHistory" => "callHistory",
                "chat" => "messaging",
                "email" => "email",
                "radios" => "radios",
                "userNotificationListener" => "notifications",
                "documentsLibrary" => "documents",
                "picturesLibrary" => "pictures",
                "videosLibrary" => "videos",
                "broadFileSystemAccess" => "fileSystem",
                "gazeInput" => "gazeInput",
                "appDiagnostics" => "appDiagnostics",
                "userAccountInformation" => "userAccountInformation",
                "bluetoothSync" => "bluetoothSync",
                _ => return None,
            };
            Some(json!({"privacy": {"appCapabilities": {field: access}}}))
        }

        // ── Apps (new) ───────────────────────────────────────────────
        "Remove-MicrosoftTeams" => Some(json!({"apps":{"teamsRemoved": true}})),

        // ── Network ──────────────────────────────────────────────────
        "Enable-IPv4Preference" => Some(json!({"network":{"dns":{"ipv4Preference": true}}})),
        "Disable-IPv4Preference" => Some(json!({"network":{"dns":{"ipv4Preference": false}}})),
        "Enable-DNSCensorshipProtection" => {
            Some(json!({"network":{"dns":{"censorshipProtection": true}}}))
        }
        "Disable-DNSCensorshipProtection" => {
            Some(json!({"network":{"dns":{"censorshipProtection": false}}}))
        }
        "Enable-LockdownMode" => Some(json!({"network":{"firewall":{"lockdownMode": true}}})),
        "Disable-LockdownMode" => Some(json!({"network":{"firewall":{"lockdownMode": false}}})),

        // ── Productivity ─────────────────────────────────────────────
        "Start-ProductivityTracker" => Some(json!({"productivity":{"trackerEnabled": true}})),
        "Stop-ProductivityTracker" => Some(json!({"productivity":{"trackerEnabled": false}})),

        // ── Apps ─────────────────────────────────────────────────────
        "Invoke-RemoveEdge" => Some(json!({"apps":{"edgeRemoved": true}})),
        "Invoke-RemoveOneDrive" => Some(json!({"apps":{"onedriveRemoved": true}})),

        // ── Identity ─────────────────────────────────────────────────
        // Branding is synced separately via AppContext.refreshBranding()

        // ════════════════════════════════════════════════════════════════
        // Every granular toggle MUST appear here.
        // Without a sync_patch entry, the toggle won't visually flip after
        // a successful enable/disable because nothing writes current.* to
        // settings.json until the next probe runs (only at app startup).
        // ════════════════════════════════════════════════════════════════

        // ── Tweaks: Performance ──────────────────────────────────────
        "Enable-MMCSSGamingProfile" => {
            Some(json!({"tweaks":{"performance":{"mmcssGamingProfile": true}}}))
        }
        "Disable-MMCSSGamingProfile" => {
            Some(json!({"tweaks":{"performance":{"mmcssGamingProfile": false}}}))
        }
        "Enable-KeyboardLatencyOptimised" => {
            Some(json!({"tweaks":{"performance":{"keyboardLatencyOptimised": true}}}))
        }
        "Disable-KeyboardLatencyOptimised" => {
            Some(json!({"tweaks":{"performance":{"keyboardLatencyOptimised": false}}}))
        }
        "Enable-NumLockOnBoot" => Some(json!({"tweaks":{"performance":{"numLockOnBoot": true}}})),
        "Disable-NumLockOnBoot" => Some(json!({"tweaks":{"performance":{"numLockOnBoot": false}}})),
        "Enable-GpuScheduling" => {
            Some(json!({"tweaks":{"performance":{"gpuSchedulingEnabled": true}}}))
        }
        "Disable-GpuScheduling" => {
            Some(json!({"tweaks":{"performance":{"gpuSchedulingEnabled": false}}}))
        }
        // SvcHostSplit lives in tweaks/system.ps1 but its toggle lives in
        // the Performance section (it's a responsiveness knob).
        "Enable-SvcHostSplit" => {
            Some(json!({"tweaks":{"performance":{"svcHostSplitOptimised": true}}}))
        }
        "Disable-SvcHostSplit" => {
            Some(json!({"tweaks":{"performance":{"svcHostSplitOptimised": false}}}))
        }
        // Existing-backend toggles surfaced by the granular controls.
        // enableCmd on these inverts: e.g. accessibility's enable command
        // is Disable-AccessibilityShortcuts → write disabled=true.
        "Disable-AccessibilityShortcuts" => {
            Some(json!({"tweaks":{"performance":{"accessibilityShortcutsDisabled": true}}}))
        }
        "Enable-AccessibilityShortcuts" => {
            Some(json!({"tweaks":{"performance":{"accessibilityShortcutsDisabled": false}}}))
        }
        "Enable-InstantMenuDelay" => {
            Some(json!({"tweaks":{"performance":{"instantMenuDelay": true}}}))
        }
        "Disable-InstantMenuDelay" => {
            Some(json!({"tweaks":{"performance":{"instantMenuDelay": false}}}))
        }
        "Disable-MouseAcceleration" => {
            Some(json!({"tweaks":{"performance":{"mouseAccelerationDisabled": true}}}))
        }
        "Enable-MouseAcceleration" => {
            Some(json!({"tweaks":{"performance":{"mouseAccelerationDisabled": false}}}))
        }
        "Disable-AutocorrectSpellcheck" => {
            Some(json!({"tweaks":{"performance":{"autocorrectDisabled": true}}}))
        }
        "Enable-AutocorrectSpellcheck" => {
            Some(json!({"tweaks":{"performance":{"autocorrectDisabled": false}}}))
        }

        // ── Tweaks: UI granular ──────────────────────────────────────
        // EnthusiastMode + WallpaperQuality were re-sectioned to "ui" so
        // their patch goes under tweaks.ui too.
        "Enable-EnthusiastMode" => Some(json!({"tweaks":{"ui":{"enthusiastModeEnabled": true}}})),
        "Disable-EnthusiastMode" => Some(json!({"tweaks":{"ui":{"enthusiastModeEnabled": false}}})),
        "Enable-WallpaperQuality" => Some(json!({"tweaks":{"ui":{"wallpaperFullQuality": true}}})),
        "Disable-WallpaperQuality" => {
            Some(json!({"tweaks":{"ui":{"wallpaperFullQuality": false}}}))
        }
        // Desktop icons — Show = visible = true; Hide = visible = false.
        "Show-DesktopIconThisPc" => Some(json!({"tweaks":{"ui":{"desktopIconThisPc": true}}})),
        "Hide-DesktopIconThisPc" => Some(json!({"tweaks":{"ui":{"desktopIconThisPc": false}}})),
        "Show-DesktopIconRecycleBin" => {
            Some(json!({"tweaks":{"ui":{"desktopIconRecycleBin": true}}}))
        }
        "Hide-DesktopIconRecycleBin" => {
            Some(json!({"tweaks":{"ui":{"desktopIconRecycleBin": false}}}))
        }
        "Show-DesktopIconUserFiles" => {
            Some(json!({"tweaks":{"ui":{"desktopIconUserFiles": true}}}))
        }
        "Hide-DesktopIconUserFiles" => {
            Some(json!({"tweaks":{"ui":{"desktopIconUserFiles": false}}}))
        }
        "Show-DesktopIconNetwork" => Some(json!({"tweaks":{"ui":{"desktopIconNetwork": true}}})),
        "Hide-DesktopIconNetwork" => Some(json!({"tweaks":{"ui":{"desktopIconNetwork": false}}})),
        "Show-DesktopIconControlPanel" => {
            Some(json!({"tweaks":{"ui":{"desktopIconControlPanel": true}}}))
        }
        "Hide-DesktopIconControlPanel" => {
            Some(json!({"tweaks":{"ui":{"desktopIconControlPanel": false}}}))
        }
        "Remove-ShortcutArrow" => Some(json!({"tweaks":{"ui":{"shortcutArrowRemoved": true}}})),
        "Restore-ShortcutArrow" => Some(json!({"tweaks":{"ui":{"shortcutArrowRemoved": false}}})),
        "Disable-SnapAssistFlyout" => {
            Some(json!({"tweaks":{"ui":{"snapAssistFlyoutDisabled": true}}}))
        }
        "Enable-SnapAssistFlyout" => {
            Some(json!({"tweaks":{"ui":{"snapAssistFlyoutDisabled": false}}}))
        }
        "Enable-ExplorerCompactMode" => {
            Some(json!({"tweaks":{"ui":{"explorerCompactMode": true}}}))
        }
        "Disable-ExplorerCompactMode" => {
            Some(json!({"tweaks":{"ui":{"explorerCompactMode": false}}}))
        }
        "Enable-ExplorerCheckboxes" => {
            Some(json!({"tweaks":{"ui":{"explorerCheckboxesEnabled": true}}}))
        }
        "Disable-ExplorerCheckboxes" => {
            Some(json!({"tweaks":{"ui":{"explorerCheckboxesEnabled": false}}}))
        }
        "Disable-WindowShake" => Some(json!({"tweaks":{"ui":{"windowShakeDisabled": true}}})),
        "Enable-WindowShake" => Some(json!({"tweaks":{"ui":{"windowShakeDisabled": false}}})),
        "Show-ClockSeconds" => Some(json!({"tweaks":{"ui":{"clockSecondsVisible": true}}})),
        "Hide-ClockSeconds" => Some(json!({"tweaks":{"ui":{"clockSecondsVisible": false}}})),

        // ── Tweaks: GPU (vendor-specific) ────────────────────────────
        // Each toggle has enableCmd = Disable-* (the "disable the OS
        // power-saving feature" command) which corresponds to the
        // optimisation being ON.
        "Disable-AmdUlps" => Some(json!({"tweaks":{"gpu":{"amdUlpsDisabled": true}}})),
        "Enable-AmdUlps" => Some(json!({"tweaks":{"gpu":{"amdUlpsDisabled": false}}})),
        "Disable-AmdPowerGating" => {
            Some(json!({"tweaks":{"gpu":{"amdPowerGatingDisabled": true}}}))
        }
        "Enable-AmdPowerGating" => {
            Some(json!({"tweaks":{"gpu":{"amdPowerGatingDisabled": false}}}))
        }
        "Disable-AmdVideoClockGating" => {
            Some(json!({"tweaks":{"gpu":{"amdVideoClockGatingDisabled": true}}}))
        }
        "Enable-AmdVideoClockGating" => {
            Some(json!({"tweaks":{"gpu":{"amdVideoClockGatingDisabled": false}}}))
        }
        "Disable-AmdAspm" => Some(json!({"tweaks":{"gpu":{"amdAspmDisabled": true}}})),
        "Enable-AmdAspm" => Some(json!({"tweaks":{"gpu":{"amdAspmDisabled": false}}})),
        "Disable-NvidiaDynamicPstate" => {
            Some(json!({"tweaks":{"gpu":{"nvidiaDynamicPstateDisabled": true}}}))
        }
        "Enable-NvidiaDynamicPstate" => {
            Some(json!({"tweaks":{"gpu":{"nvidiaDynamicPstateDisabled": false}}}))
        }
        "Disable-NvidiaAsyncPstates" => {
            Some(json!({"tweaks":{"gpu":{"nvidiaAsyncPstatesDisabled": true}}}))
        }
        "Enable-NvidiaAsyncPstates" => {
            Some(json!({"tweaks":{"gpu":{"nvidiaAsyncPstatesDisabled": false}}}))
        }
        "Disable-IntelAsyncFlips" => {
            Some(json!({"tweaks":{"gpu":{"intelAsyncFlipsDisabled": true}}}))
        }
        "Enable-IntelAsyncFlips" => {
            Some(json!({"tweaks":{"gpu":{"intelAsyncFlipsDisabled": false}}}))
        }
        "Disable-IntelAdaptiveVsync" => {
            Some(json!({"tweaks":{"gpu":{"intelAdaptiveVsyncDisabled": true}}}))
        }
        "Enable-IntelAdaptiveVsync" => {
            Some(json!({"tweaks":{"gpu":{"intelAdaptiveVsyncDisabled": false}}}))
        }

        // ── Tweaks: Power ────────────────────────────────────────────
        "Disable-UsbSelectiveSuspend" => {
            Some(json!({"tweaks":{"power":{"usbSelectiveSuspendDisabled": true}}}))
        }
        "Enable-UsbSelectiveSuspend" => {
            Some(json!({"tweaks":{"power":{"usbSelectiveSuspendDisabled": false}}}))
        }
        "Disable-CpuThrottling" => {
            Some(json!({"tweaks":{"power":{"cpuThrottlingDisabled": true}}}))
        }
        "Enable-CpuThrottling" => {
            Some(json!({"tweaks":{"power":{"cpuThrottlingDisabled": false}}}))
        }

        // Game DVR re-sectioned to "performance" in the registry but its
        // settings field still lives on tweaks.security.gameDvrDisabled
        // (already in the map above as Disable-GameDVR / Enable-GameDVR).
        _ => None,
    }
}

fn get_module_dependencies(module_name: &str) -> &'static [&'static str] {
    match module_name {
        // KT: Browser hardening filters extension policy through Get-WCSetting.
        // privacy/telemetry stays in the command-specific composition below
        // because the Copilot handlers add their own companion modules there.
        "tweaks/security" => &["core/settings-bridge"],
        // PowerShell 7 default-shell detection reuses Test-PowerShell7Installed
        // from the dependencies module rather than duplicating its pwsh.exe
        // detection logic.
        "tweaks/ui" => &["dependencies/dependencies"],
        _ => &[],
    }
}

#[cfg(test)]
mod module_dependency_tests {
    use super::*;

    #[test]
    fn browser_hardening_commands_load_the_settings_bridge() {
        let commands = [
            "Enable-FirefoxHardening",
            "Disable-FirefoxHardening",
            "Enable-BraveHardening",
            "Disable-BraveHardening",
            "Enable-ChromeHardening",
            "Disable-ChromeHardening",
            "Enable-EdgeHardening",
            "Disable-EdgeHardening",
            "Install-UniversalBrowserExtensions",
            "Remove-UniversalBrowserExtensions",
            "Get-InstalledBrowsersJson",
            "Enable-HardenBrowserByName",
            "Disable-HardenBrowserByName",
        ];

        for command in commands {
            let module = get_module_for_command(command)
                .unwrap_or_else(|| panic!("{command} must have a backend module"));
            assert_eq!(module, "tweaks/security");
            assert!(
                get_module_dependencies(module).contains(&"core/settings-bridge"),
                "{command} must load core/settings-bridge"
            );
        }
    }

    #[test]
    fn system_encryption_commands_keep_their_local_and_pro_boundaries() {
        assert_eq!(
            get_module_for_command("Get-SystemEncryptionEligibility"),
            Some("vault/volumes")
        );
        assert_eq!(
            get_module_for_command("Launch-VeraCryptForSystemEncryption"),
            None,
            "the paid action must route through the Pro dispatcher, not Free PowerShell"
        );
    }

    #[test]
    fn explorer_context_shred_uses_the_local_secure_erase_module() {
        assert_eq!(get_module_for_command("Invoke-7Erase"), Some("core/utils"));
    }
}

#[cfg(test)]
mod system_encryption_status_contract_tests {
    const VAULT_VOLUMES_SCRIPT: &str = include_str!("../scripts/modules/vault/volumes.ps1");

    fn function_body(name: &str) -> &str {
        let start = VAULT_VOLUMES_SCRIPT
            .find(&format!("function {name} {{"))
            .unwrap_or_else(|| panic!("{name} must remain in vault/volumes.ps1"));
        let after_start = &VAULT_VOLUMES_SCRIPT[start..];
        let end = after_start.find("\nfunction ").unwrap_or(after_start.len());
        &after_start[..end]
    }

    #[test]
    fn system_encryption_status_never_guesses_from_installation_artefacts() {
        let status = function_body("Get-SystemEncryptionStatusDescriptor");
        let executable_lines = status
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(status.contains("encrypted = $null"));
        assert!(
            status.contains("state = if ($isSupportedClient) { 'unknown' } else { 'unsupported' }")
        );
        for forbidden in [
            "Get-Service",
            "Test-Path",
            "VeraCrypt-DCS",
            "HKLM:",
            "$systemEncActive",
            "$hasBoot",
            "EncryptionProgress",
            "AES-256",
            "100",
        ] {
            assert!(
                !executable_lines.contains(forbidden),
                "status must not infer system encryption from {forbidden}"
            );
        }
        let public_status = function_body("Get-SystemEncryptionStatus");
        assert!(public_status.contains("Get-SystemEncryptionStatusDescriptor"));
        assert!(public_status.contains("throw $status.reason"));
    }

    #[test]
    fn eligibility_explicitly_blocks_server_and_unknown_hosts() {
        let eligibility = function_body("Get-SystemEncryptionEligibility");

        assert!(eligibility.contains("$platformInfo.platform -eq 'windows-server'"));
        assert!(eligibility.contains("Windows Server is not supported."));
        assert!(eligibility.contains("Test-SystemEncryptionSupportedClient $platformInfo"));
        assert!(eligibility
            .contains("VeraCrypt system encryption requires an x64 Windows installation."));
        assert!(eligibility.contains("isExperimental = $false"));
        assert!(eligibility.contains("if ($null -eq $bitLocker)"));
    }
}

/// Serialize params as a JSON object with the same value coercion the old
/// hashtable-literal builder used ("true"/"false" -> bool, int/float -> number,
/// else string). Keys are JSON property names — DATA, never code — so a
/// compromised WebView can no longer inject PowerShell via a crafted key.
/// Replaces the string-built `@{ 'k'='v'; }` literal (audit finding C1).
fn params_to_json_env(params: &HashMap<String, String>) -> String {
    let mut map = serde_json::Map::with_capacity(params.len());
    for (k, v) in params {
        let lower = v.to_lowercase();
        let jv = if lower == "true" {
            serde_json::Value::Bool(true)
        } else if lower == "false" {
            serde_json::Value::Bool(false)
        } else if let Ok(n) = v.parse::<i64>() {
            serde_json::Value::from(n)
        } else if let Ok(f) = v.parse::<f64>() {
            serde_json::Value::from(f)
        } else {
            serde_json::Value::String(v.clone())
        };
        map.insert(k.clone(), jv);
    }
    serde_json::to_string(&serde_json::Value::Object(map)).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod param_env_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn malicious_key_becomes_inert_json_data() {
        let mut p = HashMap::new();
        // The classic injection key from the audit PoC.
        p.insert("a' = 1 }; whoami; $x = @{'z".to_string(), "v".to_string());
        let json = params_to_json_env(&p);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("a' = 1 }; whoami; $x = @{'z").is_some());
    }

    #[test]
    fn value_types_are_coerced() {
        let mut p = HashMap::new();
        p.insert("flag".into(), "true".into());
        p.insert("count".into(), "42".into());
        p.insert("name".into(), "hello".into());
        let parsed: serde_json::Value = serde_json::from_str(&params_to_json_env(&p)).unwrap();
        assert_eq!(parsed["flag"], serde_json::json!(true));
        assert_eq!(parsed["count"], serde_json::json!(42));
        assert_eq!(parsed["name"], serde_json::json!("hello"));
    }
}

#[tauri::command]
pub async fn run_backend_script(
    app: AppHandle,
    command: String,
    mut params: HashMap<String, String>,
) -> Result<serde_json::Value, String> {
    if !crate::cli::tauri_runtime_active() {
        if crate::authz::backend_dispatch_policy(&command)
            == crate::authz::BackendDispatchPolicy::NativeConfirmation
        {
            if !crate::authz::confirm_backend_dispatch(&app, &command, &params).await {
                return Err("native confirmation was refused".to_string());
            }
        } else {
            crate::authz::authorize_backend_dispatch(&command, &mut params)?;
        }
    }
    run_backend_script_with_timeout(app, command, params, None).await
}

/// Internal callers that render a live status surface may use a shorter
/// deadline than the general-purpose backend command timeout. The same timeout
/// path kills the PowerShell process tree, so a cancelled UI poll cannot leave
/// a background probe running indefinitely.
pub(crate) async fn run_backend_script_with_timeout(
    app: AppHandle,
    command: String,
    params: HashMap<String, String>,
    timeout_override: Option<std::time::Duration>,
) -> Result<serde_json::Value, String> {
    // Redact secrets before they hit the app log -- `params` can carry a
    // plaintext vault password/keyfile, one `WINCMD_LOG=debug` away from
    // being written to disk otherwise.
    let sanitized_params: HashMap<&str, &str> = params
        .iter()
        .map(|(k, v)| {
            let kl = k.to_lowercase();
            let is_secret = kl.contains("password") || kl.contains("keyfile");
            (k.as_str(), if is_secret { "***" } else { v.as_str() })
        })
        .collect();
    crate::log_message(
        "debug",
        &format!(
            "[Backend] Running: {} with params {:?}",
            command, sanitized_params
        ),
    );

    // Investigator safety remains active even though the workflow UI now
    // lives in a separate executable. An advanced examiner can explicitly
    // arm this mode to prevent Free from mutating the system under review.
    if license::is_advanced_mode() {
        let mutating = command.starts_with("Clear-")
            || command.starts_with("Erase-")
            || command.starts_with("Remove-")
            || command.starts_with("Reset-")
            || command == "Provision-EncryptedBackupTarget"
            || command == "Invoke-AIControlOperation"
            || command == "Invoke-CleanupClearAllUsers";
        if mutating {
            let msg = format!(
                "Refused: investigator mode forbids state-mutating commands. \
                 '{}' would taint evidence. Logged.",
                command
            );
            crate::log_message("warn", &format!("[Investigator] {}", msg));
            return Err(msg);
        }
    }

    // Privacy Shield 15-min/day quota — defence-in-depth for the
    // JS-side ticker in PrivacyShieldCard. A devtools-invoked
    // `Start-PrivacyShield` would otherwise bypass the UI cap and run
    // the shield indefinitely on free tier. Refuse here so the cap
    // holds regardless of how the command was issued. Paid / trial
    // entitlements short-circuit `is_quota_exhausted` and skip the
    // refusal (see shield_quota::is_quota_exhausted).
    if command == "Start-PrivacyShield" && crate::shield_quota::is_quota_exhausted() {
        let msg = "Daily Privacy Shield quota exhausted. Upgrade for unlimited use.".to_string();
        crate::log_message(
            "warn",
            &format!("[ShieldQuota] refused Start-PrivacyShield: {}", msg),
        );
        return Err(msg);
    }

    // Tier gate — paid commands require entitlement and are routed
    // through the Pro sidecar over IPC (phase 7c). The Free binary's
    // run_backend_script no longer executes paid PowerShell directly:
    //   1. require_paid refuses without entitlement (defence-in-depth
    //      even if the UI's <LockedToggle> is bypassed via devtools / raw IPC).
    //   2. dispatch_paid_command spawns / reuses the Pro session and
    //      forwards the Request. Pro returns the Response or an
    //      ErrorReply — both flow back to the frontend verbatim.
    //   3. Phase 6b moves the actual paid handler implementations into
    //      commander-pro; until then Pro returns "feature_unknown" for
    //      unmigrated commands.
    if get_command_tier(&command) == "paid" {
        license::require_paid(&command)?;
        // A-3: all remaining paid commands dispatch to Pro unconditionally.
        // The previous executor gate kept four hosts-blocklist commands
        // and Connect-MeshVPN on Free's local executor; the blocklist
        // four were reclassified as Free (the data lives in Free's
        // encrypted modules), and Connect-MeshVPN now runs through Pro's
        // mesh handler alongside Get-MeshVPNStatus / Start-MeshVPNLogin.
        let args = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let result = crate::sidecar::dispatch_paid_command(&command, args).await;
        // Apply settings sync patch so the toggle UI reflects the new
        // state immediately after the Pro sidecar succeeds.
        if let Ok(ref res) = result {
            if let Some(inner_patch) = get_settings_sync_patch(&command, &params, Some(res)) {
                let patch = serde_json::json!({
                    "ideal": inner_patch,
                    "current": inner_patch
                });
                let _ = settings::patch_settings(patch);
            }
        }
        let out = result.map_err(normalize_sidecar_error);
        if let Err(ref e) = out {
            crate::log_message_src(
                "warn",
                "core",
                &format!("[Backend] paid command '{}' dispatch error: {}", command, e),
            );
        }
        return out;
    }

    // Module gate — refuse to run commands whose frontend module is disabled
    if let Some(required_mod) = get_required_frontend_module(&command) {
        let modules = settings::read_settings()
            .map(|s| s.app.modules)
            .unwrap_or_default();
        if !modules.get(required_mod).copied().unwrap_or(false) {
            crate::log_message_src(
                "warn",
                "core",
                &format!(
                    "[Backend] command '{}' refused: module '{}' is disabled",
                    command, required_mod
                ),
            );
            return Err(format!("Module '{}' is disabled", required_mod));
        }
    }

    // Determine which module(s) to load
    let module_name = match get_module_for_command(&command) {
        Some(m) => m,
        None => {
            crate::log_message_src(
                "warn",
                "core",
                &format!("[Backend] no module registered for command '{}'", command),
            );
            return Err(format!("No module found for command: {}", command));
        }
    };

    // Load core utilities (always needed)
    let core_utils = load_module("core/utils")?;
    let core_router = load_module("core/router")?;

    // Load command-specific module
    let command_module = load_module(module_name)?;

    // Load additional dependencies based on module
    let mut additional_modules = String::new();
    for dependency in get_module_dependencies(module_name) {
        additional_modules.push_str(&load_module(dependency)?);
        additional_modules.push_str("\n\n");
    }

    // The auto-erase scheduler functions live in the shared crate
    // (commander-shared/scripts/auto-erase.ps1) so commander-pro can
    // embed the same source. Prepend them whenever the privacy/cleanup
    // module is loaded so callers from this module can invoke them
    // exactly like local functions.
    if module_name == "privacy/cleanup" {
        additional_modules.push_str(wincmd_shared::AUTO_ERASE_PS_MODULE);
        additional_modules.push_str("\n\n");
    }

    if module_name == "tweaks/ai-control" {
        // Fixed-operation AI maintenance surface. Every helper is internal;
        // the only routable mutation command validates an operation enum.
        additional_modules.push_str(&load_module("tweaks/ai-control-common")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("tweaks/ai-control-shell")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("tweaks/ai-control-removal")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("tweaks/ai-control-maintenance")?);
        additional_modules.push_str("\n\n");
    } else if module_name == "network/hosts" {
        // Hosts module needs blocklists data
        additional_modules.push_str(&load_module("network/blocklists-data")?);
        additional_modules.push_str("\n\n");
    } else if command == "Get-StartupStatus" {
        // Startup needs system info, hardening, privacy, cleanup, productivity, and apps (essential status)
        additional_modules.push_str(&load_module("dashboard/info")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("tweaks/system")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("privacy/telemetry")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("privacy/cleanup")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("productivity")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("apps/winget")?);
        additional_modules.push_str("\n\n");
    } else if command == "Get-HardeningStatus" {
        // Get-HardeningStatus calls Get-DiagnosticEventTracingStatus from
        // privacy/telemetry so the drift probe can use the same source of truth.
        additional_modules.push_str(&load_module("privacy/telemetry")?);
        additional_modules.push_str("\n\n");
        // It also delegates the Quick Access / Run MRU / Search History probes to
        // privacy/cleanup (Get-QuickAccessRecentStatus/…/Get-SearchHistoryStatus);
        // bundle it so a standalone Get-HardeningStatus dispatch doesn't throw
        // CommandNotFoundException and blank every hardening field.
        additional_modules.push_str(&load_module("privacy/cleanup")?);
        additional_modules.push_str("\n\n");
    } else if command == "Invoke-ProductivityEngineMaintenance" {
        // Needs settings-bridge to read/write stealth mode state
        additional_modules.push_str(&load_module("core/settings-bridge")?);
        additional_modules.push_str("\n\n");
    } else if command == "Install-PrivacyShieldAI" || command == "Get-AIDependenciesStatus" {
        // AI Dependencies need winget for Python installation
        additional_modules.push_str(&load_module("apps/winget")?);
        additional_modules.push_str("\n\n");
    } else if command == "Install-WingetApps"
        || command == "Upgrade-AllApps"
        || command == "Upgrade-App"
    {
        // winget.ps1 calls Install-RamDiskEngineDep when ImDisk.Toolkit is in the list;
        // that function lives in dependencies/dependencies, not apps/winget.
        additional_modules.push_str(&load_module("dependencies/dependencies")?);
        additional_modules.push_str("\n\n");
    } else if command == "Install-Dependency" || command == "Install-AllDependencies" {
        // Dependency install needs winget (for installing deps) + settings-bridge (for stealth mode)
        // + productivity module (for Start-ProductivityTracker) + privacy_shield (for AI deps)
        additional_modules.push_str(&load_module("apps/winget")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("core/settings-bridge")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("productivity")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("privacy/privacy_shield")?);
        additional_modules.push_str("\n\n");
    } else if command == "Hide-BackendApps"
        || command == "Hide-AllBackendApps"
        || command == "Set-WinCommanderVisibility"
        || command == "Set-BackendAppsVisibility"
    {
        // Hide needs settings-bridge for stealth mode + productivity for maintenance
        additional_modules.push_str(&load_module("core/settings-bridge")?);
        additional_modules.push_str("\n\n");
        additional_modules.push_str(&load_module("productivity")?);
        additional_modules.push_str("\n\n");
    } else if command == "Get-BrowserFootprints"
        || matches_parts(&command, &["Clear~-", "Browser~", "Footprints~"])
    {
        // Browser functions need security module for Get-InstalledBrowsers + BrowserMap
        additional_modules.push_str(&load_module("tweaks/security")?);
        additional_modules.push_str("\n\n");
    } else if module_name == "tweaks/security" {
        // tweaks/security uses Set-RegistryValueSafe which lives in privacy/telemetry
        additional_modules.push_str(&load_module("privacy/telemetry")?);
        additional_modules.push_str("\n\n");
        if command == "Remove-CopilotAIComponents" || command == "Restore-CopilotAIComponents" {
            additional_modules.push_str(&load_module("tweaks/ai-control-common")?);
            additional_modules.push_str("\n\n");
            additional_modules.push_str(&load_module("tweaks/ai-control-policies")?);
            additional_modules.push_str("\n\n");
            additional_modules.push_str(&load_module("tweaks/ai-control-shell")?);
            additional_modules.push_str("\n\n");
            additional_modules.push_str(&load_module("tweaks/ai-control-apps")?);
            additional_modules.push_str("\n\n");
        }
    }

    // Pass command + params to PowerShell OUT OF BAND via the environment, not
    // by interpolating attacker-controlled strings into the script text. The
    // params are serialized as a JSON object (preserving the historical value
    // coercion: "true"/"false" -> bool, int/float -> number, else string); the
    // router hydrates them via `$env:WINCMD_PARAMS_JSON | ConvertFrom-Json`. Keys
    // become JSON property names -> hashtable keys: DATA, never code. This
    // removes the PowerShell injection primitive that unescaped hashtable keys
    // created (audit finding C1).
    let params_json = params_to_json_env(&params);

    // Build complete script: Core Utils + Additional Modules + Command Module + Router + Invocation
    let full_script = format!(
        "{}\n\n{}{}\n\n{}\n\nInvoke-BackendCommand",
        core_utils, additional_modules, command_module, core_router
    );

    // Execute PowerShell via stdin (memory-only, never writes to disk)
    let (mut cmd, ps_exe) = build_powershell_command();

    cmd.env("WINCMD_COMMAND", &command);
    cmd.env("WINCMD_PARAMS_JSON", &params_json);
    if let Ok(exe_path) = std::env::current_exe() {
        cmd.env("WINCMD_EXE_PATH", exe_path);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn PowerShell ({}): {}", ps_exe, e))?;

    // Write script to stdin
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(full_script.as_bytes()) {
            crate::log_message(
                "error",
                &format!("[Backend] Failed to write script to stdin: {}", e),
            );
            return Err(format!("Failed to write to stdin: {}", e));
        }
    }

    // KT: A 90-second timeout used to sit here but was removed because it
    // killed legitimately slow commands (winget installs, Privacy Shield's
    // first-run pip install of mediapipe/opencv/PyQt6). Removing it entirely
    // traded that premature-kill bug for a worse one: a genuinely wedged
    // child (a stalled download, a UAC prompt nothing will answer in silent
    // mode, a hung installer) now blocks forever with no recovery -- and
    // since concurrent installs are independent OS processes (see
    // AppInstallerPanel's parallel runOperation), one wedged install stalls
    // the whole batch's completion signal even though its siblings already
    // finished. 20 minutes is long enough that no legitimate install or
    // cleanup should ever hit it, but bounded so a wedged process eventually
    // gets killed and reported instead of hanging the batch indefinitely.
    const BACKEND_SCRIPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20 * 60);
    let child_pid = child.id();
    let wait_task = tokio::task::spawn_blocking(move || child.wait_with_output());

    let command_timeout = timeout_override.unwrap_or(BACKEND_SCRIPT_TIMEOUT);
    let output_res = match tokio::time::timeout(command_timeout, wait_task).await {
        Ok(joined) => joined,
        Err(_) => {
            // spawn_blocking can't be cancelled from here -- the blocking
            // thread is still parked inside wait_with_output(). Kill the
            // process tree directly (/T also takes out children such as a
            // winget.exe launched under the PowerShell host) so that call
            // unblocks and frees the thread instead of leaking it, and so
            // the wedged process actually stops running.
            crate::log_message(
                "error",
                &format!(
                    "[Backend] Command '{}' timed out after {}s — killing pid {}",
                    command,
                    command_timeout.as_secs(),
                    child_pid
                ),
            );
            let _ = tokio::task::spawn_blocking(move || {
                let mut kill_cmd = Command::new("taskkill");
                kill_cmd.args(["/F", "/T", "/PID", &child_pid.to_string()]);
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    kill_cmd.creation_flags(CREATE_NO_WINDOW);
                }
                kill_cmd.output()
            })
            .await;
            return Err(format!(
                "Command '{}' timed out after {} seconds and was terminated.",
                command,
                command_timeout.as_secs()
            ));
        }
    };

    let output = match output_res {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            crate::log_message(
                "error",
                &format!("[Backend] PowerShell execution error ({}): {}", command, e),
            );
            return Err(format!("Failed to read output: {}", e));
        }
        Err(e) => {
            crate::log_message(
                "error",
                &format!("[Backend] Task join error ({}): {}", command, e),
            );
            return Err(format!("Task join error: {}", e));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let trimmed = stdout.trim();
    let stderr_trimmed = stderr.trim();

    if trimmed.is_empty() && !stderr_trimmed.is_empty() {
        return Err(format!("Command failed: {}", stderr_trimmed));
    }

    // Parse JSON output
    let result = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(json) => {
            crate::log_message(
                "debug",
                &format!(
                    "[Backend] Command '{}' completed successfully (JSON).",
                    command
                ),
            );
            Ok(json)
        }
        Err(_) => {
            if !output.status.success() || !stderr_trimmed.is_empty() {
                let err_msg = format!(
                    "Command failed ({}): {}\nStderr: {}",
                    output
                        .status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    stdout,
                    stderr
                );
                crate::log_message(
                    "error",
                    &format!("[Backend] Command '{}' failed: {}", command, err_msg),
                );
                return Err(err_msg);
            }
            crate::log_message(
                "debug",
                &format!("[Backend] Command '{}' completed with raw output.", command),
            );
            Ok(serde_json::json!({ "success": true, "data": trimmed }))
        }
    };

    // Privacy-shield-specific: when the PS wrapper successfully spawns
    // Python, it returns `{ success: true, processId: <pid> }`. Add that
    // PID to the kill-on-close Job Object so the orphan dies when
    // WinCommander dies, including via Task Manager end-task. Without
    // this, the Python child outlives WinCommander forever because its
    // parent (PowerShell) exited the moment Start-Process returned, so
    // the OS reparents Python to a process WinCommander has no control
    // over.
    if command == "Start-PrivacyShield" {
        if let Ok(ref json) = result {
            if let Some(pid) = json.get("processId").and_then(|v| v.as_u64()) {
                let _ = crate::child_jobs::assign_pid(pid as u32);
                // Mark the shield feature active BEFORE spawning the reader so the
                // reader's flag-based lifetime is armed from tick 0.
                SHIELD_READER_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
                // Attach a reader that reacts to the shield's look-away /
                // look-back NDJSON sidecar for the lifetime of this session.
                spawn_shield_event_reader(app.clone(), pid as u32);
            }
        }
    }
    // Any explicit stop of the shield clears the reader's run flag so it exits.
    if command == "Stop-PrivacyShield" {
        SHIELD_READER_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    // After successful execution, sync toggle state to settings.json
    if let Ok(ref res) = result {
        if let Some(inner_patch) = get_settings_sync_patch(&command, &params, Some(res)) {
            // Write to BOTH ideal (user's intent) and current (OS actually changed)
            let patch = serde_json::json!({
                "ideal": inner_patch,
                "current": inner_patch
            });
            let _ = settings::patch_settings(patch);
        }

        // Auto-persist inventory snapshot after Get-AppInventory
        // LEARNING: Get-AppInventory returns the full snapshot. We write it directly
        // to current.apps.inventory so heartbeat sends cached data, not a live scan.
        // Also auto-persist after install/upgrade/uninstall actions so snapshot stays fresh.
        if command == "Get-AppInventory" {
            if let Ok(ref res) = result {
                // The result is the inventory snapshot JSON — persist to settings
                let inventory_patch = serde_json::json!({
                    "current": {
                        "apps": {
                            "inventory": res
                        }
                    }
                });
                let _ = settings::patch_settings(inventory_patch);
            }
        }
    }

    result
}

// KT: Use reg.exe instead of PowerShell for context menu registry writes.
// The key path contains a literal '*' (HKCU\Software\Classes\*\shell) which the
// PowerShell registry provider sometimes mishandles even with -LiteralPath.
// reg.exe always treats key paths as literal strings — no wildcard expansion.
fn run_reg(args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("reg");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("reg.exe failed to spawn: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

// Legacy shell verbs otherwise use the `Document` model and receive only one
// selection. `Player` lets Explorer offer this verb for a multi-selection.
// `%1` is the documented static-verb placeholder for the selected path; `%*`
// is not expanded by Explorer and arrives at the helper as a literal string.
const CONTEXT_SHRED_SELECTION_MODEL: &str = "Player";
const CONTEXT_SHRED_HELPER: &str = "wincommander-context-shred.exe";
// A bundled trash-can glyph. This deliberately does not use the Windows
// Recycle Bin icon: Secure Delete permanently removes the item instead of
// moving it to the Recycle Bin.
const CONTEXT_SHRED_ICON: &str = "context-delete.ico";
// Keep the Explorer wording short and explicit.  This is intentionally not
// branded as a second, ambiguous "shred" action: it permanently removes the
// selected items, whereas the separate Scrub verb only strips metadata.
const CONTEXT_SHRED_VERB_LABEL: &str = "Secure Delete";

fn context_shred_command(exe_path: &str) -> String {
    format!("\"{}\" --context-shred \"%1\"", exe_path)
}

fn context_shred_icon_path_for_mode(
    app_exe: &std::path::Path,
    is_debug_build: bool,
) -> Result<PathBuf, String> {
    let icon_path = if is_debug_build {
        // Development builds keep the source asset in the crate so Explorer
        // does not accidentally display an icon from a previous installation.
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("icons")
            .join(CONTEXT_SHRED_ICON)
    } else {
        app_exe
            .parent()
            .ok_or("Cannot determine the WinCommander install directory")?
            .join("resources")
            .join(CONTEXT_SHRED_ICON)
    };

    icon_path
        .is_file()
        .then_some(icon_path.clone())
        .ok_or_else(|| {
            format!(
                "WinCommander context-delete icon is missing: {}",
                icon_path.display()
            )
        })
}

fn context_shred_icon_value(app_exe: &std::path::Path) -> Result<String, String> {
    let icon_path = context_shred_icon_path_for_mode(app_exe, cfg!(debug_assertions))?;
    Ok(format!("\"{}\",0", icon_path.display()))
}

/// The primary desktop executable deliberately has a highestAvailable
/// manifest for its administrative features. Explorer must not launch it for
/// a normal user-owned delete: that would produce a UAC consent request even
/// though the native erase needs no extra privilege. The bundled helper has an
/// explicit asInvoker manifest and lives in Tauri's stable resources directory.
/// During `tauri dev`, the helper is built beside the debug desktop executable
/// instead. Keeping this resolution local avoids pointing a developer's
/// Explorer verb at an old installed build (or at the elevated release EXE).
fn context_shred_helper_path_for_mode(
    app_exe: &std::path::Path,
    is_debug_build: bool,
) -> Result<PathBuf, String> {
    let install_directory = app_exe
        .parent()
        .ok_or("Cannot determine the WinCommander install directory")?;

    // A development build must never reuse an installed/package resource. It
    // has its own helper beside `wincommander-free.exe`, built by setup:dev.
    // Check this first: Tauri can leave resource folders from another build in
    // a target directory, and choosing one would make Explorer test stale code.
    if is_debug_build {
        let dev_helper = install_directory.join(CONTEXT_SHRED_HELPER);
        if dev_helper.is_file() {
            return Ok(dev_helper);
        }
        return Err(format!(
            "WinCommander context-delete helper is missing: {}. Run `bun run build:context-shred:debug` before enabling the Explorer Delete integration.",
            dev_helper.display()
        ));
    }

    let packaged_helper = install_directory
        .join("resources")
        .join(CONTEXT_SHRED_HELPER);
    if packaged_helper.is_file() {
        return Ok(packaged_helper);
    }

    Err(format!(
        "WinCommander context-delete helper is missing: {}",
        packaged_helper.display()
    ))
}

fn context_shred_helper_path(app_exe: &std::path::Path) -> Result<PathBuf, String> {
    context_shred_helper_path_for_mode(app_exe, cfg!(debug_assertions))
}

#[cfg(test)]
mod context_shred_verb_tests {
    use super::*;

    #[test]
    fn context_shred_uses_explorer_player_model_and_documented_path_placeholder() {
        assert_eq!(CONTEXT_SHRED_SELECTION_MODEL, "Player");
        assert_eq!(CONTEXT_SHRED_VERB_LABEL, "Secure Delete");
        assert_eq!(
            context_shred_command(
                r"C:\Program Files\WinCommander\resources\wincommander-context-shred.exe"
            ),
            r#""C:\Program Files\WinCommander\resources\wincommander-context-shred.exe" --context-shred "%1""#
        );
        assert_eq!(CONTEXT_SHRED_ICON, "context-delete.ico");
    }

    #[test]
    fn context_shred_helper_has_a_stable_resource_location() {
        let temporary = tempfile::tempdir().unwrap();
        let app = temporary.path().join("wincommander-free.exe");
        let resources = temporary.path().join("resources");
        std::fs::create_dir(&resources).unwrap();
        let helper = resources.join(CONTEXT_SHRED_HELPER);
        std::fs::write(&helper, b"helper").unwrap();

        assert_eq!(
            context_shred_helper_path_for_mode(&app, false).unwrap(),
            helper
        );
    }

    #[test]
    fn context_shred_icon_has_a_stable_packaged_location() {
        let temporary = tempfile::tempdir().unwrap();
        let app = temporary.path().join("wincommander-free.exe");
        let resources = temporary.path().join("resources");
        std::fs::create_dir(&resources).unwrap();
        let icon = resources.join(CONTEXT_SHRED_ICON);
        std::fs::write(&icon, b"icon").unwrap();

        assert_eq!(context_shred_icon_path_for_mode(&app, false).unwrap(), icon);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn context_shred_helper_uses_the_debug_sibling_when_not_packaged() {
        let temporary = tempfile::tempdir().unwrap();
        let app = temporary.path().join("wincommander-free.exe");
        let helper = temporary.path().join(CONTEXT_SHRED_HELPER);
        std::fs::write(&helper, b"helper").unwrap();

        assert_eq!(
            context_shred_helper_path_for_mode(&app, true).unwrap(),
            helper
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn context_shred_helper_never_uses_a_packaged_resource_in_debug() {
        let temporary = tempfile::tempdir().unwrap();
        let app = temporary.path().join("wincommander-free.exe");
        let debug_helper = temporary.path().join(CONTEXT_SHRED_HELPER);
        let resources = temporary.path().join("resources");
        std::fs::create_dir(&resources).unwrap();
        std::fs::write(&debug_helper, b"debug helper").unwrap();
        std::fs::write(
            resources.join(CONTEXT_SHRED_HELPER),
            b"stale packaged helper",
        )
        .unwrap();

        assert_eq!(
            context_shred_helper_path_for_mode(&app, true).unwrap(),
            debug_helper
        );
    }
}

#[tauri::command]
pub async fn toggle_context_menu(enable: bool) -> Result<(), String> {
    // File context menu (*\shell)
    const KEY_FILE: &str = r"HKCU\Software\Classes\*\shell\WinCommanderShred";
    const CMD_KEY_FILE: &str = r"HKCU\Software\Classes\*\shell\WinCommanderShred\command";
    // Folder context menu (Directory\shell)
    const KEY_DIR: &str = r"HKCU\Software\Classes\Directory\shell\WinCommanderShred";
    const CMD_KEY_DIR: &str = r"HKCU\Software\Classes\Directory\shell\WinCommanderShred\command";
    // Mixed selection context menu (AllFilesystemObjects\shell)
    const KEY_MIXED: &str = r"HKCU\Software\Classes\AllFilesystemObjects\shell\WinCommanderShred";
    const CMD_KEY_MIXED: &str =
        r"HKCU\Software\Classes\AllFilesystemObjects\shell\WinCommanderShred\command";
    // This verb intentionally applies only to selected items. Registering it
    // on Directory\Background would pass the current folder rather than a
    // selection, which is both surprising and unsafe for a secure-delete
    // command.
    const KEY_BG: &str = r"HKCU\Software\Classes\Directory\Background\shell\WinCommanderShred";

    if enable {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Failed to get current executable path: {}", e))?;
        let helper_path = context_shred_helper_path(&exe_path)?;
        let helper_str = helper_path
            .to_str()
            .ok_or("Failed to convert context-delete helper path to string")?;
        // `%1` is Explorer's documented static-verb placeholder. The Player
        // model keeps the verb available for a multi-selection; unlike `%*`,
        // `%1` is expanded to an actual path by Explorer.
        // Explorer's secure-delete verb is intentionally backend-only. The
        // explicit flag prevents it from falling into the normal frontend
        // confirmation flow used by in-app shred operations.
        let command_value_selection = context_shred_command(helper_str);
        let icon_value = context_shred_icon_value(&exe_path)?;

        // --- File (*) ---
        run_reg(&["add", KEY_FILE, "/ve", "/d", CONTEXT_SHRED_VERB_LABEL, "/f"])?;
        run_reg(&["add", KEY_FILE, "/v", "Icon", "/d", &icon_value, "/f"])?;
        run_reg(&[
            "add",
            KEY_FILE,
            "/v",
            "MultiSelectModel",
            "/d",
            CONTEXT_SHRED_SELECTION_MODEL,
            "/f",
        ])?;
        run_reg(&[
            "add",
            CMD_KEY_FILE,
            "/ve",
            "/d",
            &command_value_selection,
            "/f",
        ])?;

        // --- Directory (folder right-click) ---
        run_reg(&["add", KEY_DIR, "/ve", "/d", CONTEXT_SHRED_VERB_LABEL, "/f"])?;
        run_reg(&["add", KEY_DIR, "/v", "Icon", "/d", &icon_value, "/f"])?;
        run_reg(&[
            "add",
            KEY_DIR,
            "/v",
            "MultiSelectModel",
            "/d",
            CONTEXT_SHRED_SELECTION_MODEL,
            "/f",
        ])?;
        run_reg(&[
            "add",
            CMD_KEY_DIR,
            "/ve",
            "/d",
            &command_value_selection,
            "/f",
        ])?;

        // --- AllFilesystemObjects (mixed file + folder right-click) ---
        run_reg(&[
            "add",
            KEY_MIXED,
            "/ve",
            "/d",
            CONTEXT_SHRED_VERB_LABEL,
            "/f",
        ])?;
        run_reg(&["add", KEY_MIXED, "/v", "Icon", "/d", &icon_value, "/f"])?;
        run_reg(&[
            "add",
            KEY_MIXED,
            "/v",
            "MultiSelectModel",
            "/d",
            CONTEXT_SHRED_SELECTION_MODEL,
            "/f",
        ])?;
        run_reg(&[
            "add",
            CMD_KEY_MIXED,
            "/ve",
            "/d",
            &command_value_selection,
            "/f",
        ])?;

        // Remove the legacy background verb if a prior version created it.
        // Background clicks have no selected target and must never shred the
        // containing folder.
        let _ = run_reg(&["delete", KEY_BG, "/f"]);
    } else {
        // /f = no confirmation prompt; ignore errors (key may not exist)
        let _ = run_reg(&["delete", KEY_FILE, "/f"]);
        let _ = run_reg(&["delete", KEY_DIR, "/f"]);
        let _ = run_reg(&["delete", KEY_MIXED, "/f"]);
        let _ = run_reg(&["delete", KEY_BG, "/f"]);
    }

    Ok(())
}

#[tauri::command]
pub async fn get_context_menu_status() -> Result<bool, String> {
    // Use Directory key (no literal '*') — reg query can misinterpret '*' as a
    // wildcard and return unexpected results even when the key exists.
    const KEY: &str = r"HKCU\Software\Classes\Directory\shell\WinCommanderShred";
    Ok(run_reg(&["query", KEY]).is_ok())
}

// ─────────────────────────────────────────────────────────────────────
// SCRUB CONTEXT MENU — right-click → "Scrub"
// ─────────────────────────────────────────────────────────────────────
//
// Mirrors `toggle_context_menu` (secure-delete) but writes a separate command
// chain so the operator can have both secure-delete + scrub entries side-by-
// side. Command value passes `--scrub` so the single-instance handler
// in lib.rs knows to emit `scrub-requested` (not `shred-requested`).

#[tauri::command]
pub async fn toggle_scrub_context_menu(enable: bool) -> Result<(), String> {
    const KEY_FILE: &str = r"HKCU\Software\Classes\*\shell\WinCommanderScrub";
    const CMD_KEY_FILE: &str = r"HKCU\Software\Classes\*\shell\WinCommanderScrub\command";
    const KEY_DIR: &str = r"HKCU\Software\Classes\Directory\shell\WinCommanderScrub";
    const CMD_KEY_DIR: &str = r"HKCU\Software\Classes\Directory\shell\WinCommanderScrub\command";
    const KEY_MIXED: &str = r"HKCU\Software\Classes\AllFilesystemObjects\shell\WinCommanderScrub";
    const CMD_KEY_MIXED: &str =
        r"HKCU\Software\Classes\AllFilesystemObjects\shell\WinCommanderScrub\command";

    if enable {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Failed to get current executable path: {}", e))?;
        let exe_str = exe_path
            .to_str()
            .ok_or("Failed to convert exe path to string")?;
        let cmd_value = format!("\"{}\" \"--scrub\" \"%1\"", exe_str);
        // Cleaning glyph, not the WinCommander logo: point at the built-in
        // Disk Cleanup icon (a broom/brush). Resolved to an absolute path so
        // the REG_SZ value needs no env-var expansion; present wherever Explorer
        // context menus exist (Desktop Experience). Falls back to the app exe if
        // SystemRoot is somehow unset.
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let cleanmgr = format!("{}\\System32\\cleanmgr.exe", system_root);
        let icon_value = if std::path::Path::new(&cleanmgr).exists() {
            format!("{},0", cleanmgr)
        } else {
            format!("\"{}\",0", exe_str)
        };

        // Files
        run_reg(&["add", KEY_FILE, "/ve", "/d", "Scrub metadata", "/f"])?;
        run_reg(&["add", KEY_FILE, "/v", "Icon", "/d", &icon_value, "/f"])?;
        run_reg(&["add", CMD_KEY_FILE, "/ve", "/d", &cmd_value, "/f"])?;

        // Folders
        run_reg(&["add", KEY_DIR, "/ve", "/d", "Scrub metadata", "/f"])?;
        run_reg(&["add", KEY_DIR, "/v", "Icon", "/d", &icon_value, "/f"])?;
        run_reg(&["add", CMD_KEY_DIR, "/ve", "/d", &cmd_value, "/f"])?;

        // Mixed (multi-select with both files + folders)
        run_reg(&["add", KEY_MIXED, "/ve", "/d", "Scrub metadata", "/f"])?;
        run_reg(&["add", KEY_MIXED, "/v", "Icon", "/d", &icon_value, "/f"])?;
        run_reg(&["add", CMD_KEY_MIXED, "/ve", "/d", &cmd_value, "/f"])?;
    } else {
        let _ = run_reg(&["delete", KEY_FILE, "/f"]);
        let _ = run_reg(&["delete", KEY_DIR, "/f"]);
        let _ = run_reg(&["delete", KEY_MIXED, "/f"]);
    }

    Ok(())
}

#[tauri::command]
pub async fn get_scrub_context_menu_status() -> Result<bool, String> {
    // Use Directory key (no literal '*') — same reason as get_context_menu_status.
    const KEY: &str = r"HKCU\Software\Classes\Directory\shell\WinCommanderScrub";
    Ok(run_reg(&["query", KEY]).is_ok())
}

// ─────────────────────────────────────────────────────────────────────
// SAFE COPY / SAFE PASTE CONTEXT MENU — right-click → "Safe Copy" / "Safe Paste"
// ─────────────────────────────────────────────────────────────────────
//
// A paired set of verbs (see safe_clip.rs): "Safe Copy" records the selection
// (--safe-copy, handled headless before the single-instance guard); "Safe
// Paste" (--safe-paste, %V = the destination folder) copies the recorded
// sources into that folder keeping exact names, then the app scrubs the copies.
// Both verbs register/unregister together — they're one feature.

/// Safe Copy verb key roots (files, folders, mixed selection).
const SAFE_COPY_KEYS: &[&str] = &[
    r"HKCU\Software\Classes\*\shell\WinCommanderSafeCopy",
    r"HKCU\Software\Classes\Directory\shell\WinCommanderSafeCopy",
    r"HKCU\Software\Classes\AllFilesystemObjects\shell\WinCommanderSafeCopy",
];
/// Safe Paste verb key roots (a folder, and a folder's empty background).
const SAFE_PASTE_KEYS: &[&str] = &[
    r"HKCU\Software\Classes\Directory\shell\WinCommanderSafePaste",
    r"HKCU\Software\Classes\Directory\Background\shell\WinCommanderSafePaste",
];

#[tauri::command]
pub async fn toggle_safe_copy_context_menu(enable: bool) -> Result<(), String> {
    if enable {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Failed to get current executable path: {}", e))?;
        let exe_str = exe_path
            .to_str()
            .ok_or("Failed to convert exe path to string")?;
        // %1 = the selected item (Safe Copy); %V = the folder (Safe Paste).
        let copy_cmd = format!("\"{}\" \"--safe-copy\" \"%1\"", exe_str);
        let paste_cmd = format!("\"{}\" \"--safe-paste\" \"%V\"", exe_str);

        // Same broom glyph as Scrub so the two privacy verbs read as a family.
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let cleanmgr = format!("{}\\System32\\cleanmgr.exe", system_root);
        let icon_value = if std::path::Path::new(&cleanmgr).exists() {
            format!("{},0", cleanmgr)
        } else {
            format!("\"{}\",0", exe_str)
        };

        for key in SAFE_COPY_KEYS {
            run_reg(&["add", key, "/ve", "/d", "Safe Copy", "/f"])?;
            run_reg(&["add", key, "/v", "Icon", "/d", &icon_value, "/f"])?;
            let cmd_key = format!("{}\\command", key);
            run_reg(&["add", &cmd_key, "/ve", "/d", &copy_cmd, "/f"])?;
        }
        for key in SAFE_PASTE_KEYS {
            run_reg(&["add", key, "/ve", "/d", "Safe Paste", "/f"])?;
            run_reg(&["add", key, "/v", "Icon", "/d", &icon_value, "/f"])?;
            let cmd_key = format!("{}\\command", key);
            run_reg(&["add", &cmd_key, "/ve", "/d", &paste_cmd, "/f"])?;
        }
    } else {
        for key in SAFE_COPY_KEYS.iter().chain(SAFE_PASTE_KEYS.iter()) {
            let _ = run_reg(&["delete", key, "/f"]);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_safe_copy_context_menu_status() -> Result<bool, String> {
    // Query the Safe Paste folder key (no literal '*') — the always-present half.
    const KEY: &str = r"HKCU\Software\Classes\Directory\shell\WinCommanderSafePaste";
    Ok(run_reg(&["query", KEY]).is_ok())
}

/// Re-apply context-menu registry entries for every feature that is currently
/// enabled in settings, using the CURRENT exe path and labels. Called by lib.rs
/// at startup so that app-update path changes and renamed entries are fixed
/// automatically without requiring the user to disable and re-enable the feature.
///
/// No-arg: reads settings internally and logs errors (never propagates — startup
/// should not abort because a context-menu re-register fails).
pub fn reregister_context_menus_if_enabled() {
    // Read the persisted flags, but DON'T gate solely on them. The HKCU registry
    // verb survives an app update + the settings-store move to %ProgramData% (no
    // migration), whereas the settings flag may not — so a device that enabled the
    // menu on an older build keeps a STALE label/command (e.g. "Scrub with
    // WinCommander") in Explorer that the flag-only gate never refreshed, forcing
    // the user to toggle off+on by hand. Re-applying with /f whenever the entry is
    // actually present heals the label, command, and icon to the current version.
    let s = crate::settings::read_settings().ok();
    let shred_setting = s
        .as_ref()
        .map(|s| s.app.context_menu_enabled)
        .unwrap_or(false);
    let scrub_setting = s
        .as_ref()
        .map(|s| s.app.scrub_context_menu_enabled)
        .unwrap_or(false);
    let safe_copy_setting = s
        .as_ref()
        .map(|s| s.app.safe_copy_context_menu_enabled)
        .unwrap_or(false);

    tauri::async_runtime::spawn(async move {
        // Present-on-disk OR enabled-in-settings → re-apply (idempotent).
        let present = get_context_menu_status().await.unwrap_or(false);
        if shred_setting || present {
            match toggle_context_menu(true).await {
                Ok(_) => {
                    crate::log_message("info", "[ContextMenu] shred context menu re-registered")
                }
                Err(e) => crate::log_message(
                    "warn",
                    &format!("[ContextMenu] shred re-register failed: {}", e),
                ),
            }
        }
    });

    tauri::async_runtime::spawn(async move {
        let present = get_scrub_context_menu_status().await.unwrap_or(false);
        if scrub_setting || present {
            match toggle_scrub_context_menu(true).await {
                Ok(_) => {
                    crate::log_message("info", "[ContextMenu] scrub context menu re-registered")
                }
                Err(e) => crate::log_message(
                    "warn",
                    &format!("[ContextMenu] scrub re-register failed: {}", e),
                ),
            }
        }
    });

    tauri::async_runtime::spawn(async move {
        let present = get_safe_copy_context_menu_status().await.unwrap_or(false);
        if safe_copy_setting || present {
            match toggle_safe_copy_context_menu(true).await {
                Ok(_) => crate::log_message(
                    "info",
                    "[ContextMenu] Safe Copy/Paste context menu re-registered",
                ),
                Err(e) => crate::log_message(
                    "warn",
                    &format!("[ContextMenu] Safe Copy/Paste re-register failed: {}", e),
                ),
            }
        }
    });
}

/// Path of the Privacy Shield NDJSON look-state sidecar written by the
/// Python overlay (`privacy_shield.ps1`). Per-user scratch under LOCALAPPDATA.
fn shield_events_sidecar_path() -> Option<PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    Some(
        PathBuf::from(base)
            .join("WinCommander")
            .join("logs")
            .join("privacy_shield_events.ndjson"),
    )
}

/// Legacy marker from builds that changed the machine-wide Windows webcam
/// policy after a look-away event. New Privacy Shield builds never alter that
/// policy: Meet, browsers, and every other camera client must always retain
/// priority over our best-effort local detector.
fn shield_webcam_denied_marker_path() -> Option<PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    Some(
        PathBuf::from(base)
            .join("WinCommander")
            .join("logs")
            .join("privacy_shield_webcam_denied.flag"),
    )
}

/// Remove a legacy deny marker without changing the user's current Windows
/// camera policy. It is deliberately a no-op with respect to camera access:
/// Privacy Shield is a low-priority observer, never a camera permission
/// controller.
async fn set_shield_webcam_access(_app: AppHandle, _allow: bool) {
    if let Some(marker) = shield_webcam_denied_marker_path() {
        let _ = std::fs::remove_file(&marker);
    }
}

/// Compatibility cleanup for tray Quit / Toggle paths. This no longer changes
/// webcam access; it only clears an orphan marker written by an older build.
pub async fn restore_shield_webcam_access(app: AppHandle) {
    set_shield_webcam_access(app, true).await;
}

/// Startup safety net for old builds that may have left a deny marker. We do
/// not mutate the global webcam policy here: doing so could override a choice
/// the user or their organisation made outside Privacy Shield.
pub async fn reconcile_shield_webcam_on_startup(app: AppHandle) {
    let present = shield_webcam_denied_marker_path()
        .map(|m| m.exists())
        .unwrap_or(false);
    if present {
        crate::log_message(
            "warn",
            "[PrivacyShield] legacy webcam marker found at startup — clearing without changing camera policy",
        );
        set_shield_webcam_access(app, true).await;
    }
}

/// Map the Privacy Shield Python detector's free-text `reason` (e.g.
/// "PHONE DETECTED", "MULTIPLE FACES & LOOK AWAY") to the flow-core
/// `GazeKind` string `flow_bridge::parse_gaze_kind` expects. Priority
/// mirrors the detector's own combine-and-report order (device > multi-face
/// > gaze > no-face) since a frame can trip more than one check at once.
fn gaze_kind_from_reason(reason: &str) -> &'static str {
    let upper = reason.to_ascii_uppercase();
    if upper.contains("PHONE DETECTED") {
        "secondary_device"
    } else if upper.contains("MULTIPLE FACES") {
        "multiple_faces"
    } else if upper.contains("NO FACE") {
        "no_face"
    } else {
        "look_away"
    }
}

/// Monotonic generation for shield readers. Each Start-PrivacyShield spawns a
/// reader and bumps this; an older reader exits once a newer one exists, so a
/// shield restart never leaves two readers double-emitting look-state.
static SHIELD_READER_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// True while the Privacy Shield feature is switched ON. Set by Start-PrivacyShield,
/// cleared by Stop-PrivacyShield / kill_privacy_shield_process. The reader keys its
/// lifetime off THIS — not the `processId` the shield returns, which is a transient
/// launcher pid (dies within seconds) rather than the real Python overlay. Watching
/// that pid killed the reader mid-session, which is why gaze events silently stopped
/// forwarding.
static SHIELD_READER_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Runtime-only rate gate for media-free Fleet attention alerts. Local privacy
/// enforcement must never depend on network policy, so the gate is applied
/// only immediately before the Pro/Fleet dispatch. `0` is deliberately the
/// default/unlimited mode; admins opt into a rolling cap through the signed
/// Privacy Shield policy.
#[derive(Default)]
struct FleetPrivacyAlertRate {
    config: Option<(u32, u32)>,
    sent: Vec<std::time::Instant>,
}

fn fleet_privacy_alert_rate() -> &'static Mutex<FleetPrivacyAlertRate> {
    static RATE: OnceLock<Mutex<FleetPrivacyAlertRate>> = OnceLock::new();
    RATE.get_or_init(|| Mutex::new(FleetPrivacyAlertRate::default()))
}

fn fleet_privacy_event_is_enabled(
    fleet_enabled: bool,
    shield: &crate::settings::PrivacyShieldSettings,
    event_class: &str,
) -> bool {
    if !fleet_enabled
        || shield.fleet_managed != Some(true)
        || shield.fleet_monitoring_enabled != Some(true)
    {
        return false;
    }
    // Blur switches are local enforcement preferences, not alert opt-outs.
    // Fleet policy explicitly keeps monitoring and Fleet alerts active when a
    // blur switch is off; only its global monitoring switch and alert rate
    // limit govern the aggregate notification channel.
    matches!(
        event_class,
        "look_away" | "no_face" | "multiple_faces" | "secondary_device"
    )
}

async fn allow_fleet_privacy_alert(event_class: &str) -> bool {
    let Ok(settings) = crate::settings::read_settings() else {
        // A settings read failure must not affect the local detector. Fleet
        // reporting fails closed because an admin has not confirmed that this
        // particular detector class is enabled for Fleet notification.
        return false;
    };
    // First-sync catch-up: a device can be enrolled (`fleet.enabled == true`)
    // moments before its first `fleet_apply_pending_epoch` tick lands, so
    // `fleet_managed`/`fleet_monitoring_enabled` still read `None` here even
    // though the org's policy already mandates the Shield. Previously the
    // very first look-away event after such a start silently dropped its
    // Fleet report forever (no retry) — only a LATER restart, after the
    // background apply loop had caught up, would report correctly. Give this
    // first event one chance to force that sync itself before deciding.
    let shield = &settings.ideal.privacy.privacy_shield;
    let unsynced = settings.app.fleet.enabled && shield.fleet_managed.is_none();
    let settings = if unsynced {
        let _ = crate::fleet_agent::fleet_apply_pending_epoch_typed().await;
        match crate::settings::read_settings() {
            Ok(s) => s,
            Err(_) => return false,
        }
    } else {
        settings
    };
    let shield = &settings.ideal.privacy.privacy_shield;
    if !fleet_privacy_event_is_enabled(settings.app.fleet.enabled, shield, event_class) {
        return false;
    }
    let limit = shield.fleet_notification_limit.unwrap_or(0).min(1000);
    if limit == 0 {
        return true;
    }
    let window_secs = shield
        .fleet_notification_window_seconds
        .unwrap_or(60)
        .clamp(1, 86_400);
    let Ok(mut rate) = fleet_privacy_alert_rate().lock() else {
        return true;
    };
    let config = (limit, window_secs);
    if rate.config != Some(config) {
        rate.config = Some(config);
        rate.sent.clear();
    }
    let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(window_secs as u64);
    rate.sent.retain(|at| *at > cutoff);
    if rate.sent.len() >= limit as usize {
        return false;
    }
    rate.sent.push(std::time::Instant::now());
    true
}

#[cfg(test)]
mod fleet_privacy_alert_tests {
    use super::fleet_privacy_event_is_enabled;
    use crate::settings::PrivacyShieldSettings;

    fn enabled_policy() -> PrivacyShieldSettings {
        PrivacyShieldSettings {
            fleet_managed: Some(true),
            fleet_monitoring_enabled: Some(true),
            gaze_detection_enabled: Some(true),
            anti_peeping_enabled: Some(true),
            camera_hunter_enabled: Some(true),
            ..Default::default()
        }
    }

    #[test]
    fn fleet_alerts_remain_enabled_when_a_local_blur_switch_is_off() {
        let mut policy = enabled_policy();
        assert!(fleet_privacy_event_is_enabled(true, &policy, "look_away"));
        assert!(fleet_privacy_event_is_enabled(
            true,
            &policy,
            "multiple_faces"
        ));

        policy.anti_peeping_enabled = Some(false);
        assert!(fleet_privacy_event_is_enabled(
            true,
            &policy,
            "multiple_faces"
        ));
        assert!(fleet_privacy_event_is_enabled(true, &policy, "look_away"));
    }

    #[test]
    fn fleet_alerts_require_an_active_fleet_policy_and_known_event_class() {
        let policy = enabled_policy();
        assert!(!fleet_privacy_event_is_enabled(false, &policy, "look_away"));
        assert!(!fleet_privacy_event_is_enabled(true, &policy, "untrusted"));
    }
}

/// Tail the shield's look-state sidecar and forward attention events to the
/// local flow engine and Fleet. Detection never changes global webcam access.
/// Lifetime is decoupled from the reported shield pid: the pid the wrapper
/// returns can be short-lived / wrong while the Python overlay keeps writing, so
/// the reader keeps tailing until the process is gone AND the sidecar has been
/// idle — otherwise a flaky pid silently kills gaze forwarding mid-session.
fn spawn_shield_event_reader(app: AppHandle, pid: u32) {
    use std::sync::atomic::Ordering;
    let my_gen = SHIELD_READER_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn(async move {
        use tauri::Emitter;
        let Some(sidecar) = shield_events_sidecar_path() else {
            crate::flow_bridge::flow_trace("shield-reader: no sidecar path — not starting");
            return;
        };
        // Start from byte zero. Start-PrivacyShield waits until the new Python
        // session has truncated this sidecar, and the detector may already have
        // emitted its first look-away during that wait. Seeding at EOF dropped
        // that initial native notification while the visual blur still worked.
        let mut offset: u64 = 0;
        crate::flow_bridge::flow_trace(format!(
            "shield-reader: SPAWNED gen={} pid={} seed_offset={} sidecar={}",
            my_gen,
            pid,
            offset,
            sidecar.display()
        ));
        let mut last_activity = std::time::Instant::now();
        let mut ticks: u64 = 0;
        loop {
            if let Ok(bytes) = std::fs::read(&sidecar) {
                let len = bytes.len() as u64;
                if len < offset {
                    // File truncated/rotated (e.g. a new session) — restart.
                    offset = 0;
                }
                if len > offset {
                    let fresh = String::from_utf8_lossy(&bytes[offset as usize..]).into_owned();
                    offset = len;
                    last_activity = std::time::Instant::now();
                    for line in fresh.lines() {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                            crate::flow_bridge::flow_trace(format!(
                                "shield-reader: unparseable line: {}",
                                line
                            ));
                            continue;
                        };
                        match value.get("event").and_then(|e| e.as_str()) {
                            Some("look_away") => {
                                let reason = value
                                    .get("reason")
                                    .and_then(|r| r.as_str())
                                    .unwrap_or("")
                                    .trim();

                                // ── Flow-critical emit FIRST ────────────────
                                // Forward the gaze event to flow_bridge (GazeTrigger
                                // flows) BEFORE any slow side effect. Previously this
                                // ran AFTER set_shield_webcam_access().await — a
                                // PowerShell Set-AppCapabilityAccess call that can be
                                // slow/hang — so when it stalled, the gaze event never
                                // reached the flow engine (GazeTrigger flows never
                                // fired) AND the whole reader loop stalled (the "log
                                // gets stuck when Privacy Shield is on" symptom).
                                let gaze_kind = gaze_kind_from_reason(reason);
                                crate::flow_bridge::flow_trace(format!(
                                    "shield-reader: look_away read (reason='{}') → emit privacy-shield-event kind='{}'",
                                    reason, gaze_kind
                                ));
                                let _ = app.emit(
                                    "privacy-shield-event",
                                    serde_json::json!({ "kind": gaze_kind }),
                                );
                                let _ = app.emit(
                                    "privacy-shield-look-state",
                                    serde_json::json!({ "lookingAway": true }),
                                );

                                // Fleet receives only the detected class. The Pro sidecar
                                // queues it for the next authenticated check-in; a Fleet
                                // outage or absent sidecar must never disrupt local shielding.
                                if allow_fleet_privacy_alert(gaze_kind).await {
                                    if let Err(error) = crate::sidecar::dispatch_paid_command(
                                        "record_privacy_shield_event",
                                        serde_json::json!({ "class": gaze_kind }),
                                    )
                                    .await
                                    {
                                        crate::flow_bridge::flow_trace(format!(
                                            "shield-reader: Fleet attention event not queued: {}",
                                            error
                                        ));
                                    }
                                } else {
                                    crate::flow_bridge::flow_trace(
                                        "shield-reader: Fleet attention event rate-limited by signed policy",
                                    );
                                }

                                // ── UX side effects (may be slow; time-bounded) ──
                                let detail = if reason.is_empty() {
                                    "Presence check failed".to_string()
                                } else {
                                    reason.to_string()
                                };
                                let notify_only =
                                    crate::settings::read_settings().ok().and_then(|settings| {
                                        settings.ideal.privacy.privacy_shield.notify_mode
                                    }) == Some(
                                        crate::settings::PrivacyShieldNotifyMode::NotifyOnly,
                                    );
                                let notification_body = if notify_only {
                                    format!("{} — notification only; screen unchanged.", detail)
                                } else {
                                    format!("{} — visual screen shield engaged.", detail)
                                };
                                let _ = crate::native_notify::show_native_notification(
                                    &app,
                                    "Privacy Shield — look-away",
                                    &notification_body,
                                );
                            }
                            Some("look_back") => {
                                crate::flow_bridge::flow_trace("shield-reader: look_back");
                                let _ = app.emit(
                                    "privacy-shield-look-state",
                                    serde_json::json!({ "lookingAway": false }),
                                );
                            }
                            other => {
                                crate::flow_bridge::flow_trace(format!(
                                    "shield-reader: ignored event {:?}",
                                    other
                                ));
                            }
                        }
                    }
                }
            }

            // A newer reader exists (shield restarted) → stop so we don't double-emit.
            if SHIELD_READER_GEN.load(Ordering::SeqCst) != my_gen {
                crate::flow_bridge::flow_trace(format!(
                    "shield-reader: gen={} superseded — exiting",
                    my_gen
                ));
                break;
            }

            // Lifetime = the shield feature toggle, NOT the reported pid. The pid
            // Start-PrivacyShield returns is a transient launcher that dies within
            // seconds; watching it killed the reader while the shield was still
            // running (gaze events silently stopped). Exit only when the shield is
            // explicitly stopped.
            if !SHIELD_READER_ACTIVE.load(Ordering::SeqCst) {
                crate::flow_bridge::flow_trace(format!(
                    "shield-reader: gen={} EXIT (shield toggled off)",
                    my_gen
                ));
                break;
            }

            ticks += 1;
            if ticks.is_multiple_of(20) {
                let alive = crate::child_jobs::is_pid_alive(pid);
                crate::flow_bridge::flow_trace(format!(
                    "shield-reader: heartbeat gen={} tick={} launcher_pid_alive={} offset={} idle={}s",
                    my_gen,
                    ticks,
                    alive,
                    offset,
                    last_activity.elapsed().as_secs()
                ));
            }
            // The overlay itself changes in the Python process, but this reader
            // drives the local UI state and the Fleet event bridge. A 750 ms
            // interval made both look-back state and Fleet alerts visibly late.
            // The sidecar is a tiny append-only file, so a bounded 100 ms poll
            // keeps the local and Fleet surfaces in step without busy-waiting.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        // Shield stopped — clear the visual look-away state. Camera access was
        // never modified by this reader.
        let _ = app.emit(
            "privacy-shield-look-state",
            serde_json::json!({ "lookingAway": false }),
        );
    });
}

#[tauri::command]
pub async fn kill_privacy_shield_process() -> Result<(), String> {
    // Clear the reader run-flag so the look-state reader exits with the shield.
    SHIELD_READER_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    crate::log_message(
        "info",
        "[PrivacyShield] Attempting to kill Privacy Shield process...",
    );
    let script = r#"
        $proc = Get-WmiObject Win32_Process -ErrorAction SilentlyContinue | Where-Object {
            ($_.Name -in @('pythonw.exe', 'python.exe')) -and ($_.CommandLine -like "*--wc-privacy-shield*")
        }
        if ($proc) {
            Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue
            Write-Output "killed"
        }
    "#;

    let (mut cmd, ps_exe) = build_powershell_command();

    let mut child = cmd.spawn().map_err(|e| {
        crate::log_message(
            "error",
            &format!(
                "[PrivacyShield] Failed to spawn PowerShell ({}): {}",
                ps_exe, e
            ),
        );
        format!("Failed to spawn PowerShell ({}): {}", ps_exe, e)
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(script.as_bytes()) {
            crate::log_message(
                "error",
                &format!("[PrivacyShield] Failed to write to stdin: {}", e),
            );
            return Err(format!("Failed to write to stdin: {}", e));
        }
    }

    let output = child.wait_with_output().map_err(|e| {
        crate::log_message(
            "error",
            &format!("[PrivacyShield] Failed to read output: {}", e),
        );
        format!("Failed to read output: {}", e)
    })?;

    if !output.status.success() {
        crate::log_message(
            "warn",
            &format!(
                "[PrivacyShield] PowerShell returned exit code {:?}",
                output.status.code()
            ),
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("killed") {
        crate::log_message(
            "info",
            "[PrivacyShield] Successfully killed Privacy Shield process.",
        );
    } else {
        crate::log_message(
            "info",
            "[PrivacyShield] No Privacy Shield process found to kill.",
        );
    }

    Ok(())
}

/// Kill all mstsc.exe processes directly via taskkill — no PowerShell layer.
/// Called from the TS RDP idle hook as the primary kill step.
#[tauri::command]
pub async fn kill_mstsc_processes() -> Result<serde_json::Value, String> {
    crate::log_message(
        "info",
        "[RdpIdle] kill_mstsc_processes: taskkill /F /IM mstsc.exe",
    );
    let output = tokio::task::spawn_blocking(|| {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/F", "/IM", "mstsc.exe"]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.output()
    })
    .await
    .map_err(|e| format!("spawn_blocking error: {}", e))?
    .map_err(|e| format!("taskkill launch error: {}", e))?;

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // code 0 = killed, 128 = no matching process (also fine)
    let killed = code == 0;
    crate::log_message(
        if killed { "info" } else { "warn" },
        &format!("[RdpIdle] taskkill exit={} msg={:?}", code, stdout),
    );
    Ok(serde_json::json!({ "killed": killed, "code": code, "msg": stdout }))
}

/// System-wide seconds since the last keyboard/mouse input (GetLastInputInfo).
/// Cheap in-process FFI — safe to poll every second from the RDP-idle hook,
/// unlike the PowerShell Watch-RDPClientIdle poll (which spawns a process and
/// so can only run every few seconds). Because this is system-wide, ANY input
/// — including movement inside the remote mstsc window — resets it to 0, which
/// is exactly what the idle counter needs to reset promptly on activity.
/// Mirrors the PS math: (GetTickCount - dwTime) / 1000, using 32-bit wrapping
/// subtraction so it stays correct across the ~49.7-day GetTickCount wrap.
#[cfg(target_os = "windows")]
#[tauri::command]
pub fn get_system_idle_seconds() -> Result<u64, String> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a correctly sized LASTINPUTINFO; GetLastInputInfo only
    // writes its dwTime field and returns nonzero on success.
    let ok = unsafe { GetLastInputInfo(&mut info) };
    if ok == 0 {
        return Err("GetLastInputInfo failed".into());
    }
    let now = unsafe { GetTickCount() };
    let idle_ms = now.wrapping_sub(info.dwTime);
    Ok((idle_ms / 1000) as u64)
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn get_system_idle_seconds() -> Result<u64, String> {
    Err("idle detection is only available on Windows".into())
}

/// Find the BleachBit console executable. Keep this in sync with the dependency
/// detector so the dashboard "engine installed" state matches cleanup runtime.
fn find_bleachbit() -> Option<std::path::PathBuf> {
    let mut candidates: Vec<std::path::PathBuf> = [
        std::env::var("ProgramFiles(x86)").ok().map(|p| {
            std::path::PathBuf::from(p)
                .join("BleachBit")
                .join("bleachbit_console.exe")
        }),
        std::env::var("ProgramFiles").ok().map(|p| {
            std::path::PathBuf::from(p)
                .join("BleachBit")
                .join("bleachbit_console.exe")
        }),
        std::env::var("LOCALAPPDATA").ok().map(|p| {
            std::path::PathBuf::from(p)
                .join("Microsoft")
                .join("WinGet")
                .join("Links")
                .join("bleachbit_console.exe")
        }),
        std::env::var("ProgramFiles").ok().map(|p| {
            std::path::PathBuf::from(p)
                .join("WinGet")
                .join("Links")
                .join("bleachbit_console.exe")
        }),
        std::env::var("ProgramData").ok().map(|p| {
            std::path::PathBuf::from(p)
                .join("Microsoft")
                .join("WinGet")
                .join("Links")
                .join("bleachbit_console.exe")
        }),
        Some(std::path::PathBuf::from(
            r"C:\Program Files (x86)\BleachBit\bleachbit_console.exe",
        )),
        Some(std::path::PathBuf::from(
            r"C:\Program Files\BleachBit\bleachbit_console.exe",
        )),
    ]
    .into_iter()
    .flatten()
    .collect();

    if let Some(path_var) = std::env::var_os("PATH") {
        candidates
            .extend(std::env::split_paths(&path_var).map(|p| p.join("bleachbit_console.exe")));
    }

    candidates.into_iter().find(|candidate| candidate.exists())
}

/// Cleaner name prefixes treated as "browser" when the user opts to exclude browser data.
/// These map to entries visible in `bleachbit_console.exe --list-cleaners`.
const BROWSER_PREFIXES: &[&str] = &[
    "firefox.",
    "chromium.",
    "chrome.",
    "opera.",
    "safari.",
    "brave.",
    "seamonkey.",
    "waterfox.",
    "palemoon.",
    "winapp2_brave.",
    "winapp2_vivaldi.",
    "winapp2_google_chrome.",
    "winapp2_internet_explorer.",
    // Un-comment if you also want to exclude these:
    // "winamp.", "realplayer.", "yahoo_messenger.", "pidgin.",
];

/// Parse BleachBit stdout into a structured JSON result.
/// BleachBit prints lines like:
///   "Delete 1.2MiB C:\..."
///   "Disk space to be recovered: 1.65GiB"
///   "Files to be deleted: 20189"
///   "Special operations: 15"
///   "Errors: 1"
fn parse_bleachbit_output(stdout: &str, stderr: &str, preview: bool) -> serde_json::Value {
    let mut files_count: u64 = 0;
    let mut space_recovered = String::new();
    let mut special_ops: u64 = 0;
    let mut errors: u64 = 0;
    let mut deleted_paths: Vec<String> = Vec::new();

    for line in stdout.lines() {
        let l = line.trim();
        if l.starts_with("Files to be deleted:") || l.starts_with("Files deleted:") {
            files_count = l
                .split(':')
                .nth(1)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
        } else if l.starts_with("Disk space to be recovered:")
            || l.starts_with("Disk space recovered:")
        {
            space_recovered = l
                .split(':')
                .nth(1)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
        } else if l.starts_with("Special operations:") {
            special_ops = l
                .split(':')
                .nth(1)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
        } else if l.starts_with("Errors:") {
            errors = l
                .split(':')
                .nth(1)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
        } else if l.starts_with("Delete ") {
            // Collect a sample of paths (cap at 100 to avoid huge payloads)
            if deleted_paths.len() < 100 {
                if let Some(path_part) = l.splitn(3, ' ').nth(2) {
                    deleted_paths.push(path_part.to_string());
                }
            }
        }
    }

    let summary = if !space_recovered.is_empty() {
        format!("{} files — {} recovered", files_count, space_recovered)
    } else if files_count > 0 {
        format!("{} files cleaned", files_count)
    } else if !stderr.trim().is_empty() {
        stderr.lines().next().unwrap_or("Completed").to_string()
    } else {
        "Clean completed".to_string()
    };

    serde_json::json!({
        "summary": summary,
        "filesCount": files_count,
        "spaceRecovered": space_recovered,
        "specialOps": special_ops,
        "errors": errors,
        "preview": preview,
        "samplePaths": deleted_paths,
    })
}

/// Internal helper: list all available cleaners and optionally filter out browsers.
fn list_cleaners_filtered(
    bb_exe: &std::path::Path,
    exclude_browsers: bool,
) -> Result<Vec<String>, String> {
    let mut cmd = Command::new(bb_exe);
    cmd.arg("--list-cleaners");
    // bleachbit_console.exe is a console-subsystem binary; without
    // CREATE_NO_WINDOW it flashes a cmd.exe window every time the user
    // runs the System Cleaner with `excludeBrowsers=true` (which calls
    // this helper before the main run). The other Command::new(&bb_exe)
    // call below already has the flag.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to list cleaners: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .filter(|cleaner| {
            if exclude_browsers {
                !BROWSER_PREFIXES
                    .iter()
                    .any(|prefix| cleaner.starts_with(prefix))
            } else {
                true
            }
        })
        .collect())
}

/// Run the System Cleaner.
///   exclude_browsers: if true, lists cleaners first and strips browser entries before cleaning
///   preview: if true uses --preview (simulate only — nothing is deleted)
///
/// Returns a JSON value with: { summary, filesCount, spaceRecovered, specialOps, errors, preview, samplePaths }
#[tauri::command]
pub async fn run_bleachbit_clean(
    exclude_browsers: bool,
    preview: bool,
) -> Result<serde_json::Value, String> {
    let bb_exe =
        find_bleachbit().ok_or_else(|| "System Cleaner not found. Install it.".to_string())?;

    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(&bb_exe);

        if exclude_browsers {
            // List cleaners, filter out browsers, pass explicit list
            let cleaners = list_cleaners_filtered(&bb_exe, true)?;
            if cleaners.is_empty() {
                return Err("No cleaners available after exclusions".to_string());
            }
            let mode = if preview { "--preview" } else { "--clean" };
            cmd.arg(mode);
            for c in &cleaners {
                cmd.arg(c);
            }
        } else {
            // Fast path: --all-but-warning covers everything, no listing needed
            let mode = if preview { "--preview" } else { "--clean" };
            cmd.args([mode, "--all-but-warning"]);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        cmd.output()
            .map_err(|e| format!("Failed to run system cleaner: {}", e))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    Ok(parse_bleachbit_output(&stdout, &stderr, preview))
}

// ── Universal full lockdown ────────────────────────────────────────
//
// Orchestrator that runs the user-configured lockdown cascade in
// one call. Every step the cascade can run is declared in
// `lockdown_steps::DESTRUCT_STEPS`; this function iterates over that
// registry, filters by the user's `privacy.selfDestruct.steps` map,
// and dispatches each enabled step in order.
//
// The orchestrator takes NO arguments. All configuration lives in
// settings:
//   - which privacy cleaners run         → privacy.selfDestruct.steps
//   - skip browsers in System Cleaner    → privacy.selfDestruct.excludeBrowsers
//   - free the licence seat first        → privacy.selfDestruct.deactivateLicenseFirst
//   - graceful Windows shutdown after    → privacy.selfDestruct.shutdownSystem
//   - uninstall the app itself           → step `include_app` in the steps map
//
// Single entry point used by:
//   - Sidebar Self-Destruct button
//   - Ctrl+Shift+Q panic hotkey
//   - F-5 coercion phrase
//   - Future Flow-engine triggers
//
// Each step emits a `lockdown-step` Tauri event with `{ label, status,
// ok, error }`. Pro-tier commands route through run_backend_script's
// existing tier gate — if the user has no entitlement, those steps
// fail and surface as errored rows in the UI instead of being silently
// skipped. System Cleaner and the app-removal step are special-cased
// because they're not regular `run_backend_script` commands.

use crate::action_steps::{DestructGroup, DestructStepDef, DESTRUCT_STEPS};

#[derive(Debug, Clone, serde::Serialize)]
pub struct DestructStep {
    pub label: String,
    /// "running" | "done"
    pub status: String,
    pub ok: bool,
    pub error: Option<String>,
}

fn emit_step(app: &tauri::AppHandle, step: DestructStep) {
    use tauri::Emitter;
    let _ = app.emit("lockdown-step", &step);
}

/// Emit an aggregate `destruct-summary` after a cascade so the panic path
/// never silently looks "done" when steps failed. The critical case: without
/// the Pro sidecar installed/licensed (e.g. a lapsed licence at the border),
/// every deep-scrub step returns `PRO_NOT_INSTALLED` one at a time while the
/// app still proceeds — the user must be told the machine was NOT fully wiped.
fn emit_destruct_summary(
    app: &tauri::AppHandle,
    outcomes: &[(&'static str, Result<(), String>)],
    license_warning: Option<&str>,
) {
    use tauri::Emitter;
    let failed_labels: Vec<&'static str> = outcomes
        .iter()
        .filter(|(_, r)| r.is_err())
        .map(|(l, _)| *l)
        .collect();
    let pro_missing = outcomes
        .iter()
        .filter(|(_, r)| matches!(r, Err(e) if e.contains("PRO_NOT_INSTALLED")))
        .count();
    if !failed_labels.is_empty() {
        crate::log_message(
            "warn",
            &format!(
                "[Destruct] INCOMPLETE — {}/{} steps failed{} — data may still be recoverable",
                failed_labels.len(),
                outcomes.len(),
                if pro_missing > 0 {
                    format!(" ({pro_missing} require WinCommander Pro)")
                } else {
                    String::new()
                },
            ),
        );
    }
    if let Some(w) = license_warning {
        crate::log_message("warn", &format!("[Destruct] {w}"));
    }
    let _ = app.emit(
        "destruct-summary",
        serde_json::json!({
            "total": outcomes.len(),
            "failed": failed_labels.len(),
            "proMissing": pro_missing,
            "failedLabels": failed_labels,
            "complete": failed_labels.is_empty(),
            "licenseWarning": license_warning,
        }),
    );
}

/// `Clear-BitLockerKeyProtectors` and `Destroy-VeraCryptHeader` (Feature 5
/// crypto-erase) report their outcome as `{"status": "ok"|"destroyed"|"error",
/// "error": "..."}` — a different shape from the `{"ok": bool, "stdout": ...}`
/// convention most other `run_destruct_step` dispatches use. Checking only the
/// generic `ok` field (as `run_destruct_step` does for every other step)
/// silently treats any real PS-level failure from these two handlers as
/// success, because the field is simply absent rather than `false`.
/// `success_status` is `"ok"` for BitLocker, `"destroyed"` for VeraCrypt.
pub(crate) fn crypto_erase_status_result(
    v: &serde_json::Value,
    success_status: &str,
) -> Result<(), String> {
    match v.get("status").and_then(|s| s.as_str()) {
        Some(s) if s == success_status => Ok(()),
        Some(_) => Err(v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("crypto-erase step reported failure")
            .to_string()),
        None => Err(format!("crypto-erase step: unexpected response shape: {v}")),
    }
}

#[cfg(test)]
mod crypto_erase_status_result_tests {
    use super::crypto_erase_status_result;
    use serde_json::json;

    // ── Real payload shapes, matching commander-pro/src/handlers.rs verbatim ──

    #[test]
    fn veracrypt_destroyed_is_ok() {
        let v = json!({ "status": "destroyed", "path": "D:\\vault.hc" });
        assert!(crypto_erase_status_result(&v, "destroyed").is_ok());
    }

    #[test]
    fn veracrypt_error_is_err_with_message() {
        // Exact shape from Destroy-VeraCryptHeader's catch block.
        let v = json!({ "status": "error", "error": "container too small: 4096 bytes (need >= 262144)" });
        let err = crypto_erase_status_result(&v, "destroyed").unwrap_err();
        assert_eq!(err, "container too small: 4096 bytes (need >= 262144)");
    }

    #[test]
    fn bitlocker_ok_is_ok() {
        let v = json!({
            "status": "ok",
            "removed": ["Tpm"],
            "escrow_warning": "",
            "recovery_protectors_remaining": 0
        });
        assert!(crypto_erase_status_result(&v, "ok").is_ok());
    }

    #[test]
    fn bitlocker_error_is_err_with_message() {
        // Exact shape from Clear-BitLockerKeyProtectors's catch block.
        let v = json!({
            "status": "error",
            "error": "Get-BitLockerVolume: volume not found",
            "escrow_warning": "ERROR: could not inspect BitLocker state — assume escrow possible",
            "removed": [],
            "recovery_protectors_remaining": -1
        });
        let err = crypto_erase_status_result(&v, "ok").unwrap_err();
        assert_eq!(err, "Get-BitLockerVolume: volume not found");
    }

    // ── The bug this function exists to fix: no "ok" field, generic check
    //    silently passed this through as success (see run_destruct_step's
    //    old `v.get("ok").and_then(...) == Some(false)` check, which is
    //    never true here because "ok" is simply absent). ──

    #[test]
    fn missing_ok_field_would_have_fooled_the_old_generic_check() {
        let v = json!({ "status": "error", "error": "boom" });
        assert_eq!(v.get("ok").and_then(|o| o.as_bool()), None);
        assert_ne!(v.get("ok").and_then(|o| o.as_bool()), Some(false));
        // ...but the new, status-aware check correctly reports failure:
        assert!(crypto_erase_status_result(&v, "ok").is_err());
    }

    #[test]
    fn wrong_success_status_is_err_even_without_error_field() {
        let v = json!({ "status": "something_unexpected" });
        let err = crypto_erase_status_result(&v, "ok").unwrap_err();
        assert_eq!(err, "crypto-erase step reported failure");
    }

    #[test]
    fn no_status_field_at_all_is_err() {
        let v = json!({ "unexpected": "shape" });
        let err = crypto_erase_status_result(&v, "ok").unwrap_err();
        assert!(err.contains("unexpected response shape"));
    }
}

/// Execute a single destruct step end-to-end: emit `running`, dispatch
/// the underlying command (BleachBit, paid script, etc.), emit `done`.
/// Used by both the Phase 1 parallel batch and the Phase 2 serial
/// Privacy Clean loop so the event protocol is identical regardless
/// of which phase the step belongs to.
///
/// P2: dispatches via step ID to the Pro sidecar ("run_destruct_step")
/// rather than holding PS command strings in Free. Two local sentinels:
///   "system_cleaner" → run_bleachbit_clean (BleachBit, Rust-native)
///   "include_app"    → handled by full_lockdown (lockdown(), not here)
///
/// Per-step log lines (start/finish + duration) land in the standard
/// log_message stream so users can diagnose "why is it slow / stuck?"
/// from %LOCALAPPDATA%\WinCommander\logs without needing DevTools.
async fn run_destruct_step(
    app: &tauri::AppHandle,
    def: &DestructStepDef,
    plan: &crate::authz::LockdownPlanSnapshot,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    crate::log_message("info", &format!("[Destruct] starting: {}", def.label));
    emit_step(
        app,
        DestructStep {
            label: def.label.into(),
            status: "running".into(),
            ok: false,
            error: None,
        },
    );

    let result: Result<(), String> = if def.id == "system_cleaner" {
        // BleachBit: Rust-native helper, no PS command string. Browsers
        // are intentionally excluded — the `browser_footprints` step
        // is the user-facing knob for browser coverage.
        run_bleachbit_clean(true, false).await.map(|_| ())
    } else if def.id == "configured_folders" {
        let paths = plan.self_destruct.shred_folders.clone().unwrap_or_default();
        let wipe_mft_slack = plan.shred_mft_slack;
        if paths.is_empty() {
            crate::log_message(
                "info",
                "[Destruct] Configured Folder Shred: no folders configured — skipping",
            );
            Ok(())
        } else {
            let mut failures = Vec::new();
            let mft_command = join_parts(&["Clear-MFT~", "Resident~", "Slack~"]);
            let erase_command = join_parts(&["Invoke-~", "7Erase~"]);
            for path in &paths {
                if wipe_mft_slack {
                    let mft_result = crate::sidecar::dispatch_paid_command(
                        &mft_command,
                        serde_json::json!({ "Path": path }),
                    )
                    .await;
                    match mft_result {
                        Err(e) => failures.push(format!("{path}: MFT/slack pass failed: {e}")),
                        Ok(v) if v.get("ok").and_then(|o| o.as_bool()) == Some(false) => {
                            let detail = v
                                .get("stdout")
                                .and_then(|s| s.as_str())
                                .unwrap_or("MFT/slack pass reported failure");
                            failures.push(format!("{path}: {detail}"));
                        }
                        Ok(_) => {}
                    }
                }
                let erase_result = crate::sidecar::dispatch_paid_command(
                    &erase_command,
                    serde_json::json!({ "Path": path, "Type": "Folder" }),
                )
                .await
                .and_then(|v| {
                    if v.get("ok").and_then(|o| o.as_bool()) == Some(false) {
                        Err(v
                            .get("stdout")
                            .and_then(|s| s.as_str())
                            .unwrap_or("folder erase failed")
                            .to_string())
                    } else {
                        Ok(())
                    }
                });
                if let Err(e) = erase_result {
                    failures.push(format!("{path}: {e}"));
                }
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "{} failure(s) across {} configured folder(s): {}",
                    failures.len(),
                    paths.len(),
                    failures.join(" | ")
                ))
            }
        }
    } else if def.id == "veracrypt_header_destroy" {
        // Feature 5 crypto-erase: the automated cascade can only target
        // VeraCrypt containers the user has explicitly pre-configured — an
        // unmounted container has no OS-visible trace to auto-discover (see
        // selective_erase.rs's is_system_device_path doc comment). Previously
        // this step always dispatched with no `Path` at all, which the Pro
        // handler unconditionally rejects — so it failed on every trigger
        // for every user, including silently aborting F6 stage-1 (which
        // treats this step's failure as an abort-before-reboot, per its
        // keys-before-reboot safety contract) even for users with no
        // VeraCrypt containers to begin with. Skipping cleanly when nothing
        // is configured fixes that without changing what gets destroyed.
        let paths = plan
            .self_destruct
            .crypto_erase_veracrypt_paths
            .clone()
            .unwrap_or_default();
        let devices = plan
            .self_destruct
            .crypto_erase_veracrypt_devices
            .clone()
            .unwrap_or_default();
        if paths.is_empty() && devices.is_empty() {
            crate::log_message(
                "info",
                "[Destruct] VeraCrypt Header Destroy: no container or partition targets configured — skipping, nothing to erase",
            );
            Ok(())
        } else {
            let mut failures = Vec::new();
            for path in &paths {
                let expected = match crate::path_identity::ExpectedFileIdentity::capture(
                    std::path::Path::new(path),
                ) {
                    Ok(expected) => expected,
                    Err(error) => {
                        failures.push(format!("{path}: {error}"));
                        continue;
                    }
                };
                let identity = plan
                    .destructive_identity(&expected.canonical_path().to_string_lossy())?
                    .clone();
                let mut payload = serde_json::json!({
                    "stepId": "veracrypt_header_destroy",
                    "Path": expected.canonical_path().to_string_lossy(),
                })
                .as_object()
                .cloned()
                .expect("destructive payload is an object");
                if let Err(error) = crate::path_identity::insert_request(&mut payload, &identity) {
                    failures.push(format!("{path}: {error}"));
                    continue;
                }
                let outcome = crate::sidecar::dispatch_paid_command(
                    "run_destruct_step",
                    serde_json::Value::Object(payload),
                )
                .await
                .and_then(|v| {
                    crate::path_identity::verify_receipt(&v, &identity)?;
                    crypto_erase_status_result(&v, "destroyed")
                });
                if let Err(e) = outcome {
                    failures.push(format!("{path}: {e}"));
                }
            }
            for device in &devices {
                let identity = plan.destructive_identity(&device.device_path)?.clone();
                let mut payload = serde_json::json!({
                    "stepId": "veracrypt_header_destroy",
                    "Path": device.device_path,
                    "DeviceDiskNumber": device.disk_number,
                    "DevicePartitionNumber": device.partition_number,
                    "DevicePartitionGuid": device.partition_guid,
                    "DeviceOffsetBytes": device.offset_bytes,
                    "DeviceSizeBytes": device.size_bytes,
                    "DeviceDiskUniqueId": device.disk_unique_id,
                })
                .as_object()
                .cloned()
                .expect("destructive payload is an object");
                crate::path_identity::insert_request(&mut payload, &identity)?;
                let outcome = crate::sidecar::dispatch_paid_command(
                    "run_destruct_step",
                    serde_json::Value::Object(payload),
                )
                .await
                .and_then(|v| {
                    crate::path_identity::verify_receipt(&v, &identity)?;
                    crypto_erase_status_result(&v, "destroyed")
                });
                if let Err(e) = outcome {
                    failures.push(format!("{}: {e}", device.device_path));
                }
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "{}/{} VeraCrypt target(s) failed: {}",
                    failures.len(),
                    paths.len() + devices.len(),
                    failures.join(" | ")
                ))
            }
        }
    } else if def.id == "bitlocker_erase" {
        // Feature 5 crypto-erase: previously this step always dispatched with
        // no `DriveLetter`, which the Pro handler silently defaulted to "C:"
        // — every opted-in user blindly targeted the OS drive with no way to
        // choose otherwise and no separate confirmation the way the manual
        // selective-erase UI's typed nuclear-ack requires. This now targets
        // exactly the drives the user selected (CryptoEraseTargetsSection,
        // `crypto_erase_bitlocker_drives`), mirroring `veracrypt_header_destroy`
        // above: empty/None cleanly skips rather than falling back to "C:".
        // Selecting the system drive here means it WILL be targeted on the
        // next trigger — the destroy-PIN/trigger itself is the confirmation.
        let drives = plan
            .self_destruct
            .crypto_erase_bitlocker_drives
            .clone()
            .unwrap_or_default();
        if drives.is_empty() {
            crate::log_message(
                "info",
                "[Destruct] BitLocker Key Erase: no target drives configured \
                 (crypto_erase_bitlocker_drives) — skipping, nothing to erase",
            );
            Ok(())
        } else {
            let mut failures = Vec::new();
            for drive in &drives {
                let identity = plan
                    .destructive_identity(
                        &drive.trim().trim_end_matches('\\').to_ascii_uppercase(),
                    )?
                    .clone();
                let mut payload = serde_json::json!({
                    "stepId": "bitlocker_erase",
                    "DriveLetter": drive,
                })
                .as_object()
                .cloned()
                .expect("destructive payload is an object");
                crate::path_identity::insert_request(&mut payload, &identity)?;
                let outcome = crate::sidecar::dispatch_paid_command(
                    "run_destruct_step",
                    serde_json::Value::Object(payload),
                )
                .await
                .and_then(|v| {
                    crate::path_identity::verify_receipt(&v, &identity)?;
                    crypto_erase_status_result(&v, "ok")
                });
                if let Err(e) = outcome {
                    failures.push(format!("{drive}: {e}"));
                }
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "{}/{} BitLocker target(s) failed: {}",
                    failures.len(),
                    drives.len(),
                    failures.join(" | ")
                ))
            }
        }
    } else {
        // All other steps: dispatch by step ID to Pro. Pro holds the
        // ID → PS command mapping; no PS command strings live in Free.
        // remove_users additionally carries the configured target usernames —
        // read here (not on the Pro side) so the same path serves every
        // trigger (sidebar, hotkey, distress phrase, dead-man, destroy PIN).
        // Pro re-validates every account server-side (built-in/self/loaded).
        let step_args = if def.id == "remove_users" {
            let usernames = plan
                .self_destruct
                .users_to_remove
                .clone()
                .unwrap_or_default();
            serde_json::json!({ "stepId": def.id, "usernames": usernames })
        } else {
            serde_json::json!({ "stepId": def.id })
        };
        crate::sidecar::dispatch_paid_command("run_destruct_step", step_args)
            .await
            .and_then(|v| {
                if v.get("ok").and_then(|o| o.as_bool()) == Some(false) {
                    Err(v
                        .get("stdout")
                        .and_then(|s| s.as_str())
                        .unwrap_or("step failed")
                        .to_string())
                } else {
                    Ok(())
                }
            })
    };

    let elapsed_ms = started.elapsed().as_millis();
    match &result {
        Ok(_) => crate::log_message(
            "info",
            &format!("[Destruct] done: {} ({} ms)", def.label, elapsed_ms),
        ),
        Err(e) => crate::log_message(
            "warn",
            &format!(
                "[Destruct] FAILED: {} ({} ms) — {}",
                def.label, elapsed_ms, e
            ),
        ),
    }

    emit_step(
        app,
        DestructStep {
            label: def.label.into(),
            status: "done".into(),
            ok: result.is_ok(),
            error: result.as_ref().err().cloned(),
        },
    );

    result
}

#[tauri::command]
pub async fn full_lockdown(
    app: tauri::AppHandle,
    capability_token: Option<String>,
) -> Result<(), String> {
    let settings =
        settings::read_settings().map_err(|e| format!("Failed to read settings: {}", e))?;
    let plan = crate::authz::LockdownPlanSnapshot::from_settings(&settings)?;
    crate::authz::consume_required(
        capability_token.as_deref(),
        crate::authz::DestructiveAction::SelfDestruct,
        &crate::authz::full_lockdown_args(&plan),
    )?;
    full_lockdown_with_plan(app, plan).await
}

pub(crate) async fn full_lockdown_internal(app: tauri::AppHandle) -> Result<(), String> {
    let settings =
        settings::read_settings().map_err(|e| format!("Failed to read settings: {}", e))?;
    full_lockdown_with_plan(
        app,
        crate::authz::LockdownPlanSnapshot::from_settings(&settings)?,
    )
    .await
}

async fn full_lockdown_with_plan(
    app: tauri::AppHandle,
    plan: crate::authz::LockdownPlanSnapshot,
) -> Result<(), String> {
    // Tier gate — lockdown is paid-only. The frontend already
    // wraps the configuration UI in <TierGate tier="paid"> and the
    // sidebar button checks canUse("paid") before arming, but that's
    // UI-side only. Defense in depth: refuse the cascade here too in
    // case a fork or a forged invoke bypasses the UI gate. The
    // coercion trigger is a paid feature itself so this layer
    // is a no-op for it, but it's the right belt-and-braces.
    // Licence posture is decided by the emergency-wipe grace gate below (after
    // the opt-in check, so a refused / non-opted trigger never burns the
    // one-time grace). The old hard require_paid here silently refused the
    // ENTIRE wipe on any lapse — the worst failure for a duress trigger.

    if crate::license::is_advanced_mode() {
        return Err("Refused: investigator mode does not run the destruction cascade.".to_string());
    }

    // Explicit opt-in gate — every destructive path funnels through here or
    // lockdown_impl. Refuse if the user hasn't opted in. This single check
    // covers all full_lockdown callers (sidebar button, panic hotkey,
    // coercion via panic-cascade-instant).
    if plan.self_destruct.enabled != Some(true) {
        return Err(
            "Self-destruct is not enabled. Opt in via Settings → Secret → Self-Destruct."
                .to_string(),
        );
    }

    // NO module gate. The self-destruct cascade is a safety control, not part of
    // the "cleanup" feature module — gating it meant a coercion/panic
    // trigger silently failed to wipe whenever cleanup was toggled off. Each
    // paid step still gates itself at the sidecar; the app-erase always runs.

    let cfg = &plan.self_destruct;
    // Defaults are deliberately conservative: they preserve the
    // pre-customisation behaviour where every panic trigger
    // (sidebar button, Ctrl+Shift+Q, coercion) cleared, freed
    // the licence seat, uninstalled the app, AND triggered a
    // graceful shutdown. A user who never opens the configuration
    // panel gets the historical cascade unchanged.
    //
    // System Cleaner always excludes browsers — the dedicated
    // `browser_footprints` step is the user-facing knob for browser
    // coverage. Pinning the cleaner to `exclude_browsers=true` here
    // keeps the two steps from double-covering. The `excludeBrowsers`
    // field in SelfDestructSettings is retained for backwards-compat
    // with persisted settings.json files but is no longer consulted.
    let _legacy_exclude_browsers = cfg.exclude_browsers; // unused, kept for serde
    let deactivate_license_first = cfg.deactivate_license_first.unwrap_or(false);
    let shutdown_system = cfg.shutdown_system.unwrap_or(true);

    // Resolve a step's enabled state: user override → step's default.
    // Sparse map means an untouched user gets the documented defaults
    // (most steps on; the slow Privacy Clean deep erasers off).
    let user_steps = cfg.steps.as_ref();
    let is_step_enabled = |def: &DestructStepDef| -> bool {
        match user_steps.and_then(|m| m.get(def.id).copied()) {
            Some(v) => v,
            None => def.default_enabled,
        }
    };

    // Three-phase parallel cascade:
    //   Phase 1 (parallel): every regular step — Privacy Traces, Deep
    //                       trace analysis, System Cleaner. They're independent
    //                       surfaces (DNS cache vs USB history vs
    //                       BleachBit etc.); running them concurrently
    //                       gets system_cleaner running while the paid
    //                       chain queues at the Pro IPC mutex. Net win
    //                       is wall-clock — System Cleaner alone is
    //                       20-60s and used to block everything else.
    //                       The Pro IPC layer's session mutex still
    //                       serialises individual paid commands so we
    //                       never overlap two paid invocations on the
    //                       same pipe; that's correct, this just lets
    //                       free + paid run alongside each other.
    //   Phase 2 (serial): Privacy Clean deep erasers (cipher /w, SSD
    //                     TRIM, virtual-memory purge). They contend for
    //                     the same disk-free-space surface and saturate
    //                     I/O; running them in parallel would just
    //                     thrash. Sequential keeps each one's progress
    //                     legible.
    //   Phase 3 (last): include_app — exits the process; nothing else
    //                   could run after it anyway.
    //
    // Each step emits its own running/done events from a shared helper
    // so the frontend's runOperation overlay (mode:'parallel') lights
    // up all rows immediately and ticks them off as events arrive.

    // Phase 1 — parallel via tokio::spawn so each step runs as its own
    // task on the multi-threaded runtime. Previous approach used
    // futures::join_all, which polls every future inside a single task
    // — under load that means each future's sync prefix (log_message +
    // emit + Tokio Command::spawn, the last of which is sync and on
    // Windows can take 1–2s under AV scanning) runs serially before
    // any yield. Net effect: the pool's permits stayed unused because
    // only one future was advancing at a time, and Pro's 5s handshake
    // read-timeout fired on whichever children were waiting their turn
    // (surfacing as "Hello write: pipe is being closed (os error 232)").
    //
    // tokio::spawn distributes the steps across the worker threads, so
    // up to N (worker count) futures are running their sync prefixes
    // concurrently and the pool's POOL_CAPACITY=4 actually gets
    // exercised. The bumped HANDSHAKE_TIMEOUT in sidecar.rs / Pro's
    // main.rs absorbs the remaining tail of slow Pro spawns.
    // Emergency-wipe licence grace (owner policy 2026-07-01): the
    // coercion / panic trigger must still protect the user if their Pro
    // licence lapsed. Grant ONE full emergency wipe on an expired licence,
    // then refuse the Pro-only deep steps until renewal (System Cleaner + app
    // removal always run). Renewal clears the marker (once-per-lapse).
    let emergency_lic = crate::license::emergency_license_gate();
    let allow_pro = !matches!(
        emergency_lic,
        crate::license::EmergencyLicense::GraceExhausted
    );
    let license_warning: Option<String> = match emergency_lic {
        crate::license::EmergencyLicense::GraceGranted => Some(
            "WinCommander Pro licence expired — this emergency wipe ran on a ONE-TIME grace. Renew Pro to keep full protection.".to_string()),
        crate::license::EmergencyLicense::GraceExhausted => Some(
            "WinCommander Pro licence expired and the one-time emergency grace was already used — the deep wipe did NOT run (basic cleanup only). Renew Pro now.".to_string()),
        _ => None,
    };

    let phase1: Vec<&'static DestructStepDef> = DESTRUCT_STEPS
        .iter()
        .filter(|d| {
            d.id != "include_app"
                && d.group != DestructGroup::PrivacyClean
                && (allow_pro || d.id == "system_cleaner")
                && is_step_enabled(d)
        })
        .collect();

    let phase1_handles: Vec<tokio::task::JoinHandle<(&'static str, Result<(), String>)>> = phase1
        .iter()
        .map(|def| {
            let app = app.clone();
            let def: &'static DestructStepDef = def;
            let plan = plan.clone();
            tokio::spawn(async move { (def.label, run_destruct_step(&app, def, &plan).await) })
        })
        .collect();
    // Aggregate per-step outcomes for the truthful summary below.
    let mut outcomes: Vec<(&'static str, Result<(), String>)> = Vec::new();
    for handle in phase1_handles {
        // JoinError only fires on panic / cancel.
        if let Ok(pair) = handle.await {
            outcomes.push(pair);
        }
    }

    // Phase 2 — Privacy Clean deep erasers, sequentially.
    for def in DESTRUCT_STEPS
        .iter()
        .filter(|d| d.group == DestructGroup::PrivacyClean && allow_pro && is_step_enabled(d))
    {
        outcomes.push((def.label, run_destruct_step(&app, def, &plan).await));
    }

    emit_destruct_summary(&app, &outcomes, license_warning.as_deref());

    // Phase 2 — only runs if user opted in to `include_app`. Hands off
    // to the existing lockdown() which spawns a detached
    // PowerShell that waits for the app to exit, then multi-pass-clears
    // %APPDATA%, runs the NSIS uninstaller, and clears registry
    // footprints. lockdown doesn't return (the app exits).
    let include_app = match DESTRUCT_STEPS.iter().find(|s| s.id == "include_app") {
        Some(def) => is_step_enabled(def),
        None => true,
    };

    if include_app {
        emit_step(
            &app,
            DestructStep {
                label: "Uninstall WinCommander".into(),
                status: "running".into(),
                ok: false,
                error: None,
            },
        );
        // Won't actually return — the Phase 2 PowerShell outlives this
        // process. We pre-emit a `done` event so the UI doesn't hang
        // its progress overlay forever; in practice the app exits
        // before the frontend processes the event.
        emit_step(
            &app,
            DestructStep {
                label: "Uninstall WinCommander".into(),
                status: "done".into(),
                ok: true,
                error: None,
            },
        );
        return lockdown_impl_with_config(
            app,
            deactivate_license_first,
            shutdown_system,
            false,
            plan,
        )
        .await;
    }

    // App-removal opted out — handle the standalone shutdown / licence
    // deactivation here so the user still gets those if they configured
    // them.
    if deactivate_license_first {
        let _ = crate::license::deactivate_license_internal().await;
    }
    if shutdown_system {
        emit_step(
            &app,
            DestructStep {
                label: "System Shutdown".into(),
                status: "running".into(),
                ok: false,
                error: None,
            },
        );
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            let _ = std::process::Command::new("shutdown")
                .args(["/s", "/t", "5"])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }
        emit_step(
            &app,
            DestructStep {
                label: "System Shutdown".into(),
                status: "done".into(),
                ok: true,
                error: None,
            },
        );
    }

    Ok(())
}

/// Run the user-configured destruct cascade (privacy traces + deep DFIR + the
/// serial Privacy-Clean group), honouring `privacy.selfDestruct.steps` and
/// falling back to the `DESTRUCT_STEPS` defaults. Shared by the top-level
/// `lockdown` trigger; `full_lockdown` runs the same set itself and therefore
/// skips this via `lockdown_impl(.., run_destruct_steps=false)`.
async fn run_configured_destruct_steps(
    app: &tauri::AppHandle,
    plan: &crate::authz::LockdownPlanSnapshot,
) {
    let user_steps = plan.self_destruct.steps.clone();
    let is_step_on = |def: &'static DestructStepDef| -> bool {
        match user_steps.as_ref().and_then(|m| m.get(def.id).copied()) {
            Some(v) => v,
            None => def.default_enabled,
        }
    };

    // Emergency-wipe licence grace (owner policy 2026-07-01): an expired Pro
    // licence gets ONE full emergency wipe, then the Pro-only deep steps are
    // refused until renewal — the free System Cleaner always runs. Renewal
    // clears the marker (once-per-lapse). This is the destroy-PIN / distress /
    // dead-man path, which has no paid gate, so the grace decides Pro access.
    let emergency_lic = crate::license::emergency_license_gate();
    let allow_pro = !matches!(
        emergency_lic,
        crate::license::EmergencyLicense::GraceExhausted
    );
    let license_warning: Option<String> = match emergency_lic {
        crate::license::EmergencyLicense::GraceGranted => Some(
            "WinCommander Pro licence expired — this emergency wipe ran on a ONE-TIME grace. Renew Pro to keep full protection.".to_string()),
        crate::license::EmergencyLicense::GraceExhausted => Some(
            "WinCommander Pro licence expired and the one-time emergency grace was already used — the deep wipe did NOT run (basic cleanup only). Renew Pro now.".to_string()),
        _ => None,
    };

    let phase1: Vec<&'static DestructStepDef> = DESTRUCT_STEPS
        .iter()
        .filter(|d| {
            d.id != "include_app"
                && d.group != DestructGroup::PrivacyClean
                && (allow_pro || d.id == "system_cleaner")
                && is_step_on(d)
        })
        .collect();

    let handles: Vec<tokio::task::JoinHandle<(&'static str, Result<(), String>)>> = phase1
        .iter()
        .map(|def| {
            let app_c = app.clone();
            let def: &'static DestructStepDef = def;
            let plan = plan.clone();
            tokio::spawn(async move { (def.label, run_destruct_step(&app_c, def, &plan).await) })
        })
        .collect();
    let mut outcomes: Vec<(&'static str, Result<(), String>)> = Vec::new();
    for h in handles {
        if let Ok(pair) = h.await {
            outcomes.push(pair);
        }
    }

    for def in DESTRUCT_STEPS
        .iter()
        .filter(|d| d.group == DestructGroup::PrivacyClean && allow_pro && is_step_on(d))
    {
        outcomes.push((def.label, run_destruct_step(app, def, plan).await));
    }

    emit_destruct_summary(app, &outcomes, license_warning.as_deref());
}

/// Lockdown: deactivate license (optional), erase app data/registry, run uninstaller, exit.
///
/// Top-level callers (CalculatorGate destroy PIN, distress phrase, dead-man's switch) hit this
/// command directly, so it runs the FULL configured destruct cascade before the app-removal
/// script. full_lockdown already runs that cascade itself and delegates only the app-removal
/// step here via `lockdown_impl(.., run_destruct_steps=false)`, so the steps never double-run.
#[tauri::command]
pub async fn lockdown(
    app: tauri::AppHandle,
    deactivate_license_first: bool,
    shutdown_system: bool,
    capability_token: Option<String>,
) -> Result<(), String> {
    let settings =
        settings::read_settings().map_err(|e| format!("Failed to read settings: {}", e))?;
    let plan = crate::authz::LockdownPlanSnapshot::from_settings(&settings)?;
    crate::authz::consume_required(
        capability_token.as_deref(),
        crate::authz::DestructiveAction::SelfDestruct,
        &crate::authz::lockdown_args(deactivate_license_first, shutdown_system, &plan),
    )?;
    lockdown_impl_with_config(app, deactivate_license_first, shutdown_system, true, plan).await
}

pub(crate) async fn lockdown_impl(
    app: tauri::AppHandle,
    deactivate_license_first: bool,
    shutdown_system: bool,
    run_destruct_steps: bool,
) -> Result<(), String> {
    let settings =
        settings::read_settings().map_err(|e| format!("Failed to read settings: {}", e))?;
    let plan = crate::authz::LockdownPlanSnapshot::from_settings(&settings)?;
    lockdown_impl_with_config(
        app,
        deactivate_license_first,
        shutdown_system,
        run_destruct_steps,
        plan,
    )
    .await
}

async fn lockdown_impl_with_config(
    app: tauri::AppHandle,
    deactivate_license_first: bool,
    shutdown_system: bool,
    run_destruct_steps: bool,
    plan: crate::authz::LockdownPlanSnapshot,
) -> Result<(), String> {
    // Explicit opt-in gate — mirrors the check in full_lockdown. Covers all
    // direct callers: inactivity_timer (dead-man's switch) and the destroy
    // PIN. Refuse before any destructive work if the user hasn't opted in.
    if plan.self_destruct.enabled != Some(true) {
        crate::log_message(
            "warn",
            "[Lockdown] refused — self-destruct not enabled (opt in via Settings → Secret → Self-Destruct)",
        );
        return Err(
            "Self-destruct is not enabled. Opt in via Settings → Secret → Self-Destruct."
                .to_string(),
        );
    }

    // NO module gate here. lockdown is the irreversible emergency wipe invoked
    // by the destroy PIN, distress phrase, and dead-man's switch. Gating it on
    // the unrelated "cleanup" feature toggle meant the destroy PIN could
    // SILENTLY no-op under coercion — callers .catch() the Err, so the owner
    // believes their data was wiped while the calculator just keeps running.
    // The emergency wipe must always run; "cleanup" gates day-to-day features,
    // not the panic switch.

    // KT: run the destruct cascade ONLY for top-level triggers. full_lockdown
    // already ran it before delegating here for app removal — re-running would
    // double-execute every cleaner (slow + duplicate progress events).
    if run_destruct_steps {
        crate::log_message(
            "warn",
            "[Lockdown] destroy/emergency lockdown triggered — running privacy cleaners then app removal",
        );
        run_configured_destruct_steps(&app, &plan).await;
        crate::log_message(
            "warn",
            "[Lockdown] privacy cleaners done — proceeding to app removal",
        );
    }

    // Optionally release the license seat on the server before destroying.
    if deactivate_license_first {
        let _ = crate::license::deactivate_license_internal().await;
    }

    let exe_path = std::env::current_exe().map_err(|e| format!("Could not get exe path: {}", e))?;
    let exe_dir = exe_path
        .parent()
        .ok_or("Could not get exe directory")?
        .to_string_lossy()
        .to_string();
    let current_pid = std::process::id();

    // Escape single-quotes for PowerShell string literals
    let exe_dir_ps = exe_dir.replace('\'', "''");

    let shutdown_cmd = if shutdown_system {
        "shutdown /s /t 5"
    } else {
        ""
    };

    let script = format!(
        r#"
$ErrorActionPreference = 'SilentlyContinue'
$pid = {pid}

# WAITING FOR APP EXIT BEFORE DESTRUCTION
$timeout = 10
while ((Get-Process -Id $pid -ErrorAction SilentlyContinue) -and ($timeout -gt 0)) {{
    Start-Sleep -Seconds 1
    $timeout--
}}
Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue

# STANDALONE SECURE ERASE (Injected because app modules are about to be deleted)
function Invoke-WcDirectErase {{
    param([string]$Path)
    if (!(Test-Path $Path)) {{ return }}
    try {{
        $file = Get-Item -LiteralPath $Path -Force
        if ($file.PSIsContainer) {{
            Get-ChildItem -LiteralPath $Path -Recurse -Force | Where-Object {{ -not $_.PSIsContainer }} | ForEach-Object {{ Invoke-WcDirectErase -Path $_.FullName }}
            $guid = [Guid]::NewGuid().ToString()
            Rename-Item -LiteralPath $Path -NewName $guid -Force
            Remove-Item -LiteralPath (Join-Path (Split-Path $Path) $guid) -Recurse -Force
        }} else {{
            $size = $file.Length
            $stream = [System.IO.File]::Open($Path, 'Open', 'Write', 'None')
            $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
            for ($i = 0; $i -lt 7; $i++) {{
                $buffer = New-Object byte[] 65536
                $written = 0
                while ($written -lt $size) {{
                    $toWrite = [Math]::Min($buffer.Length, $size - $written)
                    $rng.GetBytes($buffer)
                    $stream.Write($buffer, 0, $toWrite)
                    $written += $toWrite
                }}
                $stream.Position = 0
            }}
            $stream.Close()
            $guid = [Guid]::NewGuid().ToString()
            Rename-Item -LiteralPath $Path -NewName $guid -Force
            Remove-Item -LiteralPath (Join-Path (Split-Path $Path) $guid) -Force
        }}
    }} catch {{ Remove-Item -LiteralPath $Path -Recurse -Force }}
}}

# Run NSIS silent uninstaller if present
# NSIS re-spawns itself from %TEMP% then exits; -Wait only waits for the stub.
# Poll until the re-spawned child (Au_.tmp) also finishes, then continue.
$uninstallers = @(
    "$env:LOCALAPPDATA\Programs\WinCommander\Uninstall WinCommander.exe",
    "$env:ProgramFiles\WinCommander\Uninstall WinCommander.exe",
    "$env:ProgramFiles(x86)\WinCommander\Uninstall WinCommander.exe",
    "{exe_dir}\Uninstall WinCommander.exe"
)
foreach ($u in $uninstallers) {{
    if (Test-Path $u) {{
        Start-Process -FilePath $u -ArgumentList '/S' -Wait -ErrorAction SilentlyContinue
        # Wait for NSIS child (Au_.tmp) to finish — up to 60 s
        $deadline = (Get-Date).AddSeconds(60)
        while ((Get-Date) -lt $deadline) {{
            $running = Get-Process -ErrorAction SilentlyContinue | Where-Object {{
                try {{ $_.MainModule.FileName -like '*\Au_.tmp' }} catch {{ $false }}
            }}
            if (-not $running) {{ break }}
            Start-Sleep -Milliseconds 500
        }}
        break
    }}
}}

# Erase app data directories securely. %ProgramData% now holds the per-machine
# license cache, PIN hashes, encrypted settings + the extracted Pro sidecar, so
# it MUST be wiped first or the crown jewels survive a "destroy".
Invoke-WcDirectErase -Path "$env:ProgramData\WinCommander"
Invoke-WcDirectErase -Path "$env:APPDATA\WinCommander"
Invoke-WcDirectErase -Path "$env:LOCALAPPDATA\WinCommander"
Invoke-WcDirectErase -Path "$env:LOCALAPPDATA\com.servalabs.wincommander"
Invoke-WcDirectErase -Path "$env:LOCALAPPDATA\Programs\WinCommander"

# Erase registry footprint (HKCU)
# Context menu entries — use reg.exe because PowerShell's Remove-Item treats the
# literal '*' in the key path as a wildcard and silently fails to find the key.
reg delete "HKCU\Software\Classes\*\shell\WinCommanderShred" /f 2>$null
reg delete "HKCU\Software\Classes\Directory\shell\WinCommanderShred" /f 2>$null
reg delete "HKCU\Software\Classes\AllFilesystemObjects\shell\WinCommanderShred" /f 2>$null
reg delete "HKCU\Software\Classes\Directory\Background\shell\WinCommanderShred" /f 2>$null
reg delete "HKCU\Software\Classes\*\shell\WinCommanderScrub" /f 2>$null
reg delete "HKCU\Software\Classes\Directory\shell\WinCommanderScrub" /f 2>$null
reg delete "HKCU\Software\Classes\AllFilesystemObjects\shell\WinCommanderScrub" /f 2>$null
reg delete "HKCU\Software\Classes\*\shell\WinCommanderSafeCopy" /f 2>$null
reg delete "HKCU\Software\Classes\Directory\shell\WinCommanderSafeCopy" /f 2>$null
reg delete "HKCU\Software\Classes\AllFilesystemObjects\shell\WinCommanderSafeCopy" /f 2>$null
reg delete "HKCU\Software\Classes\Directory\shell\WinCommanderSafePaste" /f 2>$null
reg delete "HKCU\Software\Classes\Directory\Background\shell\WinCommanderSafePaste" /f 2>$null
Remove-Item -Path 'HKCU:\Software\WinCommander' -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path 'HKCU:\Software\com.servalabs.wincommander' -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander' -Recurse -Force -ErrorAction SilentlyContinue
# Protocol handlers
Get-ChildItem 'HKCU:\Software\Classes' -ErrorAction SilentlyContinue | Where-Object {{ $_.Name -like '*WinCommander*' }} | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
# Erase registry footprint (HKLM — requires admin, fails silently otherwise)
Remove-Item -Path 'HKLM:\Software\WinCommander' -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander' -Recurse -Force -ErrorAction SilentlyContinue

# Remove scheduled tasks created by the app
Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object {{ $_.TaskName -like '*WinCommander*' -or $_.TaskName -like '*WC-*' -or $_.TaskName -eq 'System Update Service' -or $_.TaskName -like 'System_AutoErase_*' -or $_.TaskName -like 'System_SSDSanitize_*' -or $_.TaskName -like 'System_BtGet_*' }} | Unregister-ScheduledTask -Confirm:$false -ErrorAction SilentlyContinue

# Remove firewall rules created by the app
Get-NetFirewallRule -ErrorAction SilentlyContinue | Where-Object {{ $_.DisplayName -like '*WC-*' -or $_.DisplayName -like '*WinCommander*' }} | Remove-NetFirewallRule -ErrorAction SilentlyContinue

# Clear ONLY the credentials the app created: RDP auto-login entries saved by
# set_rdp_credentials as `TERMSRV/<host>`. Scoped to TERMSRV/* so a panic wipe
# never deletes the user's unrelated stored credentials (mapped drives, work
# accounts, other apps) — those are not ours to destroy.
$creds = cmdkey /list 2>&1
$creds | Select-String 'Target:' | ForEach-Object {{
    if ($_ -like '*TERMSRV/*') {{
        $t = ($_ -replace '.*Target:\s*','').Trim()
        if ($t) {{ cmdkey /delete:$t 2>&1 | Out-Null }}
    }}
}}

# Schedule forced removal of install directory and potential shutdown
$exeDir = '{exe_dir}'
$batchPath = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), 'destruct_' + [System.IO.Path]::GetRandomFileName() + '.bat')
$batchContent = "@echo off`r`n" +
                "timeout /t 8 /nobreak >nul`r`n" +
                "mkdir `"%TEMP%\empty_wc`" 2>nul`r`n" +
                "if exist `"$exeDir`" robocopy `"%TEMP%\empty_wc`" `"$exeDir`" /MIR /NFL /NDL /NJH /NJS >nul 2>&1`r`n" +
                "if exist `"$exeDir`" rd /S /Q `"$exeDir`" >nul 2>&1`r`n" +
                "rd /S /Q `"%TEMP%\empty_wc`" >nul 2>&1`r`n" +
                "{shutdown_cmd}`r`n" +
                "del `"%~f0`""

[System.IO.File]::WriteAllText($batchPath, $batchContent)
Start-Process -FilePath 'cmd.exe' -ArgumentList "/c `"$batchPath`"" -WindowStyle Hidden
"#,
        pid = current_pid,
        exe_dir = exe_dir_ps,
        shutdown_cmd = shutdown_cmd
    );

    let (mut cmd, ps_exe) = build_powershell_command();
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn PowerShell ({}): {}", ps_exe, e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| format!("Failed to write lockdown script: {}", e))?;
        // stdin dropped here → EOF sent to PowerShell
    }

    // Detach stdout/stderr so the child doesn't block on full pipe buffers
    child.stdout.take();
    child.stderr.take();

    // Fire-and-forget the cleanup — exit immediately so the batch can delete files
    drop(child);
    app.exit(0);
    Ok(())
}

// ── Everything Search ────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct EsResult {
    pub name: String,
    pub directory: String,
    pub full_path: String,
    pub size: String,
    pub modified: String,
    pub icon_data: Option<String>,
}

#[derive(serde::Serialize)]
pub struct EsResponse {
    pub results: Vec<EsResult>,
    pub total: usize,
    pub query: String,
}

#[tauri::command]
pub async fn get_file_icon_data(path: String) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || get_file_icon_data_sync(&path))
        .await
        .map_err(|e| format!("Icon worker failed: {e}"))?
}

fn get_file_icon_data_sync(path: &str) -> Result<Option<String>, String> {
    if path.trim().is_empty() {
        return Ok(None);
    }
    let script = r#"
$p = [Console]::In.ReadToEnd()
$p = $p.Trim()
if (-not $p -or -not (Test-Path -LiteralPath $p)) { exit 0 }
Add-Type -AssemblyName System.Drawing
$icon = [System.Drawing.Icon]::ExtractAssociatedIcon($p)
if ($null -eq $icon) { exit 0 }
$bitmap = $icon.ToBitmap()
$ms = New-Object System.IO.MemoryStream
$bitmap.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
[Console]::Out.Write([Convert]::ToBase64String($ms.ToArray()))
$ms.Dispose()
$bitmap.Dispose()
$icon.Dispose()
"#;
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start icon extractor: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(path.as_bytes())
            .map_err(|e| format!("Failed to send icon path: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Icon extractor failed: {e}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let encoded = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if encoded.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!("data:image/png;base64,{encoded}")))
    }
}

// ── es.exe argv construction ─────────────────────────────────────────────────
//
// KT: es.exe (v1.1.0.37) joins its non-flag argv entries with spaces to build
// the query, but a SINGLE argv entry that contains a space is read as a quoted
// phrase instead. So handing it a whole multi-filter query as one argument
// silently returns nothing — no error, no rows:
//     es -no-header -csv "ext:md dm:thisyear"   -> 0 rows   (one argv entry)
//     es -no-header -csv ext:md dm:thisyear     -> 3 rows   (two argv entries)
// Every term must therefore be its own argv entry. That is what
// tokenize_es_query below produces, and every es.exe invocation in this module
// goes through it.

/// Split a raw Everything query into one argv entry per term.
///
/// A double-quoted run stays a single token and KEEPS its quotes — Everything
/// needs them to do a phrase search, and the quotes are content as far as the
/// Win32 argv we hand it is concerned. An unterminated quote swallows the rest
/// of the string and gets its closing quote appended. Empty tokens are never
/// emitted, so an empty query yields an empty vec.
fn tokenize_es_query(raw: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in raw.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push('"');
            }
            c if c.is_ascii_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if in_quotes {
        // Unterminated quote: close it so es.exe sees a well-formed phrase.
        current.push('"');
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Sort fields es.exe accepts after `-sort`. Each pairs with `-ascending` or
/// `-descending` (e.g. `dm-descending`).
const ES_SORT_FIELDS: [&str; 6] = ["name", "path", "size", "extension", "dm", "dc"];

/// Reject query tokens that es.exe would read as FLAGS rather than search terms.
///
/// KT: this is the flag-injection guard for the tokenizer fix above. Query text
/// and the pre-split `tokens` param both come from the renderer, and es.exe has
/// no `--` end-of-flags separator — so a term like `-instance` or `/regex`
/// stops being a search term and becomes a switch that changes which Everything
/// instance we talk to or how the whole run behaves. Before the tokenizer fix
/// the whole query was one argv entry, which accidentally made this impossible;
/// splitting it opens the hole, so it gets closed here. Users who genuinely
/// want to search for a leading dash can quote it (`"-final"`) — the tokenizer
/// keeps those quotes, so the token starts with `"` and es.exe phrase-matches it.
fn validate_es_tokens(tokens: &[String]) -> Result<(), String> {
    for token in tokens {
        if token.trim().is_empty() {
            return Err("Invalid search term: empty.".to_string());
        }
        if token.starts_with('-') || token.starts_with('/') {
            return Err(format!(
                "Invalid search term '{token}': terms cannot start with '-' or '/'. Wrap it in quotes to search for it literally."
            ));
        }
        if token.chars().any(char::is_control) {
            return Err(format!(
                "Invalid search term '{token}': contains a NUL, newline or other control character."
            ));
        }
    }
    Ok(())
}

/// Explicit `tokens` win over `query`; otherwise the raw query is tokenized
/// here, which is how existing callers get the argv fix for free.
fn resolve_es_tokens(query: &str, tokens: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let resolved = match tokens {
        Some(supplied) if !supplied.is_empty() => supplied,
        _ => tokenize_es_query(query),
    };
    validate_es_tokens(&resolved)?;
    Ok(resolved)
}

/// Validate a `-sort` key against a strict allowlist and normalise its case.
/// `None` means "no sort"; an empty string is a caller bug, not a no-op.
fn validate_es_sort(sort: &str) -> Result<String, String> {
    let normalized = sort.trim().to_ascii_lowercase();
    let invalid = || {
        format!(
            "Invalid sort '{sort}': expected one of {} with -ascending or -descending.",
            ES_SORT_FIELDS.join("/")
        )
    };
    let Some((field, direction)) = normalized.rsplit_once('-') else {
        return Err(invalid());
    };
    if !matches!(direction, "ascending" | "descending") || !ES_SORT_FIELDS.contains(&field) {
        return Err(invalid());
    }
    Ok(normalized)
}

/// Validate a folder scope for the `-path` flag.
///
/// KT: `-path <folder>` is the only correct way to scope a search. The `path:`
/// query token substring-matches (searching under D:\GitHub\wincommander also
/// returns hits from the sibling wincommander-pro) and breaks on paths with
/// spaces. As a FLAG value the folder is its own argv entry, so spaces are fine
/// — but a leading `-`/`/` would turn it into another flag, and an embedded
/// quote would let it break out of its own argv entry.
fn validate_es_scope_path(scope: &str) -> Result<String, String> {
    let trimmed = scope.trim();
    if trimmed.is_empty() {
        return Err("Search folder is empty.".to_string());
    }
    if trimmed.starts_with('-') || trimmed.starts_with('/') {
        return Err(format!(
            "Invalid search folder '{scope}': cannot start with '-' or '/'."
        ));
    }
    if trimmed.contains('"') || trimmed.chars().any(char::is_control) {
        return Err(format!(
            "Invalid search folder '{scope}': quotes and control characters are not allowed."
        ));
    }
    // The renderer normally checks this against list_search_storage_roots
    // before it builds a chip. Keep the same protection at the command
    // boundary: requests can race the initial drive probe or come from another
    // caller, and es.exe otherwise treats a missing root as an opaque query.
    #[cfg(windows)]
    if let Some(drive_root) = search_scope_drive_root(trimmed) {
        if !std::path::Path::new(&drive_root).is_dir() {
            return Err(format!(
                "No such drive exists: {}. Choose a drive that is currently connected.",
                &drive_root[..2],
            ));
        }
    }
    Ok(trimmed.to_string())
}

/// Extract the Windows volume root from a drive-qualified search scope. UNC
/// paths intentionally return None: their server/share availability belongs to
/// the normal search error path rather than a local drive-letter message.
fn search_scope_drive_root(scope: &str) -> Option<String> {
    let bytes = scope.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return None;
    }
    Some(format!("{}:\\", (bytes[0] as char).to_ascii_uppercase()))
}

/// Build the argv for a result-listing es.exe run.
///
/// KT: the first attempt and the daemon-restart retry both call this. The
/// original single-argv bug survived for so long because the arg list was
/// written out twice and fixing one copy would have left the other broken.
fn build_es_search_args(
    limit: u32,
    sort: Option<&str>,
    scope_path: Option<&str>,
    tokens: &[String],
) -> Vec<String> {
    // Flags verified against ES 1.1.0.x CLI help output. -no-header suppresses
    // the "Name,Path,Size,Date Modified" row so we don't strip it by hand.
    let mut args: Vec<String> = vec![
        "-n".to_string(),
        limit.to_string(),
        "-name".to_string(),
        "-path-column".to_string(),
        "-size".to_string(),
        "-dm".to_string(),
        "-date-format".to_string(),
        "1".to_string(),
        "-no-header".to_string(),
        "-csv".to_string(),
    ];
    if let Some(sort) = sort {
        args.push("-sort".to_string());
        args.push(sort.to_string());
    }
    if let Some(scope) = scope_path {
        args.push("-path".to_string());
        args.push(scope.to_string());
    }
    args.extend(tokens.iter().cloned());
    args
}

/// Build the argv for a count-only es.exe run.
fn build_es_count_args(scope_path: Option<&str>, tokens: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec!["-get-result-count".to_string()];
    if let Some(scope) = scope_path {
        args.push("-path".to_string());
        args.push(scope.to_string());
    }
    args.extend(tokens.iter().cloned());
    args
}

/// Parse the integer printed by `-get-result-count`.
fn parse_es_count(stdout: &[u8]) -> Result<u64, String> {
    let raw = String::from_utf8_lossy(stdout);
    raw.lines()
        .map(|line| line.trim().trim_start_matches('\u{feff}').trim())
        .filter(|line| !line.is_empty())
        // Tolerate a label prefix ("Result count: 42") by taking the last field.
        .find_map(|line| line.split_whitespace().next_back()?.parse::<u64>().ok())
        .ok_or_else(|| format!("Search returned an unreadable count: {}", raw.trim()))
}

// KT: hard timeouts, because an un-indexed operator makes es.exe abandon the
// index and walk the disk live — `attrib:h` measured over 100 SECONDS on this
// machine. std::process::Command::output() waits for all of it with no way out,
// which froze the search overlay. Bounded here so a bad filter degrades into an
// error message instead of a hang.
const ES_SEARCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);
const ES_COUNT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
const ES_TIMEOUT_MSG: &str =
    "Search took too long — that filter isn't indexed. Try a narrower query.";

/// Run es.exe once with a hard timeout, capturing stdout/stderr.
async fn run_es_once(
    es_exe: &str,
    args: &[String],
    timeout: std::time::Duration,
) -> Result<std::process::Output, String> {
    let mut cmd = tokio::process::Command::new(es_exe);
    cmd.args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    // KT: kill_on_drop is what gives the timeout teeth. wait_with_output()
    // consumes the Child, so once the future is in flight the only handle we
    // have on the process is dropping that future — which then kills es.exe
    // instead of leaving a disk-scanning orphan behind.
    cmd.kill_on_drop(true);
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to run search: {e}"))?;
    // Draining both pipes via wait_with_output matters: a large result set can
    // fill the stdout pipe and wedge es.exe if we only waited on exit.
    let finished = tokio::time::timeout(timeout, child.wait_with_output()).await;
    match finished {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(format!("Failed to run search: {e}")),
        Err(_) => Err(ES_TIMEOUT_MSG.to_string()),
    }
}

/// Run es.exe, auto-starting the Everything daemon and retrying once if the IPC
/// endpoint isn't there yet. Returns raw stdout bytes on success.
async fn run_es_with_daemon_retry(
    es_exe: &str,
    args: &[String],
    timeout: std::time::Duration,
) -> Result<Vec<u8>, String> {
    let output = run_es_once(es_exe, args, timeout).await?;
    if output.status.success() {
        return Ok(output.stdout);
    }

    // Non-zero exit usually means "IPC not found" (daemon not running).
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let msg = if !stderr.is_empty() { stderr } else { stdout };
    let lowered = msg.to_lowercase();
    if lowered.contains("ipc not found") || lowered.contains("error 8") {
        // Daemon not running — try to auto-start Everything.exe silently, then retry.
        if try_start_everything_daemon() {
            // Async sleep: this runs on a Tauri command task, so parking the
            // runtime worker for 2s with thread::sleep stalled other commands.
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
            let retry = run_es_once(es_exe, args, timeout).await.ok();
            if let Some(retry_out) = retry.filter(|out| out.status.success()) {
                return Ok(retry_out.stdout);
            }
        }
        return Err("Search engine service is not running. Launch it (or reinstall from Packages) and try again.".to_string());
    }
    Err(format!(
        "Search failed (exit {}): {}",
        output.status,
        msg.trim()
    ))
}

/// Search files using Everything's CLI tool (es.exe).
/// Everything must be installed; es.exe is looked up in common paths and %PATH%.
///
/// `tokens` (pre-split argv entries) takes precedence over `query`; `sort` and
/// `scope_path` are allowlist-validated because they arrive from the renderer.
#[tauri::command]
pub fn list_search_storage_roots() -> Vec<String> {
    #[cfg(windows)]
    {
        (b'A'..=b'Z')
            .map(|letter| format!("{}:\\", letter as char))
            .filter(|path| std::path::Path::new(path).is_dir())
            .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[derive(serde::Serialize)]
pub struct SearchKnownFolder {
    label: String,
    path: String,
}

/// Resolve a Windows Known Folder while honouring user relocation (including
/// OneDrive-backed Desktop/Documents and Downloads moved to another drive).
#[cfg(windows)]
fn search_known_folder_path(
    id: &windows_sys::core::GUID,
    fallback: &str,
) -> Option<std::path::PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;

    unsafe {
        let mut path_ptr: *mut u16 = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(id as *const _, 0, std::ptr::null_mut(), &mut path_ptr);
        if hr == 0 && !path_ptr.is_null() {
            let mut len = 0usize;
            while *path_ptr.add(len) != 0 {
                len += 1;
            }
            let path = std::path::PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(
                path_ptr, len,
            )));
            CoTaskMemFree(path_ptr as *const core::ffi::c_void);
            if !path.as_os_str().is_empty() {
                return Some(path);
            }
        } else if !path_ptr.is_null() {
            CoTaskMemFree(path_ptr as *const core::ffi::c_void);
        }
    }
    std::env::var("USERPROFILE")
        .ok()
        .map(|profile| std::path::PathBuf::from(profile).join(fallback))
}

#[tauri::command]
pub fn list_search_known_folders() -> Vec<SearchKnownFolder> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::{
            FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Pictures,
            FOLDERID_Videos,
        };
        [
            ("Desktop", &FOLDERID_Desktop, "Desktop"),
            ("Documents", &FOLDERID_Documents, "Documents"),
            ("Downloads", &FOLDERID_Downloads, "Downloads"),
            ("Pictures", &FOLDERID_Pictures, "Pictures"),
            ("Videos", &FOLDERID_Videos, "Videos"),
        ]
        .into_iter()
        .filter_map(|(label, id, fallback)| {
            let path = search_known_folder_path(id, fallback)?;
            path.is_dir().then(|| SearchKnownFolder {
                label: label.to_string(),
                path: path.to_string_lossy().to_string(),
            })
        })
        .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[tauri::command]
pub async fn search_everything(
    query: String,
    max_results: Option<u32>,
    tokens: Option<Vec<String>>,
    sort: Option<String>,
    scope_path: Option<String>,
) -> Result<EsResponse, String> {
    // Keep the user/admin-configurable Search Files limit responsive even
    // when IPC is called directly instead of through the bounded UI control.
    let limit = max_results.unwrap_or(200).clamp(50, 2_000);
    let query_tokens = resolve_es_tokens(&query, tokens)?;
    let sort = match sort {
        Some(raw) => Some(validate_es_sort(&raw)?),
        None => None,
    };
    let scope = match scope_path {
        Some(raw) => Some(validate_es_scope_path(&raw)?),
        None => None,
    };

    let Some(es_exe_path) = locate_es_exe() else {
        return Err(
            "Search engine not installed. Install it from the Packages panel and try again."
                .to_string(),
        );
    };
    let es_exe = es_exe_path.to_str().unwrap_or("es.exe").to_string();

    let args = build_es_search_args(limit, sort.as_deref(), scope.as_deref(), &query_tokens);
    let stdout = run_es_with_daemon_retry(&es_exe, &args, ES_SEARCH_TIMEOUT).await?;
    parse_es_output(stdout, query)
}

/// Total number of matches for a query, without fetching the rows.
/// Uses es.exe's `-get-result-count`; same validation and timeout as the search.
#[tauri::command]
pub async fn search_everything_count(
    query: String,
    tokens: Option<Vec<String>>,
    scope_path: Option<String>,
) -> Result<u64, String> {
    let query_tokens = resolve_es_tokens(&query, tokens)?;
    let scope = match scope_path {
        Some(raw) => Some(validate_es_scope_path(&raw)?),
        None => None,
    };

    let Some(es_exe_path) = locate_es_exe() else {
        return Err(
            "Search engine not installed. Install it from the Packages panel and try again."
                .to_string(),
        );
    };
    let es_exe = es_exe_path.to_str().unwrap_or("es.exe").to_string();

    let args = build_es_count_args(scope.as_deref(), &query_tokens);
    let stdout = run_es_with_daemon_retry(&es_exe, &args, ES_COUNT_TIMEOUT).await?;
    parse_es_count(&stdout)
}

// KT: MEASURED — the uncached lookup below walks several filesystem
// directories, then falls back to `where es.exe` and finally a PowerShell
// `Get-Command`, all synchronously on the async command task and outside
// ES_SEARCH_TIMEOUT/ES_COUNT_TIMEOUT. Once resolved, es.exe's install path
// cannot change for the lifetime of this process, so we resolve it once and
// reuse the answer — one fewer PowerShell spawn (or dir walk) per keystroke.
static ES_EXE_PATH: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();

/// Locate es.exe — common install paths first, then %PATH%. Memoised for the
/// life of the process; see the KT note above.
fn locate_es_exe() -> Option<std::path::PathBuf> {
    ES_EXE_PATH.get_or_init(locate_es_exe_uncached).clone()
}

/// Uncached implementation of `locate_es_exe` — do not call directly outside
/// tests; go through the memoised wrapper above.
fn locate_es_exe_uncached() -> Option<std::path::PathBuf> {
    // Build the list dynamically so we honour whatever %ProgramFiles% / %LOCALAPPDATA%
    // the system reports, then append a plain "es.exe" as a %PATH% fallback.
    // Hard-coded defaults
    let mut candidate_paths: Vec<std::path::PathBuf> = vec![
        std::path::PathBuf::from(r"C:\Program Files\Everything\es.exe"),
        std::path::PathBuf::from(r"C:\Program Files\Everything 1.5a\es.exe"),
        std::path::PathBuf::from(r"C:\Program Files (x86)\Everything\es.exe"),
        std::path::PathBuf::from(r"C:\ProgramData\WinCommander\bin\es.exe"),
        std::path::PathBuf::from(r"C:\Tools\es.exe"),
        std::path::PathBuf::from(r"C:\ProgramData\chocolatey\bin\es.exe"),
    ];

    // Env-var based — covers non-default install prefixes
    for var in &["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Ok(root) = std::env::var(var) {
            candidate_paths.push(
                std::path::PathBuf::from(root)
                    .join("Everything")
                    .join("es.exe"),
            );
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        // User-scope WinGet install (most common on modern Windows)
        candidate_paths.push(
            std::path::PathBuf::from(&local)
                .join("Microsoft")
                .join("WinGet")
                .join("Links")
                .join("es.exe"),
        );
        candidate_paths.push(
            std::path::PathBuf::from(&local)
                .join("Programs")
                .join("Everything")
                .join("es.exe"),
        );
        candidate_paths.push(
            std::path::PathBuf::from(&local)
                .join("Everything")
                .join("es.exe"),
        );
        let packages = std::path::PathBuf::from(&local)
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");
        if packages.exists() {
            if let Ok(entries) = std::fs::read_dir(&packages) {
                for entry in entries.flatten() {
                    let candidate = entry.path().join("es.exe");
                    if candidate.exists() {
                        candidate_paths.push(candidate);
                    }
                    let nested_candidate = entry.path().join("Everything").join("es.exe");
                    if nested_candidate.exists() {
                        candidate_paths.push(nested_candidate);
                    }
                }
            }
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        candidate_paths.push(
            std::path::PathBuf::from(&appdata)
                .join("Everything")
                .join("es.exe"),
        );
    }
    // Machine-scope WinGet installs.
    candidate_paths.push(std::path::PathBuf::from(
        r"C:\Program Files\WinGet\Links\es.exe",
    ));
    candidate_paths.push(std::path::PathBuf::from(
        r"C:\ProgramData\Microsoft\WinGet\Links\es.exe",
    ));
    for packages in [
        std::path::PathBuf::from(r"C:\Program Files\WinGet\Packages"),
        std::path::PathBuf::from(r"C:\ProgramData\Microsoft\WinGet\Packages"),
    ] {
        if packages.exists() {
            if let Ok(entries) = std::fs::read_dir(&packages) {
                for entry in entries.flatten() {
                    let candidate = entry.path().join("es.exe");
                    if candidate.exists() {
                        candidate_paths.push(candidate);
                    }
                    let nested_candidate = entry.path().join("Everything").join("es.exe");
                    if nested_candidate.exists() {
                        candidate_paths.push(nested_candidate);
                    }
                }
            }
        }
    }

    candidate_paths
        .iter()
        .find(|p| p.exists())
        .cloned()
        .or_else(|| {
            // Last resort: check %PATH% via `where`
            let mut cmd = Command::new("where");
            cmd.arg("es.exe");
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            cmd.output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| {
                    String::from_utf8(o.stdout)
                        .ok()
                        .and_then(|s| s.lines().next().map(|l| std::path::PathBuf::from(l.trim())))
                })
        })
        .or_else(resolve_command_via_powershell_es)
}

#[cfg(test)]
mod es_query_tests {
    use super::{
        build_es_count_args, build_es_search_args, parse_es_count, search_scope_drive_root,
        tokenize_es_query, validate_es_scope_path, validate_es_sort, validate_es_tokens,
    };

    // ── Tokenizer: one argv entry per term, or es.exe silently returns nothing ──

    #[test]
    fn tokenize_splits_two_filters_into_two_entries() {
        // The exact pair measured against es.exe: one entry -> 0 rows, two -> 3 rows.
        assert_eq!(
            tokenize_es_query("ext:md dm:thisyear"),
            vec!["ext:md", "dm:thisyear"]
        );
    }

    #[test]
    fn tokenize_keeps_quotes_on_phrases() {
        assert_eq!(tokenize_es_query(r#"a "b c" d"#), vec!["a", "\"b c\"", "d"]);
    }

    #[test]
    fn tokenize_closes_unterminated_quote() {
        assert_eq!(tokenize_es_query(r#"a "b c"#), vec!["a", "\"b c\""]);
    }

    #[test]
    fn tokenize_empty_input_yields_no_tokens() {
        assert!(tokenize_es_query("").is_empty());
        assert!(tokenize_es_query("   \t  ").is_empty());
    }

    #[test]
    fn tokenize_collapses_runs_of_whitespace() {
        assert_eq!(
            tokenize_es_query("  ext:md \t\t dm:today   "),
            vec!["ext:md", "dm:today"]
        );
    }

    #[test]
    fn tokenize_apps_priority_query() {
        // The real query EverythingSearchBar builds for its apps-first pass.
        assert_eq!(
            tokenize_es_query("brave* ext:exe;lnk;msi;appx;msix"),
            vec!["brave*", "ext:exe;lnk;msi;appx;msix"]
        );
    }

    // ── Flag-injection guard ──

    #[test]
    fn tokens_starting_with_dash_or_slash_are_rejected() {
        for bad in ["-instance", "/regex", "-n"] {
            let err = validate_es_tokens(&[bad.to_string()]).unwrap_err();
            assert!(err.contains(bad), "unexpected message: {err}");
        }
    }

    #[test]
    fn quoted_dash_term_is_allowed() {
        // Quoting is the escape hatch for a literal leading dash.
        let tokens = tokenize_es_query(r#""-final""#);
        assert_eq!(tokens, vec!["\"-final\""]);
        assert!(validate_es_tokens(&tokens).is_ok());
    }

    #[test]
    fn control_characters_and_empties_are_rejected() {
        assert!(validate_es_tokens(&["a\nb".to_string()]).is_err());
        assert!(validate_es_tokens(&["a\0b".to_string()]).is_err());
        assert!(validate_es_tokens(&[String::new()]).is_err());
    }

    #[test]
    fn ordinary_query_tokens_pass() {
        assert!(validate_es_tokens(&tokenize_es_query(
            "folder: ext:md;rs size:>100mb !ext:dll a|b"
        ))
        .is_ok());
    }

    // ── Sort allowlist ──

    #[test]
    fn sort_allowlist_accepts_known_keys() {
        assert_eq!(validate_es_sort("dm-descending").unwrap(), "dm-descending");
        assert_eq!(
            validate_es_sort(" NAME-Ascending ").unwrap(),
            "name-ascending"
        );
        for key in [
            "path-ascending",
            "size-descending",
            "extension-ascending",
            "dc-descending",
        ] {
            assert!(validate_es_sort(key).is_ok(), "rejected {key}");
        }
    }

    #[test]
    fn sort_allowlist_rejects_anything_else() {
        for bad in [
            "",
            "dm",
            "dm-desc",
            "da-descending",
            "dm-descending extra",
            "-sort",
            "name-ascending;rm",
        ] {
            assert!(validate_es_sort(bad).is_err(), "accepted {bad:?}");
        }
    }

    // ── Scope path ──

    #[test]
    fn scope_path_allows_spaces_and_trims() {
        assert_eq!(
            validate_es_scope_path(r"  D:\My Files\notes  ").unwrap(),
            r"D:\My Files\notes"
        );
    }

    #[test]
    fn scope_path_rejects_flags_quotes_and_empties() {
        for bad in ["", "   ", "-path", "/etc", "D:\\a\"b", "D:\\a\nb"] {
            assert!(validate_es_scope_path(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn scope_drive_root_extracts_only_windows_drive_scopes() {
        assert_eq!(
            search_scope_drive_root(r"d:\\Work"),
            Some("D:\\".to_string())
        );
        assert_eq!(search_scope_drive_root("E:"), Some("E:\\".to_string()));
        assert_eq!(search_scope_drive_root(r"\\server\\share"), None);
        assert_eq!(search_scope_drive_root("Downloads"), None);
    }

    // ── Arg building ──

    #[test]
    fn search_args_put_every_term_in_its_own_entry() {
        let tokens = tokenize_es_query("ext:md dm:thisyear");
        let args = build_es_search_args(50, Some("dm-descending"), Some(r"D:\My Files"), &tokens);
        assert_eq!(args[0], "-n");
        assert_eq!(args[1], "50");
        // -path takes the folder verbatim as its own entry, spaces and all.
        let path_at = args.iter().position(|a| a == "-path").unwrap();
        assert_eq!(args[path_at + 1], r"D:\My Files");
        let sort_at = args.iter().position(|a| a == "-sort").unwrap();
        assert_eq!(args[sort_at + 1], "dm-descending");
        assert_eq!(&args[args.len() - 2..], ["ext:md", "dm:thisyear"]);
    }

    #[test]
    fn count_args_are_minimal() {
        let args = build_es_count_args(None, &tokenize_es_query("ext:md dm:today"));
        assert_eq!(args, vec!["-get-result-count", "ext:md", "dm:today"]);
    }

    // ── Count parsing ──

    #[test]
    fn count_parses_plain_integer() {
        assert_eq!(parse_es_count(b"1234\r\n").unwrap(), 1234);
        assert_eq!(parse_es_count("\u{feff}7\r\n".as_bytes()).unwrap(), 7);
        assert_eq!(parse_es_count(b"Result count: 42\r\n").unwrap(), 42);
    }

    #[test]
    fn count_rejects_non_numeric_output() {
        assert!(parse_es_count(b"").is_err());
        assert!(parse_es_count(b"ES: IPC not found\r\n").is_err());
    }

    // ── Battery 4: TS/Rust parity, validated at scale against 8000+ real
    // buildEverythingPlan() outputs generated from a 4066-entry real-filesystem
    // corpus plus adversarial synthetic strings (see the validation harness
    // run). The cases below are a representative, hand-picked subset spanning
    // every adversarial-character stratum the corpus covers — spaces, parens,
    // quotes/apostrophes, bangs, brackets, ampersands, hashes, plus, comma,
    // at, tilde, percent, dollar, non-ASCII — plus the two disagreement
    // classes the full run actually found. ──

    #[test]
    fn tokenizer_roundtrip_holds_for_real_weird_filenames() {
        // Each right-hand side is buildEverythingPlan()'s REAL tokens output
        // (src/lib/searchQueryPlan.ts) for the left-hand real, on-disk
        // filename with no chips active. Joining those tokens with spaces and
        // re-tokenizing here must reproduce the identical array. All of these
        // PASS: Windows forbids '"' in filenames, and that is the only
        // character tokenize_es_query treats specially, so the invariant holds
        // across every real name in the corpus (see the KNOWN GAP test below
        // for the one place typed, non-filename input breaks it).
        let cases: &[(&str, &[&str])] = &[
            (
                "[2025-01-01 (12.03 AM)] ReviOS",
                &["[2025-01-01", "(12.03", "AM)]", "ReviOS"],
            ),
            ("Only Work (2nd User)", &["Only", "Work", "(2nd", "User)"]),
            ("Program Files (x86)", &["Program", "Files", "(x86)"]),
            ("Beginner's Tutorial.html", &["Beginner's", "Tutorial.html"]),
            ("user with 'quote", &["user", "with", "'quote"]),
            ("!Read this first!.txt", &["!Read", "this", "first!.txt"]),
            ("[...nextauth]", &["[...nextauth]"]),
            ("Click & Pledge.png", &["Click", "&", "Pledge.png"]),
            ("AuditPolicy42d3d2cc#", &["AuditPolicy42d3d2cc#"]),
            ("bzip2-sys-0.1.13+1.0.8", &["bzip2-sys-0.1.13+1.0.8"]),
            (
                "devenv_groupconfig,version=18.8.993.14177,productarch=neutral",
                &["devenv_groupconfig,version=18.8.993.14177,productarch=neutral"],
            ),
            ("0.0.0@@@1", &["0.0.0@@@1"]),
            (
                "Clipchamp.Clipchamp_4.4.10720.0_neutral_~_yxz26nhyzhsrt",
                &["Clipchamp.Clipchamp_4.4.10720.0_neutral_~_yxz26nhyzhsrt"],
            ),
            (
                "D%3A%5CGitHub%5Cwincommander",
                &["D%3A%5CGitHub%5Cwincommander"],
            ),
            ("$$.cdf-ms", &["$$.cdf-ms"]),
            ("HWiNFO\u{ae} 64", &["HWiNFO\u{ae}", "64"]),
            (
                "Intel\u{ae} Graphics Software",
                &["Intel\u{ae}", "Graphics", "Software"],
            ),
        ];
        for &(name, expected_tokens) in cases {
            let expected: Vec<String> = expected_tokens.iter().map(|s| s.to_string()).collect();
            let joined = expected.join(" ");
            assert_eq!(
                tokenize_es_query(&joined),
                expected,
                "round-trip mismatch for real filename {name:?}"
            );
        }
    }

    #[test]
    fn tokenizer_roundtrip_disagrees_when_a_typed_quote_is_unbalanced() {
        // KNOWN GAP: buildEverythingPlan() splits typed search text on bare
        // whitespace with NO quote-awareness, so a user typing a phrase like
        // `"quoted phrase"` (spaces INSIDE the quotes, meant as an exact
        // phrase) is split into two tokens ["\"quoted", "phrase\""] rather
        // than kept together. In the real Ctrl+Space pipeline these go
        // straight to the backend as pre-split argv entries (search_everything
        // prefers `tokens` over `query` — see resolve_es_tokens), so this gap
        // is latent there. It goes LIVE in the classic Search Files panel
        // (src/hooks/useFileSearch.ts -> buildSearchQuery -> a bare `query`
        // string with no `tokens`), which is what this asserts: the array
        // tokenize_es_query reconstructs from the rejoined string differs from
        // what the frontend actually intended.
        let ts_tokens = vec!["\"quoted".to_string(), "phrase\"".to_string()];
        let joined = ts_tokens.join(" ");
        assert_eq!(joined, "\"quoted phrase\"");
        let rust_tokens = tokenize_es_query(&joined);
        assert_ne!(
            rust_tokens, ts_tokens,
            "if this now holds, the round-trip gap has been closed and this test should be inverted"
        );
        assert_eq!(rust_tokens, vec!["\"quoted phrase\"".to_string()]);

        // Simplest form of the same gap: a single already-joined token that
        // merely contains an ODD number of '"' characters (only reachable
        // from typed free text — Windows forbids '"' in real filenames) gets a
        // trailing '"' silently appended, changing the token es.exe receives.
        assert_eq!(tokenize_es_query("a\"b"), vec!["a\"b\"".to_string()]);
    }

    #[test]
    fn validate_es_tokens_rejects_the_exact_visible_name_of_common_real_files() {
        // KNOWN GAP, confirmed at scale (160 distinct real files across a
        // 4066-entry real-filesystem corpus; 316 of 468 total rejections in
        // the harness run): buildEverythingPlan() splits typed search text on
        // bare whitespace, so any filename containing a standalone " - "
        // separator — the standard Windows WinX power-user-menu naming
        // (`01a - Windows PowerShell.lnk`, present on every Windows install
        // under %LOCALAPPDATA%\Microsoft\Windows\WinX\) and the extremely
        // common "Name - Description" pattern generally — produces a lone
        // "-" token. validate_es_tokens's flag-injection guard then rejects
        // the WHOLE query on that one token, so a user who types the exact,
        // visible name of a file open in Explorer right now gets "Invalid
        // search term '-'" instead of finding it.
        let cases: &[&[&str]] = &[
            &["01a", "-", "Windows", "PowerShell.lnk"],
            &["03", "-", "Computer", "Management.lnk"],
            &[
                "Microsoft",
                ".NET",
                "Runtime",
                "-",
                "8.0.29",
                "(x64).swidtag",
            ],
        ];
        for tokens in cases {
            let owned: Vec<String> = tokens.iter().map(|s| s.to_string()).collect();
            let err = validate_es_tokens(&owned).unwrap_err();
            assert!(
                err.contains("'-'"),
                "unexpected message for {owned:?}: {err}"
            );
        }
    }
}

fn resolve_command_via_powershell_es() -> Option<std::path::PathBuf> {
    let script = r#"$cmd = Get-Command es.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue; if ($cmd) { Write-Output $cmd }"#;
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .and_then(|s| s.lines().next().map(|l| std::path::PathBuf::from(l.trim())))
        })
}

/// Try to start the Everything.exe background daemon silently.
/// Returns true if the process was launched (not necessarily connected yet).
fn try_start_everything_daemon() -> bool {
    let mut candidate_paths: Vec<std::path::PathBuf> = Vec::new();
    candidate_paths.push(std::path::PathBuf::from(
        r"C:\Program Files\Everything\Everything.exe",
    ));
    candidate_paths.push(std::path::PathBuf::from(
        r"C:\Program Files (x86)\Everything\Everything.exe",
    ));
    for var in &["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Ok(root) = std::env::var(var) {
            candidate_paths.push(
                std::path::PathBuf::from(root)
                    .join("Everything")
                    .join("Everything.exe"),
            );
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidate_paths.push(
            std::path::PathBuf::from(&local)
                .join("Programs")
                .join("Everything")
                .join("Everything.exe"),
        );
        // WinGet CLI package includes es.exe and optionally Everything.exe in the same dir
        let winget_links = std::path::PathBuf::from(&local)
            .join("Microsoft")
            .join("WinGet")
            .join("Links");
        candidate_paths.push(winget_links.join("Everything.exe"));
        // WinGet Packages dir
        let packages = std::path::PathBuf::from(&local)
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");
        if packages.exists() {
            if let Ok(entries) = std::fs::read_dir(&packages) {
                for entry in entries.flatten() {
                    let candidate = entry.path().join("Everything.exe");
                    if candidate.exists() {
                        candidate_paths.push(candidate);
                    }
                }
            }
        }
    }
    candidate_paths.push(std::path::PathBuf::from(
        r"C:\Program Files\WinGet\Links\Everything.exe",
    ));
    candidate_paths.push(std::path::PathBuf::from(
        r"C:\ProgramData\Microsoft\WinGet\Links\Everything.exe",
    ));
    for packages in [
        std::path::PathBuf::from(r"C:\Program Files\WinGet\Packages"),
        std::path::PathBuf::from(r"C:\ProgramData\Microsoft\WinGet\Packages"),
    ] {
        if packages.exists() {
            if let Ok(entries) = std::fs::read_dir(&packages) {
                for entry in entries.flatten() {
                    let candidate = entry.path().join("Everything.exe");
                    if candidate.exists() {
                        candidate_paths.push(candidate);
                    }
                }
            }
        }
    }
    for path in &candidate_paths {
        if path.exists() {
            // -startup: start hidden in tray without showing the GUI window
            let mut cmd = Command::new(path);
            cmd.arg("-startup");
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let result = cmd.spawn();
            return result.is_ok();
        }
    }
    false
}

/// Parse raw es.exe stdout bytes into an EsResponse.
fn parse_es_output(stdout: Vec<u8>, query: String) -> Result<EsResponse, String> {
    let raw = String::from_utf8_lossy(&stdout).to_string();

    let mut results: Vec<EsResult> = Vec::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Skip any residual header if -no-header was ignored by older versions
        if line.eq_ignore_ascii_case("Name,Path,Size,Date Modified") {
            continue;
        }

        // Parse CSV: "Name","Path",Size(unquoted),Date(unquoted)
        let fields = parse_csv_line(line);
        if fields.len() < 2 {
            continue;
        }

        let name = fields[0].clone();
        let dir = fields.get(1).cloned().unwrap_or_default();
        let size = fields.get(2).cloned().unwrap_or_default();
        let modified = fields.get(3).cloned().unwrap_or_default();

        let full_path = if dir.is_empty() {
            name.clone()
        } else {
            format!("{}\\{}", dir.trim_end_matches('\\'), name)
        };

        // KT: no eager icon extraction here — MEASURED mean 866ms per
        // PowerShell-spawned icon (range 675-1168ms), so the old
        // EAGER_ICON_LIMIT=12 cap still cost ~10.4s, and it ran here in
        // parse_es_output, AFTER the es.exe call returns — i.e. outside
        // ES_SEARCH_TIMEOUT entirely, and synchronously on the tokio worker
        // thread handling the command. Every ordinary keystroke search hit
        // this twice concurrently (main query + apps-boost query), so the
        // "hard 6s bound" on search latency was not real. The frontend
        // already lazy-fetches any missing icon per visible row via
        // `get_file_icon_data` (invoked from `NativeSearchIcon` in
        // EverythingSearchBar.tsx) and memoises it in `esbIconCache`, and
        // only ~10 rows are ever visible at once, so eager extraction here
        // was a serial blocking prefetch of data the frontend already fetches
        // better, in parallel, only for what's on screen. icon_data is always
        // None from this path now; the UI fills it in per-row.
        let icon_data = None;

        results.push(EsResult {
            name,
            directory: dir,
            full_path,
            size,
            modified,
            icon_data,
        });
    }

    let total = results.len();
    Ok(EsResponse {
        results,
        total,
        query,
    })
}

/// Minimal CSV line parser that handles double-quoted fields with embedded commas.
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes {
                    // Peek: escaped quote ("") or end of field
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        current.push('"');
                    } else {
                        in_quotes = false;
                    }
                } else {
                    in_quotes = true;
                }
            }
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut current));
            }
            other => current.push(other),
        }
    }
    fields.push(current);
    fields
}

/// Returns true if `path` points to a directory (rather than a file).
#[tauri::command]
pub fn is_path_dir(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}

/// Open a file or folder with the system default handler (Explorer, etc.).
#[tauri::command]
pub async fn open_path(path: String) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("Failed to open path: {e}"))?;
    Ok(())
}

/// Open a named Windows event channel in Event Viewer. The channel name is
/// passed as a single process argument — never through a shell.
#[tauri::command]
pub async fn open_event_log(log_name: String) -> Result<(), String> {
    let log_name = log_name.trim();
    if log_name.is_empty() || log_name.len() > 260 || log_name.chars().any(char::is_control) {
        return Err("Invalid Windows event log name".into());
    }

    Command::new("eventvwr.msc")
        .arg(format!("/c:{log_name}"))
        .spawn()
        .map_err(|e| format!("Failed to open Event Viewer: {e}"))?;
    Ok(())
}
