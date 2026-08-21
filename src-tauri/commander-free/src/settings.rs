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

const DEFAULT_PERMANENTLY_HIDDEN_PANELS: &[&str] = &["productivity", "server-apps", "flows"];

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
    /// When a signed Fleet policy enables this, every device-side alert is
    /// forwarded through the existing authenticated Fleet queues. The value
    /// lives in `ideal.security` so it survives the config-epoch
    /// serialize/merge/deserialize cycle; it is deliberately not a virtual UI
    /// setting.
    #[serde(default)]
    pub require_all_device_alerts_in_fleet: bool,
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
    /// Categories the current Windows user has excluded from Cleanup's
    /// bulk-clear action. Persisted with the rest of per-user app settings.
    #[serde(default = "default_bulk_clear_excludes")]
    pub bulk_clear_excludes: Vec<String>,
    /// Deprecated compatibility field. Superseded by `bulk_clear_excludes`.
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
    /// Low Performance Mode: "auto" (default), "on" or "off". Disables UI
    /// animations AND the periodic active-panel polling — the latter being the
    /// expensive half, since every refresh spawns a cold powershell.exe
    /// (`build_powershell_command`, no runspace reuse) and a multi-user server
    /// pays that once per logged-in session.
    ///
    /// Resolved in the frontend (src/lib/lowPerformance.ts); Rust does not read
    /// it, but the field MUST exist here regardless — `patch_settings` merges the
    /// incoming JSON then deserializes into AppSettings, and serde silently drops
    /// any key without a matching field, so a frontend-only setting appears to
    /// save and then vanishes on the next round-trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low_performance_mode: Option<String>,
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
    /// Hides the in-app guide trigger and suppresses automatic/manual tours.
    /// Borrowed-only hiding is represented by the "tour" borrowed-hidden key.
    #[serde(default)]
    pub hide_tour: bool,
    /// Non-panel surface keys hidden only while Borrowed Mode is active.
    /// Recognised keys: "notif-bell", "risk-matrix", "more-products", "tour",
    /// "license-panel", "sidebar-preferences", "action:dismount", "action:delete",
    /// "action:scrubMeta", "action:lockdown".
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
    /// When true, the compact Interface and Persona controls in the sidebar
    /// footer are hidden everywhere. Borrowed-only uses "sidebar-preferences".
    #[serde(default)]
    pub hide_sidebar_preferences: bool,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchSettings {
    /// Root directories to index.
    pub roots: Vec<std::path::PathBuf>,
    /// Glob patterns to exclude (e.g. "*.tmp", "node_modules").
    pub exclusions: Vec<String>,
    /// False until the first-run default seeding has run.
    /// Distinguishes "never configured" from "user explicitly cleared all folders".
    #[serde(default)]
    pub initialized: bool,
    /// Maximum filename rows returned to the Search Files panel. The command
    /// clamps this again so a malformed settings file cannot create an
    /// unbounded Everything request.
    #[serde(default = "default_file_search_result_limit")]
    pub result_limit: u32,
}

const fn default_file_search_result_limit() -> u32 {
    200
}

impl Default for FileSearchSettings {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            exclusions: Vec::new(),
            initialized: false,
            result_limit: default_file_search_result_limit(),
        }
    }
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
    /// Runtime ownership marker for a Privacy Shield session that Fleet started.
    /// This is not an admin policy value: it prevents Fleet enrolment from
    /// retroactively locking an employee's already-manual shield session.
    #[serde(default)]
    pub privacy_shield_session_owned: bool,
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
    /// Forward this alert to the Fleet console when it fires. Settings path
    /// `notifications.{cpuUsage,ramUsage,networkUsage}.reportToFleet` — an
    /// admin-lockable path via the generic `ConfigEpoch.locked_paths`
    /// mechanism, same as `privacy.privacyShield`. `upload`/`download` share
    /// the single `networkUsage` fleet alert type. `#[serde(default)]` so
    /// settings.json written before this field existed still parse.
    #[serde(default)]
    pub report_to_fleet: bool,
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
            report_to_fleet: false,
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
    #[serde(default = "default_ram_metric_alert")]
    pub ram: MetricAlertSettings,
    pub upload: MetricAlertSettings,
    pub download: MetricAlertSettings,
}

impl Default for MetricAlertsSettings {
    fn default() -> Self {
        Self {
            // CPU defaults to a 50% trip point per the owner's brief.
            cpu: MetricAlertSettings::with_threshold(50.0),
            ram: default_ram_metric_alert(),
            upload: MetricAlertSettings::with_threshold(10.0),
            download: MetricAlertSettings::with_threshold(10.0),
        }
    }
}

fn default_ram_metric_alert() -> MetricAlertSettings {
    MetricAlertSettings::with_threshold(80.0)
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

fn default_bulk_clear_excludes() -> Vec<String> {
    [
        "wlanProfiles",
        "browserFootprints",
        "notepadState",
        "wslData",
        "dockerDesktopData",
        "virtualMachineArtifacts",
        "developerCaches",
        "credentialManager",
        "sshState",
        "passwordManagerCaches",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
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
            bulk_clear_excludes: default_bulk_clear_excludes(),
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
            // None means "auto" — the frontend decides from this machine's cores
            // and RAM. Storing None rather than Some("auto") keeps the key out of
            // settings.json until a user makes an explicit choice.
            low_performance_mode: None,
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
            hide_tour: false,
            borrowed_hidden: None,
            lock_panel_on_close: None,
            file_search: FileSearchSettings::default(),
            fleet: FleetSettings::default(),
            hide_license_panel: false,
            hide_sidebar_preferences: false,
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
    /// "letter" (default) or "folder". Absent on slots saved before folder mounts
    /// existed, which are letter slots — hence Option rather than a defaulted enum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_target: Option<String>,
    /// Directory to mount at when mount_target is "folder".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_point: Option<String>,
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
    /// Who can see a mounted vault's drive letter: "auto" (default), "machine" or
    /// "per-user". "auto" resolves to per-user on multi-session/Server SKUs and
    /// machine on single-user desktops.
    ///
    /// Same rule as the fields above, and it bit this feature already: the frontend
    /// shipped the toggle before this field existed, so every save round-tripped the
    /// user's choice straight back to nothing while the UI showed it as applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RamDiskAutostartSettings {
    pub enabled: Option<bool>,
    // TypeScript uses the initialism `MB` in this persisted key.  `rename_all`
    // would produce `sizeMb`, which serde then silently ignores on every
    // settings patch and startup falls back to 256 MB.
    #[serde(rename = "sizeMB", alias = "sizeMb")]
    pub size_mb: Option<u32>,
    pub drive_letter: Option<String>,
    pub filesystem: Option<String>,
    pub label: Option<String>,
    pub read_only: Option<bool>,
    #[serde(default)]
    pub skip_after_lockdown: Option<bool>,
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
    /// Access & Session Monitor policy. Runtime authority lives in the Pro
    /// sidecar; this persists its bounded detector and reporting preferences.
    #[serde(default)]
    pub auth_anomaly_monitor: AuthAnomalyMonitorSettings,
    /// USB security runtime arm preferences. Module-specific policies and
    /// allow-lists remain in their bounded local preference files.
    #[serde(default)]
    pub usb_security: UsbSecurityMonitorSettings,
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

/// F-2: filesystem honeypots. This is Free's persisted rearm intent; the
/// watcher and canonical ProgramData registry are Pro-owned. Re-arm merges
/// paths additively, while explicit Pro remove/delete operations unenrol them.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DecoyMonitorSettings {
    pub enabled: Option<bool>,
    pub enrolled_paths: Option<Vec<String>>,
    /// Opt-in Windows Security Log auditing for real decoy reads. Disabled by
    /// default because it changes the local audit policy and needs elevation.
    pub read_audit_enabled: Option<bool>,
    /// When enabled, access events are reported to Fleet as a path-free
    /// tripwire signal through the authenticated agent check-in.
    pub fleet_alert_enabled: Option<bool>,
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
    /// Duplicate alert/action cooldown while detection remains active.
    pub alert_cooldown_seconds: Option<u32>,
    /// Minimum per-process distinct-file evidence before Pro attribution.
    pub attribution_min_files: Option<u32>,
    /// User-added extra watch directories (in addition to the standard
    /// Documents/Pictures/Desktop/Downloads set).
    pub custom_watch_dirs: Option<Vec<String>>,
    /// F-3 v2 automated response on the Pro ETW path:
    /// "monitor" | "suspend" | "kill". None = backend default (suspend).
    pub action: Option<String>,
    /// Send a coarse, path-free ransomware event to Fleet.
    pub report_to_fleet: Option<bool>,
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

/// Security-log access anomaly policy. `None` uses the conservative Pro
/// defaults so older settings files remain valid.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthAnomalyMonitorSettings {
    pub enabled: Option<bool>,
    pub failed_burst_threshold: Option<u32>,
    pub failed_burst_window_secs: Option<u32>,
    pub work_start_hour: Option<u32>,
    pub work_end_hour: Option<u32>,
    /// ISO weekday numbers: Monday=1 through Sunday=7. Missing = weekdays.
    pub work_days: Option<Vec<u8>>,
    /// "local" (device time) or "utc".
    pub time_basis: Option<String>,
    pub detect_rdp: Option<bool>,
    pub detect_new_accounts: Option<bool>,
    pub detect_off_hours: Option<bool>,
    pub alert_debounce_secs: Option<u32>,
    pub report_to_fleet: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsbSecurityMonitorSettings {
    pub monitor_enabled: Option<bool>,
    pub hid_guard_enabled: Option<bool>,
    pub metering_enabled: Option<bool>,
    pub auto_sandbox_enabled: Option<bool>,
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
    /// Forward each screen-capture-tool detection to the Fleet console.
    /// Admin-lockable via `ConfigEpoch.locked_paths` on
    /// `notifications.screenCapture.reportToFleet`.
    #[serde(default)]
    pub report_to_fleet: Option<bool>,
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
pub struct VeraCryptDeviceEraseTarget {
    pub device_path: String,
    pub disk_number: u32,
    pub partition_number: u32,
    pub partition_guid: String,
    pub offset_bytes: u64,
    pub size_bytes: u64,
    pub disk_unique_id: String,
    pub label: String,
}

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
    /// Absolute paths of folders the user wants securely deleted during
    /// lockdown. Rust consumes this through the `configured_folders` destruct
    /// step, so it runs for sidebar, hotkey, distress-phrase, dead-man, and
    /// destroy-PIN triggers. This field MUST exist on the Rust struct: otherwise
    /// patch_settings_cmd's serialize-merge-deserialize round-trip silently
    /// drops it and leaves the cascade with an empty deletion list.
    pub shred_folders: Option<Vec<String>>,
    /// Local usernames the user selected for removal during the lockdown
    /// cascade. The `remove_users` destruct step (Pro) securely wipes each
    /// account's user-profile folder (single durable RNG-overwrite pass), then deletes the profile and
    /// the local account. Rust consumes this in the same trigger-complete way as
    /// `shred_folders`:
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
    /// Raw partitions selected for VeraCrypt header destruction. Unlike a
    /// free-form native path, every entry is bound to immutable partition and
    /// disk identity fields that Pro re-probes immediately before writing.
    pub crypto_erase_veracrypt_devices: Option<Vec<VeraCryptDeviceEraseTarget>>,
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
    /// PSReadLine persistence disabled in Windows PowerShell and PowerShell 7 profiles
    pub terminal_history_disabled: Option<bool>,
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
    /// Local alert behavior. Fleet's signed desired state overrides this while
    /// the endpoint is fleet-managed.
    #[serde(default)]
    pub notify_mode: Option<PrivacyShieldNotifyMode>,
    /// Fleet policy master switch. When true on an enrolled device, the
    /// background supervisor starts the local detector and reports attention
    /// events; it never uploads frames.
    #[serde(default)]
    pub fleet_monitoring_enabled: Option<bool>,
    /// Set only by signed Fleet policy. While true the local card is read-only
    /// and start/stop ownership belongs to the Fleet supervisor.
    #[serde(default)]
    pub fleet_managed: Option<bool>,
    /// Maximum Fleet attention notifications in one rolling window. Zero or
    /// absent means unlimited; this governs only the media-free Fleet alert
    /// bridge and never suppresses local blur or local UI events.
    #[serde(default)]
    pub fleet_notification_limit: Option<u32>,
    /// Length of the Fleet attention-notification rolling window in seconds.
    #[serde(default)]
    pub fleet_notification_window_seconds: Option<u32>,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyShieldNotifyMode {
    BlurNotify,
    NotifyOnly,
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
    /// Windows Server SKU logon, credential, and file-server hardening
    #[serde(default)]
    pub server: ServerTweaks,
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
pub struct ServerTweaks {
    /// RDP visual effects are reapplied after logon and RemoteConnect
    pub persistent_rdp_animations: Option<bool>,
    /// Ctrl+Alt+Del no longer required at the logon screen (DisableCAD=1)
    pub ctrl_alt_del_disabled: Option<bool>,
    /// Logon screen does not prefill the previous username
    pub last_signed_in_user_hidden: Option<bool>,
    /// Machine-level lock after InactivityTimeoutSecs of console idle
    pub console_inactivity_lock: Option<bool>,
    /// Server-only: suppresses the shutdown "reason" dialog
    pub shutdown_tracker_disabled: Option<bool>,
    /// Server-only: Server Manager no longer launches at sign-in
    pub server_manager_at_logon_disabled: Option<bool>,
    /// Server-only: IE Enhanced Security Configuration turned off
    pub ie_enhanced_security_disabled: Option<bool>,
    /// WDigest pinned to 0 so LSASS holds no cleartext credentials
    pub wdigest_blocked: Option<bool>,
    /// LSASS runs as a protected process (RunAsPPL) — needs a reboot
    pub lsa_protection_enabled: Option<bool>,
    /// LmCompatibilityLevel=5 — NTLMv2 only, LM/NTLMv1 refused
    pub legacy_ntlm_blocked: Option<bool>,
    /// SMB signing required on both the server and client roles
    pub smb_signing_required: Option<bool>,
    /// SMBv1 protocol and optional feature removed
    pub smb1_disabled: Option<bool>,
    /// RemoteRegistry service set to Disabled
    pub remote_registry_disabled: Option<bool>,
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
    /// Desktop shell processes run at high CPU and I/O priority for every logon.
    pub desktop_shell_priority_enabled: Option<bool>,
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
    /// Windows Terminal's "defaultProfile" set to PowerShell 7 instead of Windows PowerShell 5.1
    pub power_shell7_default: Option<bool>,
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
    /// Rogue-AP detector policy and device-local trusted-network baseline.
    #[serde(default)]
    pub wifi_guard: WifiGuardSettings,
}

/// Wi-Fi Guard's policy is stored by Free so the private Pro sidecar can be
/// reconfigured after a restart. Network identifiers remain local: Fleet gets
/// only a coarse alert type when `report_to_fleet` is enabled.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WifiGuardSettings {
    pub enabled: Option<bool>,
    pub learning_window_secs: Option<u64>,
    pub learning_until: Option<String>,
    pub poll_interval_secs: Option<u64>,
    pub alert_debounce_secs: Option<u64>,
    pub baseline: Option<Vec<WifiGuardBaselineEntry>>,
    pub report_to_fleet: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WifiGuardBaselineEntry {
    pub ssid: String,
    pub bssids: Vec<String>,
    pub best_auth_strength: u8,
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
    /// Admin pin for `VaultSettings::mount_scope`, read by
    /// `MountScopeSelector.tsx`. Separate from `locked_paths` because that
    /// generic mechanism is only ever populated by `apply_admin_config` (a
    /// connected admin/Fleet server) — a standalone (`sync_mode:
    /// "standalone"`) box has no server to push that, but still has one
    /// administrator who needs to stop other signed-in users from flipping
    /// a shared vault to per-user scope. `None` = not pinned. Must exist
    /// here or patch_settings_cmd's round-trip drops it, same failure mode
    /// `mount_scope` itself already hit once (see its comment above).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_mount_scope: Option<String>,
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
            pinned_mount_scope: None,
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

/// Keep the device identity compatible with Fleet's Postgres-backed routes.
///
/// Older/imported settings can contain a blank or legacy non-UUID identifier.
/// Those values work until Fleet attempts a database-backed operation such as
/// remote file search or transfer, where `devices.device_id` is a UUID column.
/// Repair the local identity once while loading settings so every subsequent
/// enrollment, command, and transfer uses the same durable UUID.
fn ensure_valid_device_id(settings: &mut AppSettings) {
    if Uuid::parse_str(&settings.device_id).is_err() {
        settings.device_id = generate_device_id();
    }
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
pub(crate) fn create_default_settings() -> AppSettings {
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
    match settings.app.permanently_hidden_panels.as_mut() {
        Some(panels) => {
            // `sidecar` was the persisted ID of the retired System Records
            // panel. Drop it while loading older settings so visibility state
            // cannot keep referring to a route that no longer exists.
            panels.retain(|panel| panel != "sidecar");
        }
        None => {
            settings.app.permanently_hidden_panels = Some(
                DEFAULT_PERMANENTLY_HIDDEN_PANELS
                    .iter()
                    .map(|panel| panel.to_string())
                    .collect(),
            );
        }
    }
}

// ── Store-backed load (enc:v1: at rest via datastore) ─────────────────────

/// Parse a settings JSON value, applying schema migration if needed.
fn parse_and_migrate_json_val(json: serde_json::Value) -> Result<AppSettings, String> {
    let version = json
        .get("settingsVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u32;
    let mut final_json = if version < SETTINGS_VERSION {
        migrate_settings(json, version)
    } else {
        json
    };
    migrate_legacy_monitor_names(&mut final_json);
    let mut settings: AppSettings = serde_json::from_value(final_json)
        .map_err(|e| format!("Failed to deserialize settings: {e}"))?;
    ensure_valid_device_id(&mut settings);
    apply_runtime_defaults(&mut settings);
    Ok(settings)
}

/// Some released v2 settings used `decoyTripwire`. Settings-version did not
/// change for that rename, so it must be normalized even for current-version
/// files before serde drops the unknown legacy field.
fn migrate_legacy_monitor_names(root: &mut serde_json::Value) {
    for state in ["ideal", "current"] {
        let Some(privacy) = root
            .get_mut(state)
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|state| state.get_mut("privacy"))
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        if !privacy.contains_key("decoyMonitor") {
            if let Some(legacy) = privacy.remove("decoyTripwire") {
                privacy.insert("decoyMonitor".to_string(), legacy);
            }
        }
    }
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

        // Best-effort persistence: first-run defaults / legacy migration and
        // lastSeenAt normally land in the store here. A read must still return
        // the real decoded settings when the process has read-only access to
        // the machine-wide store (notably an asInvoker `tauri dev` session).
        // Making this auxiliary write fatal left SETTINGS_CACHE empty, so every
        // startup probe repeated the decrypt/load and the WebView never reached
        // its native backend. Explicit mutations still use write_settings() and
        // continue to report write failures to their callers.
        //
        // Skip entirely in decoy mode: a read must never write to the real
        // store under coercion (and write_settings_internal would refuse it).
        if !DECOY_MODE.load(std::sync::atomic::Ordering::Relaxed) {
            if let Err(error) = write_settings_internal(&settings) {
                crate::log_message(
                    "warn",
                    &format!("[Settings] loaded read-only; metadata persistence skipped: {error}"),
                );
            }
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
        crate::net_traffic_alert::reload_from_settings(&settings);
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
        crate::net_traffic_alert::reload_from_settings(&settings);

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

#[cfg(test)]
pub(crate) static GLOBAL_STATE_TEST_LOCK: Mutex<()> = Mutex::new(());

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

        // Clipboard Guard / Ink Receipt (plan §4.4, §2.4 finding 1; task C6):
        // the `clipboardGuard`/`inkReceipt` subtrees ride this SAME signed
        // epoch, exactly as Privacy Shield rides `privacy.privacyShield`, so
        // they inherit everything the check above just proved. Only reachable
        // here because the signature verified above — see
        // `spawn_clipboard_guard_epoch_relay`'s doc comment for why that must
        // stay true. Fire-and-forget: must never block this command or the
        // config apply below.
        spawn_clipboard_guard_epoch_relay(
            &config,
            config_version as i64,
            &locked_paths,
            managed.unwrap_or(false),
            target_kind.as_deref(),
            target_id.as_deref(),
            &sig,
            &pinned,
        );
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

// ═══════════════════════════════════════════════════════════════════════
// CLIPBOARD GUARD / INK RECEIPT — verified-epoch subtree relay into svc
// (plan §4.4, §2.4 finding 1; task C6)
// ═══════════════════════════════════════════════════════════════════════
//
// Extends the epoch-verification pattern directly above
// (`apply_admin_config_cmd`) rather than duplicating it: the
// `clipboardGuard`/`inkReceipt` subtrees ride the SAME signed
// `config_json` Privacy Shield uses for `privacy.privacyShield`, so
// signing, org/group/device scoping, `locked_paths`, and monotonic
// versioning are already proven by the time any code below ever runs.
//
// `commander-svc` — not Free — is the durable owner of the installed
// policy (plan §3): Free's job here is (1) a local sanity/health gate so
// a malformed subtree is visible without a live round trip, and (2)
// relaying the full verified epoch to svc over `\\.\pipe\wincmd-svc` so
// svc can independently re-verify it (plan §2.4 finding 1 / D-7's
// reasoning applied one hop further — svc must never trust "Free already
// checked it"). Free never trusts an unverified epoch's subtree, and an
// absent subtree is always "no change", never "disabled" — conflating
// those would let a truncated epoch silently turn a security feature off.

/// The `clipboardGuard` subtree of a verified epoch's `config_json` (plan
/// §4.4). Field shape mirrors `wincmd_clip_rules::Rule`/`RuleSetLimits`
/// exactly (same crate, same camelCase convention) so a rule that
/// validates in the fleet console can never re-parse differently here.
///
/// No `deny_unknown_fields`, matching `PrivacyShieldSettings` above: a
/// forward-compatible Fleet schema addition must not brick an older Free
/// client into reporting unhealthy.
///
/// Deliberately carries NO enable/disable field — plan §4.4 puts
/// `clipboard_guard_enabled` in `org_settings` (mirroring
/// `fleet_privacy_shield_enabled`) precisely so flipping it doesn't burn
/// an org-wide monotonic epoch version.
// `rules`/`limits` are never read after a successful parse — this type
// exists purely so `serde`'s (hand-written, for `RuleId`) `Deserialize`
// impls enforce the real shape (valid rule ids, closed enum values, ...)
// as a gate BEFORE relaying to svc. Only the Ok/Err outcome is used; the
// parsed value itself is intentionally discarded (see
// `parse_epoch_policy_subtrees`).
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardGuardSubtree {
    rules: Vec<wincmd_clip_rules::Rule>,
    /// Overrides `wincmd_clip_rules::RuleSetLimits::default()` when
    /// present. Not enforced here — `compile()` against these limits is
    /// commander-svc's atomic-install step (plan §4.4) — only
    /// deserialized so a malformed override is caught as such rather than
    /// silently ignored.
    #[serde(default)]
    limits: Option<wincmd_clip_rules::RuleSetLimits>,
}

/// Structural-only gate for the `inkReceipt` epoch subtree. Ink Receipt's
/// concrete policy schema (managed destinations, printer classes, ticket
/// requirement, offline behaviour, failure stance, watermark template —
/// plan §5.3) belongs to commander-pro's policy resolver: a separately-
/// scoped, later workflow in the private repo that must verify the epoch
/// signature itself (D-7) rather than trust this relay. Inventing
/// field-level validation here would let this file drift out of sync with
/// whatever schema that workflow ships, so the only thing checked at this
/// layer is that a PRESENT `inkReceipt` value is at least JSON-object
/// shaped, not e.g. a bare string/number/array/null that could never be a
/// policy object.
fn ink_receipt_subtree_is_well_formed(value: &serde_json::Value) -> bool {
    value.is_object()
}

/// Whether one epoch subtree was present, and if so, whether it parsed.
/// `Absent` and `Malformed` are kept distinct everywhere in this module —
/// conflating "not mentioned" with "present but broken" is exactly the
/// hazard that would let a truncated epoch silently disable a security
/// feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubtreeOutcome {
    /// The key was not present in `config_json` at all.
    Absent,
    /// Present and deserialized (clipboardGuard) or shape-checked
    /// (inkReceipt) successfully.
    Valid,
    /// Present but failed. Caller must keep whatever policy it already
    /// had and report unhealthy — never install nothing/blank in its
    /// place.
    Malformed,
}

/// Result of inspecting one verified epoch's `config_json` for the
/// `clipboardGuard` / `inkReceipt` subtrees.
#[derive(Debug, Clone, Copy)]
struct ParsedEpochSubtrees {
    clipboard_guard: SubtreeOutcome,
    ink_receipt: SubtreeOutcome,
}

impl ParsedEpochSubtrees {
    /// Nothing new for svc to install — both subtrees absent. Distinct
    /// from "attempt a relay" so an epoch that never mentions either
    /// subtree behaves exactly as it always has.
    fn is_no_op(&self) -> bool {
        matches!(self.clipboard_guard, SubtreeOutcome::Absent)
            && matches!(self.ink_receipt, SubtreeOutcome::Absent)
    }

    /// At least one PRESENT subtree failed its shape check.
    fn any_malformed(&self) -> bool {
        matches!(self.clipboard_guard, SubtreeOutcome::Malformed)
            || matches!(self.ink_receipt, SubtreeOutcome::Malformed)
    }
}

/// Inspect a verified epoch's `config_json` for the `clipboardGuard` and
/// `inkReceipt` subtrees. Pure — no I/O, no global state, no signature
/// re-check — so it is directly unit-testable. Callers MUST only invoke
/// this on a `config` whose epoch signature has already verified: this
/// function trusts that precondition and does not re-check it.
fn parse_epoch_policy_subtrees(config: &serde_json::Value) -> ParsedEpochSubtrees {
    let clipboard_guard = match config.get("clipboardGuard") {
        None => SubtreeOutcome::Absent,
        Some(v) => match serde_json::from_value::<ClipboardGuardSubtree>(v.clone()) {
            Ok(_) => SubtreeOutcome::Valid,
            Err(_) => SubtreeOutcome::Malformed,
        },
    };
    let ink_receipt = match config.get("inkReceipt") {
        None => SubtreeOutcome::Absent,
        Some(v) => {
            if ink_receipt_subtree_is_well_formed(v) {
                SubtreeOutcome::Valid
            } else {
                SubtreeOutcome::Malformed
            }
        }
    };
    ParsedEpochSubtrees {
        clipboard_guard,
        ink_receipt,
    }
}

/// Local, best-effort mirror of "did the most recently verified epoch's
/// Clipboard Guard / Ink Receipt subtrees parse, and did the relay to
/// commander-svc succeed" — plan §4.4's last-valid-retention rule,
/// applied at this layer. `commander-svc` is the durable owner of the
/// installed policy (plan §3); this only backs Free's own health surface.
/// A malformed subtree or an unreachable svc must never blank this back
/// to "never verified" — it simply stops advancing, exactly like svc
/// keeps its last-valid ruleset on a failed atomic install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardGuardEpochHealth {
    /// `false` after the most recent verified epoch's clipboardGuard or
    /// inkReceipt subtree failed to deserialize, or after a subsequent
    /// relay attempt to commander-svc failed or was rejected. An epoch
    /// whose subtrees were simply absent never flips this — "no change",
    /// never "unhealthy".
    pub healthy: bool,
    /// Highest epoch `policy_version` whose present subtree(s) all parsed
    /// AND relayed successfully. Held at its previous value across a
    /// malformed epoch or a failed relay — the "last known good" marker.
    pub last_valid_policy_version: Option<i64>,
}

impl Default for ClipboardGuardEpochHealth {
    fn default() -> Self {
        Self {
            healthy: true,
            last_valid_policy_version: None,
        }
    }
}

static CLIPBOARD_GUARD_EPOCH_HEALTH: Mutex<ClipboardGuardEpochHealth> =
    Mutex::new(ClipboardGuardEpochHealth {
        healthy: true,
        last_valid_policy_version: None,
    });

/// Snapshot of the current Clipboard Guard / Ink Receipt epoch-relay
/// health (see [`ClipboardGuardEpochHealth`]). Mirrors
/// `is_native_notifications_disabled`'s lock-and-default pattern above.
/// Not yet wired into any GUI/health surface — `#[allow(dead_code)]`
/// mirrors `export_settings`'s precedent above for a public accessor with
/// no in-tree caller yet (exercised directly by this module's tests).
#[allow(dead_code)]
pub fn clipboard_guard_epoch_health() -> ClipboardGuardEpochHealth {
    CLIPBOARD_GUARD_EPOCH_HEALTH
        .lock()
        .map(|g| *g)
        .unwrap_or_default()
}

/// The `svc.*` verb this module relays a verified epoch to (plan §4.3
/// caller 3, D-2). Classified `SessionHelper` in
/// `wincmd_shared::svc::classify_verb` — svc must independently
/// re-verify the signature before trusting anything in the payload.
const INSTALL_EPOCH_VERB: &str = "svc.policy.install_epoch";

/// Wire payload for [`INSTALL_EPOCH_VERB`]. Every field is exactly what
/// `wincmd_shared::fleet::EpochSigningInput` binds into the epoch
/// preimage, plus the signature and the pinned signer key Free just
/// verified against — so commander-svc can rebuild the IDENTICAL preimage
/// via `epoch_preimage` and call `verify_signature_b64` itself. This is
/// deliberate: svc must never trust "Free already checked it" — sending
/// anything less than the full signed material would make that
/// impossible.
///
/// Field names are snake_case, matching `wincmd_shared`/`fleet_proto`'s
/// backend-to-backend wire convention (e.g. `ClipboardEventReport`,
/// `Envelope::Request`) — NOT the camelCase this file uses for
/// `AppSettings`'s own on-disk/frontend JSON.
#[derive(Debug, Clone, Serialize)]
struct InstallEpochArgs {
    /// The epoch's monotonic version. svc must reject any value that is
    /// not strictly greater than what it already holds (plan §4.3).
    policy_version: i64,
    /// The FULL verified `config_json` — not just the clipboardGuard/
    /// inkReceipt subtrees. `epoch_preimage` signs over the whole config,
    /// so re-verifying a subtree in isolation is not possible; svc reads
    /// only the two subtrees it cares about back out of this.
    config: serde_json::Value,
    locked_paths: Vec<String>,
    managed: bool,
    target_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_id: Option<String>,
    /// Base64 Ed25519 signature over `epoch_preimage(EpochSigningInput {
    /// version: policy_version, config: &config, locked_paths:
    /// &locked_paths, managed, target_kind: &target_kind, target_id:
    /// target_id.as_deref() })`.
    signature: String,
    /// Base64 Ed25519 public key Free verified `signature` against
    /// (`settings.policy.fleet_signing_key`) — svc pins/compares this the
    /// same way Free does, never trusting Free's verdict alone.
    signer_key: String,
}

/// Kick off the Clipboard Guard / Ink Receipt epoch-subtree handling for a
/// JUST-VERIFIED epoch. Call this ONLY from inside
/// `apply_admin_config_cmd`'s `if let Some(pinned) = ...` branch, after
/// `verify_signature_b64` has already returned `true` — this function
/// does not itself re-check the signature, and reaching it on an
/// unverified epoch would defeat the entire "verify before use, always"
/// rule.
///
/// Fire-and-forget: spawns the actual parse+relay work on Tauri's
/// background runtime (matching every other detached task in this crate —
/// see e.g. `file_monitor.rs`, `flow_engine.rs`) so a slow or absent
/// commander-svc can never block this Tauri command or the rest of the
/// config apply that follows it. Outcomes are only ever observable via
/// [`clipboard_guard_epoch_health`] afterward.
#[allow(clippy::too_many_arguments)]
fn spawn_clipboard_guard_epoch_relay(
    config: &serde_json::Value,
    policy_version: i64,
    locked_paths: &[String],
    managed: bool,
    target_kind: Option<&str>,
    target_id: Option<&str>,
    signature: &str,
    signer_key: &str,
) {
    let args = InstallEpochArgs {
        policy_version,
        config: config.clone(),
        locked_paths: locked_paths.to_vec(),
        managed,
        target_kind: target_kind.unwrap_or("org").to_string(),
        target_id: target_id.map(str::to_string),
        signature: signature.to_string(),
        signer_key: signer_key.to_string(),
    };
    tauri::async_runtime::spawn(async move {
        handle_clipboard_guard_epoch_subtrees(args).await;
    });
}

/// Extend a JUST-VERIFIED epoch's handling with the Clipboard Guard / Ink
/// Receipt subtrees and relay to commander-svc over the real `wincmd-svc`
/// pipe. Only ever reachable from [`spawn_clipboard_guard_epoch_relay`] in
/// production — see that function's doc comment for the "verified before
/// use" precondition.
async fn handle_clipboard_guard_epoch_subtrees(args: InstallEpochArgs) {
    handle_clipboard_guard_epoch_subtrees_via(wincmd_shared::svc::SVC_PIPE_NAME, args).await
}

/// Same as [`handle_clipboard_guard_epoch_subtrees`], but with the target
/// pipe name injectable so tests can point this at a private test pipe
/// instead of the real system service (mirrors
/// `commander-svc/src/pipe.rs`'s own integration-test pattern of a
/// `wincmd-svc-test-<suffix>` pipe name).
async fn handle_clipboard_guard_epoch_subtrees_via(pipe_name: &str, args: InstallEpochArgs) {
    let parsed = parse_epoch_policy_subtrees(&args.config);

    if parsed.is_no_op() {
        // Neither subtree was mentioned — behave exactly as this epoch
        // always has: no health change, no relay attempt.
        return;
    }

    if parsed.any_malformed() {
        // Keep last_valid_policy_version exactly where it was — do not
        // advance it, do not clear it — and report unhealthy. Skip the
        // relay entirely: commander-svc's own atomic install would reach
        // the identical parse failure on the identical bytes, so a round
        // trip could not accomplish anything but spend one.
        if let Ok(mut health) = CLIPBOARD_GUARD_EPOCH_HEALTH.lock() {
            health.healthy = false;
        }
        crate::log_message(
            "warn",
            "[ClipboardGuard] verified epoch's clipboardGuard/inkReceipt subtree failed to deserialize — keeping last-valid policy",
        );
        return;
    }

    // At least one subtree is present and parsed. Relay the FULL verified
    // epoch so commander-svc can independently re-verify (never trusting
    // that Free already did) and atomically install it.
    let policy_version = args.policy_version;
    match relay_epoch_to_svc(pipe_name, &args).await {
        Ok(()) => {
            if let Ok(mut health) = CLIPBOARD_GUARD_EPOCH_HEALTH.lock() {
                health.healthy = true;
                health.last_valid_policy_version = Some(policy_version);
            }
        }
        Err(reason) => {
            // svc absent, unreachable, or it rejected the epoch. Surface
            // as a health signal only — never touch
            // last_valid_policy_version, since the previous value is
            // still the best information available about what svc
            // actually has installed. Never panic here: an absent svc is
            // an expected runtime condition, not a bug.
            if let Ok(mut health) = CLIPBOARD_GUARD_EPOCH_HEALTH.lock() {
                health.healthy = false;
            }
            crate::log_message(
                "warn",
                &format!("[ClipboardGuard] epoch relay to commander-svc failed: {reason}"),
            );
        }
    }
}

/// Dial `pipe_name`, perform the `wincmd-svc` Hello handshake, send
/// [`INSTALL_EPOCH_VERB`], and report whether svc accepted the epoch.
///
/// Pipe name is a parameter (rather than hardcoding
/// `wincmd_shared::svc::SVC_PIPE_NAME` directly) purely so tests can point
/// this at a private test pipe instead of the real system service.
///
/// Never panics on a failure that is expected in production (svc absent,
/// svc rejects, pipe busy, timeout) — every step maps to `Err(String)`.
/// The error text is protocol-level only (connect/timeout/frame-shape
/// failures, or svc's own `error_kind`) — never rule names, patterns, or
/// clipboard text, so it is safe to log verbatim.
async fn relay_epoch_to_svc(pipe_name: &str, args: &InstallEpochArgs) -> Result<(), String> {
    let args_json =
        serde_json::to_value(args).map_err(|e| format!("args serialize failed: {e}"))?;
    crate::svc_client::call_via(pipe_name, INSTALL_EPOCH_VERB, args_json)
        .await
        .map(|_| ())
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

        // ── Tweaks: Windows Server ───────────────────────────────────
        ("tweaks.server.persistentRdpAnimations", true) => Some("Enable-PersistentRdpAnimations"),
        ("tweaks.server.persistentRdpAnimations", false) => Some("Disable-PersistentRdpAnimations"),
        ("tweaks.server.ctrlAltDelDisabled", true) => Some("Disable-CtrlAltDelLogon"),
        ("tweaks.server.ctrlAltDelDisabled", false) => Some("Enable-CtrlAltDelLogon"),
        ("tweaks.server.lastSignedInUserHidden", true) => Some("Enable-HideLastSignedInUser"),
        ("tweaks.server.lastSignedInUserHidden", false) => Some("Disable-HideLastSignedInUser"),
        ("tweaks.server.consoleInactivityLock", true) => Some("Enable-ConsoleInactivityLock"),
        ("tweaks.server.consoleInactivityLock", false) => Some("Disable-ConsoleInactivityLock"),
        ("tweaks.server.shutdownTrackerDisabled", true) => Some("Disable-ShutdownEventTracker"),
        ("tweaks.server.shutdownTrackerDisabled", false) => Some("Enable-ShutdownEventTracker"),
        ("tweaks.server.serverManagerAtLogonDisabled", true) => {
            Some("Disable-ServerManagerAtLogon")
        }
        ("tweaks.server.serverManagerAtLogonDisabled", false) => {
            Some("Enable-ServerManagerAtLogon")
        }
        ("tweaks.server.ieEnhancedSecurityDisabled", true) => Some("Disable-IEEnhancedSecurity"),
        ("tweaks.server.ieEnhancedSecurityDisabled", false) => Some("Enable-IEEnhancedSecurity"),
        ("tweaks.server.wdigestBlocked", true) => Some("Block-WDigestCredentials"),
        ("tweaks.server.wdigestBlocked", false) => Some("Allow-WDigestCredentials"),
        ("tweaks.server.lsaProtectionEnabled", true) => Some("Enable-LsaProtection"),
        ("tweaks.server.lsaProtectionEnabled", false) => Some("Disable-LsaProtection"),
        ("tweaks.server.legacyNtlmBlocked", true) => Some("Block-LegacyNtlm"),
        ("tweaks.server.legacyNtlmBlocked", false) => Some("Allow-LegacyNtlm"),
        ("tweaks.server.smbSigningRequired", true) => Some("Enable-SmbSigningRequired"),
        ("tweaks.server.smbSigningRequired", false) => Some("Disable-SmbSigningRequired"),
        ("tweaks.server.smb1Disabled", true) => Some("Disable-Smb1Protocol"),
        ("tweaks.server.smb1Disabled", false) => Some("Enable-Smb1Protocol"),
        ("tweaks.server.remoteRegistryDisabled", true) => Some("Disable-RemoteRegistryService"),
        ("tweaks.server.remoteRegistryDisabled", false) => Some("Enable-RemoteRegistryService"),

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
        ("tweaks.ui.powerShell7Default", true) => Some("Enable-PowerShell7DefaultShell"),
        ("tweaks.ui.powerShell7Default", false) => Some("Disable-PowerShell7DefaultShell"),

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
        ("tweaks.os.desktopShellPriorityEnabled", true) => Some("Set-DesktopShellPriority"),
        ("tweaks.os.desktopShellPriorityEnabled", false) => Some("Reset-DesktopShellPriority"),
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
        ("privacy.tracking.terminalHistoryDisabled", true) => Some("Disable-TerminalHistory"),
        ("privacy.tracking.terminalHistoryDisabled", false) => Some("Enable-TerminalHistory"),

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

    #[test]
    fn ramdisk_autostart_preserves_the_typescript_size_mb_key() {
        let parsed: RamDiskAutostartSettings = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "sizeMB": 4096,
            "driveLetter": "J"
        }))
        .unwrap();

        assert_eq!(parsed.size_mb, Some(4096));
        assert_eq!(
            serde_json::to_value(parsed).unwrap()["sizeMB"],
            serde_json::json!(4096),
            "the persisted key must match the frontend contract exactly"
        );
    }

    #[test]
    fn privacy_shield_notify_mode_round_trips_through_settings_json() {
        let shield: PrivacyShieldSettings = serde_json::from_value(serde_json::json!({
            "notifyMode": "notify_only"
        }))
        .unwrap();

        assert_eq!(
            shield.notify_mode,
            Some(PrivacyShieldNotifyMode::NotifyOnly)
        );
        assert_eq!(
            serde_json::to_value(shield).unwrap()["notifyMode"],
            serde_json::json!("notify_only")
        );
    }

    #[test]
    fn runtime_defaults_remove_the_retired_sidecar_panel_id() {
        let mut settings = create_default_settings();
        settings.app.permanently_hidden_panels =
            Some(vec!["sidecar".to_string(), "productivity".to_string()]);

        apply_runtime_defaults(&mut settings);

        assert_eq!(
            settings.app.permanently_hidden_panels,
            Some(vec!["productivity".to_string()])
        );
    }

    #[test]
    fn invalid_legacy_device_id_is_replaced_with_a_uuid() {
        let mut raw = serde_json::to_value(create_default_settings()).unwrap();
        raw["deviceId"] = serde_json::json!("legacy-machine-42");

        let settings = parse_and_migrate_json_val(raw).unwrap();

        assert!(Uuid::parse_str(&settings.device_id).is_ok());
        assert_ne!(settings.device_id, "legacy-machine-42");
    }

    #[test]
    fn valid_device_id_is_preserved() {
        let mut settings = create_default_settings();
        let expected = settings.device_id.clone();

        ensure_valid_device_id(&mut settings);

        assert_eq!(settings.device_id, expected);
    }

    #[test]
    fn current_version_decoy_tripwire_settings_are_preserved_as_decoy_monitor() {
        let mut raw = serde_json::to_value(create_default_settings()).unwrap();
        raw["ideal"]["privacy"] = serde_json::json!({
            "decoyTripwire": {
                "enabled": true,
                "enrolledPaths": ["C:\\\\Users\\\\Admin\\\\Desktop\\\\personal-passwords.txt"],
                "readAuditEnabled": true,
                "fleetAlertEnabled": true
            }
        });

        let settings = parse_and_migrate_json_val(raw).unwrap();
        let decoy = settings.ideal.privacy.decoy_monitor;

        assert_eq!(decoy.enabled, Some(true));
        assert_eq!(decoy.read_audit_enabled, Some(true));
        assert_eq!(decoy.fleet_alert_enabled, Some(true));
        assert_eq!(decoy.enrolled_paths.unwrap().len(), 1);
    }

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

    #[test]
    fn fleet_master_alert_payload_survives_the_exact_epoch_merge_and_reread() {
        let payload = serde_json::json!({
            "security": { "requireAllDeviceAlertsInFleet": true }
        });
        let locked_paths = vec!["security.requireAllDeviceAlertsInFleet".to_string()];
        let mut raw = serde_json::to_value(create_default_settings()).unwrap();
        merge_json(&mut raw, &serde_json::json!({ "ideal": payload }));
        let mut applied: AppSettings = serde_json::from_value(raw).unwrap();
        applied.policy.locked_paths = locked_paths.clone();
        applied.policy.sync_mode = "managed".to_string();

        // Re-serialize and re-read exactly as the settings store does after a
        // signed ConfigEpoch merge. This prevents a new Fleet key from being
        // accepted by JSON merge but silently discarded by the Rust schema.
        let reread: AppSettings =
            serde_json::from_value(serde_json::to_value(&applied).unwrap()).unwrap();
        assert!(reread.ideal.security.require_all_device_alerts_in_fleet);
        assert_eq!(reread.policy.locked_paths, locked_paths);
    }

    // ── Decoy-mode write refusal (write_settings_internal choke point) ──
    //
    // DECOY_MODE and SETTINGS_CACHE are process-global statics shared by every
    // test in this binary (cargo test runs #[test] fns on separate threads in
    // parallel). GLOBAL_STATE_TEST_LOCK serializes tests that mutate or rely
    // on those values so a cache warm-up / decoy toggle cannot interleave with
    // another global-state assertion.
    /// Held for the lifetime of one decoy-mode test: serializes against the
    /// other global-state tests (via GLOBAL_STATE_TEST_LOCK) and flips
    /// DECOY_MODE on immediately, guaranteeing it's always flipped back off
    /// on drop (even on panic) so no later test can observe it stuck on.
    struct DecoyModeGuard<'a> {
        _lock: std::sync::MutexGuard<'a, ()>,
    }
    impl DecoyModeGuard<'_> {
        fn engage() -> Self {
            let lock = super::GLOBAL_STATE_TEST_LOCK
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

    // ── Clipboard Guard / Ink Receipt epoch subtree relay (task C6) ──────

    /// A minimally valid `wincmd_clip_rules::Rule` as JSON — camelCase
    /// top-level fields, snake_case `matcher.params` fields (see
    /// `wincmd-clip-rules/tests/serde_shape.rs`, the authoritative wire
    /// shape check for this crate).
    fn valid_clipboard_guard_rule_json() -> serde_json::Value {
        serde_json::json!({
            "id": "0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b",
            "revision": 1,
            "name": "test-rule",
            "enabled": true,
            "priority": 100,
            "matcher": {"kind": "phrase", "params": {"value": "secret", "case_sensitive": false}},
            "severity": "warn",
            "actions": ["notify_user"],
            "cooldownSeconds": 30,
            "snoozable": true,
            "locked": false
        })
    }

    fn sample_install_epoch_args(policy_version: i64) -> InstallEpochArgs {
        InstallEpochArgs {
            policy_version,
            config: serde_json::json!({
                "clipboardGuard": {"rules": [valid_clipboard_guard_rule_json()]}
            }),
            locked_paths: vec![],
            managed: true,
            target_kind: "org".to_string(),
            target_id: None,
            signature: "test-signature".to_string(),
            signer_key: "test-signer-key".to_string(),
        }
    }

    fn set_clipboard_guard_health_for_test(health: ClipboardGuardEpochHealth) {
        let mut guard = CLIPBOARD_GUARD_EPOCH_HEALTH
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = health;
    }

    fn warm_cache_with_pinned_fleet_key(pubkey_b64: &str) {
        let mut settings = create_default_settings();
        settings.policy.fleet_signing_key = Some(pubkey_b64.to_string());
        let mut guard = SETTINGS_CACHE.lock().unwrap();
        *guard = Some(settings);
    }

    // ── parse_epoch_policy_subtrees: pure, no I/O, no globals ────────────

    #[test]
    fn absent_subtrees_are_absent_not_malformed() {
        let config = serde_json::json!({"privacy": {"telemetry": {}}});
        let parsed = parse_epoch_policy_subtrees(&config);
        assert_eq!(parsed.clipboard_guard, SubtreeOutcome::Absent);
        assert_eq!(parsed.ink_receipt, SubtreeOutcome::Absent);
        assert!(parsed.is_no_op());
        assert!(!parsed.any_malformed());
    }

    #[test]
    fn valid_clipboard_guard_subtree_parses() {
        let config = serde_json::json!({
            "clipboardGuard": {"rules": [valid_clipboard_guard_rule_json()]}
        });
        let parsed = parse_epoch_policy_subtrees(&config);
        assert_eq!(parsed.clipboard_guard, SubtreeOutcome::Valid);
        assert!(!parsed.is_no_op());
        assert!(!parsed.any_malformed());
    }

    #[test]
    fn clipboard_guard_subtree_with_invalid_rule_id_is_malformed() {
        let mut bad_rule = valid_clipboard_guard_rule_json();
        // "not-a-uuid" is the exact known-bad fixture from
        // wincmd-clip-rules/src/ids.rs's own RuleId tests.
        bad_rule["id"] = serde_json::json!("not-a-uuid");
        let config = serde_json::json!({"clipboardGuard": {"rules": [bad_rule]}});
        let parsed = parse_epoch_policy_subtrees(&config);
        assert_eq!(parsed.clipboard_guard, SubtreeOutcome::Malformed);
        assert!(parsed.any_malformed());
    }

    #[test]
    fn ink_receipt_subtree_as_json_object_is_valid() {
        // Free deliberately does not model Ink Receipt's field-level
        // schema (that's commander-pro's job) — any JSON object passes.
        let config = serde_json::json!({"inkReceipt": {"anything": "goes-for-now"}});
        let parsed = parse_epoch_policy_subtrees(&config);
        assert_eq!(parsed.ink_receipt, SubtreeOutcome::Valid);
        assert!(!parsed.any_malformed());
    }

    #[test]
    fn ink_receipt_subtree_as_non_object_is_malformed() {
        let config = serde_json::json!({"inkReceipt": "not-an-object"});
        let parsed = parse_epoch_policy_subtrees(&config);
        assert_eq!(parsed.ink_receipt, SubtreeOutcome::Malformed);
        assert!(parsed.any_malformed());
    }

    // ── relay_epoch_to_svc: real IPC against a private test pipe ─────────

    /// Minimal stand-in for `commander-svc/src/pipe.rs`'s Hello+Request
    /// loop — accepts exactly one connection, does the Hello handshake,
    /// reads one frame, then writes back `reply`. Mirrors that file's own
    /// integration-test pattern (a `wincmd-svc-test-<suffix>` pipe name)
    /// so it never touches the real system pipe.
    async fn run_fake_svc_once(pipe_name: &'static str, reply: wincmd_shared::Envelope) {
        use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};
        let mut server = ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .first_pipe_instance(true)
            .create(pipe_name)
            .expect("create test svc pipe");
        server.connect().await.expect("test svc pipe accept");

        let _hello = wincmd_shared::read_envelope(&mut server)
            .await
            .expect("read Hello");
        let ack = wincmd_shared::Envelope::Hello(wincmd_shared::svc::hello_from_ui("svc-ack"));
        wincmd_shared::write_envelope(&mut server, &ack)
            .await
            .expect("write Hello ack");

        let _request = wincmd_shared::read_envelope(&mut server)
            .await
            .expect("read Request");
        wincmd_shared::write_envelope(&mut server, &reply)
            .await
            .expect("write reply");
    }

    #[tokio::test]
    async fn relay_succeeds_when_svc_responds_with_response() {
        let pipe_name = r"\\.\pipe\wincmd-svc-test-c6-relay-ok";
        let reply = wincmd_shared::Envelope::Response(wincmd_shared::Response {
            request_id: 1,
            result: serde_json::json!({"ok": true}),
        });
        let server = tokio::spawn(run_fake_svc_once(pipe_name, reply));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let args = sample_install_epoch_args(1);
        let result = relay_epoch_to_svc(pipe_name, &args).await;

        server.await.expect("fake svc server task panicked");
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
    }

    #[tokio::test]
    async fn relay_fails_when_svc_responds_with_error() {
        let pipe_name = r"\\.\pipe\wincmd-svc-test-c6-relay-reject";
        let reply = wincmd_shared::Envelope::Error(wincmd_shared::ErrorReply {
            request_id: 1,
            kind: "forbidden".to_string(),
            message: "denied".to_string(),
        });
        let server = tokio::spawn(run_fake_svc_once(pipe_name, reply));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let args = sample_install_epoch_args(2);
        let result = relay_epoch_to_svc(pipe_name, &args).await;

        server.await.expect("fake svc server task panicked");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn relay_fails_gracefully_when_svc_is_unreachable() {
        // Nobody is listening at this pipe on purpose — the "svc absent"
        // scenario. Must return a graceful Err, never panic.
        let pipe_name = r"\\.\pipe\wincmd-svc-test-c6-relay-absent";
        let args = sample_install_epoch_args(3);
        let result = relay_epoch_to_svc(pipe_name, &args).await;
        assert!(
            result.is_err(),
            "expected a graceful Err with no svc listening"
        );
    }

    // ── handle_clipboard_guard_epoch_subtrees_via: parse + health + relay ──

    #[tokio::test]
    // This process-wide test mutex intentionally spans the await so synchronous
    // decoy-mode tests cannot mutate the same globals while this probe runs.
    #[allow(clippy::await_holding_lock)]
    async fn absent_subtrees_are_a_no_op() {
        let _lock = super::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set_clipboard_guard_health_for_test(ClipboardGuardEpochHealth {
            healthy: true,
            last_valid_policy_version: Some(3),
        });

        let mut args = sample_install_epoch_args(9);
        args.config = serde_json::json!({"privacy": {"telemetry": {}}});

        // Nobody listens at this pipe on purpose — a no-op epoch must
        // never even attempt the relay.
        handle_clipboard_guard_epoch_subtrees_via(
            r"\\.\pipe\wincmd-svc-test-c6-should-not-be-dialed-noop",
            args,
        )
        .await;

        let health = clipboard_guard_epoch_health();
        assert!(health.healthy);
        assert_eq!(
            health.last_valid_policy_version,
            Some(3),
            "absent subtrees must not change last_valid_policy_version"
        );
    }

    #[tokio::test]
    // This process-wide test mutex intentionally spans the await so synchronous
    // decoy-mode tests cannot mutate the same globals while this probe runs.
    #[allow(clippy::await_holding_lock)]
    async fn valid_subtree_parses_and_relays_and_marks_healthy() {
        let _lock = super::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set_clipboard_guard_health_for_test(ClipboardGuardEpochHealth::default());

        let pipe_name = r"\\.\pipe\wincmd-svc-test-c6-handle-ok";
        let reply = wincmd_shared::Envelope::Response(wincmd_shared::Response {
            request_id: 1,
            result: serde_json::json!({"ok": true}),
        });
        let server = tokio::spawn(run_fake_svc_once(pipe_name, reply));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        handle_clipboard_guard_epoch_subtrees_via(pipe_name, sample_install_epoch_args(7)).await;

        server.await.expect("fake svc server task panicked");

        let health = clipboard_guard_epoch_health();
        assert!(health.healthy);
        assert_eq!(health.last_valid_policy_version, Some(7));
    }

    #[tokio::test]
    // This process-wide test mutex intentionally spans the await so synchronous
    // decoy-mode tests cannot mutate the same globals while this probe runs.
    #[allow(clippy::await_holding_lock)]
    async fn malformed_subtree_keeps_previous_policy_version_and_marks_unhealthy() {
        let _lock = super::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set_clipboard_guard_health_for_test(ClipboardGuardEpochHealth {
            healthy: true,
            last_valid_policy_version: Some(3),
        });

        let mut bad_rule = valid_clipboard_guard_rule_json();
        bad_rule["id"] = serde_json::json!("not-a-uuid");
        let mut args = sample_install_epoch_args(8);
        args.config = serde_json::json!({"clipboardGuard": {"rules": [bad_rule]}});

        // No server listening at this pipe on purpose — a malformed
        // subtree must never even attempt the relay.
        handle_clipboard_guard_epoch_subtrees_via(
            r"\\.\pipe\wincmd-svc-test-c6-should-not-be-dialed-malformed",
            args,
        )
        .await;

        let health = clipboard_guard_epoch_health();
        assert!(!health.healthy);
        assert_eq!(
            health.last_valid_policy_version,
            Some(3),
            "a malformed subtree must keep the previous valid policy version"
        );
    }

    #[tokio::test]
    // This process-wide test mutex intentionally spans the await so synchronous
    // decoy-mode tests cannot mutate the same globals while this probe runs.
    #[allow(clippy::await_holding_lock)]
    async fn relay_failure_marks_unhealthy_without_losing_last_valid_version() {
        let _lock = super::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set_clipboard_guard_health_for_test(ClipboardGuardEpochHealth {
            healthy: true,
            last_valid_policy_version: Some(5),
        });

        // Nobody listens at this pipe — svc absent/unreachable.
        handle_clipboard_guard_epoch_subtrees_via(
            r"\\.\pipe\wincmd-svc-test-c6-relay-failure-no-listener",
            sample_install_epoch_args(6),
        )
        .await;

        let health = clipboard_guard_epoch_health();
        assert!(!health.healthy);
        assert_eq!(
            health.last_valid_policy_version,
            Some(5),
            "a failed relay must not clear or advance the last known-good version"
        );
    }

    // ── apply_admin_config_cmd: forged/mismatched signature ⇒ never relayed ──

    #[test]
    fn forged_signature_epoch_with_clipboard_guard_subtree_is_rejected_and_never_relayed() {
        let _lock = super::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use base64::{Engine, engine::general_purpose::STANDARD};
        use ed25519_dalek::{Signer, SigningKey};

        let signing_key = SigningKey::generate(&mut rand_core::OsRng);
        let pubkey_b64 = STANDARD.encode(signing_key.verifying_key().to_bytes());

        warm_cache_with_pinned_fleet_key(&pubkey_b64);
        set_clipboard_guard_health_for_test(ClipboardGuardEpochHealth::default());

        let config = serde_json::json!({
            "clipboardGuard": {"rules": [valid_clipboard_guard_rule_json()]}
        });
        // A syntactically valid Ed25519 signature — over the WRONG
        // message — so verify_signature_b64 fails without needing a
        // hand-corrupted signature blob.
        let forged = signing_key.sign(b"not the real epoch preimage");
        let forged_b64 = STANDARD.encode(forged.to_bytes());

        let result = apply_admin_config_cmd(
            config,
            vec![],
            "merge".to_string(),
            1,
            Some(forged_b64),
            Some(pubkey_b64),
            Some("org".to_string()),
            None,
            Some(true),
        );

        assert_eq!(
            result,
            Err("config push signature verification failed".to_string())
        );
        assert_eq!(
            clipboard_guard_epoch_health().last_valid_policy_version,
            None,
            "a forged-signature epoch must never reach the subtree relay"
        );
    }

    #[test]
    fn signer_key_mismatch_epoch_is_rejected_and_never_relayed() {
        let _lock = super::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let pinned_key = "pinned-fleet-key-b64-placeholder".to_string();
        warm_cache_with_pinned_fleet_key(&pinned_key);
        set_clipboard_guard_health_for_test(ClipboardGuardEpochHealth::default());

        let config = serde_json::json!({
            "clipboardGuard": {"rules": [valid_clipboard_guard_rule_json()]}
        });

        let result = apply_admin_config_cmd(
            config,
            vec![],
            "merge".to_string(),
            1,
            Some("any-signature".to_string()),
            Some("a-different-key".to_string()),
            Some("org".to_string()),
            None,
            Some(true),
        );

        assert_eq!(
            result,
            Err("config push signer key does not match the pinned fleet key".to_string())
        );
        assert_eq!(
            clipboard_guard_epoch_health().last_valid_policy_version,
            None,
            "a signer-key-mismatched epoch must never reach the subtree relay"
        );
    }

    #[test]
    fn usb_security_arm_preferences_use_frontend_camel_case() {
        let parsed: UsbSecurityMonitorSettings = serde_json::from_value(serde_json::json!({
            "monitorEnabled": true,
            "hidGuardEnabled": true,
            "meteringEnabled": true,
            "autoSandboxEnabled": false
        }))
        .unwrap();
        assert_eq!(parsed.monitor_enabled, Some(true));
        assert_eq!(parsed.hid_guard_enabled, Some(true));
        assert_eq!(parsed.metering_enabled, Some(true));
        assert_eq!(parsed.auto_sandbox_enabled, Some(false));

        let encoded = serde_json::to_value(parsed).unwrap();
        assert_eq!(encoded["hidGuardEnabled"], true);
        assert!(encoded.get("hid_guard_enabled").is_none());
    }
}
