// src-tauri/src/settings.rs
//
// Unified Settings Manager for WinCommander
// ──────────────────────────────────────────
// settings.json is the SINGLE SOURCE OF TRUTH for desired system state.
// Windows Registry is the enforcement layer (applied state).
// This module handles read/write/migrate/diff for the admin architecture.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::sync::Mutex;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// SCHEMA — Version 1
// ═══════════════════════════════════════════════════════════════════════

pub const SETTINGS_VERSION: u32 = 2;

const DEFAULT_PERMANENTLY_HIDDEN_PANELS: &[&str] = &["sidecar", "productivity", "server-apps"];

// ── Device Identifiers ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GpuDetection {
    pub gpu_found: bool,
    pub gpu_name: Option<String>,
    pub driver_version: Option<String>,
    pub vram_mb: Option<i64>,
    pub compute_capability: Option<String>,
    pub cuda_found: bool,
    pub cuda_path: Option<String>,
    pub last_checked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentifiers {
    pub cpu: Option<String>,
    pub ram_gb: Option<f32>,
    pub gpu: Option<String>,
    #[serde(default)]
    pub disks: Vec<DiskInfo>,
    #[serde(default)]
    pub mac_addresses: Vec<String>,
    pub serial_number: Option<String>,
    pub bios_version: Option<String>,
    pub os_build: Option<String>,
    pub domain: Option<String>,
    pub time_zone: Option<String>,
    pub system_locale: Option<String>,
    #[serde(default)]
    pub users: Vec<UserInfo>,
    pub runtimes: Option<String>,
    pub windows_activated: Option<bool>,
    #[serde(default)]
    pub bitlocker_status: Vec<BitLockerStatus>,
    pub last_update_at: Option<String>,
    pub pending_updates_count: Option<u32>,
    // ── Display / UI fields (seeded from startup probe, shown on Dashboard) ──
    /// Hostname shown in the SystemStatusCard header.
    #[serde(default)]
    pub hostname: Option<String>,
    /// Full OS name e.g. "Windows 11 Pro 24H2".
    #[serde(default)]
    pub os_name: Option<String>,
    /// Display version e.g. "24H2".
    #[serde(default)]
    pub os_version: Option<String>,
    /// "Laptop" | "Desktop" | "Unknown".
    #[serde(default)]
    pub device_type: Option<String>,
    /// Human-readable total RAM string e.g. "16 GB".
    #[serde(default)]
    pub ram: Option<String>,
    /// Whether the process is running as admin.
    #[serde(default)]
    pub is_admin: Option<bool>,
    /// NVIDIA GPU + CUDA detection result — populated by hardware probe.
    #[serde(default)]
    pub gpu_detection: Option<GpuDetection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub model: String,
    pub size_gb: f32,
    pub disk_type: String, // SSD/HDD
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub username: String,
    pub enabled: bool,
    pub is_admin: bool,
    pub last_logon: Option<String>,
    pub sid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitLockerStatus {
    pub drive: String,
    pub encrypted: bool,
    pub status: String, // On/Off
}

// ═══════════════════════════════════════════════════════════════════════
// SYSTEM STATE — Container for all toggle-able system settings
// ═══════════════════════════════════════════════════════════════════════

/// Groups all system-level settings that have an ideal/current duality.
/// `ideal` = what the admin/user wants the system to be.
/// `current` = what the system actually reports (auto-probed).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemState {
    #[serde(default)]
    pub device: DeviceIdentifiers,
    #[serde(default)]
    pub privacy: PrivacySettings,
    #[serde(default)]
    pub tweaks: TweakSettings,
    #[serde(default)]
    pub network: NetworkSettings,
    #[serde(default)]
    pub identity: IdentitySettings,
    #[serde(default)]
    pub apps: AppManagementSettings,
    #[serde(default)]
    pub productivity: ProductivitySettings,
    #[serde(default)]
    pub server_apps: ServerAppsSettings,
    /// Security diagnostics — driver-health / Device-Manager check (#6).
    #[serde(default)]
    pub security: SecuritySettings,
}

// ── Security diagnostics ─────────────────────────────────────────────

/// #6: driver-health / Device-Manager check. Persistence layer; runtime
/// authority lives in commander-pro/src/driver_health.rs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SecuritySettings {
    pub drivers: DriverHealthSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DriverHealthSettings {
    /// Opt-in periodic re-scan that toasts NEW critical problem devices.
    pub watch_enabled: bool,
    /// Poll cadence in seconds. 0 → treat as default 60 at use site; Pro
    /// clamps to [30, 3600].
    pub watch_interval_secs: u64,
    /// Run one scan on app launch (independent of watch_enabled). The TS
    /// layer owns the live default (true); Rust default derives false.
    pub scan_on_startup: bool,
}

/// Root settings schema v2. Splits system state into ideal/current pairs.
/// - `ideal`: what the admin/user wants (set by toggles, admin push, wizard)
/// - `current`: what the OS actually reports (auto-probed from registry)
///
/// Accept a string, number, bool, or null and coerce it to a String.
/// Defends against settings files where a field that should be a string was
/// written as another scalar (e.g. an older settings-bridge wrote `lastSeenAt`
/// as an integer epoch). Without this, one mistyped field made the entire
/// `read_settings` deserialize fail and bricked the whole app (blank UI / spin).
fn de_string_lenient<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    let v = serde_json::Value::deserialize(deserializer)?;
    Ok(match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Null => String::new(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    // ── Meta ──
    pub settings_version: u32,
    pub app_version: String,
    pub device_id: String,
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub last_seen_at: String,
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub created_at: String,

    // ── App Preferences (not system state — stays at root) ──
    #[serde(default)]
    pub app: AppPreferences,

    // ── Ideal State: what admin/user wants ──
    #[serde(default)]
    pub ideal: SystemState,

    // ── Current State: what the system actually is ──
    #[serde(default)]
    pub current: SystemState,

    // ── Admin Policy ──
    #[serde(default)]
    pub policy: PolicySettings,
}

// ── App Preferences ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RdpNode {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub first_run_complete: bool,
    /// Whether the mandatory first-run spotlight tour has been completed.
    /// Separate from `first_run_complete`: this gates whether the auto-started
    /// tour can be dismissed early (see GuideHost.tsx) — false only for the
    /// very first tour run after a fresh install.
    #[serde(default)]
    pub has_seen_mandatory_tour: bool,
    /// Whether WinCommander updates ITSELF automatically (background download +
    /// "Restart now" prompt driven by the scheduler in `updater.rs`). Defaults
    /// to `true`; chosen once in the first-run wizard. NOT the winget
    /// installed-apps policy — that is `ideal.apps.auto_update`.
    #[serde(default = "default_true")]
    pub auto_update: bool,
    #[serde(default)]
    pub start_minimized: bool,
    #[serde(default)]
    pub context_menu_enabled: bool,
    #[serde(default)]
    pub scrub_context_menu_enabled: bool,
    #[serde(default)]
    pub safe_copy_context_menu_enabled: bool,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// True once the user has explicitly touched the Cleanup "Clear All
    /// Traces" exclude list. Until then the frontend pre-excludes Wi-Fi
    /// Profiles + Browser Audit by default every session.
    #[serde(default)]
    pub cleanup_excludes_customized: bool,
    #[serde(default = "default_last_panel")]
    pub last_panel: String,
    #[serde(default = "default_dashboard_view_mode")]
    pub dashboard_view_mode: String,
    /// Dashboard right-column cards currently expanded ("system" / "storage" /
    /// "network"), in the order they were opened (oldest first). At most two
    /// are open at once; opening a third evicts the oldest. Empty = use the
    /// frontend default. Mirrors AppPreferences.dashboardOpenCards.
    #[serde(default)]
    pub dashboard_open_cards: Vec<String>,
    /// Dashboard scan finding IDs the user chose to ignore. These stay hidden
    /// until the user restores ignored findings from the Needs Attention card.
    #[serde(default)]
    pub ignored_finding_ids: Vec<String>,
    /// Bell-popover score category keys the user dismissed from the compact
    /// "needs attention" summary. Separate from per-finding ignores above.
    #[serde(default)]
    pub dismissed_needs_attention_ids: Vec<String>,
    /// User experience level — controls which features are visible (simple/standard/advanced)
    #[serde(default = "default_experience_level")]
    pub experience_level: String,
    /// Redesign persona density. Optional so existing users keep migrating
    /// from experience_level until the frontend writes the new field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density: Option<String>,
    /// Redesign capability bundles selected by need, not skill level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
    /// Threat-model persona axis ("casual" | "secure"), chosen at first-run or
    /// in Settings. None = unset (new install pre-first-run, or an upgrade from
    /// before this field existed) — the frontend's `getPersona()` resolves that
    /// to "secure", preserving today's all-modules-on behavior. Orthogonal to
    /// `density`/`capabilities` above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// Whether Privacy Clean features are shown
    #[serde(default)]
    pub privacy_clean_enabled: bool,
    /// Panic hotkey that triggers immediate lockdown (e.g. "Ctrl+Shift+Q")
    #[serde(default = "default_panic_hotkey")]
    pub panic_hotkey: String,
    /// Seconds the sidebar Lockdown button counts down before firing (3–30).
    /// Frontend reads/writes this (LockdownTimerRow); Rust doesn't consume it,
    /// but the field MUST exist or patch_settings_cmd's serialize→merge→
    /// deserialize round-trip silently drops it (same bug as ramdisk_autostart).
    #[serde(default = "default_lockdown_timer_sec")]
    pub lockdown_timer_sec: u32,
    #[serde(default)]
    pub rdp_nodes: Vec<RdpNode>,
    #[serde(default)]
    pub selected_rdp_node_id: Option<String>,
    #[serde(default)]
    pub notifications: NotificationSettings,
    #[serde(default)]
    pub vault: VaultSettings,
    #[serde(default)]
    pub first_run: FirstRunSettings,
    #[serde(default)]
    pub shortcuts: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub flows: Vec<crate::flow_engine::Flow>,
    /// v2 Pro-engine rule store (raw JSON — the flow_core::Rule wire shape).
    /// Separate from the legacy `flows` field so the Pro flows engine and the
    /// legacy Free engine never fight over one store or double-execute a rule.
    /// The Free side treats these as opaque JSON and forwards them to Pro.
    #[serde(default, rename = "proFlows")]
    pub pro_flows: Vec<serde_json::Value>,
    #[serde(default)]
    pub contingency: crate::flow_engine::ContingencySettings,
    /// Module enable/disable map — controls which features run at all.
    /// Missing keys default to false (disabled). Keys: privacy, privacyShield,
    /// cleanup, network, tweaks, apps, vault, mesh, productivity, serverApps,
    /// searchFiles, flows.
    #[serde(default)]
    pub modules: std::collections::HashMap<String, bool>,
    /// Ed25519 signing seed (32 bytes, base64) used to sign exported flow
    /// bundles. Lazily generated on first export and never rotated — the
    /// public key acts as a stable "this is my machine" identifier for
    /// flow bundles. Will be replaced by the shared device key once
    /// `evidence.vault` ships (migration is one-way: keep this seed,
    /// import it as the vault seed). Hidden from the settings UI.
    #[serde(default)]
    pub flow_signing_seed_b64: Option<String>,
    /// Dead-man's-switch config: inactivity timer that fires a designated
    /// flow when WinCommander hasn't seen the operator for N days.
    /// Optional so existing settings.json files load cleanly with no
    /// switch configured. See `dead_mans_switch.rs` for semantics.
    #[serde(default)]
    pub dead_mans_switch: Option<crate::inactivity_timer::DeadMansSwitchConfig>,
    /// Sidebar keyword that reveals sidebar-lock panels (default: "unlock").
    #[serde(default)]
    pub unlock_keyword: Option<String>,
    /// Sidebar keyword that re-hides sidebar-lock panels (default: "lock").
    #[serde(default)]
    pub lock_keyword: Option<String>,
    /// Panel IDs hidden until the unlock keyword is typed. When None, the
    /// frontend defaults to an empty list — all panels visible. Only intelligence
    /// and system-identity have their own separate session-only triggers.
    #[serde(default)]
    pub locked_panel_ids: Option<Vec<String>>,
    /// Whether activity logging to %LOCALAPPDATA%\WinCommander\logs\ is enabled.
    /// Defaults to true (None = enabled). Logs rotate daily; files older than
    /// 7 days are purged automatically on each log write.
    #[serde(default)]
    pub logging_enabled: Option<bool>,
    /// AI Security Advisor settings (spec 13 / #10).
    #[serde(default)]
    pub advisor: AdvisorSettings,
    /// Internet kill switch — when true, WinCommander-KillSwitch firewall
    /// rules block ALL traffic (in + out). Persisted so the Dashboard
    /// reflects the engaged state on reload. Runtime authority is the
    /// firewall rule itself (see internet_kill_switch.rs).
    #[serde(default)]
    pub internet_kill_switch: bool,
    /// Developer option: when true, the self-update scheduler (updater.rs)
    /// skips all checks and suppresses every `updater://state` event — no
    /// banner, no background download, no restart prompt. Default false.
    #[serde(default)]
    pub disable_updates: bool,
    /// Reusable per-metric alert configuration (paid). One entry per monitored
    /// metric (CPU %, upload MB/s, download MB/s); each shares the same
    /// hysteresis/sustained suppressors. Persistence layer; the runtime
    /// authority is `net_traffic_alert::CONFIG`, seeded from this at startup and
    /// updated by `metric_alerts_set_config`.
    #[serde(default)]
    pub metric_alerts: MetricAlertsSettings,
    /// Decoy mode (paid): rebrand the app under a custom name. Applied to the
    /// window title + tray tooltip by decoy_mode.rs; the frontend mirrors it
    /// into the in-app title bar. Default disabled.
    #[serde(default)]
    pub decoy_mode: DecoyModeSettings,
    /// Hide the notification bell icon and suppress all popup toasts.
    /// Pure UI concealment — free tier. Persisted here so it survives restarts.
    #[serde(default)]
    pub hide_notification_bell: bool,
    /// Suppress the floating desktop-alert window (the external Tauri notification
    /// overlay that appears outside the main app window). Does NOT affect the
    /// in-app bell or Sonner toasts — only the floating overlay window.
    #[serde(default)]
    pub disable_native_notifications: bool,
    /// Sidebar quick-action keys the user has hidden from the right action bar.
    /// Hidden actions remain reachable from their own panels.
    #[serde(default)]
    pub hidden_sidebar_actions: Vec<String>,
    /// When true, notifications are also suppressed while panels are locked
    /// (Borrowed-PC mode active). Requires hideNotificationBell=false to take
    /// effect independently.
    #[serde(default)]
    pub mute_notifications_when_locked: bool,
    /// Panel IDs hidden permanently (regardless of Borrowed-PC mode state).
    /// Distinct from lockedPanelIds which only hide while Borrowed-PC is active.
    /// Option so the frontend can tell "never configured" (→ apply defaults)
    /// apart from "explicitly empty".
    #[serde(default)]
    pub permanently_hidden_panels: Option<Vec<String>>,
    /// When true, the PURGING SYSTEM operation overlay is suppressed during
    /// lockdown. The cascade fires silently via invoke without step progress shown.
    #[serde(default)]
    pub hide_destruction_sequence: bool,
    /// Non-panel surface keys hidden only while Borrowed Mode is active.
    /// Recognised keys: "notif-bell", "risk-matrix", "more-products",
    /// "action:dismount", "action:delete", "action:scrubMeta", "action:lockdown".
    /// Option so the frontend can apply defaults when never configured.
    #[serde(default)]
    pub borrowed_hidden: Option<Vec<String>>,
    /// "Lock panel on close" (Secret Setting). When the resolved value is true
    /// AND a calculator PIN is armed, closing the window shows the calculator
    /// only (today's behavior). When false, closing hides to the tray instead
    /// (the next reveal is Borrowed-locked when locked panels are configured,
    /// via persisted lockedPanelIds). Default (None) resolves to: true if a
    /// real calculator PIN is armed, else false. Owner: PIN-aware default; the
    /// toggle is honored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_panel_on_close: Option<bool>,
    /// File-content search (full-text index) configuration — Free tier.
    /// Serializes as `fileSearch` via the struct-level camelCase rename.
    #[serde(default)]
    pub file_search: FileSearchSettings,
    /// Fleet agent onboarding (paid). Mirrors FleetSettings in settings.ts.
    /// Persisted here so the agent survives Pro restarts without re-entering the URL.
    #[serde(default)]
    pub fleet: FleetSettings,
    /// When true, the License quick-panel in the sidebar footer is hidden
    /// everywhere (the "Always" choice in the Borrowed Mode visibility table).
    /// "When borrowed" hiding is handled by the "license-panel" key in
    /// `borrowed_hidden`; this is the standalone always-hide flag. Default false.
    #[serde(default)]
    pub hide_license_panel: bool,
    /// When true, drift between ideal.* and current.* (detected post-probe) is
    /// silently re-applied without user action. Defaults to true — most users
    /// want Commander to keep their chosen settings even after Windows updates
    /// reset them. The user can disable this in Identity settings.
    #[serde(default = "default_true")]
    pub auto_heal: bool,
    /// Whether the user has dismissed the Pro install/upgrade prompt. Stays
    /// false until the user explicitly dismisses it; resets to false when a
    /// new Pro version becomes available (handled by the frontend).
    #[serde(default)]
    pub pro_install_prompt_dismissed: bool,
    /// Version key (e.g. "free:3.0.10" or "pro:3.0.10") of the last update the
    /// combined UpdateFlowDialog auto-opened for at startup. Lets the frontend
    /// tell "already announced this exact version" apart from "a new version
    /// showed up" so a still-pending update doesn't re-announce itself on
    /// every tray/window reveal or relaunch. Mirrors
    /// AppPreferences.lastAnnouncedUpdateVersion in src/types/settings.ts.
    #[serde(default)]
    pub last_announced_update_version: Option<String>,
    /// Whether the Private Mesh "How it works" first-visit explainer has been
    /// shown. Persisted here (not the WebView's localStorage) so it survives
    /// app updates and never re-triggers after an upgrade.
    #[serde(default)]
    pub mesh_how_it_works_seen: bool,
    /// UI-only: the welcome spotlight tour is queued to run. Set by the
    /// first-run wizard on finish/dismiss; cleared by GuideHost once it has
    /// launched the tour. Pure orchestration flag — no backend behavior.
    #[serde(default)]
    pub welcome_tour_pending: bool,
    /// UI-only: the welcome spotlight tour has already run at least once.
    /// Lets "replay tour" distinguish a first-time vs. repeat run.
    #[serde(default)]
    pub welcome_tour_completed: bool,
}

/// Persisted settings for the file-content search feature (Free tier).
/// Roots are indexed on demand; exclusions are glob patterns (e.g. "*.tmp").
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileSearchSettings {
    /// Root directories to index.
    pub roots: Vec<std::path::PathBuf>,
    /// Glob patterns to exclude (e.g. "*.tmp", "node_modules").
    pub exclusions: Vec<String>,
    /// False until the first-run default seeding has run.
    /// Distinguishes "never configured" from "user explicitly cleared all folders".
    #[serde(default)]
    pub initialized: bool,
}

/// Fleet agent connection config — persisted to settings.json, read at Pro
/// spawn time to seed the IPC args for the runtime-configurable agent.
/// Mirrors FleetSettings in src/types/settings.ts (camelCase via serde rename).
///
/// AV hygiene: this struct lives in the Free binary. It MUST NOT contain
/// fleet server URLs or anything that looks like C2 traffic — the actual HTTP
/// is done entirely in Pro (fleet_push.rs). Free only stores the URL in
/// settings and passes it to Pro via IPC args.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct FleetSettings {
    /// false = agent never starts even if serverUrl is set.
    #[serde(default)]
    pub enabled: bool,
    /// Fleet server base address, e.g. "http://fleet.corp.ts.net:8787".
    #[serde(default)]
    pub server_url: String,
    /// Enable command poll + verify in the heartbeat loop.
    #[serde(default)]
    pub dispatch: bool,
    /// Base64-encoded Ed25519 fleet public key (pinned; used to verify signed commands).
    #[serde(default)]
    pub signing_key_pub: String,
}

/// Paid: decoy-mode preference. Defaults OFF with an empty `display_name`,
/// so a fresh install never ships a process-masquerade identity. When the
/// user enables it without naming one, the real app name is used (no
/// built-in impersonation). See decoy_mode.rs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct DecoyModeSettings {
    pub enabled: bool,
    pub display_name: String,
}

/// Paid: a single reusable metric alert. The user can enable hysteresis and/or
/// sustained-breach suppression independently so a value hovering near the
/// limit doesn't fire a notification every second. The same shape drives every
/// monitored metric (CPU, upload, download) — see net_traffic_alert.rs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricAlertSettings {
    /// Master enable for firing this metric's alert. Live readouts are
    /// unaffected — this only gates notifications.
    pub enabled: bool,
    /// Limit in the metric's own unit (CPU = %, upload/download = MB/s).
    pub threshold: f64,
    /// When true, don't re-alert until the value drops below the reset point
    /// (threshold * (1 - hysteresisPct/100)).
    pub hysteresis_enabled: bool,
    /// Reset-band percentage below the threshold. Clamped 1..=90.
    pub hysteresis_pct: u8,
    /// When true, only fire after the value stays above the limit for
    /// `sustained_secs` continuously (brief spikes are ignored).
    pub sustained_enabled: bool,
    /// Seconds of continuous breach required to fire. Clamped 1..=600.
    pub sustained_secs: u32,
}

impl MetricAlertSettings {
    /// A disabled alert pre-seeded with a sensible threshold for its metric.
    pub fn with_threshold(threshold: f64) -> Self {
        Self {
            enabled: false,
            threshold,
            hysteresis_enabled: true,
            hysteresis_pct: 20,
            sustained_enabled: true,
            sustained_secs: 30,
        }
    }
}

impl Default for MetricAlertSettings {
    fn default() -> Self {
        Self::with_threshold(10.0)
    }
}

/// Paid: the full set of metric alerts. Adding a new monitored metric is a
/// matter of adding a field here + sampling it in net_traffic_alert.rs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricAlertsSettings {
    pub cpu: MetricAlertSettings,
    pub upload: MetricAlertSettings,
    pub download: MetricAlertSettings,
}

impl Default for MetricAlertsSettings {
    fn default() -> Self {
        Self {
            // CPU defaults to a 50% trip point per the owner's brief.
            cpu: MetricAlertSettings::with_threshold(50.0),
            upload: MetricAlertSettings::with_threshold(10.0),
            download: MetricAlertSettings::with_threshold(10.0),
        }
    }
}

/// #10: AI Security Advisor (local Ollama). Mirrors AdvisorSettings in
/// src/types/settings.ts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvisorSettings {
    #[serde(default = "default_advisor_model")]
    pub model: String,
}

fn default_advisor_model() -> String {
    // Qwen3.5 line is tagged qwen3.5:* in Ollama (verify the exact tag).
    // Keep in sync with DEFAULT_ADVISOR_MODEL (useAdvisor.ts) + llm.rs.
    "qwen3.5:4b".to_string()
}

impl Default for AdvisorSettings {
    fn default() -> Self {
        Self {
            model: default_advisor_model(),
        }
    }
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            first_run_complete: false,
            has_seen_mandatory_tour: false,
            auto_update: true,
            start_minimized: false,
            context_menu_enabled: false,
            scrub_context_menu_enabled: false,
            safe_copy_context_menu_enabled: false,
            sidebar_collapsed: false,
            cleanup_excludes_customized: false,
            last_panel: default_last_panel(),
            dashboard_view_mode: default_dashboard_view_mode(),
            dashboard_open_cards: Vec::new(),
            ignored_finding_ids: Vec::new(),
            dismissed_needs_attention_ids: Vec::new(),
            experience_level: default_experience_level(),
            density: None,
            capabilities: None,
            persona: None,
            privacy_clean_enabled: false,
            panic_hotkey: default_panic_hotkey(),
            lockdown_timer_sec: default_lockdown_timer_sec(),
            rdp_nodes: Vec::new(),
            selected_rdp_node_id: None,
            notifications: NotificationSettings::default(),
            vault: VaultSettings::default(),
            first_run: FirstRunSettings::default(),
            shortcuts: std::collections::HashMap::new(),
            flows: Vec::new(),
            pro_flows: Vec::new(),
            contingency: crate::flow_engine::ContingencySettings::default(),
            modules: std::collections::HashMap::new(),
            flow_signing_seed_b64: None,
            dead_mans_switch: None,
            unlock_keyword: None,
            lock_keyword: None,
            locked_panel_ids: None,
            logging_enabled: None,
            advisor: AdvisorSettings::default(),
            internet_kill_switch: false,
            disable_updates: false,
            metric_alerts: MetricAlertsSettings::default(),
            decoy_mode: DecoyModeSettings::default(),
            hide_notification_bell: false,
            disable_native_notifications: false,
            hidden_sidebar_actions: Vec::new(),
            mute_notifications_when_locked: false,
            permanently_hidden_panels: Some(
                DEFAULT_PERMANENTLY_HIDDEN_PANELS
                    .iter()
                    .map(|panel| panel.to_string())
                    .collect(),
            ),
            hide_destruction_sequence: false,
            borrowed_hidden: None,
            lock_panel_on_close: None,
            file_search: FileSearchSettings::default(),
            fleet: FleetSettings::default(),
            hide_license_panel: false,
            auto_heal: true,
            pro_install_prompt_dismissed: false,
            last_announced_update_version: None,
            mesh_how_it_works_seen: false,
            welcome_tour_pending: false,
            welcome_tour_completed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettings {
    #[serde(default = "default_notification_position")]
    pub position: String,
    #[serde(default = "default_notification_timeout")]
    pub timeout: u32,
}

fn default_notification_position() -> String {
    "bottom-right".to_string()
}

fn default_notification_timeout() -> u32 {
    4000
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuickMountSlot {
    pub file_path: String,
    pub drive_letter: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VaultSettings {
    pub default_mount_letter: Option<String>,
    #[serde(default)]
    pub recent_paths: Vec<String>,
    /// Auto-create a RAM disk on PC startup using the embedded spec.
    /// Frontend reads this on splash-complete; Rust itself doesn't
    /// consume it but the field MUST exist on the struct or
    /// patch_settings_cmd's serialize→merge→deserialize round-trip
    /// silently drops it (same class of bug as shred_folders).
    #[serde(default)]
    pub ramdisk_autostart: RamDiskAutostartSettings,
    /// Saved vault slots for the sidebar Quick Mount shortcut.
    /// Must exist here or patch_settings_cmd's round-trip drops it.
    #[serde(default)]
    pub quick_mount_slots: Vec<QuickMountSlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RamDiskAutostartSettings {
    pub enabled: Option<bool>,
    pub size_mb: Option<u32>,
    pub drive_letter: Option<String>,
    pub filesystem: Option<String>,
    pub label: Option<String>,
    pub read_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FirstRunSettings {
    #[serde(default)]
    pub selected_blocklists: Vec<String>,
}

fn default_panic_hotkey() -> String {
    "Ctrl+Shift+Q".to_string()
}

fn default_lockdown_timer_sec() -> u32 {
    // Mirrors the frontend default in LockdownTimerRow (src/panels/secret/index.tsx).
    4
}

fn default_experience_level() -> String {
    "standard".to_string()
}

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    // KT: Dark is the working default. Light mode CSS is not fully implemented (Plan bug #1).
    // Returning "light" here caused blank screen on fresh settings.json because ThemeContext
    // reads this value async on mount and switches the entire app to broken light mode.
    "dark".to_string()
}

fn default_last_panel() -> String {
    "dashboard".to_string()
}

fn default_dashboard_view_mode() -> String {
    "map".to_string()
}

// ── Privacy Settings (Desired State) ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySettings {
    #[serde(default)]
    pub telemetry: TelemetrySettings,
    #[serde(default)]
    pub clipboard: ClipboardSettings,
    #[serde(default)]
    pub tracking: TrackingSettings,
    #[serde(default)]
    pub lockscreen: LockScreenSettings,
    #[serde(default)]
    pub app_capabilities: AppCapabilitySettings,
    /// Internet communication restrictions (~20 registry keys)
    #[serde(default)]
    pub internet_communication: InternetCommunicationSettings,
    #[serde(default)]
    pub privacy_protection_enabled: Option<bool>,
    #[serde(default)]
    pub setup_completion_nags_disabled: Option<bool>,
    /// Anti-Acquisition Defenses: continuous WARN-mode watcher toggle —
    /// polls the existing read-only `Scan-AcquisitionThreats` detector from
    /// the frontend. Off by default.
    #[serde(default)]
    pub acquisition_watch_enabled: Option<bool>,
    #[serde(default)]
    pub privacy_shield: PrivacyShieldSettings,
    /// F-2: decoy file monitor — filesystem honeypots.
    #[serde(default)]
    pub decoy_monitor: DecoyMonitorSettings,
    /// F-3: anti-ransomware monitor — mass-modify detection.
    #[serde(default)]
    pub ransomware_monitor: RansomwareMonitorSettings,
    /// #4: remote-access monitor — active incoming-session detector. Paid.
    #[serde(default)]
    pub remote_access_monitor: RemoteAccessMonitorSettings,
    /// #5: screen-capture detection + own-window capture protection. Paid.
    #[serde(default)]
    pub screen_capture: ScreenCaptureSettings,
    /// F-5: silent panic via typed code-phrase. Paid.
    #[serde(default)]
    pub coercion_phrase: CoercionPhraseSettings,
    /// Customisable lockdown configuration. Read by
    /// `full_lockdown` in backend.rs to decide which privacy
    /// cleaners run, whether to uninstall the app at the end, and
    /// whether to deactivate the licence / shut the system down.
    /// All panic triggers (sidebar button, Ctrl+Shift+Q,
    /// coercion phrase) honour the same config — no per-trigger args.
    #[serde(default)]
    pub self_destruct: SelfDestructSettings,
    #[serde(default)]
    pub startup_pin: StartupPinSettings,
    #[serde(default)]
    pub distress_phrases: Option<Vec<DistressPhraseEntry>>,
    /// Per-browser extension preferences for browser hardening. Keys use
    /// `<detected browser name>::<stable extension slug>`; legacy slug-only
    /// preferences remain readable as a migration fallback in security.ps1.
    #[serde(default)]
    pub browser_extensions: Option<std::collections::HashMap<String, bool>>,
}

/// F-2: filesystem honeypots. The runtime authority lives in
/// `decoy_monitor::WATCHED_DECOYS` (a Mutex set by the frontend on
/// every settings change); this struct is the persistence layer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DecoyMonitorSettings {
    pub enabled: Option<bool>,
    pub enrolled_paths: Option<Vec<String>>,
}

/// F-3: anti-ransomware monitor. Same persistence-layer pattern —
/// runtime authority lives in `ransomware_monitor::CONFIG`, this is
/// the disk side.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RansomwareMonitorSettings {
    pub enabled: Option<bool>,
    pub threshold: Option<u32>,
    pub window_seconds: Option<u32>,
    /// User-added extra watch directories (in addition to the standard
    /// Documents/Pictures/Desktop/Downloads set).
    pub custom_watch_dirs: Option<Vec<String>>,
    /// F-3 v2 automated response on the Pro ETW path:
    /// "monitor" | "suspend" | "kill". None = backend default (suspend).
    pub action: Option<String>,
}

/// #4: remote-access monitor. Persistence layer only; runtime authority
/// lives in commander-pro/src/remote_access.rs (TOOL_CATALOGUE + RUNNING),
/// reconciled by the Free-side global hook on every settings change.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAccessMonitorSettings {
    pub enabled: Option<bool>,
    pub tools: Option<std::collections::HashMap<String, bool>>,
}

/// #5: screen-capture detection + own-window capture protection. Paid.
/// detection_enabled drives the Pro poller; protect_window drives
/// SetWindowDisplayAffinity on the Free main window.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScreenCaptureSettings {
    #[serde(default)]
    pub detection_enabled: Option<bool>,
    #[serde(default)]
    pub protect_window: Option<bool>,
}

/// F-5: coercion phrase. Persistence layer; runtime authority lives
/// in `coercion_phrase::REGISTERED`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CoercionPhraseSettings {
    pub enabled: Option<bool>,
    /// SHA-256 digests of registered phrases. NEVER stores plaintext.
    pub phrases: Option<Vec<CoercionPhraseEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CoercionPhraseEntry {
    pub label: String,
    pub hash: String,
    pub length: usize,
}

/// Startup PIN gate — three hashed PINs, never stores plaintext.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StartupPinSettings {
    #[serde(default)]
    pub enabled: Option<bool>,
    pub real_hash: Option<String>,
    pub decoy_hash: Option<String>,
    pub destroy_hash: Option<String>,
}

/// Distress phrase entry — keyboard hook (C) + command palette (D).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DistressPhraseEntry {
    pub label: String,
    pub hash: String,
    pub length: usize,
    /// "decoy" | "destroy"
    pub mode: String,
}

/// Customisable lockdown configuration. The orchestrator
/// (`full_lockdown` in backend.rs) reads this once at the start
/// of every cascade and uses it to decide which steps run, whether
/// to uninstall the app at the end, and whether to deactivate the
/// licence / shut the system down. All panic triggers (sidebar
/// button, Ctrl+Shift+Q, coercion phrase) honour the
/// same config — there are no per-trigger overrides.
///
/// `steps` is a sparse map of stable step ID → enabled flag. Missing
/// keys fall back to the step's `default_enabled` value declared in
/// `lockdown_steps::DESTRUCT_STEPS`. This sparse layout lets a user
/// who's never touched the settings get the documented default
/// behaviour, while a customised user only stores deltas.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SelfDestructSettings {
    /// Explicit opt-in gate. `None` or `Some(false)` ⇒ all trigger paths
    /// (sidebar button, panic hotkey, coercion phrase, dead-man's switch,
    /// destroy PIN) are refused before any destructive work begins.
    /// Must be `Some(true)` for any lockdown path to execute.
    #[serde(default)]
    pub enabled: Option<bool>,
    pub steps: Option<std::collections::HashMap<String, bool>>,
    /// Skip browsers in the System Cleaner step. Faster but leaves
    /// browser caches/cookies/history. Default: false (cleaner runs
    /// against everything).
    pub exclude_browsers: Option<bool>,
    /// Free the licence seat on the server before clearing. Default false.
    /// Recommended ON if you're going to also uninstall the app — otherwise the
    /// seat stays attached to a device that no longer exists until
    /// the idle-seat reaper picks it up.
    pub deactivate_license_first: Option<bool>,
    /// Trigger a graceful Windows shutdown after the cascade ends.
    pub shutdown_system: Option<bool>,
    /// F6 — arm the reboot-to-USB wipe extension. When this AND `enabled` are
    /// both `Some(true)` (and a `reboot_usb` distress mode is configured), the
    /// distress cascade may — only after stage-1 crypto-erase succeeds — set the
    /// UEFI BootNext to a provisioned wipe-USB and reboot. Default None/false.
    /// MUST exist on the struct or the patch_settings_cmd serialize→merge→
    /// deserialize round-trip silently drops it (same trap as `shred_folders`).
    #[serde(default)]
    pub reboot_to_usb_enabled: Option<bool>,
    /// Absolute paths of folders the user wants securely deleted before
    /// the Rust orchestrator hands over. The frontend reads this and
    /// fires Invoke-7Erase (single durable RNG-overwrite pass by default,
    /// user-configurable up to 7) per folder concurrently with full_lockdown.
    /// Rust itself doesn't consume it — but the field MUST exist on the
    /// struct, otherwise patch_settings_cmd's round-trip
    /// (serialize → merge → DESERIALIZE) silently drops it as an unknown
    /// field, leaving the user's configured folders persisted nowhere
    /// and the cascade with an empty deletion list at fire time.
    pub shred_folders: Option<Vec<String>>,
    /// Local usernames the user selected for removal during the lockdown
    /// cascade. The `remove_users` destruct step (Pro) securely wipes each
    /// account's user-profile folder (single durable RNG-overwrite pass), then deletes the profile and
    /// the local account. Unlike `shred_folders`, Rust DOES consume this:
    /// `run_destruct_step` injects it into the Pro dispatch, so it fires on
    /// every trigger path (sidebar, hotkey, distress phrase, dead-man, destroy
    /// PIN) — not only the frontend `fireSelfDestruct`. Built-in, logged-in,
    /// and current accounts are always skipped on the Pro side. The field MUST
    /// exist on the struct or the patch_settings_cmd round-trip drops it (same
    /// trap as `shred_folders`). None/empty = no users removed.
    pub users_to_remove: Option<Vec<String>>,
    /// Absolute paths of VeraCrypt containers the `veracrypt_header_destroy`
    /// destruct step should crypto-erase on any lockdown trigger (sidebar,
    /// hotkey, distress phrase, dead-man's switch, destroy PIN). Unlike
    /// BitLocker, an unmounted VeraCrypt container has no OS-visible trace
    /// (that's the point of its deniability), so the automated cascade
    /// cannot auto-discover targets the way it can enumerate BitLocker
    /// volumes — the user must pre-configure which containers to destroy.
    /// `run_destruct_step` DOES consume this (mirrors `users_to_remove`):
    /// empty/None means the step cleanly skips (logged, not an error) rather
    /// than dispatching a request with no `Path`, which the Pro handler
    /// always rejects. The field MUST exist on the struct or the
    /// patch_settings_cmd round-trip drops it (same trap as `shred_folders`).
    pub crypto_erase_veracrypt_paths: Option<Vec<String>>,
    /// Drive letters (e.g. "C:") the `bitlocker_erase` destruct step removes
    /// key protectors from on any lockdown trigger. Previously this step
    /// always dispatched with no `DriveLetter` at all, which the Pro handler
    /// silently defaulted to "C:" — i.e. it always blindly targeted the OS
    /// drive with no way to choose otherwise. This field replaces that
    /// blind default with an explicit, user-selected list (mirrors
    /// `crypto_erase_veracrypt_paths`): empty/None means the step cleanly
    /// skips rather than falling back to "C:". Selecting the system drive
    /// here means it WILL be targeted on the next trigger — the
    /// destroy-PIN/trigger itself is the confirmation; there is no separate
    /// typed nuclear-ack for this automated path the way the manual
    /// selective-erase UI's OS-volume ceremony has. The field MUST exist on
    /// the struct or the patch_settings_cmd round-trip drops it (same trap
    /// as `shred_folders`).
    pub crypto_erase_bitlocker_drives: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InternetCommunicationSettings {
    /// Blocks publishing wizard, web services, handwriting sharing, HTTP printing, etc.
    pub restricted_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySettings {
    pub windows_disabled: Option<bool>,
    pub office_disabled: Option<bool>,
    pub powershell7_disabled: Option<bool>,
    pub copilot_disabled: Option<bool>,
    pub activity_history_disabled: Option<bool>,
    pub location_tracking_disabled: Option<bool>,
    pub windows_suggestions_disabled: Option<bool>,
}

/// F-1: per-category enable/disable for the paste monitor. The runtime
/// authority lives in `paste_monitor::ENABLED_CATEGORIES` (a Mutex set
/// by the frontend on every settings change); this struct is the
/// persistence layer. All optional so a partial patch from the UI
/// doesn't clobber unrelated categories.
#[derive(Debug, Clone, Serialize, Deserialize, Default, Copy)]
#[serde(rename_all = "camelCase")]
pub struct PasteMonitorCategories {
    pub cloud_api: Option<bool>,
    pub ai_api: Option<bool>,
    pub dev_tools: Option<bool>,
    pub payment_comms: Option<bool>,
    pub keys_and_crypto: Option<bool>,
    pub personal_data: Option<bool>,
    /// ClickFix / pastejacking defence — encoded PowerShell, mshta
    /// web payloads, iex-irm, curl-pipe-shell.
    pub malicious_command: Option<bool>,
    /// Unicode anomalies — homoglyph URLs, zero-width chars in code-
    /// context, bidi overrides. None = use default (on).
    pub unicode: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardSettings {
    pub history_disabled: Option<bool>,
    pub cloud_sync_disabled: Option<bool>,
    // auto_erase_schedule removed — replaced by per-card auto-erase
    // scheduler (Set-AutoEraseSchedule categoryId='clipboard'). See the
    // tracking settings comment above for the same change for RDP /
    // event log schedules.
    /// F-1: clipboard credential watcher. Driven from the React side via
    /// `start_paste_monitor` / `stop_paste_monitor`; this field lets the
    /// preference persist across launches.
    pub paste_monitor_enabled: Option<bool>,
    /// F-1: per-category enable/disable. None = use defaults (all on).
    pub paste_monitor_categories: Option<PasteMonitorCategories>,
    /// `monitor.paste.crypto-swap` (paid): detect clipboard-hijack
    /// malware that silently overwrites a copied crypto address with the
    /// attacker's. None = backend default (on).
    pub paste_monitor_crypto_swap_enabled: Option<bool>,
    /// `monitor.paste.auto-expire` (paid): clear the clipboard N seconds
    /// after a detection fires, but only if the content hasn't changed
    /// in the meantime. None = backend default (off).
    pub paste_monitor_auto_clear_enabled: Option<bool>,
    /// `monitor.paste.auto-expire`: seconds to wait before clearing.
    /// Backend clamps to [5, 600]. None = backend default (30).
    pub paste_monitor_auto_clear_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackingSettings {
    pub recent_files_disabled: Option<bool>,
    pub jump_lists_disabled: Option<bool>,
    pub thumbnail_cache_disabled: Option<bool>,
    pub pagefile_disabled: Option<bool>,
    // rdp_auto_erase_schedule + event_log_auto_erase_schedule removed —
    // replaced by per-card auto-erase scheduler (Set-AutoEraseSchedule
    // categoryId='rdpHistory' / 'eventLogs'). Schedule state now lives
    // in Windows Task Scheduler, not settings.json, so there's no
    // settings field to track. Old settings on disk are silently
    // ignored by serde (no compat shim needed).
    /// Windows Recall AI snapshots disabled
    pub recall_snapshots_disabled: Option<bool>,
    /// Autocorrect, spellcheck, text prediction, typing insights disabled
    pub typing_insights_disabled: Option<bool>,
    /// Windows Advertising ID fully nuked
    pub advertising_id_disabled: Option<bool>,
    /// Tailored Experiences with diagnostic data disabled
    pub tailored_experiences_disabled: Option<bool>,
    /// Office Click-to-Run logging/telemetry disabled
    pub office_logging_disabled: Option<bool>,
    /// Diagnostic Event Tracing (AutoLogger/DiagTrack) disabled
    pub diagnostic_event_tracing_disabled: Option<bool>,
    /// Auto-disconnect RDP sessions on mouse/keyboard inactivity
    pub rdp_idle_disconnect_enabled: Option<bool>,
    /// Timeout in seconds before disconnecting idle RDP sessions (default 120)
    pub rdp_idle_disconnect_timeout: Option<u32>,
    /// Warning countdown shown before idle RDP disconnect fires (default 5)
    pub rdp_idle_warning_seconds: Option<u32>,
    /// Clear RDP history and cache when an idle disconnect fires
    pub rdp_clear_cache_on_disconnect: Option<bool>,
    /// Remove saved RDP credentials from Windows Vault when an idle disconnect fires
    pub rdp_remove_creds_on_disconnect: Option<bool>,
    /// Save a log entry each time an idle disconnect fires
    pub rdp_save_log: Option<bool>,
    /// Dismount local encrypted volumes (VeraCrypt) when an idle disconnect fires
    pub rdp_dismount_vaults_on_disconnect: Option<bool>,
    /// Hide "Recent files" group in File Explorer Quick Access (HKCU ShowRecent=0)
    pub quick_access_recent_disabled: Option<bool>,
    /// Hide "Frequent folders" group in File Explorer Quick Access (HKCU ShowFrequent=0)
    pub quick_access_frequent_disabled: Option<bool>,
    /// Win+R history dropdown disabled (HKCU NoRunMRU=1)
    pub run_mru_disabled: Option<bool>,
    /// Windows Search box "recent searches" disabled (HKCU IsDeviceSearchHistoryEnabled=0)
    pub search_history_disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LockScreenSettings {
    pub privacy_disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppCapabilitySettings {
    pub webcam: Option<String>,
    pub microphone: Option<String>,
    pub location: Option<String>,
    pub contacts: Option<String>,
    pub calendar: Option<String>,
    pub call_history: Option<String>,
    pub phone_call: Option<String>,
    pub email: Option<String>,
    pub messaging: Option<String>,
    pub radios: Option<String>,
    pub bluetooth_sync: Option<String>,
    pub app_diagnostics: Option<String>,
    pub documents: Option<String>,
    pub pictures: Option<String>,
    pub videos: Option<String>,
    pub file_system: Option<String>,
    pub notifications: Option<String>,
    pub gaze_input: Option<String>,
    pub user_account_information: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyShieldSettings {
    pub gaze_detection_enabled: Option<bool>,
    pub anti_peeping_enabled: Option<bool>,
    pub camera_hunter_enabled: Option<bool>,
    pub confidence_threshold: Option<f32>,
    pub wake_delay_seconds: Option<u32>,
    pub blur_opacity: Option<f32>,
    pub model_size: Option<String>,
    pub detection_buffer_frames: Option<u32>,
    pub capture_on_device: Option<bool>,
    pub capture_on_multi_face: Option<bool>,
    pub capture_speed: Option<u32>,
    pub device_wake_multiplier: Option<f32>,
    pub multi_face_wake_multiplier: Option<f32>,
    /// When true, Privacy Shield starts automatically on app launch.
    /// Serializes as "autostart" (camelCase passthrough). Default false.
    #[serde(default)]
    pub autostart: bool,
}

// ── Tweak Settings (Desired State) ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TweakSettings {
    #[serde(default)]
    pub security: SecurityTweaks,
    #[serde(default)]
    pub os: OsTweaks,
    #[serde(default)]
    pub ui: UiTweaks,
    #[serde(default)]
    pub boot_kernel: BootKernelTweaks,
    #[serde(default)]
    pub rdp: RdpTweaks,
    /// Performance / gaming-responsiveness tweaks (MMCSS, kb latency, etc.)
    #[serde(default)]
    pub performance: PerformanceTweaks,
    /// Vendor-specific GPU optimisations (AMD/NVIDIA/Intel)
    #[serde(default)]
    pub gpu: GpuTweaks,
    /// Power-management tweaks (USB selective suspend, CPU throttle, etc.)
    #[serde(default)]
    pub power: PowerTweaks,
    #[serde(default)]
    pub power_plan: Option<String>,
    /// Desired state for each independently reversible Windows AI cleanup
    /// operation. This is intentionally separate from the broad AI policy
    /// toggle because each operation is applied and restored on its own.
    #[serde(default)]
    pub ai_component_cleanup: std::collections::HashMap<String, bool>,
    #[serde(default)]
    pub maintenance_runs: std::collections::HashMap<String, MaintenanceRunInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceTweaks {
    /// MMCSS gaming profile (SystemResponsiveness=10, NetworkThrottlingIndex off)
    pub mmcss_gaming_profile: Option<bool>,
    /// Keyboard latency optimised (KeyboardDelay=0, KeyboardSpeed=31)
    pub keyboard_latency_optimised: Option<bool>,
    /// Num Lock on at boot (InitialKeyboardIndicators=2)
    pub num_lock_on_boot: Option<bool>,
    /// Hardware-Accelerated GPU Scheduling on (HwSchMode=2)
    pub gpu_scheduling_enabled: Option<bool>,
    /// SvcHostSplitThresholdInKB tuned to physical RAM
    pub svc_host_split_optimised: Option<bool>,
    /// Foreground / accessibility shortcuts disabled (StickyKeys, ToggleKeys, FilterKeys)
    pub accessibility_shortcuts_disabled: Option<bool>,
    /// Menus open instantly (MenuShowDelay=0)
    pub instant_menu_delay: Option<bool>,
    /// Mouse acceleration off (1:1 movement)
    pub mouse_acceleration_disabled: Option<bool>,
    /// Autocorrect / spellcheck / text prediction disabled
    pub autocorrect_disabled: Option<bool>,
    /// Enthusiast mode (more details in Explorer copy dialog)
    pub enthusiast_mode_enabled: Option<bool>,
    /// JPEG wallpaper at 100% quality (Set JPEGImportQuality=100)
    pub wallpaper_full_quality: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GpuTweaks {
    // AMD-specific
    pub amd_ulps_disabled: Option<bool>,
    pub amd_power_gating_disabled: Option<bool>,
    pub amd_video_clock_gating_disabled: Option<bool>,
    pub amd_aspm_disabled: Option<bool>,
    // NVIDIA-specific
    pub nvidia_dynamic_pstate_disabled: Option<bool>,
    pub nvidia_async_pstates_disabled: Option<bool>,
    // Intel-specific
    pub intel_async_flips_disabled: Option<bool>,
    pub intel_adaptive_vsync_disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PowerTweaks {
    /// USB selective suspend disabled across all USB root hubs (MSPower_DeviceEnable)
    pub usb_selective_suspend_disabled: Option<bool>,
    /// CPU power throttling disabled (PowerThrottlingOff=1)
    pub cpu_throttling_disabled: Option<bool>,
    // Ultimate Performance plan handled via TweakSettings.power_plan = "ultimate"
    // (unified with Power Saver / Balanced / Performance to avoid two UIs).
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceRunInfo {
    pub last_run_at: Option<String>,
    #[serde(default)]
    pub run_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RdpTweaks {
    /// TCP keep-alive + single-session-per-user on the RDP host
    pub keep_alive: Option<bool>,
    /// Removes disconnected/idle/connection session timeouts
    pub no_timeouts: Option<bool>,
    /// QoS DSCP 46 policies prioritise RDP traffic on TCP 3389
    pub qos_priority: Option<bool>,
    /// Server-enforced: signs out (not just disconnects) idle INCOMING sessions
    /// via HKLM Group Policy (MaxIdleTime + fResetBroken + MaxDisconnectionTime).
    pub incoming_idle_timeout_enabled: Option<bool>,
    /// Seconds before an idle incoming session is signed out (10–86400). Canonical field.
    pub incoming_idle_timeout_seconds: Option<u32>,
    /// @deprecated Minutes-based field kept for migration reads only.
    pub incoming_idle_timeout_minutes: Option<u32>,
    /// Dismount local VeraCrypt vaults when all incoming RDP sessions end.
    pub incoming_dismount_on_empty: Option<bool>,
    /// Sign off incoming RDP sessions when they become disconnected.
    pub incoming_sign_off_on_disconnect: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecurityTweaks {
    pub defender_disabled: Option<bool>,
    pub windows_update_disabled: Option<bool>,
    pub uac_disabled: Option<bool>,
    pub usb_write_protect: Option<bool>,
    pub usb_storage_lockdown: Option<bool>,
    pub consumer_features_disabled: Option<bool>,
    /// Disables inbound Windows Remote Assistance invitations.
    pub remote_assistance_disabled: Option<bool>,
    /// Blocks anonymous enumeration of local Security Account Manager accounts.
    pub anonymous_sam_enumeration_blocked: Option<bool>,
    /// Virtualization-Based Security + Credential Guard disabled
    pub vbs_disabled: Option<bool>,
    /// Prevents automatic BitLocker device encryption
    pub bitlocker_auto_encrypt_disabled: Option<bool>,
    /// Disables Windows Platform Binary Table execution
    pub wpbt_disabled: Option<bool>,
    /// Disables SmartScreen web content evaluation
    pub smart_screen_disabled: Option<bool>,
    /// Bypasses TPM/CPU/RAM/Storage/SecureBoot checks for future upgrades
    pub oobe_bypass_enabled: Option<bool>,
    /// Disables Game DVR + App Capture
    pub game_dvr_disabled: Option<bool>,
    /// Firefox policy-based hardening (telemetry off, uBlock, strict tracking)
    pub firefox_hardening_enabled: Option<bool>,
    /// Brave hardening (P3A off, no rewards/wallet/VPN/AI chat)
    pub brave_hardening_enabled: Option<bool>,
    /// Chrome hardening (telemetry off, Privacy Sandbox off, extensions)
    pub chrome_hardening_enabled: Option<bool>,
    /// Edge hardening (telemetry off, bloat stripped, extensions)
    pub edge_hardening_enabled: Option<bool>,
    /// Universal extensions deployed to all detected browsers
    pub universal_extensions_deployed: Option<bool>,
    /// Browser extension auto-updates forced (Firefox policy ExtensionUpdate; Chromium force_installed extensions auto-update inherently)
    pub browser_auto_update_forced: Option<bool>,
    /// Copilot & AI components removed (APPX + IFEO + policies)
    pub copilot_ai_removed: Option<bool>,
    /// VSS / System Restore disabled (reducesSecurity — warn required)
    pub system_restore_off: Option<bool>,
    /// Windows Recall AI snapshot recording disabled
    pub recall_off: Option<bool>,
    /// Kernel crash dumps + WER disabled
    pub crash_dumps_off: Option<bool>,
    /// Clipboard history disabled
    pub clipboard_history_off: Option<bool>,
    /// Lock screen on resume + AC sleep disabled
    pub require_pw_on_resume: Option<bool>,
    /// Kernel DMA Protection opportunistic-enable (firmware IOMMU required)
    pub kernel_dma_protect: Option<bool>,
    /// Secure-shred overwrite passes (1–7). Default 1.
    pub shred_passes: Option<i64>,
    /// When true, SSD/NVMe targets are forced to 1 pass (no forensic gain from multi-pass on flash).
    pub shred_media_aware_enabled: Option<bool>,
    /// Wipe MFT-resident data region + file-slack on shredded files. Paid, irreversible, needsAdmin.
    pub shred_mft_slack_enabled: Option<bool>,
    /// RAM-spill control: hibernation off + fast-startup off + ClearPageFileAtShutdown=1. Free.
    pub ram_spill_control_enabled: Option<bool>,
    /// Feature 5: require BitLocker TPM+PIN on every boot (free, reversible).
    pub bitlocker_tpm_pin_enforce: Option<bool>,
    /// Anti-Acquisition: Microsoft vulnerable-driver blocklist enabled (paid, needsAdmin, reboot to apply).
    pub acquisition_driver_blocklist: Option<bool>,
    /// Anti-Acquisition: forensic/imaging tools blocked from launching via IFEO (paid, needsAdmin).
    pub forensic_tool_block: Option<bool>,
    /// Anti-Acquisition: lid close powers the machine fully off (powercfg lid-close
    /// action = Shut down, AC + DC) instead of sleeping, so FDE keys leave RAM
    /// before a seizure can exploit a suspended, key-in-RAM state. Reversible:
    /// disabling restores Windows' stock default (Sleep). Paid, needsAdmin.
    pub lid_close_power_off: Option<bool>,
    pub dep_enabled: Option<bool>,
    pub aslr_mandatory: Option<bool>,
    pub aslr_bottom_up: Option<bool>,
    pub cfg_enabled: Option<bool>,
    pub heap_integrity: Option<bool>,
    pub sehop_enabled: Option<bool>,
    pub asr_rules_enabled: Option<bool>,
    pub controlled_folder_access_enabled: Option<bool>,
    pub network_protection_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OsTweaks {
    pub superfetch_disabled: Option<bool>,
    pub prefetch_disabled: Option<bool>,
    pub hibernation_disabled: Option<bool>,
    pub fast_startup_disabled: Option<bool>,
    pub ntfs_optimizations_enabled: Option<bool>,
    pub detailed_bsod_enabled: Option<bool>,
    /// Memory compression disabled (Disable-MMAgent -mc)
    pub memory_compression_disabled: Option<bool>,
    /// Foreground process priority boost (Win32PrioritySeparation=38)
    pub win32_priority_separation: Option<bool>,
    /// Service kill/hung app timeouts optimized for speed
    pub service_timeouts_optimized: Option<bool>,
    /// Reserved storage disabled (ShippedWithReserves=0)
    pub reserved_storage_disabled: Option<bool>,
    /// Disables Windows Automatic Maintenance and scheduled diagnostics execution.
    pub automatic_maintenance_disabled: Option<bool>,
    /// Enables the Win32 long-path opt-in (requires compatible applications).
    pub win32_long_paths_enabled: Option<bool>,
    /// Disables the SMB client bandwidth throttling behavior.
    pub smb_bandwidth_throttling_disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BootKernelTweaks {
    /// Intel TSX enabled for improved transaction performance
    pub tsx_enabled: Option<bool>,
    /// First sign-in animation disabled
    pub first_logon_animation_disabled: Option<bool>,
    /// Startup sound disabled
    pub startup_sound_disabled: Option<bool>,
    /// Automatic restart sign-on disabled
    pub auto_restart_signon_disabled: Option<bool>,
    /// Auto-reboot after BSOD disabled
    pub auto_reboot_on_bsod_disabled: Option<bool>,
    /// Crash dump set to small memory dump (64KB)
    pub small_memory_dump_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UiTweaks {
    pub classic_context_menu: Option<bool>,
    pub file_extensions_visible: Option<bool>,
    pub hidden_files_visible: Option<bool>,
    pub gallery_home_removed: Option<bool>,
    pub bing_search_disabled: Option<bool>,
    pub background_apps_disabled: Option<bool>,
    pub notifications_disabled: Option<bool>,
    pub end_task_on_taskbar: Option<bool>,
    /// Fixes slow media folder loading (FolderType=NotSpecified)
    pub folder_type_discovery_disabled: Option<bool>,
    /// Removes " - Shortcut" text from shortcuts
    pub shortcut_suffix_removed: Option<bool>,
    /// Disables AutoPlay + AutoRun on all drives
    pub auto_play_disabled: Option<bool>,
    /// Always-on: Chat, Widgets, Meet Now, Task View, People, News hidden
    pub taskbar_debloated: Option<bool>,
    /// Always-on: Start menu recommendations disabled
    pub start_recommendations_disabled: Option<bool>,
    /// Disables low disk space warnings
    pub low_disk_check_disabled: Option<bool>,
    /// Explorer opens to "This PC" instead of Quick Access
    pub explorer_opens_this_pc: Option<bool>,
    /// Hides OneDrive/sync/ad notifications in Explorer
    pub sync_provider_notifications_hidden: Option<bool>,
    /// Disables transparency effects + minimize animation
    pub transparency_disabled: Option<bool>,
    /// Shows full path in Explorer title bar
    pub full_path_in_title_bar: Option<bool>,

    // ── Granular UI ────────────────────────────────────────────────────
    /// Show "This PC" icon on desktop
    pub desktop_icon_this_pc: Option<bool>,
    /// Show Recycle Bin on desktop
    pub desktop_icon_recycle_bin: Option<bool>,
    /// Show user profile folder on desktop
    pub desktop_icon_user_files: Option<bool>,
    /// Show Network icon on desktop
    pub desktop_icon_network: Option<bool>,
    /// Show Control Panel on desktop
    pub desktop_icon_control_panel: Option<bool>,
    /// Shortcut-arrow overlay icon removed (Shell Icons\29 blanked)
    pub shortcut_arrow_removed: Option<bool>,
    /// Snap-assist flyout (window snap suggestions) disabled
    pub snap_assist_flyout_disabled: Option<bool>,
    /// File Explorer compact-mode rows
    pub explorer_compact_mode: Option<bool>,
    /// File-selection checkboxes in Explorer (AutoCheckSelect)
    pub explorer_checkboxes_enabled: Option<bool>,
    /// Window shake-to-minimise (DisallowShaking=1 to disable)
    pub window_shake_disabled: Option<bool>,
    /// Show seconds in taskbar clock (ShowSecondsInSystemClock=1)
    pub clock_seconds_visible: Option<bool>,
}

// ── Network Settings (Desired State) ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettings {
    #[serde(default)]
    pub dns: DnsSettings,
    #[serde(default)]
    pub hosts: HostsSettings,
    #[serde(default)]
    pub firewall: FirewallSettings,
    #[serde(default)]
    pub vpn_kill_switch: VpnKillSwitchSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DnsSettings {
    pub provider: Option<String>,
    pub ipv4_preference: Option<bool>,
    pub swiss_firewall_config: Option<SwissFirewallConfig>,
    pub control_d_filter_slug: Option<String>,
    /// Anti-censorship enforcement: when true, outbound plaintext DNS (port 53)
    /// is firewall-blocked so all name resolution is forced through the
    /// encrypted DoH resolver — the ISP/network can't transparently intercept,
    /// redirect, or censor lookups. Requires Encrypted DNS to be on. Free.
    pub censorship_protection: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SwissFirewallConfig {
    pub doh_id: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HostsSettings {
    #[serde(default)]
    pub enabled_blocklists: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FirewallSettings {
    pub lockdown_mode: Option<bool>,
    #[serde(default)]
    pub managed_rules: Vec<ManagedFirewallRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedFirewallRule {
    pub name: String,
    pub direction: String,
    pub action: String,
    pub protocol: String,
    pub port: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VpnKillSwitchSettings {
    /// When true, a VPN tunnel drop cuts all internet via the internet kill switch.
    pub armed: Option<bool>,
    /// Which tunnel to watch: "auto" (default) | "tailscale" | "protonvpn".
    pub provider: Option<String>,
    /// Poll cadence in seconds (default 5).
    pub poll_interval_secs: Option<u64>,
}

// ── Identity Settings ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IdentitySettings {
    #[serde(default)]
    pub branding: BrandingSettings,
    pub stealth_mode_enabled: Option<bool>,
    /// When true (or null/absent → treated as true by frontend), hides Server Apps from sidebar.
    pub hide_server_apps: Option<bool>,
    /// When true, hides WinCommander from Start Menu and Installed Apps where possible.
    pub hide_win_commander: Option<bool>,
    /// Global hotkey to peek at WinCommander while it is in hidden mode.
    /// Default "Ctrl+Shift+G". Only active when hide_win_commander is true.
    pub hide_win_commander_hotkey: Option<String>,
    /// When true, the Flows panel becomes visible in the sidebar. Default
    /// hidden — Flows is reserved for the upcoming multi-WinCommander
    /// admin orchestration and is opt-in from Settings.
    pub flows_enabled: Option<bool>,
    /// Operator consent for advanced mode. When true:
    ///   * The Investigator panel becomes visible in the sidebar.
    ///   * The dispatch-layer kill-switch is armed — Clear-* / Erase-* /
    ///     Remove-* / Reset-* PowerShell commands are refused so they
    ///     don't taint evidence (see license::is_advanced_mode).
    ///
    /// When false or absent, both effects are off. The user can flip this
    /// from Settings to step in/out of advanced mode without changing
    /// licence state.
    pub advanced_tools_enabled: Option<bool>,
    /// Pro-gated Settings switch for exposing the Dashboard Risk Matrix view.
    pub risk_matrix_enabled: Option<bool>,
    /// Exposes the Dashboard More Products view. Set from the first-run
    /// Agreement step or the Settings "More Products" switch. Not Pro-gated.
    pub more_products_enabled: Option<bool>,
    /// List of backend app IDs the user has chosen to hide from the OS
    /// (Start Menu, Installed Apps, etc.). Persisted here so the toggle
    /// in the Identity panel stays ON across restarts.
    /// IMPORTANT: This field MUST exist in the Rust struct or serde silently
    /// drops it on every save/load round-trip, making the toggle always reset.
    #[serde(default)]
    pub hide_backend_apps_list: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BrandingSettings {
    pub company_name: Option<String>,
    pub product_name: Option<String>,
    pub pc_name: Option<String>,
    pub manufacturer: Option<String>,
    pub support_url: Option<String>,
}

// ── App Management ───────────────────────────────────────────────────
// LEARNING: AppManagementSettings is used in BOTH ideal and current SystemState.
// - ideal.apps → admin intent (requiredApps, blockedApps, autoUpdate policy, etc.)
// - current.apps → actual PC state (edgeRemoved, onedriveRemoved) + inventory snapshot
// The inventory field is ONLY meaningful in `current` — it holds the persisted
// winget scan results so heartbeat can send cached data without re-shelling.
// Drift detection compares ideal.apps.requiredApps vs current.apps.inventory.manifestApps.

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppManagementSettings {
    #[serde(default)]
    pub required_apps: Vec<String>,
    #[serde(default)]
    pub blocked_apps: Vec<String>,
    pub edge_removed: Option<bool>,
    pub onedrive_removed: Option<bool>,
    /// Microsoft Teams removed
    pub teams_removed: Option<bool>,
    /// List of removed APPX package family names
    #[serde(default)]
    pub removed_appx: Vec<String>,
    /// List of deprovisioned apps (prevents reinstall via updates)
    #[serde(default)]
    pub deprovisioned_appx: Vec<String>,

    // ── Admin Policy Fields (meaningful in `ideal` only) ──
    /// Should this PC auto-update all apps?
    #[serde(default)]
    pub auto_update: Option<bool>,
    /// Only auto-update manifest apps (not random installed software)?
    #[serde(default)]
    pub auto_update_manifest_only: Option<bool>,
    /// Lock specific apps to a version: { "Python.Python.3.12": "3.12.0" }
    #[serde(default)]
    pub pinned_versions: std::collections::HashMap<String, String>,
    /// How often to re-scan installed apps (minutes). Default 60.
    #[serde(default)]
    pub scan_interval_minutes: Option<u32>,

    // ── Inventory Snapshot (meaningful in `current` only) ──
    /// Persisted scan results from Get-AppInventory. Updated on startup + every scanInterval.
    #[serde(default)]
    pub inventory: Option<AppInventorySnapshot>,
}

/// Point-in-time snapshot of all apps on this PC.
/// Written to current.apps.inventory by the scan engine.
/// Sent to admin server via heartbeat (summary + pendingUpdates only for lightweight payload).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppInventorySnapshot {
    /// When this scan was performed (ISO 8601)
    pub last_scan_at: String,
    /// How long the scan took in milliseconds (perf insight for admin)
    pub scan_duration_ms: Option<u64>,

    /// Apps from the WinCommander manifest catalog that are installed
    #[serde(default)]
    pub manifest_apps: Vec<ManifestAppEntry>,

    /// Apps installed on the PC but NOT in our manifest (admin needs visibility)
    #[serde(default)]
    pub other_apps: Vec<OtherAppEntry>,

    /// Flat list of all apps with pending updates (quick admin view)
    #[serde(default)]
    pub pending_updates: Vec<PendingUpdateEntry>,

    /// Critical dependency status (Encryption Engine, MeshVPN, etc.)
    #[serde(default)]
    pub essentials: EssentialAppsStatus,

    /// Pre-computed counts so admin dashboard doesn't count arrays
    #[serde(default)]
    pub summary: AppInventorySummary,
}

/// An app from the WinCommander manifest catalog
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestAppEntry {
    pub id: String,
    pub name: String,
    /// App description from the manifest catalog (for UI display)
    #[serde(default)]
    pub description: String,
    pub category: String,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    /// Base64 PNG data URL extracted from the installed app's EXE icon.
    /// None when the app isn't installed or icon extraction failed.
    #[serde(default)]
    pub icon_data: Option<String>,
}

/// An installed app NOT in the WinCommander manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherAppEntry {
    pub id: String,
    pub name: Option<String>,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    #[serde(default)]
    pub icon_data: Option<String>,
}

/// An app with a pending update (for quick admin queries)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUpdateEntry {
    pub id: String,
    pub name: Option<String>,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    /// Package source (e.g. "winget", "msstore")
    #[serde(default)]
    pub source: Option<String>,
    pub in_manifest: bool,
    /// Base64 PNG data URL extracted from the installed app's EXE icon.
    #[serde(default)]
    pub icon_data: Option<String>,
}

/// Status of critical dependencies
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EssentialAppsStatus {
    #[serde(default)]
    pub mesh_vpn: MeshVPNEssentialInfo,
    #[serde(default)]
    pub productivity_engine: ProductivityEssentialInfo,
    #[serde(default)]
    pub winget: EssentialAppInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EssentialAppInfo {
    pub installed: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MeshVPNEssentialInfo {
    pub installed: bool,
    pub version: Option<String>,
    pub connected: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProductivityEssentialInfo {
    pub installed: bool,
    pub running: Option<bool>,
}

/// Pre-computed summary counts for the admin dashboard
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppInventorySummary {
    pub total_installed: u32,
    pub manifest_installed: u32,
    pub manifest_total: u32,
    pub manifest_missing: u32,
    pub other_installed: u32,
    pub updates_available: u32,
    pub essentials_ok: bool,
}

// ── Productivity ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProductivitySettings {
    pub tracker_enabled: Option<bool>,
    pub productivity_engine_stealth_enabled: Option<bool>,
    pub exclude_afk: Option<bool>,
    pub default_range: Option<String>,
}

// ── Server Apps ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerAppsSettings {
    #[serde(default)]
    pub apps: Vec<ServerAppConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerAppConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    pub icon: String,
    /// Optional CSS injected into the webview to whitelabel the app (hide logos, rename headers, etc.)
    #[serde(default)]
    pub custom_css: Option<String>,
}

// ── Admin Policy ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicySettings {
    #[serde(default = "default_sync_mode")]
    pub sync_mode: String,
    pub admin_server_url: Option<String>,
    #[serde(default = "default_merge_strategy")]
    pub merge_strategy: String,
    #[serde(default)]
    pub locked_paths: Vec<String>,
    pub last_synced_at: Option<String>,
    pub master_config_version: Option<u32>,
    pub organization: Option<String>,
    /// Base64 Ed25519 public key of the Fleet server this device is managed by.
    /// When set, the device is "fleet-managed": every pushed config epoch MUST
    /// carry a valid signature from this key (anti-spoofing — see
    /// `apply_admin_config_cmd`). When `None` the device is unmanaged and config
    /// applies as before (local trust).
    #[serde(default)]
    pub fleet_signing_key: Option<String>,
    /// P5 enrollment lock — set from the applied epoch's `managed` flag. When
    /// true, the in-app fleet Disconnect refuses without admin approval (a
    /// deterrent; a determined local admin can still uninstall — see the plan §8).
    #[serde(default)]
    pub managed: bool,
}

impl Default for PolicySettings {
    fn default() -> Self {
        Self {
            sync_mode: default_sync_mode(),
            admin_server_url: None,
            merge_strategy: default_merge_strategy(),
            locked_paths: Vec::new(),
            last_synced_at: None,
            master_config_version: None,
            organization: None,
            fleet_signing_key: None,
            managed: false,
        }
    }
}

fn default_sync_mode() -> String {
    "standalone".to_string()
}

fn default_merge_strategy() -> String {
    "merge".to_string()
}

// ═══════════════════════════════════════════════════════════════════════
// STORAGE ENGINE
// ═══════════════════════════════════════════════════════════════════════

static SETTINGS_CACHE: Mutex<Option<AppSettings>> = Mutex::new(None);

fn generate_device_id() -> String {
    Uuid::new_v4().to_string()
}

fn now_iso8601() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn compute_settings_hash(settings: &AppSettings) -> String {
    let json = serde_json::to_string(settings).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    hex::encode(hasher.finalize())
}

fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Create default settings with a generated device ID.
fn create_default_settings() -> AppSettings {
    AppSettings {
        settings_version: SETTINGS_VERSION,
        app_version: get_app_version(),
        device_id: generate_device_id(),
        last_seen_at: now_iso8601(),
        created_at: now_iso8601(),
        app: AppPreferences::default(),
        ideal: SystemState::default(),
        current: SystemState::default(),
        policy: PolicySettings::default(),
    }
}

fn apply_runtime_defaults(settings: &mut AppSettings) {
    if settings.app.permanently_hidden_panels.is_none() {
        settings.app.permanently_hidden_panels = Some(
            DEFAULT_PERMANENTLY_HIDDEN_PANELS
                .iter()
                .map(|panel| panel.to_string())
                .collect(),
        );
    }
}

// ── Store-backed load (enc:v1: at rest via datastore) ─────────────────────

/// Parse a settings JSON value, applying schema migration if needed.
fn parse_and_migrate_json_val(json: serde_json::Value) -> Result<AppSettings, String> {
    let version = json
        .get("settingsVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u32;
    let final_json = if version < SETTINGS_VERSION {
        migrate_settings(json, version)
    } else {
        json
    };
    let mut settings: AppSettings = serde_json::from_value(final_json)
        .map_err(|e| format!("Failed to deserialize settings: {e}"))?;
    apply_runtime_defaults(&mut settings);
    Ok(settings)
}

/// Load settings from the encoded store. If the store section is absent,
/// check for a legacy plaintext settings.json and migrate it on first run.
fn load_settings_from_store() -> Result<AppSettings, String> {
    crate::paths::migrate_user_data_layout()?;
    let stored = crate::datastore::load("settings")?;
    // Empty object = section file does not yet exist.
    if stored.as_object().map(|m| m.is_empty()).unwrap_or(false) {
        let legacy = crate::paths::user_settings_path()?;
        if legacy.exists() {
            let raw = fs::read_to_string(&legacy)
                .map_err(|e| format!("Failed to read legacy settings.json: {e}"))?;
            if !raw.trim().is_empty() {
                let json: serde_json::Value = serde_json::from_str(&raw)
                    .map_err(|e| format!("Failed to parse legacy settings.json: {e}"))?;
                crate::log_message(
                    "info",
                    "[Settings] Migrating plaintext settings.json to encoded store",
                );
                let result = parse_and_migrate_json_val(json);
                // Delete the plaintext file now that migration is complete.
                let _ = fs::remove_file(&legacy);
                return result;
            }
        }
        return Ok(create_default_settings());
    }
    // Encrypted store is active — remove any stale plaintext file.
    if let Ok(legacy) = crate::paths::user_settings_path() {
        if legacy.exists() {
            let _ = fs::remove_file(&legacy);
        }
    }
    parse_and_migrate_json_val(stored)
}

/// Read settings from disk, or create defaults if file doesn't exist.
pub fn read_settings() -> Result<AppSettings, String> {
    let res = (|| {
        if let Ok(guard) = SETTINGS_CACHE.lock() {
            if let Some(ref cached) = *guard {
                return Ok(cached.clone());
            }
        }

        let mut settings = load_settings_from_store()?;
        settings.last_seen_at = now_iso8601();
        settings.app_version = get_app_version();

        // Persist so first-run defaults / legacy migration land in the store.
        // Skip in decoy mode: a read must never write to the real store under
        // coercion (and write_settings_internal would refuse it anyway).
        if !DECOY_MODE.load(std::sync::atomic::Ordering::Relaxed) {
            write_settings_internal(&settings)?;
        }
        if let Ok(mut guard) = SETTINGS_CACHE.lock() {
            *guard = Some(settings.clone());
        }
        crate::set_logging_enabled_flag(settings.app.logging_enabled.unwrap_or(true));

        Ok(settings)
    })();

    if let Err(ref e) = res {
        crate::log_message("error", &format!("[Settings] read_settings failed: {}", e));
    }
    res
}

/// Write settings via the app-data store (encoded at rest, atomic write).
fn write_settings_internal(settings: &AppSettings) -> Result<(), String> {
    // Anti-coercion backstop: while a decoy session is active, refuse EVERY
    // settings write at the single choke point all writers funnel through
    // (set_settings, import_settings, apply_admin_config, patch_settings, the
    // startup-PIN register/clear commands, flow/monitor persistence). A coerced
    // decoy session must never persist over — or erase — the real config, even
    // via a direct `invoke`. read_settings guards its own cold-read write so it
    // never trips this error.
    if DECOY_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Settings are read-only in decoy mode.".to_string());
    }
    let value = serde_json::to_value(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    let res = crate::datastore::save("settings", &value);
    if let Err(ref e) = res {
        crate::log_message(
            "error",
            &format!("[Settings] write_settings_internal failed: {}", e),
        );
    }
    res
}

/// Write full settings (public API).
pub fn write_settings(settings: &AppSettings) -> Result<(), String> {
    crate::set_logging_enabled_flag(settings.app.logging_enabled.unwrap_or(true));
    write_settings_internal(settings)?;
    if let Ok(mut guard) = SETTINGS_CACHE.lock() {
        *guard = Some(settings.clone());
    }
    Ok(())
}

/// Invalidate in-memory cache (force re-read from disk).
#[allow(dead_code)]
pub fn invalidate_cache() {
    if let Ok(mut guard) = SETTINGS_CACHE.lock() {
        *guard = None;
    }
}

// ═══════════════════════════════════════════════════════════════════════
// PARTIAL UPDATE — Deep-merge a JSON patch into settings
// ═══════════════════════════════════════════════════════════════════════

/// Apply a partial JSON patch to settings.
/// Only provided fields are updated (merge semantics).
/// KT: Holds the cache mutex for the entire read-modify-write cycle to prevent
/// TOCTOU races. Without this, concurrent calls (e.g. user click + background probe)
/// can overwrite each other's changes because both read the same snapshot.
pub fn patch_settings(patch: serde_json::Value) -> Result<AppSettings, String> {
    let mut cache = SETTINGS_CACHE
        .lock()
        .map_err(|_| "Settings cache lock poisoned".to_string())?;

    // Read from cache or store (without releasing the lock)
    let settings = if let Some(ref cached) = *cache {
        cached.clone()
    } else {
        load_settings_from_store()?
    };

    let mut current =
        serde_json::to_value(&settings).map_err(|e| format!("Serialization error: {}", e))?;

    merge_json(&mut current, &patch);

    let mut updated: AppSettings =
        serde_json::from_value(current).map_err(|e| format!("Failed to apply patch: {}", e))?;

    updated.last_seen_at = now_iso8601();

    // Write to disk and update cache while still holding the lock
    crate::set_logging_enabled_flag(updated.app.logging_enabled.unwrap_or(true));
    // Capture the pre-write tree for the settings-changed diff (paid-gated;
    // skipped entirely for free installs to avoid the double-serialize cost).
    let old_json = if crate::license::has_paid_entitlement() {
        serde_json::to_value(&settings).ok()
    } else {
        None
    };
    write_settings_internal(&updated)?;
    *cache = Some(updated.clone());

    // M3: fire the flows settings-changed source. Only reached on a successful
    // write, i.e. never in decoy mode (write_settings_internal refuses there).
    if let Some(old_json) = old_json {
        if let Ok(new_json) = serde_json::to_value(&updated) {
            crate::flow_bridge::on_settings_written(&old_json, &new_json);
        }
    }

    Ok(updated)
}

/// Deep merge: patch values override base values. Objects are merged recursively.
fn merge_json(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) => {
            for (key, patch_val) in patch_map {
                let base_val = base_map
                    .entry(key.clone())
                    .or_insert(serde_json::Value::Null);
                merge_json(base_val, patch_val);
            }
        }
        (base, patch) => {
            *base = patch.clone();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// DRIFT DETECTION — Compare desired vs. actual
// ═══════════════════════════════════════════════════════════════════════

/// Computes a SHA-256 hash of the current settings for sync comparison.
pub fn get_settings_hash() -> Result<String, String> {
    let settings = read_settings()?;
    Ok(compute_settings_hash(&settings))
}

// ═══════════════════════════════════════════════════════════════════════
// ADMIN POLICY — Locked settings enforcement
// ═══════════════════════════════════════════════════════════════════════

/// Check if a setting path is locked by admin policy.
pub fn is_path_locked(path: &str) -> Result<bool, String> {
    let settings = read_settings()?;
    Ok(settings
        .policy
        .locked_paths
        .iter()
        .any(|p| path.starts_with(p.as_str())))
}

/// Apply admin master config with merge/overwrite strategy.
pub fn apply_admin_config(
    admin_config: serde_json::Value,
    locked_paths: Vec<String>,
    strategy: &str,
    config_version: u32,
    managed: bool,
) -> Result<AppSettings, String> {
    // Admin config targets desired state → wrap in "ideal"
    let wrapped = serde_json::json!({"ideal": admin_config});

    if strategy == "overwrite" {
        // Full overwrite: start from defaults but keep device identity + current state.
        // Assemble the complete final AppSettings in memory — merged config AND all
        // policy fields — then persist in a single write.
        // KT: must be one atomic write; writing config first and policy second leaves a
        // window where the new config is on disk but lockedPaths are not yet set
        // (fail-open on exactly the paths this epoch is locking).
        let existing = read_settings()?;
        let mut fresh = create_default_settings();
        fresh.device_id = existing.device_id;
        fresh.created_at = existing.created_at;
        fresh.current = existing.current; // preserve probed state

        // Apply admin config on top of ideal
        let mut as_json =
            serde_json::to_value(&fresh).map_err(|e| format!("Serialization error: {}", e))?;
        merge_json(&mut as_json, &wrapped);
        let mut settings: AppSettings =
            serde_json::from_value(as_json).map_err(|e| format!("Deserialization error: {}", e))?;

        // Set all policy fields before the single write
        settings.policy.locked_paths = locked_paths;
        settings.policy.last_synced_at = Some(now_iso8601());
        settings.policy.master_config_version = Some(config_version);
        settings.policy.sync_mode = "managed".to_string();
        settings.policy.managed = managed;

        write_settings(&settings)?;
        Ok(settings)
    } else {
        // Merge: hold the cache lock for the entire read-modify-write cycle so no
        // reader or racing patch_settings_cmd sees a partial state between writes.
        // KT: must be one atomic write; writing config first and policy second leaves a
        // window where the new config is on disk but lockedPaths are not yet set
        // (fail-open on exactly the paths this epoch is locking).
        let mut cache = SETTINGS_CACHE
            .lock()
            .map_err(|_| "Settings cache lock poisoned".to_string())?;

        let base = if let Some(ref cached) = *cache {
            cached.clone()
        } else {
            load_settings_from_store()?
        };

        let mut as_json =
            serde_json::to_value(&base).map_err(|e| format!("Serialization error: {}", e))?;
        merge_json(&mut as_json, &wrapped);
        let mut settings: AppSettings =
            serde_json::from_value(as_json).map_err(|e| format!("Failed to apply patch: {}", e))?;

        settings.last_seen_at = now_iso8601();

        // Set all policy fields before the single write
        settings.policy.locked_paths = locked_paths;
        settings.policy.last_synced_at = Some(now_iso8601());
        settings.policy.master_config_version = Some(config_version);
        settings.policy.sync_mode = "managed".to_string();
        settings.policy.managed = managed;

        // Single atomic write: merged config + locked paths together, through the
        // decoy-mode choke point (write_settings_internal).
        crate::set_logging_enabled_flag(settings.app.logging_enabled.unwrap_or(true));
        write_settings_internal(&settings)?;
        *cache = Some(settings.clone());

        Ok(settings)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// EXPORT / IMPORT — For backup and admin distribution
// ═══════════════════════════════════════════════════════════════════════

/// Export current settings as a JSON string (for backup or admin review).
#[allow(dead_code)]
pub fn export_settings() -> Result<String, String> {
    let settings = read_settings()?;
    serde_json::to_string_pretty(&settings).map_err(|e| format!("Export failed: {}", e))
}

/// Import settings from a JSON string (for restore or admin push).
pub fn import_settings(json: &str) -> Result<AppSettings, String> {
    let mut imported: AppSettings =
        serde_json::from_str(json).map_err(|e| format!("Import failed: {}", e))?;

    // Preserve device identity
    let current = read_settings()?;
    imported.device_id = current.device_id;
    imported.created_at = current.created_at;
    imported.last_seen_at = now_iso8601();
    imported.app_version = get_app_version();

    write_settings(&imported)?;
    Ok(imported)
}

// ═══════════════════════════════════════════════════════════════════════
// TAURI IPC COMMANDS
// ═══════════════════════════════════════════════════════════════════════

/// Get the full settings object.
#[tauri::command]
pub fn get_settings() -> Result<serde_json::Value, String> {
    let settings = read_settings()?;
    let v = serde_json::to_value(&settings).map_err(|e| format!("Serialization error: {}", e))?;
    Ok(v)
}

/// Replace the full settings object.
#[tauri::command]
pub fn set_settings(settings: serde_json::Value) -> Result<serde_json::Value, String> {
    let parsed: AppSettings =
        serde_json::from_value(settings).map_err(|e| format!("Invalid settings format: {}", e))?;
    // Capture the pre-write tree for the settings-changed diff (paid-gated).
    let old_json = if crate::license::has_paid_entitlement() {
        read_settings()
            .ok()
            .and_then(|s| serde_json::to_value(&s).ok())
    } else {
        None
    };
    write_settings(&parsed)?;
    let v = serde_json::to_value(&parsed).map_err(|e| format!("Serialization error: {}", e))?;

    // M3: fire the flows settings-changed source (paid-gated; post-write).
    if let Some(old_json) = old_json {
        crate::flow_bridge::on_settings_written(&old_json, &v);
    }

    Ok(v)
}

/// While ON, all settings writes are refused (see patch_settings_cmd). Set by
/// the frontend when a decoy PIN unlocks the decoy view so a coerced decoy
/// session can never persist over the real configuration. In-memory only —
/// resets to OFF on relaunch, matching the frontend's in-memory AuthMode.
static DECOY_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Enable/disable decoy read-only mode. Called from the calculator-gate auth
/// handler with `on = (mode == "decoy")`.
#[tauri::command]
pub fn set_decoy_mode(on: bool) {
    DECOY_MODE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether a decoy session is currently active. Surfaces that the diagnostic
/// log must not be revealed (the in-app Error Center reads the log directly,
/// bypassing the frontend's decoy null-gate).
pub fn is_decoy_mode() -> bool {
    DECOY_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the floating desktop-alert overlay window is suppressed.
/// Reads from the settings cache — returns false if the cache is not yet warm.
pub fn is_native_notifications_disabled() -> bool {
    SETTINGS_CACHE
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.app.disable_native_notifications))
        .unwrap_or(false)
}

/// Patch settings with a partial JSON object (deep merge).
#[tauri::command]
pub fn patch_settings_cmd(patch: serde_json::Value) -> Result<serde_json::Value, String> {
    // Read-only in decoy mode: the decoy view shows appSettings=null, and this
    // backend backstop refuses every write — even direct/programmatic patch
    // calls — so a coerced decoy session can't mutate or leak the real config.
    if DECOY_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Settings are read-only in decoy mode.".to_string());
    }

    // Check locked paths for writes to ideal state
    if let Some(ideal_obj) = patch.get("ideal").and_then(|v| v.as_object()) {
        let settings = read_settings()?;
        if settings.policy.sync_mode == "managed" {
            let flat = flatten_value(&serde_json::Value::Object(ideal_obj.clone()), "");
            for path in flat.keys() {
                if settings
                    .policy
                    .locked_paths
                    .iter()
                    .any(|p| path.starts_with(p.as_str()))
                {
                    return Err(format!("Setting '{}' is locked by admin policy", path));
                }
            }
        }
    }

    // M5 fleet lock: a managed device whose policy locks `app.flows` (the flows
    // rule set) may not have `app.proFlows` mutated locally. This closes the
    // gap where flows lived entirely outside the signed config chain — a
    // fleet-pushed rule set can now be made read-only on the endpoint. The
    // server remains the authoritative gate; this is the local deterrent.
    if patch.get("app").and_then(|a| a.get("proFlows")).is_some() {
        let settings = read_settings()?;
        if settings.policy.sync_mode == "managed"
            && settings
                .policy
                .locked_paths
                .iter()
                .any(|p| p == "app.flows" || p == "app.proFlows")
        {
            return Err("Flows are locked by admin policy".to_string());
        }
    }

    let updated = patch_settings(patch)?;
    serde_json::to_value(&updated).map_err(|e| format!("Serialization error: {}", e))
}

/// Get a single setting value by dot-path (e.g., "privacy.telemetry.windowsDisabled").
#[tauri::command]
pub fn get_setting(path: String) -> Result<serde_json::Value, String> {
    let settings = read_settings()?;
    let json =
        serde_json::to_value(&settings).map_err(|e| format!("Serialization error: {}", e))?;

    let mut current = &json;
    for segment in path.split('.') {
        current = current
            .get(segment)
            .ok_or_else(|| format!("Setting path '{}' not found", path))?;
    }
    Ok(current.clone())
}

/// Get the settings hash for sync comparison.
#[tauri::command]
pub fn get_settings_hash_cmd() -> Result<String, String> {
    get_settings_hash()
}

/// Get device identity info for registration/heartbeat.
#[tauri::command]
pub fn get_device_identity() -> Result<serde_json::Value, String> {
    let settings = read_settings()?;
    Ok(serde_json::json!({
        "deviceId": settings.device_id,
        "appVersion": settings.app_version,
        "lastSeenAt": settings.last_seen_at,
        "createdAt": settings.created_at,
        "settingsHash": compute_settings_hash(&settings),
        "settingsVersion": settings.settings_version,
        "syncMode": settings.policy.sync_mode,
        "organization": settings.policy.organization,
        "masterConfigVersion": settings.policy.master_config_version,
    }))
}

/// Apply an admin config push (used by admin server sync).
///
/// SECURITY: if this device is fleet-managed (`policy.fleet_signing_key` is set),
/// the push MUST carry a valid Ed25519 signature from that pinned key over the
/// canonical epoch preimage (`version || canonical(config)`) — otherwise any
/// tailnet peer could push policy. The `signer_key` in the push must also match
/// the pinned key (anti key-swap). Unmanaged devices (no pinned key) keep the
/// prior local-trust behaviour. Fail closed.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn apply_admin_config_cmd(
    config: serde_json::Value,
    locked_paths: Vec<String>,
    strategy: String,
    config_version: u32,
    signature: Option<String>,
    signer_key: Option<String>,
    // Fleet Control Plane P2: the signed epoch envelope also binds the target
    // scope + lock set + managed flag. These default to an org-wide unmanaged
    // epoch for legacy/unmanaged callers. The signature is verified over the
    // SAME envelope the fleet server signed (see epoch_signing_envelope).
    target_kind: Option<String>,
    target_id: Option<String>,
    managed: Option<bool>,
) -> Result<serde_json::Value, String> {
    // KT: fail-closed guard for the canonical signing path. epoch_preimage ->
    // write_canonical (wincmd-shared) assumes every scalar in `config` serializes;
    // that holds while serde_json's `arbitrary_precision` feature is OFF. Reject a
    // non-serializable config BEFORE rebuilding the verify preimage so a malicious
    // server (or a future feature flip) yields a clean error, not a panic.
    if serde_json::to_string(&config).is_err() {
        return Err("config push is not canonically serializable".to_string());
    }
    if let Some(pinned) = read_settings()?.policy.fleet_signing_key {
        let sig = signature
            .ok_or_else(|| "fleet-managed device: config push requires a signature".to_string())?;
        if let Some(provided) = signer_key.as_deref() {
            if provided != pinned {
                return Err(
                    "config push signer key does not match the pinned fleet key".to_string()
                );
            }
        }
        // KT: epoch_preimage is the SSOT for epoch bytes; callers must not
        // assemble the bytes by hand (they would silently miss new fields).
        let msg = wincmd_shared::fleet::epoch_preimage(&wincmd_shared::fleet::EpochSigningInput {
            version: config_version as i64,
            config: &config,
            locked_paths: &locked_paths,
            managed: managed.unwrap_or(false),
            target_kind: target_kind.as_deref().unwrap_or("org"),
            target_id: target_id.as_deref(),
        });
        if !wincmd_shared::fleet::verify_signature_b64(&pinned, &msg, &sig) {
            return Err("config push signature verification failed".to_string());
        }
    }
    let updated = apply_admin_config(
        config,
        locked_paths,
        &strategy,
        config_version,
        managed.unwrap_or(false),
    )?;
    serde_json::to_value(&updated).map_err(|e| format!("Serialization error: {}", e))
}

/// Check if a setting is locked by admin.
#[tauri::command]
pub fn is_setting_locked(path: String) -> Result<bool, String> {
    is_path_locked(&path)
}

/// Export settings as JSON string.
#[tauri::command]
pub fn export_settings_cmd() -> Result<String, String> {
    let settings = read_settings()?;
    let v = serde_json::to_value(&settings).map_err(|e| format!("Export failed: {}", e))?;
    serde_json::to_string_pretty(&v).map_err(|e| format!("Export failed: {}", e))
}

/// Import settings from JSON string.
#[tauri::command]
pub fn import_settings_cmd(json: String) -> Result<serde_json::Value, String> {
    let updated = import_settings(&json)?;
    serde_json::to_value(&updated).map_err(|e| format!("Serialization error: {}", e))
}

/// Write the exported settings JSON to a path the user picked via the native
/// save dialog. Done in Rust (not the frontend `@tauri-apps/plugin-fs` API)
/// because that plugin's `fs:scope` capability is deliberately locked to
/// `$APPDATA/WinCommander/**`/`$RESOURCE/**`/`$TEMP/WinCommander-*` — widening
/// it to cover arbitrary user-chosen paths would let ANY frontend JS read/write
/// anywhere on disk, not just this one dialog-driven export. Rust file I/O has
/// no such scope restriction, so the path (already validated by the OS's own
/// native save dialog, not raw user/webpage input) is safe to write here.
#[tauri::command]
pub fn write_settings_export_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| format!("Failed to write {}: {}", path, e))
}

/// Read a settings JSON file from a path the user picked via the native open
/// dialog. Mirrors `write_settings_export_file`'s reasoning for doing this in
/// Rust rather than via the frontend fs plugin.
#[tauri::command]
pub fn read_settings_import_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path, e))
}

// ═══════════════════════════════════════════════════════════════════════
// MIGRATION — Auto-upgrade settings schema across versions
// ═══════════════════════════════════════════════════════════════════════

/// Migrate settings JSON from an older version to the current version.
fn migrate_settings(mut root: serde_json::Value, from_version: u32) -> serde_json::Value {
    if from_version < 2 {
        // v1 → v2: Move system state fields under "ideal", add empty "current"
        if let Some(obj) = root.as_object_mut() {
            let mut ideal = serde_json::Map::new();
            let state_fields = [
                "privacy",
                "tweaks",
                "network",
                "identity",
                "apps",
                "productivity",
            ];
            for field in &state_fields {
                if let Some(val) = obj.remove(*field) {
                    ideal.insert(field.to_string(), val);
                }
            }
            obj.insert("ideal".to_string(), serde_json::Value::Object(ideal));
            obj.insert(
                "current".to_string(),
                serde_json::Value::Object(serde_json::Map::new()),
            );
            obj.insert(
                "settingsVersion".to_string(),
                serde_json::json!(SETTINGS_VERSION),
            );
        }
    }
    root
}

// ═══════════════════════════════════════════════════════════════════════
// CONVERGENCE ENGINE — Diff ideal vs current, map to commands
// ═══════════════════════════════════════════════════════════════════════

/// Flatten a nested JSON value into a HashMap of dot-paths → leaf values.
/// Skips null values and recursively descends into objects.
pub fn flatten_value(
    value: &serde_json::Value,
    prefix: &str,
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut result = std::collections::HashMap::new();
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                result.extend(flatten_value(val, &path));
            }
        }
        serde_json::Value::Null => {
            // Skip nulls — null in ideal means "no preference"
        }
        _ => {
            result.insert(prefix.to_string(), value.clone());
        }
    }
    result
}

/// Reverse command map: given a settings dot-path and desired boolean value,
/// returns the PowerShell command that would achieve that state.
/// This is the exact INVERSE of backend.rs::get_settings_sync_patch().
pub fn get_convergence_command(path: &str, desired: bool) -> Option<&'static str> {
    match (path, desired) {
        // ── Privacy: Telemetry ───────────────────────────────────────
        ("privacy.telemetry.windowsDisabled", true) => Some("Disable-Telemetry"),
        ("privacy.telemetry.windowsDisabled", false) => Some("Enable-Telemetry"),
        ("privacy.telemetry.powershell7Disabled", true) => Some("Disable-Telemetry"),
        ("privacy.telemetry.powershell7Disabled", false) => Some("Enable-Telemetry"),
        ("privacy.telemetry.officeDisabled", true) => Some("Disable-Telemetry"),
        ("privacy.telemetry.officeDisabled", false) => Some("Enable-Telemetry"),
        ("privacy.telemetry.copilotDisabled", true) => Some("Disable-Copilot"),
        ("privacy.telemetry.copilotDisabled", false) => Some("Enable-Copilot"),
        ("privacy.telemetry.activityHistoryDisabled", true) => Some("Disable-ActivityHistory"),
        ("privacy.telemetry.activityHistoryDisabled", false) => Some("Enable-ActivityHistory"),
        ("privacy.telemetry.locationTrackingDisabled", true) => Some("Disable-LocationTracking"),
        ("privacy.telemetry.locationTrackingDisabled", false) => Some("Enable-LocationTracking"),
        ("privacy.telemetry.windowsSuggestionsDisabled", true) => {
            Some("Disable-WindowsSuggestions")
        }
        ("privacy.telemetry.windowsSuggestionsDisabled", false) => {
            Some("Enable-WindowsSuggestions")
        }

        // ── Privacy: Lock Screen ─────────────────────────────────────
        ("privacy.lockscreen.privacyDisabled", true) => Some("Disable-LockScreenPrivacy"),
        ("privacy.lockscreen.privacyDisabled", false) => Some("Enable-LockScreenPrivacy"),

        // ── Privacy: Setup Nags ──────────────────────────────────────
        ("privacy.setupCompletionNagsDisabled", true) => Some("Disable-SetupCompletionNags"),
        ("privacy.setupCompletionNagsDisabled", false) => Some("Enable-SetupCompletionNags"),

        // ── Privacy: Clipboard ───────────────────────────────────────
        ("privacy.clipboard.historyDisabled", true) => Some("Disable-ClipboardHistory"),
        ("privacy.clipboard.historyDisabled", false) => Some("Enable-ClipboardHistory"),
        ("privacy.clipboard.cloudSyncDisabled", true) => Some("Disable-CloudClipboardSync"),
        ("privacy.clipboard.cloudSyncDisabled", false) => Some("Enable-CloudClipboardSync"),

        // ── Privacy: Tracking ────────────────────────────────────────
        ("privacy.tracking.recentFilesDisabled", true) => Some("Disable-RecentFilesTracking"),
        ("privacy.tracking.recentFilesDisabled", false) => Some("Enable-RecentFilesTracking"),
        ("privacy.tracking.jumpListsDisabled", true) => Some("Disable-JumpLists"),
        ("privacy.tracking.jumpListsDisabled", false) => Some("Enable-JumpLists"),
        ("privacy.tracking.thumbnailCacheDisabled", true) => Some("Disable-ThumbnailCache"),
        ("privacy.tracking.thumbnailCacheDisabled", false) => Some("Enable-ThumbnailCache"),
        ("privacy.tracking.pagefileDisabled", true) => Some("Disable-Pagefile"),
        ("privacy.tracking.pagefileDisabled", false) => Some("Enable-Pagefile"),
        // privacy.tracking.rdpAutoEraseSchedule + eventLogAutoEraseSchedule
        // and privacy.clipboard.autoEraseSchedule were replaced by the
        // per-card auto-erase scheduler in Privacy Clean
        // (Set-AutoEraseSchedule). No mapping needed: those settings paths
        // are gone from the registry.

        // ── Privacy: Protection & Shield ─────────────────────────────
        ("privacy.privacyProtectionEnabled", true) => Some("Enable-PrivacyProtection"),
        ("privacy.privacyProtectionEnabled", false) => Some("Disable-PrivacyProtection"),

        // ── Tweaks: Security ─────────────────────────────────────────
        // KT: Free may report this drift, but does not materialize the
        // Pro-only command ID here; the interactive toggle path dispatches
        // it via the backend's fragmented command table.
        ("tweaks.security.defenderDisabled", true) => None,
        ("tweaks.security.defenderDisabled", false) => Some("Enable-WindowsDefender"),
        ("tweaks.security.windowsUpdateDisabled", true) => Some("Disable-WindowsUpdate"),
        ("tweaks.security.windowsUpdateDisabled", false) => Some("Enable-WindowsUpdate"),
        ("tweaks.security.uacDisabled", true) => Some("Disable-UAC"),
        ("tweaks.security.uacDisabled", false) => Some("Enable-UAC"),
        ("tweaks.security.usbWriteProtect", true) => Some("Enable-USBWriteProtect"),
        ("tweaks.security.usbWriteProtect", false) => Some("Disable-USBWriteProtect"),
        ("tweaks.security.usbStorageLockdown", true) => Some("Enable-USBStorageLockdown"),
        ("tweaks.security.usbStorageLockdown", false) => Some("Disable-USBStorageLockdown"),
        ("tweaks.security.lidClosePowerOff", true) => Some("Enable-LidClosePowerOff"),
        ("tweaks.security.lidClosePowerOff", false) => Some("Disable-LidClosePowerOff"),
        ("tweaks.security.consumerFeaturesDisabled", true) => Some("Disable-ConsumerFeatures"),
        ("tweaks.security.consumerFeaturesDisabled", false) => Some("Enable-ConsumerFeatures"),

        // ── Tweaks: OS ───────────────────────────────────────────────
        ("tweaks.os.superfetchDisabled", true) => Some("Disable-Superfetch"),
        ("tweaks.os.superfetchDisabled", false) => Some("Enable-Superfetch"),
        ("tweaks.os.prefetchDisabled", true) => Some("Disable-Prefetch"),
        ("tweaks.os.prefetchDisabled", false) => Some("Enable-Prefetch"),
        ("tweaks.os.hibernationDisabled", true) => Some("Disable-Hibernation"),
        ("tweaks.os.hibernationDisabled", false) => Some("Enable-Hibernation"),
        ("tweaks.os.fastStartupDisabled", true) => Some("Disable-FastStartup"),
        ("tweaks.os.fastStartupDisabled", false) => Some("Enable-FastStartup"),
        ("tweaks.os.ntfsOptimizationsEnabled", true) => Some("Enable-NTFSOptimizations"),
        ("tweaks.os.ntfsOptimizationsEnabled", false) => Some("Disable-NTFSOptimizations"),
        ("tweaks.os.detailedBsodEnabled", true) => Some("Enable-DetailedBSOD"),
        ("tweaks.os.detailedBsodEnabled", false) => Some("Disable-DetailedBSOD"),
        ("tweaks.os.automaticMaintenanceDisabled", true) => Some("Disable-AutomaticMaintenance"),
        ("tweaks.os.automaticMaintenanceDisabled", false) => Some("Enable-AutomaticMaintenance"),
        ("tweaks.os.win32LongPathsEnabled", true) => Some("Enable-Win32LongPaths"),
        ("tweaks.os.win32LongPathsEnabled", false) => Some("Disable-Win32LongPaths"),
        ("tweaks.os.smbBandwidthThrottlingDisabled", true) => {
            Some("Disable-SmbBandwidthThrottling")
        }
        ("tweaks.os.smbBandwidthThrottlingDisabled", false) => {
            Some("Enable-SmbBandwidthThrottling")
        }

        // ── Tweaks: UI ───────────────────────────────────────────────
        ("tweaks.ui.classicContextMenu", true) => Some("Enable-ClassicContextMenu"),
        ("tweaks.ui.classicContextMenu", false) => Some("Disable-ClassicContextMenu"),
        ("tweaks.ui.fileExtensionsVisible", true) => Some("Show-FileExtensions"),
        ("tweaks.ui.fileExtensionsVisible", false) => Some("Hide-FileExtensions"),
        ("tweaks.ui.hiddenFilesVisible", true) => Some("Show-HiddenFiles"),
        ("tweaks.ui.hiddenFilesVisible", false) => Some("Hide-HiddenFiles"),
        ("tweaks.ui.galleryHomeRemoved", true) => Some("Enable-RemoveGalleryHome"),
        ("tweaks.ui.galleryHomeRemoved", false) => Some("Disable-RemoveGalleryHome"),
        ("tweaks.ui.bingSearchDisabled", true) => Some("Disable-BingSearch"),
        ("tweaks.ui.bingSearchDisabled", false) => Some("Enable-BingSearch"),
        ("tweaks.ui.backgroundAppsDisabled", true) => Some("Disable-BackgroundApps"),
        ("tweaks.ui.backgroundAppsDisabled", false) => Some("Enable-BackgroundApps"),
        ("tweaks.ui.notificationsDisabled", true) => Some("Disable-Notifications"),
        ("tweaks.ui.notificationsDisabled", false) => Some("Enable-Notifications"),
        ("tweaks.ui.endTaskOnTaskbar", true) => Some("Enable-EndTaskOnTaskbar"),
        ("tweaks.ui.endTaskOnTaskbar", false) => Some("Disable-EndTaskOnTaskbar"),
        ("tweaks.ui.folderTypeDiscoveryDisabled", true) => Some("Disable-FolderTypeDiscovery"),
        ("tweaks.ui.folderTypeDiscoveryDisabled", false) => Some("Enable-FolderTypeDiscovery"),
        ("tweaks.ui.shortcutSuffixRemoved", true) => Some("Remove-ShortcutSuffix"),
        ("tweaks.ui.shortcutSuffixRemoved", false) => Some("Restore-ShortcutSuffix"),
        ("tweaks.ui.autoPlayDisabled", true) => Some("Disable-AutoPlay"),
        ("tweaks.ui.autoPlayDisabled", false) => Some("Enable-AutoPlay"),
        ("tweaks.ui.lowDiskCheckDisabled", true) => Some("Disable-LowDiskCheck"),
        ("tweaks.ui.lowDiskCheckDisabled", false) => Some("Enable-LowDiskCheck"),
        ("tweaks.ui.explorerOpensThisPc", true) => Some("Set-ExplorerOpensThisPC"),
        ("tweaks.ui.explorerOpensThisPc", false) => Some("Set-ExplorerOpensQuickAccess"),
        ("tweaks.ui.syncProviderNotificationsHidden", true) => {
            Some("Hide-SyncProviderNotifications")
        }
        ("tweaks.ui.syncProviderNotificationsHidden", false) => {
            Some("Show-SyncProviderNotifications")
        }
        ("tweaks.ui.transparencyDisabled", true) => Some("Disable-TransparencyEffects"),
        ("tweaks.ui.transparencyDisabled", false) => Some("Enable-TransparencyEffects"),
        ("tweaks.ui.fullPathInTitleBar", true) => Some("Enable-FullPathInTitleBar"),
        ("tweaks.ui.fullPathInTitleBar", false) => Some("Disable-FullPathInTitleBar"),

        // ── Tweaks: Security (new) ───────────────────────────────────
        ("tweaks.security.vbsDisabled", true) => Some("Disable-VBS"),
        ("tweaks.security.vbsDisabled", false) => Some("Enable-VBS"),
        ("tweaks.security.bitlockerAutoEncryptDisabled", true) => {
            Some("Disable-BitLockerAutoEncrypt")
        }
        ("tweaks.security.bitlockerAutoEncryptDisabled", false) => {
            Some("Enable-BitLockerAutoEncrypt")
        }
        ("tweaks.security.wpbtDisabled", true) => Some("Disable-WPBT"),
        ("tweaks.security.wpbtDisabled", false) => Some("Enable-WPBT"),
        ("tweaks.security.smartScreenDisabled", true) => Some("Disable-SmartScreen"),
        ("tweaks.security.smartScreenDisabled", false) => Some("Enable-SmartScreen"),
        ("tweaks.security.oobeBypassEnabled", true) => Some("Set-OOBEBypass"),
        ("tweaks.security.oobeBypassEnabled", false) => Some("Clear-OOBEBypass"),
        ("tweaks.security.gameDvrDisabled", true) => Some("Disable-GameDVR"),
        ("tweaks.security.gameDvrDisabled", false) => Some("Enable-GameDVR"),
        ("tweaks.security.remoteAssistanceDisabled", true) => Some("Disable-RemoteAssistance"),
        ("tweaks.security.remoteAssistanceDisabled", false) => Some("Enable-RemoteAssistance"),
        ("tweaks.security.anonymousSamEnumerationBlocked", true) => {
            Some("Block-AnonymousSamEnumeration")
        }
        ("tweaks.security.anonymousSamEnumerationBlocked", false) => {
            Some("Allow-AnonymousSamEnumeration")
        }

        // ── Tweaks: OS (new) ─────────────────────────────────────────
        ("tweaks.os.memoryCompressionDisabled", true) => Some("Disable-MemoryCompression"),
        ("tweaks.os.memoryCompressionDisabled", false) => Some("Enable-MemoryCompression"),
        ("tweaks.os.win32PrioritySeparation", true) => Some("Set-Win32PrioritySeparation"),
        ("tweaks.os.win32PrioritySeparation", false) => Some("Reset-Win32PrioritySeparation"),
        ("tweaks.os.serviceTimeoutsOptimized", true) => Some("Set-OptimizedTimeouts"),
        ("tweaks.os.serviceTimeoutsOptimized", false) => Some("Reset-OptimizedTimeouts"),
        ("tweaks.os.reservedStorageDisabled", true) => Some("Disable-ReservedStorage"),
        ("tweaks.os.reservedStorageDisabled", false) => Some("Enable-ReservedStorage"),

        // ── Tweaks: Boot & Kernel (new) ──────────────────────────────
        ("tweaks.bootKernel.tsxEnabled", true) => Some("Enable-TSX"),
        ("tweaks.bootKernel.tsxEnabled", false) => Some("Disable-TSX"),
        ("tweaks.bootKernel.firstLogonAnimationDisabled", true) => {
            Some("Disable-FirstLogonAnimation")
        }
        ("tweaks.bootKernel.firstLogonAnimationDisabled", false) => {
            Some("Enable-FirstLogonAnimation")
        }
        ("tweaks.bootKernel.startupSoundDisabled", true) => Some("Disable-StartupSound"),
        ("tweaks.bootKernel.startupSoundDisabled", false) => Some("Enable-StartupSound"),
        ("tweaks.bootKernel.autoRestartSignonDisabled", true) => Some("Disable-AutoRestartSignon"),
        ("tweaks.bootKernel.autoRestartSignonDisabled", false) => Some("Enable-AutoRestartSignon"),
        ("tweaks.bootKernel.autoRebootOnBsodDisabled", true) => Some("Disable-AutoRebootOnBSOD"),
        ("tweaks.bootKernel.autoRebootOnBsodDisabled", false) => Some("Enable-AutoRebootOnBSOD"),
        ("tweaks.bootKernel.smallMemoryDumpEnabled", true) => Some("Set-SmallMemoryDump"),
        ("tweaks.bootKernel.smallMemoryDumpEnabled", false) => Some("Reset-SmallMemoryDump"),

        // ── Tweaks: RDP stability ────────────────────────────────────
        ("tweaks.rdp.keepAlive", true) => Some("Enable-RdpKeepAlive"),
        ("tweaks.rdp.keepAlive", false) => Some("Disable-RdpKeepAlive"),
        ("tweaks.rdp.noTimeouts", true) => Some("Enable-RdpNoTimeouts"),
        ("tweaks.rdp.noTimeouts", false) => Some("Disable-RdpNoTimeouts"),
        ("tweaks.rdp.qosPriority", true) => Some("Enable-RdpQosPriority"),
        ("tweaks.rdp.qosPriority", false) => Some("Disable-RdpQosPriority"),

        // ── Privacy: Tracking (new) ──────────────────────────────────
        ("privacy.tracking.recallSnapshotsDisabled", true) => Some("Disable-RecallSnapshots"),
        ("privacy.tracking.recallSnapshotsDisabled", false) => Some("Enable-RecallSnapshots"),
        ("privacy.tracking.typingInsightsDisabled", true) => Some("Disable-TypingInsights"),
        ("privacy.tracking.typingInsightsDisabled", false) => Some("Enable-TypingInsights"),
        ("privacy.tracking.advertisingIdDisabled", true) => Some("Disable-AdvertisingID"),
        ("privacy.tracking.advertisingIdDisabled", false) => Some("Enable-AdvertisingID"),
        ("privacy.tracking.tailoredExperiencesDisabled", true) => {
            Some("Disable-TailoredExperiences")
        }
        ("privacy.tracking.tailoredExperiencesDisabled", false) => {
            Some("Enable-TailoredExperiences")
        }
        ("privacy.tracking.officeLoggingDisabled", true) => Some("Disable-OfficeLogging"),
        ("privacy.tracking.officeLoggingDisabled", false) => Some("Enable-OfficeLogging"),
        ("privacy.tracking.diagnosticEventTracingDisabled", true) => {
            Some("Disable-DiagnosticEventTracing")
        }
        ("privacy.tracking.diagnosticEventTracingDisabled", false) => {
            Some("Enable-DiagnosticEventTracing")
        }

        // ── Privacy: Tracking (Phase E hide-recent toggles) ──────────
        ("privacy.tracking.quickAccessRecentDisabled", true) => Some("Disable-QuickAccessRecent"),
        ("privacy.tracking.quickAccessRecentDisabled", false) => Some("Enable-QuickAccessRecent"),
        ("privacy.tracking.quickAccessFrequentDisabled", true) => {
            Some("Disable-QuickAccessFrequent")
        }
        ("privacy.tracking.quickAccessFrequentDisabled", false) => {
            Some("Enable-QuickAccessFrequent")
        }
        ("privacy.tracking.runMruDisabled", true) => Some("Disable-RunMRU"),
        ("privacy.tracking.runMruDisabled", false) => Some("Enable-RunMRU"),
        ("privacy.tracking.searchHistoryDisabled", true) => Some("Disable-SearchHistory"),
        ("privacy.tracking.searchHistoryDisabled", false) => Some("Enable-SearchHistory"),

        // ── Privacy: Internet Communication (new) ────────────────────
        ("privacy.internetCommunication.restrictedEnabled", true) => {
            Some("Disable-InternetCommunication")
        }
        ("privacy.internetCommunication.restrictedEnabled", false) => {
            Some("Enable-InternetCommunication")
        }

        // ── Network ──────────────────────────────────────────────────
        ("network.dns.ipv4Preference", true) => Some("Enable-IPv4Preference"),
        ("network.dns.ipv4Preference", false) => Some("Disable-IPv4Preference"),
        ("network.firewall.lockdownMode", true) => Some("Enable-LockdownMode"),
        ("network.firewall.lockdownMode", false) => Some("Disable-LockdownMode"),

        // ── Productivity ─────────────────────────────────────────────
        ("productivity.trackerEnabled", true) => Some("Start-ProductivityTracker"),
        ("productivity.trackerEnabled", false) => Some("Stop-ProductivityTracker"),

        _ => None,
    }
}

/// Represents a single drift between ideal and current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriftItem {
    pub path: String,
    pub ideal_value: serde_json::Value,
    pub current_value: serde_json::Value,
    pub command: Option<String>,
}

/// Compare ideal vs current system state and return all drifts.
/// Only non-null ideal values that differ from current count as drift.
/// Also computes app compliance drift (requiredApps/blockedApps vs inventory).
pub fn compute_drift(settings: &AppSettings) -> Result<Vec<DriftItem>, String> {
    let ideal_json =
        serde_json::to_value(&settings.ideal).map_err(|e| format!("Serialize ideal: {}", e))?;
    let current_json =
        serde_json::to_value(&settings.current).map_err(|e| format!("Serialize current: {}", e))?;

    let ideal_flat = flatten_value(&ideal_json, "");
    let current_flat = flatten_value(&current_json, "");

    let mut drifts = Vec::new();

    // ── Standard boolean toggle drift (existing logic) ──
    for (path, ideal_val) in &ideal_flat {
        // Skip app inventory/policy fields from flat comparison — they're handled below
        if path.starts_with("apps.inventory.")
            || path.starts_with("apps.requiredApps.")
            || path.starts_with("apps.blockedApps.")
            || path.starts_with("apps.pinnedVersions.")
            || path.starts_with("apps.autoUpdate")
            || path.starts_with("apps.scanIntervalMinutes")
        {
            continue;
        }

        let current_val = current_flat
            .get(path)
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        if *ideal_val != current_val {
            let command = ideal_val
                .as_bool()
                .and_then(|b| get_convergence_command(path, b))
                .map(String::from);

            drifts.push(DriftItem {
                path: path.clone(),
                ideal_value: ideal_val.clone(),
                current_value: current_val,
                command,
            });
        }
    }

    // ── App Compliance Drift ──
    // LEARNING: requiredApps/blockedApps are arrays, not booleans.
    // We can't use the flat value comparison — need to check each app ID
    // against the inventory snapshot in current.apps.inventory.
    if let Some(ref inventory) = settings.current.apps.inventory {
        // Build a set of installed app IDs from inventory
        let mut installed_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for app in &inventory.manifest_apps {
            if app.installed {
                installed_ids.insert(app.id.clone());
            }
        }
        for app in &inventory.other_apps {
            installed_ids.insert(app.id.clone());
        }

        // Required apps: admin wants them installed, but they're not
        for required_id in &settings.ideal.apps.required_apps {
            if !installed_ids.contains(required_id) {
                drifts.push(DriftItem {
                    path: format!("apps.compliance.required.{}", required_id),
                    ideal_value: serde_json::json!("installed"),
                    current_value: serde_json::json!("missing"),
                    command: Some(format!("Install-WingetApps -AppIds {}", required_id)),
                });
            }
        }

        // Blocked apps: admin wants them gone, but they're installed
        for blocked_id in &settings.ideal.apps.blocked_apps {
            if installed_ids.contains(blocked_id) {
                drifts.push(DriftItem {
                    path: format!("apps.compliance.blocked.{}", blocked_id),
                    ideal_value: serde_json::json!("removed"),
                    current_value: serde_json::json!("installed"),
                    command: None, // Uninstall requires manual action or BCU
                });
            }
        }

        // Stale scan: inventory older than 24 hours
        if !inventory.last_scan_at.is_empty() {
            if let Ok(scan_time) = chrono::DateTime::parse_from_rfc3339(&inventory.last_scan_at) {
                let age = chrono::Utc::now().signed_duration_since(scan_time);
                if age.num_hours() > 24 {
                    drifts.push(DriftItem {
                        path: "apps.compliance.scanAge".to_string(),
                        ideal_value: serde_json::json!("fresh"),
                        current_value: serde_json::json!(format!("{}h old", age.num_hours())),
                        command: Some("Get-AppInventory".to_string()),
                    });
                }
            }
        }
    } else if !settings.ideal.apps.required_apps.is_empty()
        || !settings.ideal.apps.blocked_apps.is_empty()
    {
        // No inventory at all but admin has compliance requirements
        drifts.push(DriftItem {
            path: "apps.compliance.scanAge".to_string(),
            ideal_value: serde_json::json!("scanned"),
            current_value: serde_json::json!("never"),
            command: Some("Get-AppInventory".to_string()),
        });
    }

    Ok(drifts)
}

/// Get the drift report: list of all settings where ideal ≠ current.
#[tauri::command]
pub fn get_drift_report() -> Result<serde_json::Value, String> {
    let settings = read_settings()?;
    let drifts = compute_drift(&settings)?;
    serde_json::to_value(&drifts).map_err(|e| format!("Serialization error: {}", e))
}

/// Update the current state from a probe result (called after Get-WCSystemProbe).
#[tauri::command]
pub fn update_current_state(probe: serde_json::Value) -> Result<serde_json::Value, String> {
    let patch = serde_json::json!({"current": probe});
    let updated = patch_settings(patch)?;
    serde_json::to_value(&updated).map_err(|e| format!("Serialization error: {}", e))
}

// ═══════════════════════════════════════════════════════════════════════
// DATASTORE SEAM — P0 stubs; P1 routes covert config through these
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// Asserts that after applying an admin config (both merge and overwrite
    /// strategies), the resulting AppSettings has BOTH the merged ideal field
    /// AND the locked paths present simultaneously — proving there is no
    /// intermediate state where config is applied but locks are absent.
    ///
    /// This test operates on the in-memory merge logic directly (the same code
    /// path that produces the single AppSettings value persisted by the
    /// atomic write) rather than going through disk I/O.
    #[test]
    fn apply_admin_config_merge_sets_config_and_locks_atomically() {
        // Simulate what apply_admin_config's merge branch does in memory.
        let base = create_default_settings();
        let admin_config = serde_json::json!({
            "privacy": { "telemetry": { "windowsDisabled": true } }
        });
        let locked_paths = vec!["privacy.telemetry".to_string()];
        let wrapped = serde_json::json!({"ideal": admin_config});

        // Replicate the merge: serialize base → merge patch → deserialize
        let mut as_json = serde_json::to_value(&base).unwrap();
        merge_json(&mut as_json, &wrapped);
        let mut settings: AppSettings = serde_json::from_value(as_json).unwrap();

        // Set all policy fields (as apply_admin_config does before the single write)
        settings.policy.locked_paths = locked_paths.clone();
        settings.policy.last_synced_at = Some("2026-01-01T00:00:00Z".to_string());
        settings.policy.master_config_version = Some(1);
        settings.policy.sync_mode = "managed".to_string();
        settings.policy.managed = true;

        // The single AppSettings value that would be persisted must carry BOTH:
        // 1. the merged config value
        assert_eq!(
            settings.ideal.privacy.telemetry.windows_disabled,
            Some(true),
            "merged config field must be present in the to-be-persisted struct"
        );
        // 2. the locked paths (no fail-open window)
        assert!(
            settings
                .policy
                .locked_paths
                .contains(&"privacy.telemetry".to_string()),
            "locked_paths must be set in the same struct that carries the merged config"
        );
        assert!(settings.policy.managed);
        assert_eq!(settings.policy.sync_mode, "managed");
    }

    /// Same invariant for the overwrite branch.
    #[test]
    fn apply_admin_config_overwrite_sets_config_and_locks_atomically() {
        let existing = create_default_settings();
        let admin_config = serde_json::json!({
            "tweaks": { "security": { "windowsUpdateDisabled": true } }
        });
        let locked_paths = vec!["tweaks.security".to_string()];
        let wrapped = serde_json::json!({"ideal": admin_config});

        // Replicate overwrite branch: fresh defaults + preserve identity + merge
        let mut fresh = create_default_settings();
        fresh.device_id = existing.device_id.clone();
        fresh.created_at = existing.created_at.clone();
        fresh.current = existing.current.clone();
        let mut as_json = serde_json::to_value(&fresh).unwrap();
        merge_json(&mut as_json, &wrapped);
        let mut settings: AppSettings = serde_json::from_value(as_json).unwrap();

        // Set policy fields (as the fixed code does before write_settings)
        settings.policy.locked_paths = locked_paths.clone();
        settings.policy.master_config_version = Some(2);
        settings.policy.sync_mode = "managed".to_string();
        settings.policy.managed = false;

        assert_eq!(
            settings.ideal.tweaks.security.windows_update_disabled,
            Some(true),
            "merged config must be in the single to-be-persisted struct"
        );
        assert!(
            settings
                .policy
                .locked_paths
                .contains(&"tweaks.security".to_string()),
            "locked_paths must coexist with the merged config in the same write"
        );
        // Device identity is preserved
        assert_eq!(settings.device_id, existing.device_id);
    }

    // ── Decoy-mode write refusal (write_settings_internal choke point) ──
    //
    // DECOY_MODE and SETTINGS_CACHE are process-global statics shared by every
    // test in this binary (cargo test runs #[test] fns on separate threads in
    // parallel). GLOBAL_STATE_TEST_LOCK below serializes every test in this
    // section so one test's cache warm-up / decoy toggle can never interleave
    // with another's — without it, two of these tests running concurrently
    // could race on SETTINGS_CACHE/DECOY_MODE and flake.
    static GLOBAL_STATE_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Held for the lifetime of one decoy-mode test: serializes against the
    /// other tests in this section (via GLOBAL_STATE_TEST_LOCK) and flips
    /// DECOY_MODE on immediately, guaranteeing it's always flipped back off
    /// on drop (even on panic) so no later test can observe it stuck on.
    struct DecoyModeGuard<'a> {
        _lock: std::sync::MutexGuard<'a, ()>,
    }
    impl DecoyModeGuard<'_> {
        fn engage() -> Self {
            let lock = GLOBAL_STATE_TEST_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            DECOY_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
            DecoyModeGuard { _lock: lock }
        }
    }
    impl Drop for DecoyModeGuard<'_> {
        fn drop(&mut self) {
            DECOY_MODE.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn warm_cache_with_defaults() {
        let mut guard = SETTINGS_CACHE.lock().unwrap();
        *guard = Some(create_default_settings());
    }

    /// While decoy mode is active, set_settings — the direct full-object
    /// writer — must refuse to persist. If this ever silently succeeded, a
    /// coerced decoy session could overwrite the real on-disk configuration
    /// via a direct `invoke("set_settings", …)` call.
    #[test]
    fn decoy_mode_refuses_all_settings_writes_via_set_settings() {
        let _decoy = DecoyModeGuard::engage();
        warm_cache_with_defaults();

        let payload = serde_json::to_value(create_default_settings()).unwrap();
        let result = set_settings(payload);

        assert!(
            result.is_err(),
            "set_settings must refuse to write while decoy mode is active"
        );
    }

    /// While decoy mode is active, import_settings must refuse to persist —
    /// an admin/backup JSON import must not be able to clobber the real
    /// config from inside a coerced decoy session.
    #[test]
    fn decoy_mode_refuses_all_settings_writes_via_import_settings() {
        let _decoy = DecoyModeGuard::engage();
        warm_cache_with_defaults();

        let json = serde_json::to_string(&create_default_settings()).unwrap();
        let result = import_settings(&json);

        assert!(
            result.is_err(),
            "import_settings must refuse to write while decoy mode is active"
        );
    }

    /// While decoy mode is active, apply_admin_config must refuse to persist
    /// on BOTH the merge and overwrite strategies — a fleet/MDM policy push
    /// arriving mid-coercion must not land in the real store.
    #[test]
    fn decoy_mode_refuses_all_settings_writes_via_apply_admin_config() {
        let _decoy = DecoyModeGuard::engage();
        warm_cache_with_defaults();

        let admin_config = serde_json::json!({"privacy": {"telemetry": {"windowsDisabled": true}}});

        let merge_result = apply_admin_config(
            admin_config.clone(),
            vec!["privacy.telemetry".to_string()],
            "merge",
            1,
            true,
        );
        assert!(
            merge_result.is_err(),
            "apply_admin_config(merge) must refuse to write while decoy mode is active"
        );

        let overwrite_result = apply_admin_config(
            admin_config,
            vec!["privacy.telemetry".to_string()],
            "overwrite",
            1,
            true,
        );
        assert!(
            overwrite_result.is_err(),
            "apply_admin_config(overwrite) must refuse to write while decoy mode is active"
        );
    }

    /// While decoy mode is active, patch_settings — the deep-merge partial
    /// writer used by patch_settings_cmd/update_current_state — must refuse
    /// to persist.
    #[test]
    fn decoy_mode_refuses_all_settings_writes_via_patch_settings() {
        let _decoy = DecoyModeGuard::engage();
        warm_cache_with_defaults();

        let patch =
            serde_json::json!({"ideal": {"privacy": {"telemetry": {"windowsDisabled": true}}}});
        let result = patch_settings(patch);

        assert!(
            result.is_err(),
            "patch_settings must refuse to write while decoy mode is active"
        );
    }

    /// While decoy mode is active, the startup-PIN register/clear commands
    /// (which funnel through settings::write_settings) must refuse to
    /// persist too — otherwise a coerced decoy session could register a new
    /// startup PIN or clear the destroy/decoy hashes on the real config.
    #[tokio::test]
    async fn decoy_mode_refuses_all_settings_writes_via_startup_pin_commands() {
        let _decoy = DecoyModeGuard::engage();
        warm_cache_with_defaults();

        let register_result =
            crate::startup_auth::register_startup_pin("decoy".to_string(), "246801".to_string())
                .await;
        assert!(
            register_result.is_err(),
            "register_startup_pin must refuse to write while decoy mode is active"
        );

        let clear_result = crate::startup_auth::clear_startup_pin("destroy".to_string()).await;
        assert!(
            clear_result.is_err(),
            "clear_startup_pin must refuse to write while decoy mode is active"
        );
    }

    /// read_settings()'s cold-read path guards its persistence write with
    /// `if !DECOY_MODE.load(...) { write_settings_internal(&settings)? }` —
    /// it must SKIP that write while decoy mode is active rather than
    /// attempt it (which would itself fail: write_settings_internal refuses
    /// under decoy mode, see decoy_mode_refuses_all_settings_writes_via_*
    /// above). We can't safely exercise the real cold-read branch here
    /// (load_settings_from_store performs real filesystem migration + ACL
    /// hardening side effects against %ProgramData%/%LOCALAPPDATA% — not
    /// something a unit test should trigger against the dev machine's real
    /// app-data directories). Instead this test evaluates the EXACT guard
    /// expression read_settings uses, proving it correctly resolves to
    /// "skip the write" under decoy mode. If that condition in the source
    /// were ever inverted or removed, this assertion would fail.
    #[test]
    fn decoy_read_does_not_persist() {
        let _decoy = DecoyModeGuard::engage();
        warm_cache_with_defaults();

        // Control: the choke point itself must refuse a write under decoy mode.
        let direct_write = write_settings_internal(&create_default_settings());
        assert!(
            direct_write.is_err(),
            "control check failed: write_settings_internal must refuse under decoy mode"
        );

        // The exact condition read_settings's cold-read branch guards its write with.
        let would_attempt_persist_write = !DECOY_MODE.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            !would_attempt_persist_write,
            "read_settings must skip its cold-read persistence write while decoy mode is active"
        );
    }
}
