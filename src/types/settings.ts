// src/types/settings.ts
//
// TypeScript mirror of the Rust AppSettings schema (settings.rs).
// Keep in sync with src-tauri/commander-free/src/settings.rs
// Schema Version: 2 — ideal/current state model

import type { ModuleConfig } from './modules';

// Legacy flows storage (`app.flows`) is the old Free-engine rule set
// (contingency/panic). The v2 Pro-backed engine uses `app.proFlows` (opaque
// flow_core::Rule JSON) — see src/panels/flows/rules.ts. Both are typed loosely
// here because neither is consumed structurally by settings code.
export type Flow = Record<string, unknown>;
export type ContingencySettings = Record<string, unknown>;
import type { CapabilityBundle, Density } from './persona';

// ═══════════════════════════════════════════════════════════════════════
// SYSTEM STATE — Container for all toggle-able system settings
// ═══════════════════════════════════════════════════════════════════════

export interface SystemState {
  device: DeviceIdentifiers;
  privacy: PrivacySettings;
  tweaks: TweakSettings;
  network: NetworkSettings;
  identity: IdentitySettings;
  apps: AppManagementSettings;
  productivity: ProductivitySettings;
  serverApps: ServerAppsSettings;
  /** Security diagnostics — driver-health / Device-Manager check (#6). */
  security: SecuritySettings;
}

// ── Security diagnostics ─────────────────────────────────────────────

export interface SecuritySettings {
  /** #6: driver-health / Device-Manager check. */
  drivers: DriverHealthSettings;
  /** Signed Fleet policy master gate. When enabled, every supported
   * device-side alert is reported through its existing Fleet queue. */
  requireAllDeviceAlertsInFleet?: boolean;
}

export interface DriverHealthSettings {
  /** Opt-in periodic re-scan that toasts NEW critical problem devices. */
  watchEnabled: boolean;
  /** Poll cadence in seconds. Clamped 30..3600 in Rust. */
  watchIntervalSecs: number;
  /** Run one scan on app launch (independent of watchEnabled). */
  scanOnStartup: boolean;
}

// ── Device Identifiers ───────────────────────────────────────────────

export interface GpuDetection {
  /** Whether an NVIDIA GPU was found. */
  gpuFound: boolean;
  /** GPU name from nvidia-smi or WMI (e.g. "NVIDIA GeForce RTX 4090"). */
  gpuName: string | null;
  /** Driver version string from nvidia-smi. */
  driverVersion: string | null;
  /** Dedicated VRAM in MB from nvidia-smi. */
  vramMb: number | null;
  /** CUDA compute capability e.g. "8.9". */
  computeCapability: string | null;
  /** Whether CUDA Toolkit is installed on this machine. */
  cudaFound: boolean;
  /** Install path of CUDA Toolkit (CUDA_PATH env var or registry). */
  cudaPath: string | null;
  /** ISO timestamp of last detection run. */
  lastCheckedAt: string | null;
}

export interface DeviceIdentifiers {
  cpu: string | null;
  ramGb: number | null;
  gpu: string | null;
  disks: DiskInfo[];
  macAddresses: string[];
  serialNumber: string | null;
  biosVersion: string | null;
  osBuild: string | null;
  domain: string | null;
  timeZone: string | null;
  systemLocale: string | null;
  users: UserInfo[];
  runtimes: string | null;
  windowsActivated: boolean | null;
  bitlockerStatus: BitLockerStatus[];
  lastUpdateAt: string | null;
  pendingUpdatesCount: number | null;
  // Display fields seeded from startup probe
  hostname?: string | null;
  osName?: string | null;
  osVersion?: string | null;
  deviceType?: string | null;
  ram?: string | null;
  isAdmin?: boolean | null;
  /** NVIDIA GPU + CUDA detection result — populated by the hardware probe. */
  gpuDetection?: GpuDetection | null;
}

export interface DiskInfo {
  model: string;
  sizeGb: number;
  diskType: string;
}

export interface UserInfo {
  username: string;
  enabled: boolean;
  isAdmin: boolean;
  lastLogon: string | null;
  sid: string;
}

export interface BitLockerStatus {
  drive: string;
  encrypted: boolean;
  status: string;
}

// ═══════════════════════════════════════════════════════════════════════
// ROOT SCHEMA — Version 2
// ═══════════════════════════════════════════════════════════════════════

export interface AppSettings {
  settingsVersion: number;
  appVersion: string;
  deviceId: string;
  // ISO-8601 UTC strings — must match the Rust Settings struct (String),
  // not numbers, or read_settings fails to deserialize the whole file.
  lastSeenAt: string;
  createdAt: string;
  app: AppPreferences;
  /** What admin/user wants the system to be */
  ideal: SystemState;
  /** What the system actually is (auto-probed) */
  current: SystemState;
  policy: PolicySettings;
}

// ── App Preferences ──────────────────────────────────────────────────

export type ExperienceLevel = 'simple' | 'standard' | 'advanced';

/** Threat-model axis: everyday privacy vs. maximum trace protection. Orthogonal
 *  to `Density`/`CapabilityBundle` (src/types/persona.ts, which is the resolved
 *  UI-skill + feature-bundle model) — distinct name chosen to avoid colliding
 *  with that existing "persona" concept. Setting key: `app.persona`. */
export type ThreatPersona = 'casual' | 'secure';

export interface RdpNode {
  id: string;
  label: string;
  hostname: string;
  username: string;
}

export interface AppPreferences {
  theme: 'dark' | 'light';
  firstRunComplete: boolean;
  /** Whether the mandatory first-run spotlight tour has been completed.
   *  Separate from `firstRunComplete`: this gates whether the auto-started
   *  tour can be dismissed early (see GuideHost.tsx) — false only for the
   *  very first tour run after a fresh install. Mirrors
   *  `has_seen_mandatory_tour` in settings.rs. */
  hasSeenMandatoryTour: boolean;
  /** Whether the Private Mesh "How it works" first-visit explainer has been
   *  shown. Persisted (not localStorage) so it survives app updates. */
  meshHowItWorksSeen?: boolean;
  /** Whether WinCommander updates ITSELF automatically (background download +
   *  "Restart now" prompt). On by default; chosen once in Help & Setup
   *  and not surfaced again in settings. NOT to be confused with
   *  `ideal.apps.autoUpdate`, which is the winget *installed-apps* policy. */
  autoUpdate: boolean;
  startMinimized: boolean;
  contextMenuEnabled: boolean;
  scrubContextMenuEnabled: boolean;
  safeCopyContextMenuEnabled: boolean;
  sidebarCollapsed: boolean;
  /** Categories this Windows user has excluded from Cleanup's bulk-clear
   *  action. Stored in the per-user settings file, so it survives relaunches
   *  and normal reinstalls; resetting settings restores the defaults. */
  bulkClearExcludes?: string[];
  /** @deprecated Superseded by `bulkClearExcludes`. Kept only so existing
   *  settings files remain compatible. */
  cleanupExcludesCustomized?: boolean;
  lastPanel: string;
  dashboardViewMode: string;
  /** Dashboard right-column cards currently expanded ("system" | "storage" |
   *  "network"), in the order they were opened (oldest first). At most two are
   *  open at once; opening a third evicts the oldest. Absent/empty = default
   *  (["system"]). Mirrors `dashboard_open_cards` in settings.rs. */
  dashboardOpenCards?: string[];
  /** Radar scan finding IDs the user chose to ignore ("dismiss"). Hidden from
   *  the dashboard "Needs Attention" list, the radar node counts, and
   *  Fix Everything. The user can clear the list to surface them again. */
  ignoredFindingIds?: string[];
  /** Score-category keys dismissed from the title-bar bell's compact
   *  "needs attention" summary. Kept separate from per-finding dashboard
   *  ignores so restoring one list does not unexpectedly restore the other. */
  dismissedNeedsAttentionIds?: string[];
  /** @deprecated Migration-only. Superseded by `density`/`capabilities`
   *  (see `src/lib/personaMigration.ts`). The frontend no longer writes this
   *  field — it is kept in the schema solely because `settings.rs` still
   *  reads it once to migrate pre-U4 installs that persisted it to disk.
   *  Do not write to this field from new code; do not read it directly —
   *  use `getDensityForSettings`/`useVisibility()` instead. */
  experienceLevel: ExperienceLevel;
  /** New persona density model. Falls back to experienceLevel while the
   *  frontend migration is still in progress. */
  density?: Density;
  /** Capability bundles selected by the user. Falls back to the legacy
   *  module + privacyClean model while panels are being ported. */
  capabilities?: CapabilityBundle[];
  /** Threat-model persona ("casual" vs "secure"), chosen at first-run or in
   *  Settings. Unset (new install, or upgrade from before this field existed)
   *  resolves to "secure" via `getPersona()` — preserves today's all-modules-on
   *  behavior and never silently disables a module someone already uses. */
  persona?: ThreatPersona;
  /** Whether Privacy Clean features are shown */
  privacyCleanEnabled: boolean;
  /** Global hotkey that triggers immediate self-destruct (e.g. "Ctrl+Shift+Q") */
  panicHotkey: string;
  /** Global hotkey that shows/hides the floating file search bar (e.g. "Ctrl+Space") */
  searchHotkey: string;
  rdpNodes: RdpNode[];
  selectedRdpNodeId: string | null;
  notifications: NotificationSettings;
  vault: VaultSettings;
  firstRun: FirstRunSettings;
  shortcuts: Record<string, string>;
  /** Sound feedback settings */
  sounds?: {
    /** Whether all mechanical feedback sounds are enabled. Default: true */
    enabled: boolean;
  };
  /** Legacy automation flows (old Free engine — contingency/panic). */
  flows: Flow[];
  /** v2 Pro-backed flow rules (opaque flow_core::Rule JSON). @see panels/flows/rules.ts */
  proFlows?: unknown[];
  /** Contingency system configuration (identities, signal transport, USB key) */
  contingency: ContingencySettings;
  /** Module enable/disable map — controls which features run at all.
   *  Missing keys default to false (disabled). Set by Help & Setup
   *  based on experience level, then user-toggleable in sidebar. */
  modules?: ModuleConfig;
  /** Sidebar keyword to reveal sidebar-lock panels (default: "unlock") */
  unlockKeyword?: string;
  /** Sidebar keyword to re-hide sidebar-lock panels (default: "lock") */
  lockKeyword?: string;
  /** Panel IDs that are hidden until the unlock keyword is typed.
   *  Defaults to ["cleanup", "vault", "private-mesh"] when absent. */
  lockedPanelIds?: string[];
  /** When true, the title-bar Alerts + Processes icons (formerly one
   *  notifications bell — see AlertsMenu.tsx/ProcessesMenu.tsx) are hidden
   *  while panels are locked. */
  muteNotificationsWhenLocked?: boolean;
  /** When true, the title-bar Alerts + Processes icons (formerly one
   *  notifications bell) are hidden from the title bar at all times (not just
   *  while locked) AND popup toasts are suppressed. Standalone concealment
   *  toggle surfaced in the Secret Settings panel. Field name kept as-is
   *  (pre-dates the Alerts/Processes icon split) so existing persisted
   *  settings keep working unchanged. */
  hideNotificationBell?: boolean;
  /** When true, the floating desktop-alert overlay window is suppressed.
   *  Does not affect the in-app bell or Sonner toasts. */
  disableNativeNotifications?: boolean;
  /** Right-sidebar quick actions the user has chosen to hide. Recognised keys:
   *  "dismount", "delete", "scrubMeta", "lockdown". Configured in Secret Settings. */
  hiddenSidebarActions?: string[];
  /** Panel IDs hidden permanently (regardless of Borrowed-PC mode state).
   *  Distinct from lockedPanelIds which only hide while Borrowed-PC is active. */
  permanentlyHiddenPanels?: string[];
  /** Non-panel surface keys hidden only while Borrowed Mode is active.
   *  Recognised keys: "notif-bell" (hides BOTH the Alerts and Processes
   *  title-bar icons — key name kept from before the two-icon split so
   *  existing persisted settings keep working), "risk-matrix",
   *  "more-products", "popup-alerts", "desktop-alerts", "engines-section",
   *  "license-panel", "sidebar-preferences", "action:ai-advisor", "action:search",
   *  "action:dismount", "action:delete", "action:scrubMeta", "action:lockdown". */
  borrowedHidden?: string[];
  /** When true, the License quick-panel in the sidebar footer is hidden
   *  everywhere ("Always" in the Borrowed Mode table). "When borrowed" hiding
   *  uses the "license-panel" key in borrowedHidden. Mirrors
   *  `app.hide_license_panel`. Default false. */
  hideLicensePanel?: boolean;
  /** When true, Interface and Persona controls in the sidebar footer are hidden
   *  everywhere. Borrowed-only hiding uses "sidebar-preferences" in borrowedHidden. */
  hideSidebarPreferences?: boolean;
  /** When true, the full-screen countdown popup is suppressed when triggering
   *  lockdown from the sidebar button. The countdown still fires silently. */
  hideDestructionSequence?: boolean;
  /** When true, the title-bar guide button and all automatic/manual tours are
   * hidden. Borrowed-only hiding uses the "tour" borrowedHidden key. */
  hideTour?: boolean;
  /** "Lock panel on close" (Secret Setting). When ON and a calculator PIN is armed,
   *  closing shows the calculator only; when OFF (or no PIN), closing hides to the tray
   *  (next reveal is Borrowed-locked when lockedPanelIds is configured). Undefined
   *  resolves (in Rust, at close time) to: true if a real PIN is armed, else false. */
  lockPanelOnClose?: boolean;
  /** Whether activity logging to %LOCALAPPDATA%\WinCommander\logs\ is enabled.
   *  Defaults to true. Logs rotate daily; files older than 7 days are deleted. */
  loggingEnabled?: boolean;
  /** AI Security Advisor settings (spec 13 / #10). */
  advisor: AdvisorSettings;
  /** Internet kill switch — when true, WinCommander-KillSwitch firewall
   *  rules block ALL traffic (in + out). Mirrors `app.internet_kill_switch`
   *  in settings.rs. Runtime authority is the firewall rule itself. */
  internetKillSwitch?: boolean;
  /** Developer option: when true, the self-update scheduler skips all checks
   *  and suppresses every updater event — no banner, no background download,
   *  no restart prompt. Mirrors `app.disable_updates` in settings.rs. */
  disableUpdates?: boolean;
  /** Low Performance Mode — disables UI animations AND the periodic active-panel
   *  polling, which is the expensive half: each refresh spawns a cold
   *  powershell.exe (no runspace reuse), and on a multi-user server every
   *  logged-in session pays it separately.
   *
   *  "auto" (the default) turns it on for constrained hardware using the same
   *  thresholds as the motion system — 4 or fewer cores, or under 8 GB RAM.
   *  "on"/"off" are explicit and always win over the hardware check.
   *  Mirrors `app.low_performance_mode` in settings.rs — a field with no Rust
   *  counterpart is silently dropped by patch_settings' merge/deserialize. */
  lowPerformanceMode?: "auto" | "on" | "off" | null;
  /** When true, detected drift (ideal ≠ current state) is automatically
   *  re-applied after each system probe — no manual "Fix" clicks needed.
   *  Off by default. Irreversible and action-type toggles are never auto-healed. */
  autoHeal?: boolean;
  /** Reusable per-metric alert configuration (paid). Mirrors
   *  `app.metric_alerts` in settings.rs. Runtime authority is the Rust
   *  sampler; the frontend reads/writes via metric_alerts_get/set_config. */
  metricAlerts?: MetricAlertsSettings;
  /** Decoy mode (paid): rebrand the app under a custom name. Mirrors
   *  `app.decoy_mode` in settings.rs. */
  decoyMode?: DecoyModeSettings;
  /** File-content search (full-text index) configuration — Free tier.
   *  Mirrors FileSearchSettings in settings.rs (camelCase via serde rename). */
  fileSearch?: FileSearchSettings;
  /** Fleet agent onboarding (paid). Mirrors FleetSettings in settings.rs.
   *  Persisted so the agent survives Pro restarts without re-typing the URL. */
  fleet?: FleetSettings;
  /** Seconds to count down before the sidebar Lockdown button fires (3–30).
   *  Default: 4. */
  lockdownTimerSec?: number;
  /** When true, the AI Advisor panel is hidden from the right-sidebar launcher. */
  hideAdvisor?: boolean;
  /** When true, the Engines & Dependencies section is hidden in the Apps panel. */
  hideEnginesSection?: boolean;
  /** Scheduled Recycle Bin clearing. */
  scheduledRecycleBin?: {
    enabled: boolean;
    /** How often to clear, in days (1–30). */
    intervalDays: number;
    /** ISO-8601 UTC timestamp of the next scheduled clear. */
    nextRunAt?: string | null;
  };
  /** When true, the startup Pro-install nag has been dismissed by the user
   *  ("Not now"). Suppresses the passive startup prompt only; on-demand
   *  prompts (triggered by invoking a paid feature) still fire every time.
   *  Mirrors `app.pro_install_prompt_dismissed` in settings.rs. Default false. */
  proInstallPromptDismissed?: boolean;
  /** Version key (e.g. "free:3.0.10" or "pro:3.0.10") of the last update the
   *  combined UpdateFlowDialog auto-opened for at startup. Lets the auto-open
   *  effect in App.tsx tell "already announced this exact version" apart from
   *  "a new version showed up" so a still-pending update doesn't re-announce
   *  itself on every tray/window reveal within the same session, or on every
   *  relaunch until it's actually installed. Mirrors
   *  `app.last_announced_update_version` in settings.rs. */
  lastAnnouncedUpdateVersion?: string | null;
  /** Legacy UI-only: the welcome spotlight tour is queued to run. Kept so
   *  older settings files deserialize cleanly; new setup opens Help & Setup
   *  directly instead. Mirrors `app.welcome_tour_pending` in settings.rs. */
  welcomeTourPending?: boolean;
  /** UI-only: the welcome spotlight tour has run at least once. Lets the help
   *  center label a re-run as "Replay tour". Mirrors `app.welcome_tour_completed`. */
  welcomeTourCompleted?: boolean;
}

/** Resolve the effective threat persona from settings. Unset `app.persona`
 *  (new install pre-first-run, or an upgrade from before this field existed)
 *  resolves to "secure" — preserves today's all-modules-on default and never
 *  silently disables a module someone already uses. */
export function getPersona(settings: { app: Pick<AppPreferences, 'persona'> } | null | undefined): ThreatPersona {
  return settings?.app.persona ?? 'secure';
}

/** Free tier: file-content search index configuration.
 *  Mirrors FileSearchSettings in settings.rs (camelCase via serde rename). */
export interface FileSearchSettings {
  /** Root directories to index. */
  roots: string[];
  /** Glob patterns to exclude (e.g. "*.tmp", "node_modules"). */
  exclusions: string[];
  /** False until default search roots have been seeded. */
  initialized: boolean;
  /** Maximum filename rows returned per search (50–2,000). Text matches use
   * their independent bounded window so filename volume never hides them. */
  resultLimit?: number;
}

/** Fleet agent connection config (paid) — persisted to settings.json, read at
 *  Pro spawn time.  The actual HTTP traffic lives entirely in Pro (fleet_push.rs);
 *  Free only stores the config and passes it over IPC.
 *  Mirrors FleetSettings in commander-free/src/settings.rs (camelCase). */
export interface FleetSettings {
  /** false = agent never starts */
  enabled: boolean;
  /** e.g. "http://fleet.corp.ts.net:8787" */
  serverUrl: string;
  /** enable command poll + verify */
  dispatch: boolean;
  /** base64 Ed25519 fleet public key */
  signingKeyPub: string;
  /** True only for a Privacy Shield session that Fleet started. Kept for
   *  back-compat display text; no longer the sole gate on the Stop button —
   *  see `shieldDesiredState.enabled`, which is derived from "is the shield
   *  fleet-mandated on right now" and does not depend on restart history. */
  privacyShieldSessionOwned?: boolean;
  /** Resolved Privacy Shield desired state from the fleet server's
   *  `/v1/agents/checkin` response (`CheckinResponse.shield_state`) — a
   *  SEPARATE, non-`policy_epoch`-versioned channel (toggling the shield
   *  never bumps the policy version). `null`/absent = not fleet-managed,
   *  or the connected server predates this field. */
  shieldDesiredState?: { enabled: boolean; mode: 'blur_notify' | 'notify_only'; updatedAt: string; commandId?: string | null } | null;
}

/** Paid: one reusable metric alert. Hysteresis and sustained-breach
 *  suppressors are independently toggleable. Mirrors MetricAlertSettings
 *  in settings.rs. Threshold unit depends on the metric (CPU = %, up/down = MB/s). */
export interface MetricAlertSettings {
  enabled: boolean;
  threshold: number;
  hysteresisEnabled: boolean;
  hysteresisPct: number;
  sustainedEnabled: boolean;
  sustainedSecs: number;
  /** Forward this alert to the Fleet console when it fires. Settings path
   *  `notifications.{cpuUsage,ramUsage,networkUsage}.reportToFleet` —
   *  locked (visible-but-disabled locally) whenever that path appears in
   *  the resolved ConfigEpoch.locked_paths. `upload`/`download` share the
   *  single `networkUsage` fleet path since the server models network as
   *  one alert type. */
  reportToFleet?: boolean | null;
}

/** Paid: the full set of metric alerts. Mirrors MetricAlertsSettings in
 *  settings.rs. */
export interface MetricAlertsSettings {
  cpu: MetricAlertSettings;
  ram: MetricAlertSettings;
  upload: MetricAlertSettings;
  download: MetricAlertSettings;
}

/** Paid: decoy-mode preference. Mirrors DecoyModeSettings in settings.rs. */
export interface DecoyModeSettings {
  enabled: boolean;
  displayName: string;
}

/** #10: AI Security Advisor (local Ollama). */
export interface AdvisorSettings {
  /** Ollama model tag the advisor uses. Default: "qwen3.5:4b".
   *  Qwen3.5 line is tagged qwen3.5:* in Ollama (verify the exact tag). */
  model: string;
}

export interface NotificationSettings {
  position: string;
  timeout: number;
}

export interface QuickMountSlot {
  /** File is the legacy/default target. Partition targets store the Windows
   * device path returned by the encrypted-volume picker. */
  targetType?: "file" | "partition";
  filePath: string;
  /** How this slot is presented once mounted: a drive letter (default) or a
   *  folder mount point. A slot saved before this field existed has no
   *  `mountTarget` at all — it is interpreted as `"letter"`, using
   *  `driveLetter` exactly as before, so old Quick Mount slots keep working
   *  unchanged. */
  mountTarget?: "letter" | "folder";
  driveLetter: string;
  /** Folder mount point path. Only meaningful when `mountTarget === "folder"`;
   *  absent for letter-target and legacy slots. Folder mounts require
   *  machine scope (see `VaultSettings.mountScope`) — a per-user mount has
   *  no folder-mount equivalent. */
  mountPoint?: string;
}

export interface VaultSettings {
  defaultMountLetter: string | null;
  recentPaths: string[];
  /** One or more saved vault slots for the sidebar Quick Mount button.
   *  Each slot stores a file path + drive letter; clicking Mount only
   *  needs a password at that point. */
  quickMountSlots?: QuickMountSlot[];
  /** Auto-create a RAM disk on PC startup. When `enabled` is true, the
   *  frontend invokes createRamDisk with the embedded spec right after
   *  the splash dismisses (and the ImDisk engine is confirmed installed).
   *  A saved size is required before startup will create a disk.  This avoids
   *  silently creating the 256 MB minimum when a legacy/partial setting is
   *  encountered. */
  ramdiskAutostart?: RamDiskAutostartSettings;
  /** Preferred mount scope for new mounts: who can see the drive once it's
   *  mounted. `"auto"` (default, and the meaning of an absent value) resolves
   *  to per-user on Windows Server / multi-session SKUs and per-machine on a
   *  single-user desktop; `"machine"`/`"per-user"` override auto-detection
   *  explicitly. Admin-lockable — a Fleet policy can pin this via
   *  `policy.lockedPaths` (path `"vault.mountScope"`), in which case the
   *  Settings UI shows the pinned value disabled rather than hiding it. */
  mountScope?: "auto" | "machine" | "per-user";
}

export interface RamDiskAutostartSettings {
  enabled?: boolean;
  sizeMB?: number;
  driveLetter?: string;
  filesystem?: "NTFS" | "FAT32" | "exFAT";
  label?: string;
  readOnly?: boolean;
  /** When true, disables autostart on the next launch after a lockdown runs. */
  skipAfterLockdown?: boolean;
}

export interface FirstRunSettings {
  selectedBlocklists: string[];
}

// ── Privacy ──────────────────────────────────────────────────────────

/** M8: Session Assurance detector knobs (mirrored from Pro attention_collector
 *  start args). All fields optional — Pro fills in safe defaults for anything
 *  absent. */
export interface SessionAssuranceDetectorSettings {
  /** Which CV model weight to load: "nano" | "small" | "medium". Default "nano". */
  modelLevel?: 'nano' | 'small' | 'medium' | null;
  /** Enable gaze-direction attention check. Default true. */
  checkGaze?: boolean | null;
  /** Enable multi-face / shoulder-surf detection. Default true. */
  checkFaces?: boolean | null;
  /** Enable secondary-device (phone screen) detection. Default false. */
  checkSecondaryDevice?: boolean | null;
  /** When true, alerts are tracked and persisted but on-screen notifications
   *  are suppressed. Detection and scoring continue unchanged. Default false. */
  silentMode?: boolean | null;
}

/** M8: Session Assurance settings — paid insider-risk / attention monitoring. */
export interface SessionAssuranceSettings {
  /** Master enable for the panel and background monitor. */
  enabled: boolean;
  /** CV detector configuration passed to start_session_monitor. */
  detector: SessionAssuranceDetectorSettings;
}

export interface PrivacySettings {
  telemetry: TelemetrySettings;
  clipboard: ClipboardSettings;
  tracking: TrackingSettings;
  lockscreen: LockScreenSettings;
  appCapabilities: AppCapabilitySettings;
  /** F-2: filesystem honeypots — plausibly-named decoy files watched
   *  for any modify / rename / delete events. Off by default. */
  decoyMonitor: DecoyMonitorSettings;
  /** F-3: anti-ransomware behavioural monitor — mass-modify detection
   *  over user-content folders. Off by default. */
  ransomwareMonitor: RansomwareMonitorSettings;
  /** #4: remote-access monitor — detect active incoming remote-control
   *  sessions (AnyDesk / TeamViewer / RustDesk / VNC / RDP / Quick Assist).
   *  Paid. Off by default. */
  remoteAccessMonitor: RemoteAccessMonitorSettings;
  /** Paid Security-log sign-in anomaly monitor. */
  authAnomalyMonitor?: AuthAnomalyMonitorSettings;
  /** USB security monitor arm preferences. Parameters/allow-lists are stored
   *  separately by their bounded Rust modules; these booleans drive re-arm. */
  usbSecurity?: UsbSecurityMonitorSettings;
  /** #5: screen-capture detection (best-effort tool-presence) + own-window
   *  capture protection (SetWindowDisplayAffinity). Paid. */
  screenCapture: ScreenCaptureSettings;
  /** F-5: silent panic via typed code-phrase. Paid. */
  coercionPhrase: CoercionPhraseSettings;
  /** F-6: file-system event lockdown trigger (file created/deleted at path). Paid. */
  fileWatchTrigger?: FileWatchTriggerSettings;
  /** Customisable self-destruct configuration. Driven by
   *  src/types/lockdownSteps.ts step registry. All lockdown triggers
   *  (sidebar button, Ctrl+Shift+Q, lockdown words)
   *  honour the same config. */
  selfDestruct: SelfDestructSettings;
  /** Calculator PIN gate — three hashed PINs for Real / Decoy / Destroy modes. */
  startupPin?: {
    enabled?:     boolean;
    realHash?:    string;
    decoyHash?:   string;
    destroyHash?: string;
  };
  /** Distress phrases — keyboard hook (C) + command palette (D). */
  distressPhrases?: Array<{
    label:  string;
    hash:   string;
    length: number;
    mode:   'decoy' | 'destroy';
  }>;
  /** Internet communication restrictions (~20 registry keys) */
  internetCommunication: InternetCommunicationSettings;
  privacyProtectionEnabled: boolean | null;
  setupCompletionNagsDisabled: boolean | null;
  /** Anti-Acquisition Defenses: continuous WARN-mode watcher toggle —
   *  polls the existing read-only Scan-AcquisitionThreats detector from
   *  the frontend and warns on a hit. Off by default. */
  acquisitionWatchEnabled: boolean | null;
  privacyShield: PrivacyShieldSettings;
  /** M8: Session Assurance (insider-risk / attention monitoring). Paid.
   *  Absent = feature not configured (disabled by default). */
  sessionAssurance?: SessionAssuranceSettings;
  /** DN-09: Advanced Activity Reduction — expert-only toggles that stop the
   *  OS from recording execution / device / network activity in the first
   *  place. Backed by tweaks/prevention.ps1 (Disable-* / Enable-* /
   *  Get-ActivityReductionStatus). */
  prevention: ActivityReductionSettings;
  /** Per-browser extension preferences for browser hardening. Keys use
   *  `${browserName}::${slug}` (see browserExtensions.ts); legacy slug-only
   *  keys remain readable as a migration fallback. */
  browserExtensions?: Record<string, boolean>;
}

/** DN-09: Advanced Activity Reduction. Each flag is `true` when the OS is
 *  configured to NOT record that activity (i.e. the Disable-* tweak is
 *  applied). Mirrors the keys returned by Get-ActivityReductionStatus. */
export interface ActivityReductionSettings {
  /** Verbose audit sub-categories (process/logon/file-share/PnP) reduced. PAID. */
  auditReductionDisabled: boolean | null;
  /** BAM/DAM background activity monitor service disabled. */
  bamDisabled: boolean | null;
  /** AppCompatCache / ShimCache generation disabled. */
  shimCacheDisabled: boolean | null;
  /** UserAssist launch-count/timestamp tracking disabled. */
  userAssistDisabled: boolean | null;
  /** setupapi + USB-storage device-event logging disabled. */
  usbEventLogDisabled: boolean | null;
  /** WLAN SSID/Wi-Fi history UI collection disabled. */
  ssidHistoryDisabled: boolean | null;
  /** Storage / partition diagnostic ETW channels disabled. */
  storageEventLogDisabled: boolean | null;
  /** Extra Explorer recent-activity tracking (WordWheel/search) disabled. */
  recentActivityDisabled: boolean | null;
  /** DNS/SMB/RDP/profile diagnostic ETW channels disabled. */
  diagnosticChannelDisabled: boolean | null;
  /** Reliability Monitor (RACAgent) disabled. */
  reliabilityMonitorDisabled: boolean | null;
  /** Windows Error Reporting (WER) disabled. */
  werDisabled: boolean | null;
  /** PowerShell script-block + module logging disabled. PAID. */
  scriptBlockLoggingDisabled: boolean | null;
  /** CompatTelRunner / Compatibility Appraiser disabled. */
  telemetryRunnerDisabled: boolean | null;
}

/** Customisable self-destruct configuration. Sparse `steps` map
 *  (step ID → enabled flag); missing keys fall back to the step's
 *  defaultEnabled. See DESTRUCT_STEPS in src/types/lockdownSteps.ts. */
export interface SelfDestructSettings {
  /** Explicit opt-in gate. `undefined`/`false` ⇒ all lockdown paths are
   *  refused before any destructive work begins. Must be `true` for any
   *  trigger (sidebar, hotkey, dead-man's switch, destroy PIN) to fire. */
  enabled?: boolean;
  /** Step ID → enabled override. Missing keys use defaultEnabled. */
  steps?: Record<string, boolean>;
  /** Skip browsers in the System Cleaner step (faster). */
  excludeBrowsers?: boolean | null;
  /** Free the licence seat on the server before erasing. Default false. */
  deactivateLicenseFirst?: boolean | null;
  /** Trigger a graceful Windows shutdown after the cascade. */
  shutdownSystem?: boolean | null;
  /** F6 — arm the reboot-to-USB wipe extension. With `enabled` also true (and a
   *  configured reboot_usb distress mode), the cascade may set UEFI BootNext to a
   *  provisioned wipe-USB and reboot, but only after stage-1 crypto-erase. Default false. */
  rebootToUsbEnabled?: boolean | null;
  /** Absolute paths of folders the user wants securely shredded (single
   *  durable RNG-overwrite pass) before the cascade hands off to the Rust
   *  orchestrator. Runs first so user data is destroyed even if a later
   *  step fails or the machine is yanked mid-cascade. Empty array (or
   *  omitted) = no user folders to shred. */
  shredFolders?: string[];
  /** Local usernames selected for removal during the lockdown cascade. The
   *  `remove_users` destruct step (Pro) securely wipes each account's
   *  user-profile folder (single durable RNG-overwrite pass), then deletes
   *  the profile and the local account. Built-in, currently-signed-in, and
   *  current accounts are always skipped server-side. Omitted/empty = no
   *  users removed. */
  usersToRemove?: string[];
  /** Absolute paths of VeraCrypt containers the `veracrypt_header_destroy`
   *  destruct step crypto-erases on any lockdown trigger. An unmounted
   *  container has no OS-visible trace to auto-discover, so this must be
   *  pre-configured (see CryptoEraseTargetsSection). Omitted/empty = the
   *  step cleanly skips — nothing configured, nothing erased. */
  cryptoEraseVeracryptPaths?: string[];
  /** Raw VeraCrypt partitions selected for lockdown header destruction. Each
   *  entry carries the storage identity reviewed in the UI; the Pro handler
   *  re-probes and matches every field immediately before opening the device. */
  cryptoEraseVeracryptDevices?: VeraCryptDeviceEraseTarget[];
  /** Drive letters (e.g. "C:") the `bitlocker_erase` destruct step removes
   *  key protectors from on any lockdown trigger. Omitted/empty = the step
   *  cleanly skips. Selecting the system drive here means it WILL be
   *  targeted on the next trigger — the destroy-PIN/trigger itself is the
   *  confirmation; there is no separate typed nuclear-ack for this
   *  automated path the way the manual Crypto-Erase picker has. */
  cryptoEraseBitlockerDrives?: string[];
}

export interface VeraCryptDeviceEraseTarget {
  devicePath: string;
  diskNumber: number;
  partitionNumber: number;
  partitionGuid: string;
  offsetBytes: number;
  sizeBytes: number;
  diskUniqueId: string;
  label: string;
}

export interface InternetCommunicationSettings {
  /** Blocks publishing wizard, web services, handwriting sharing, HTTP printing, etc. */
  restrictedEnabled: boolean | null;
}

export interface TelemetrySettings {
  windowsDisabled: boolean | null;
  officeDisabled: boolean | null;
  powershell7Disabled: boolean | null;
  copilotDisabled: boolean | null;
  activityHistoryDisabled: boolean | null;
  locationTrackingDisabled: boolean | null;
  windowsSuggestionsDisabled: boolean | null;
}

/** F-1: which pattern categories the paste monitor fires for. All on by
 *  default; a user-controllable knob so people can mute groups they
 *  don't care about (e.g. silence Crypto if they don't deal with
 *  wallets) without disabling the whole watcher. */
export interface PasteMonitorCategories {
  /** AWS, Google API, SendGrid, Mailgun, Twilio, DB connection URLs. */
  cloudApi: boolean | null;
  /** OpenAI, Anthropic. */
  aiApi: boolean | null;
  /** GitHub PAT/fine-grained, NPM tokens. */
  devTools: boolean | null;
  /** Stripe, Slack, Discord. */
  paymentComms: boolean | null;
  /** PEM, OpenSSH, JWT, Bitcoin WIF. */
  keysAndCrypto: boolean | null;
  /** Credit cards (Luhn-validated). */
  personalData: boolean | null;
  /** ClickFix / pastejacking — encoded PowerShell, mshta web payloads,
   *  iex-irm, curl-pipe-shell. Defends against the "press Win+R and
   *  paste this to verify you're human" malware vector. */
  maliciousCommand: boolean | null;
  /** Unicode anomalies — homoglyph URLs (e.g. `pаypal.com` with Cyrillic
   *  'а'), zero-width chars in code-like contexts, bidi-override chars
   *  used for filename / display spoofing. */
  unicode: boolean | null;
}

export interface ClipboardSettings {
  historyDisabled: boolean | null;
  cloudSyncDisabled: boolean | null;
  // autoEraseSchedule removed — schedule lives in Windows Task Scheduler
  // (Set-AutoEraseSchedule categoryId='clipboard') and is queried via
  // getAutoEraseSchedules(), not persisted in settings.
  /** F-1: clipboard credential watcher (AWS keys, JWTs, GitHub PATs, etc).
   *  Polls every 750 ms; on match emits a toast + native Windows
   *  notification. Clipboard contents never cross the IPC boundary —
   *  only the matched pattern's display name. */
  pasteMonitorEnabled: boolean | null;
  /** Per-category enable/disable for the paste monitor. Null = use
   *  defaults (all on). */
  pasteMonitorCategories: PasteMonitorCategories | null;
  /** `monitor.paste.crypto-swap` (paid): detect clipboard-hijack
   *  malware that silently overwrites a copied crypto address with the
   *  attacker's. Null = backend default (on). */
  pasteMonitorCryptoSwapEnabled: boolean | null;
  /** `monitor.paste.auto-expire` (paid): clear the clipboard N seconds
   *  after a detection, only if the content hasn't changed in the
   *  meantime. Null = backend default (off). */
  pasteMonitorAutoClearEnabled: boolean | null;
  /** `monitor.paste.auto-expire`: seconds to wait before clearing.
   *  Backend clamps to [5, 600]. Null = backend default (30). */
  pasteMonitorAutoClearSeconds: number | null;
  /** Forward a content-free Clipboard Monitor match summary to Fleet. When
   *  `privacy.clipboard.pasteMonitorReportToFleet` is managed, this is
   *  visible but cannot be changed locally. Clipboard contents never leave
   *  the device. */
  pasteMonitorReportToFleet?: boolean | null;
  /** `monitor.paste.clear-on-lock` (free): erase the clipboard the
   *  moment the workstation locks (Win+L / screen-lock). Fires on the
   *  unlocked→locked transition only. Null = backend default (off). */
  pasteMonitorAutoClearOnLock?: boolean | null;
}

/** F-2: filesystem honeypots. Plausibly-named decoy files
 *  (passwords.txt, bitcoin-wallet.txt, ...) that the user never
 *  touches. Any modify / rename / delete event fires a danger-severity
 *  notification — early warning that malware or a person is poking
 *  through sensitive-looking files. */
export interface DecoyMonitorSettings {
  /** Master enable. Watcher idle when off (no filesystem polling). */
  enabled: boolean | null;
  /** Absolute paths the user has enrolled. Persisted here; the runtime
   *  watcher set is rebuilt from this on app start via the global
   *  hook. */
  enrolledPaths: string[] | null;
  /** Opt-in Windows Security Event Log read detection. Off by default. */
  readAuditEnabled?: boolean | null;
  /** Send a coarse, path-free tripwire alert to the enrolled Fleet. */
  fleetAlertEnabled?: boolean | null;
}

/** F-5: coercion code-phrase trigger. Pre-registered phrases (stored
 *  as SHA-256 digests, never plaintext) that fire the panic flow when
 *  typed system-wide. PAID. */
export interface CoercionPhraseEntry {
  /** User-visible label so the row can be removed without seeing the
   *  plaintext. Free-form. */
  label: string;
  /** SHA-256(salt || normalised plaintext), hex-encoded. */
  hash: string;
  /** Length of the phrase in normalised chars — drives which
   *  buffer-tail length the matcher checks. */
  length: number;
}

export interface CoercionPhraseSettings {
  enabled: boolean | null;
  /** List of registered phrase digests. Plaintext is never stored. */
  phrases: CoercionPhraseEntry[] | null;
}

/** F-6: file-system event lockdown trigger. Watches configured paths for
 *  file creation or deletion matching a name pattern. Any match immediately
 *  emits `lockdown-trigger`. PAID. */
export interface FileWatchRule {
  id: string;
  /** Absolute directory path to watch (non-recursive). */
  path: string;
  /** Filename pattern — exact name or glob with * (e.g. "*.pdf", "secret.txt"). */
  namePattern: string;
  /** "created" fires on new file; "deleted" fires on removal. */
  event: 'created' | 'deleted';
  enabled: boolean;
}

export interface FileWatchTriggerSettings {
  enabled: boolean | null;
  rules: FileWatchRule[];
}

/** F-3: anti-ransomware behavioural monitor. Watches user-content
 *  directories (Documents, Pictures, Desktop, Downloads) recursively
 *  for mass-modification patterns. If more than `threshold` files are
 *  modified within `windowSeconds`, fires a loud notification — the
 *  textbook ransomware signature. */
/** F-3 v2 automated response (Pro ETW path only). "monitor" alerts and
 *  names the offending process; "suspend" freezes it (reversible — the
 *  roadmap "suspend-by-default"); "kill" terminates it. The notify
 *  fallback can't attribute a PID, so it always behaves as "monitor". */
export type RansomwareAction = "monitor" | "suspend" | "kill";

export interface RansomwareMonitorSettings {
  /** Master enable. Watcher idle when off. */
  enabled: boolean | null;
  /** File-modify count that triggers detection. Default 50. Clamped
   *  10..=500 in Rust. */
  threshold: number | null;
  /** Sliding window length in seconds. Default 30. Clamped 5..=300. */
  windowSeconds: number | null;
  /** Suppress repeated alerts/actions after a detection while monitoring
   *  continues. Default 300 seconds. Clamped 30..=3600. */
  alertCooldownSeconds?: number | null;
  /** Distinct files one process must touch before Pro names or acts on it.
   *  Default 5. Clamped 3..=threshold. */
  attributionMinFiles?: number | null;
  /** User-added extra watch directories. Standard set (Documents,
   *  Pictures, Desktop, Downloads) is always watched in addition. */
  customWatchDirs: string[] | null;
  /** F-3 v2: automated response on the Pro ETW path. Default "suspend".
   *  Null = backend default. */
  action?: RansomwareAction | null;
  /** Send a coarse, path-free ransomware alarm to the enrolled Fleet. */
  reportToFleet?: boolean | null;
}

/** #4: remote-access monitor persistence layer. Runtime authority lives
 *  in the Pro module's TOOL_CATALOGUE + RUNNING; this is the disk side the
 *  global hook reconciles into Pro on every settings change. */
export interface RemoteAccessMonitorSettings {
  /** Master ON/OFF. Null = backend default (off). */
  enabled: boolean | null;
  /** Per-tool enable overrides keyed by catalogue id ("teamviewer",
   *  "anydesk", …). Missing key = tool enabled by default. Null = all on. */
  tools?: Record<string, boolean> | null;
}

export type AuthAnomalyTimeBasis = "local" | "utc";

/** Access & Session Monitor policy. Null values retain the conservative
 * sidecar defaults so settings created before this policy stay compatible. */
export interface AuthAnomalyMonitorSettings {
  enabled?: boolean | null;
  failedBurstThreshold?: number | null;
  failedBurstWindowSecs?: number | null;
  workStartHour?: number | null;
  workEndHour?: number | null;
  /** ISO weekday numbers: Monday=1 through Sunday=7. Missing = weekdays. */
  workDays?: number[] | null;
  /** Device local time or UTC for off-hours schedule evaluation. */
  timeBasis?: AuthAnomalyTimeBasis | null;
  detectRdp?: boolean | null;
  detectNewAccounts?: boolean | null;
  detectOffHours?: boolean | null;
  alertDebounceSecs?: number | null;
  reportToFleet?: boolean | null;
}

export interface UsbSecurityMonitorSettings {
  /** Device attach/detach monitor. Also armed when a dependent guard is on. */
  monitorEnabled?: boolean | null;
  /** Pro low-confidence USB HID timing-anomaly detector (alert-only). */
  hidGuardEnabled?: boolean | null;
  /** Pro removable-volume transfer intelligence. */
  meteringEnabled?: boolean | null;
  /** Observe/enforce new-device auto-isolation according to its saved mode. */
  autoSandboxEnabled?: boolean | null;
  /** Pro reactive keyboard approval boundary. Pending devices remain blocked
   *  unless an operator chooses Allow once or Always trust. */
  hidApprovalGateEnabled?: boolean | null;
  /** Seconds before an unresolved keyboard approval defaults to blocked. */
  hidApprovalTtlSecs?: number | null;
}

/** #5: screen-capture detection + own-window capture protection. Paid. */
export interface ScreenCaptureSettings {
  /** Run the Pro poller that watches for known screen-capture tools.
   *  Best-effort, off by default. */
  detectionEnabled: boolean | null;
  /** Apply WDA_EXCLUDEFROMCAPTURE to WinCommander's own window so it
   *  renders black in screenshots/recordings/share. Off by default. */
  protectWindow: boolean | null;
  /** Forward each screen-capture-tool detection to the Fleet console.
   *  Settings path `notifications.screenCapture.reportToFleet` — locked
   *  (visible-but-disabled locally) whenever that path appears in the
   *  resolved ConfigEpoch.locked_paths. */
  reportToFleet?: boolean | null;
}

export interface TrackingSettings {
  recentFilesDisabled: boolean | null;
  jumpListsDisabled: boolean | null;
  thumbnailCacheDisabled: boolean | null;
  pagefileDisabled: boolean | null;
  // rdpAutoEraseSchedule + eventLogAutoEraseSchedule removed — see the
  // Privacy Clean per-card scheduler (Set-AutoEraseSchedule). Their
  // state lives in Windows Task Scheduler.
  /** Windows Recall AI snapshots disabled */
  recallSnapshotsDisabled: boolean | null;
  /** Autocorrect, spellcheck, text prediction, typing insights disabled */
  typingInsightsDisabled: boolean | null;
  /** Windows Advertising ID fully nuked */
  advertisingIdDisabled: boolean | null;
  /** Tailored Experiences with diagnostic data disabled */
  tailoredExperiencesDisabled: boolean | null;
  /** Office Click-to-Run logging/telemetry disabled */
  officeLoggingDisabled: boolean | null;
  /** Diagnostic Event Tracing (AutoLogger/DiagTrack) disabled */
  diagnosticEventTracingDisabled: boolean | null;
  /** Auto-disconnect RDP sessions on mouse/keyboard inactivity */
  rdpIdleDisconnectEnabled: boolean | null;
  /** Timeout in seconds before disconnecting idle RDP sessions (default 120) */
  rdpIdleDisconnectTimeout: number | null;
  /** Warning countdown shown before idle RDP disconnect fires (default 5) */
  rdpIdleWarningSeconds?: number | null;
  /** Clear RDP history and cache when session is auto-disconnected */
  rdpClearCacheOnDisconnect: boolean | null;
  /** Remove saved RDP credentials from Windows Vault when session is auto-disconnected */
  rdpRemoveCredsOnDisconnect: boolean | null;
  /** Dismount local encrypted volumes when RDP idle-disconnect fires */
  rdpDismountVaultsOnDisconnect?: boolean | null;
  /** Save a log entry each time an idle-disconnect fires */
  rdpSaveLog: boolean | null;
  /** Hide "Recent files" group in File Explorer Quick Access (HKCU ShowRecent=0) */
  quickAccessRecentDisabled: boolean | null;
  /** Hide "Frequent folders" group in File Explorer Quick Access (HKCU ShowFrequent=0) */
  quickAccessFrequentDisabled: boolean | null;
  /** Win+R history dropdown disabled (HKCU NoRunMRU=1).
   *  Lowercase 'mru' to match serde camelCase output —
   *  `run_mru_disabled` -> `runMruDisabled`, not `runMRUDisabled`. */
  runMruDisabled: boolean | null;
  /** Windows Search box "recent searches" disabled (HKCU IsDeviceSearchHistoryEnabled=0) */
  searchHistoryDisabled: boolean | null;
  /** PSReadLine persistence disabled in Windows PowerShell and PowerShell 7 profiles. */
  terminalHistoryDisabled: boolean | null;
}

export interface LockScreenSettings {
  privacyDisabled: boolean | null;
}

export type CapabilityAccess = 'Allow' | 'Deny' | null;

export interface AppCapabilitySettings {
  webcam: CapabilityAccess;
  microphone: CapabilityAccess;
  location: CapabilityAccess;
  contacts: CapabilityAccess;
  calendar: CapabilityAccess;
  callHistory: CapabilityAccess;
  phoneCall: CapabilityAccess;
  email: CapabilityAccess;
  messaging: CapabilityAccess;
  radios: CapabilityAccess;
  bluetoothSync: CapabilityAccess;
  appDiagnostics: CapabilityAccess;
  documents: CapabilityAccess;
  pictures: CapabilityAccess;
  videos: CapabilityAccess;
  fileSystem: CapabilityAccess;
  notifications: CapabilityAccess;
  gazeInput: CapabilityAccess;
  userAccountInformation: CapabilityAccess;
}

export interface PrivacyShieldSettings {
  /** Signed Fleet policy: enable local-only attention monitoring. */
  fleetMonitoringEnabled?: boolean | null;
  /** Signed Fleet policy owns start/stop and locks local controls. */
  fleetManaged?: boolean | null;
  /** Maximum Fleet alert notifications per rolling window; 0 means unlimited. */
  fleetNotificationLimit?: number | null;
  /** Rolling-window duration for Fleet alerts, in seconds. */
  fleetNotificationWindowSeconds?: number | null;
  gazeDetectionEnabled: boolean | null;
  antiPeepingEnabled: boolean | null;
  cameraHunterEnabled: boolean | null;
  confidenceThreshold: number | null;
  wakeDelaySeconds: number | null;
  blurOpacity: number | null;
  modelSize: 'nano' | 'small' | 'medium' | 'large' | null;
  detectionBufferFrames: number | null;
  captureOnDevice: boolean | null;
  captureOnMultiFace: boolean | null;
  captureSpeed: number | null;
  deviceWakeMultiplier: number | null;
  multiFaceWakeMultiplier: number | null;
  /** When true, Privacy Shield starts automatically on app launch.
   *  Mirrors `ideal.privacy.privacy_shield.autostart` in settings.rs. Default false. */
  autostart?: boolean;
  /** Local (non-fleet-managed) mode choice: "blur_notify" (default — blur the
   *  screen AND notify) or "notify_only" (Windows notification only, no
   *  visual blur). Mutually exclusive — see the card's segmented toggle.
   *  Ignored when `app.fleet.shieldDesiredState` is present (fleet mode wins). */
  notifyMode?: 'blur_notify' | 'notify_only' | null;
}

// ── Tweaks ───────────────────────────────────────────────────────────

export interface TweakSettings {
  security: SecurityTweaks;
  os: OsTweaks;
  ui: UiTweaks;
  bootKernel: BootKernelTweaks;
  /** Performance / gaming-responsiveness tweaks */
  performance: PerformanceTweaks;
  /** Vendor-specific GPU optimisations (AMD/NVIDIA/Intel) */
  gpu: GpuTweaks;
  /** Power-management tweaks (USB selective suspend, CPU throttle, etc.) */
  power: PowerTweaks;
  /** RDP host stability + incoming idle sign-out */
  rdp?: RdpTweaks;
  /** Windows Server SKU logon, credential, and file-server hardening */
  server?: ServerTweaks;
  /** Active power plan. 'ultimate' lazily duplicates Microsoft's
   *  Ultimate Performance scheme (e9a42b02-…) when first selected.
   *  Single source of truth — OsTweaks.powerPlan was removed as dead code. */
  powerPlan: PowerPlanMode | null;
  /** Desired state for the deeper, per-component Windows AI cleanup controls.
   *  Kept separately from the broad Copilot/AI policy toggle because every
   *  item can be enabled or restored independently. */
  aiComponentCleanup?: Partial<Record<AiComponentCleanupOperation, boolean>>;
  maintenanceRuns?: Record<string, MaintenanceRunInfo>;
}

export type PowerPlanMode = 'powersaving' | 'balanced' | 'performance' | 'ultimate';

export type AiComponentCleanupOperation =
  | 'package-guard'
  | 'appx-packages'
  | 'recall-feature'
  | 'cbs-packages'
  | 'ai-files'
  | 'scheduled-tasks'
  | 'update-cleanup';

export interface PerformanceTweaks {
  /** MMCSS gaming profile (SystemResponsiveness=10, NetworkThrottlingIndex off, Games priority) */
  mmcssGamingProfile: boolean | null;
  /** Keyboard latency optimised (KeyboardDelay=0, KeyboardSpeed=31) */
  keyboardLatencyOptimised: boolean | null;
  /** Num Lock on at boot (InitialKeyboardIndicators=2) */
  numLockOnBoot: boolean | null;
  /** Hardware-Accelerated GPU Scheduling on (HwSchMode=2) */
  gpuSchedulingEnabled: boolean | null;
  /** SvcHostSplitThresholdInKB tuned to physical RAM */
  svcHostSplitOptimised: boolean | null;
  /** Accessibility shortcuts disabled (StickyKeys / ToggleKeys / FilterKeys) */
  accessibilityShortcutsDisabled: boolean | null;
  /** Menus open instantly (MenuShowDelay=0) */
  instantMenuDelay: boolean | null;
  /** Mouse acceleration off (1:1 movement) */
  mouseAccelerationDisabled: boolean | null;
  /** Autocorrect / spellcheck / text prediction disabled */
  autocorrectDisabled: boolean | null;
  /** Enthusiast mode (more details in Explorer copy dialog) */
  enthusiastModeEnabled: boolean | null;
  /** JPEG wallpaper at 100% quality */
  wallpaperFullQuality: boolean | null;
}

export interface GpuTweaks {
  // AMD
  amdUlpsDisabled: boolean | null;
  amdPowerGatingDisabled: boolean | null;
  amdVideoClockGatingDisabled: boolean | null;
  amdAspmDisabled: boolean | null;
  // NVIDIA
  nvidiaDynamicPstateDisabled: boolean | null;
  nvidiaAsyncPstatesDisabled: boolean | null;
  // Intel
  intelAsyncFlipsDisabled: boolean | null;
  intelAdaptiveVsyncDisabled: boolean | null;
}

export interface PowerTweaks {
  /** USB selective suspend disabled across all USB root hubs */
  usbSelectiveSuspendDisabled: boolean | null;
  /** CPU power throttling disabled */
  cpuThrottlingDisabled: boolean | null;
  // Ultimate Performance plan is selected via TweakSettings.powerPlan = "ultimate"
  // — unified with the existing Power Saver / Balanced / Performance picker
  // (see PowerPlanCard) to avoid two UIs for the same setting.
}

export interface MaintenanceRunInfo {
  lastRunAt: string | null;
  runCount: number;
}

/** Windows Server SKU tweaks. Probed by Get-ServerTweakStatus and by the
 *  matching block in settings-bridge.ps1 — the two share this exact shape. */
export interface ServerTweaks {
  /** True when the OS is a Server SKU (ProductType 2 or 3). Read-only probe
   *  output — the three server-only tweaks below no-op on a client SKU. */
  isServerSku: boolean | null;
  /** RDP visual effects are reapplied after logon and RemoteConnect */
  persistentRdpAnimations: boolean | null;
  /** Ctrl+Alt+Del no longer required at the logon screen (DisableCAD=1) */
  ctrlAltDelDisabled: boolean | null;
  /** Logon screen does not prefill the previous username */
  lastSignedInUserHidden: boolean | null;
  /** Machine-level lock after InactivityTimeoutSecs of console idle */
  consoleInactivityLock: boolean | null;
  /** Server-only: suppresses the shutdown "reason" dialog */
  shutdownTrackerDisabled: boolean | null;
  /** Server-only: Server Manager no longer launches at sign-in */
  serverManagerAtLogonDisabled: boolean | null;
  /** Server-only: IE Enhanced Security Configuration turned off */
  ieEnhancedSecurityDisabled: boolean | null;
  /** WDigest pinned to 0 so LSASS holds no cleartext credentials */
  wdigestBlocked: boolean | null;
  /** LSASS runs as a protected process (RunAsPPL) — needs a reboot */
  lsaProtectionEnabled: boolean | null;
  /** LmCompatibilityLevel=5 — NTLMv2 only, LM/NTLMv1 refused */
  legacyNtlmBlocked: boolean | null;
  /** SMB signing required on both the server and client roles */
  smbSigningRequired: boolean | null;
  /** SMBv1 protocol and optional feature removed */
  smb1Disabled: boolean | null;
  /** RemoteRegistry service set to Disabled */
  remoteRegistryDisabled: boolean | null;
}

export interface RdpTweaks {
  /** TCP keep-alive + single-session-per-user on the RDP host */
  keepAlive: boolean | null;
  /** Removes disconnected/idle/connection session timeouts */
  noTimeouts: boolean | null;
  /** QoS DSCP 46 policies prioritise RDP traffic on TCP 3389 */
  qosPriority: boolean | null;
  /** Server-enforced: signs out (not just disconnects) idle INCOMING sessions */
  incomingIdleTimeoutEnabled: boolean | null;
  /** Seconds before an idle incoming session is signed out (10–86400).
   *  Sub-minute is best-effort: Windows enforces idle at ~1-min resolution. */
  incomingIdleTimeoutSeconds: number | null;
  /** @deprecated legacy minute-based field; read for migration only */
  incomingIdleTimeoutMinutes?: number | null;
  /** Dismount local VeraCrypt vaults when all incoming RDP sessions end */
  incomingDismountOnEmpty?: boolean | null;
  /** Sign off incoming RDP sessions when they become disconnected */
  incomingSignOffOnDisconnect?: boolean | null;
  /** Seconds before a disconnected incoming session is terminated (MaxDisconnectionTime).
   *  null = same as idle timeout (current default). 0 = no limit. */
  incomingDisconnectedTimeoutSeconds?: number | null;
}

export interface SecurityTweaks {
  defenderDisabled: boolean | null;
  windowsUpdateDisabled: boolean | null;
  uacDisabled: boolean | null;
  usbWriteProtect: boolean | null;
  usbStorageLockdown: boolean | null;
  consumerFeaturesDisabled: boolean | null;
  /** Windows Remote Assistance invitations disabled. */
  remoteAssistanceDisabled: boolean | null;
  /** Anonymous enumeration of local SAM accounts blocked. */
  anonymousSamEnumerationBlocked: boolean | null;
  /** Virtualization-Based Security + Credential Guard disabled */
  vbsDisabled: boolean | null;
  /** Prevents automatic BitLocker device encryption */
  bitlockerAutoEncryptDisabled: boolean | null;
  /** Disables Windows Platform Binary Table execution */
  wpbtDisabled: boolean | null;
  /** Disables SmartScreen web content evaluation */
  smartScreenDisabled: boolean | null;
  /** Bypasses TPM/CPU/RAM/Storage/SecureBoot checks for future upgrades */
  oobeBypassEnabled: boolean | null;
  /** Disables Game DVR + App Capture */
  gameDvrDisabled: boolean | null;
  /** Firefox policy-based hardening (telemetry off, uBlock, strict tracking) */
  firefoxHardeningEnabled: boolean | null;
  /** Brave hardening (P3A off, no rewards/wallet/VPN/AI chat) */
  braveHardeningEnabled: boolean | null;
  /** Chrome hardening (telemetry off, Privacy Sandbox off, extensions) */
  chromeHardeningEnabled: boolean | null;
  /** Edge hardening (telemetry off, bloat stripped, extensions) */
  edgeHardeningEnabled: boolean | null;
  /** Universal extensions deployed to all detected browsers */
  universalExtensionsDeployed: boolean | null;
  /** Browser extension auto-updates forced (Firefox policy ExtensionUpdate; Chromium force_installed extensions auto-update inherently) */
  browserAutoUpdateForced: boolean | null;
  /** Copilot & AI components removed (APPX + IFEO + policies) */
  copilotAiRemoved: boolean | null;
  /** VSS / System Restore disabled (reducesSecurity — warn required) */
  systemRestoreOff: boolean | null;
  /** Windows Recall AI snapshot recording disabled */
  recallOff: boolean | null;
  /** Kernel crash dumps + WER disabled */
  crashDumpsOff: boolean | null;
  /** Clipboard history disabled */
  clipboardHistoryOff: boolean | null;
  /** Lock screen on resume + AC sleep disabled */
  requirePwOnResume: boolean | null;
  /** Kernel DMA Protection opportunistic-enable (firmware IOMMU required) */
  kernelDmaProtect: boolean | null;
  /** Secure-shred overwrite passes (1–7). Default 1. */
  shredPasses: number | null;
  /** When true, SSD/NVMe targets are forced to 1 pass (no forensic gain from multi-pass on flash). */
  shredMediaAwareEnabled: boolean | null;
  /** Wipe MFT-resident data region + file-slack on shredded files. Paid, irreversible, needsAdmin. */
  shredMftSlackEnabled: boolean | null;
  /** RAM-spill control: hibernation off + fast-startup off + ClearPageFileAtShutdown=1. Free. */
  ramSpillControlEnabled: boolean | null;
  /** Feature 5: require BitLocker TPM+PIN on every boot (free, reversible). */
  bitlockerTpmPinEnforce: boolean | null;
  /** Microsoft's auto-updated vulnerable driver blocklist — blocks known-vulnerable
   *  and acquisition drivers (e.g. winpmem.sys) from loading. Paid, requires restart. */
  acquisitionDriverBlocklist: boolean | null;
  /** Blocks known forensic/imaging tools (FTK Imager, KAPE, Autopsy, X-Ways, WinPMEM…)
   *  from launching. Paid. */
  forensicToolBlock: boolean | null;
  /** Lid close fully powers off the machine (powercfg lid-close action = Shut down,
   *  AC + DC) instead of sleeping, so FDE keys leave RAM. Reversible. Paid. */
  lidClosePowerOff: boolean | null;
  /** Data Execution Prevention forced for all processes (Set-ProcessMitigation
   *  system-wide DEP policy). Free, requires restart. */
  depEnabled: boolean | null;
  /** Mandatory ASLR — forces address-space randomization for images not built
   *  with /DYNAMICBASE. Free, requires restart. */
  aslrMandatory: boolean | null;
  /** Bottom-up ASLR with high entropy for virtual memory allocations. Free,
   *  requires restart. */
  aslrBottomUp: boolean | null;
  /** Control Flow Guard enforced system-wide (indirect-call integrity). Free,
   *  requires restart. */
  cfgEnabled: boolean | null;
  /** Terminate a process on heap corruption detection. Free, requires restart. */
  heapIntegrity: boolean | null;
  /** SEH Overwrite Protection (SEHOP) enforced system-wide. Free, requires restart. */
  sehopEnabled: boolean | null;
  /** Microsoft Defender Attack Surface Reduction rules enforced system-wide. Free, applies live. */
  asrRulesEnabled: boolean | null;
  /** Controlled Folder Access — blocks unauthorized apps from modifying protected folders. Free, applies live. */
  controlledFolderAccessEnabled: boolean | null;
  /** Microsoft Defender Network Protection — blocks outbound connections to known-malicious hosts. Free, applies live. */
  networkProtectionEnabled: boolean | null;
}

export interface OsTweaks {
  superfetchDisabled: boolean | null;
  prefetchDisabled: boolean | null;
  hibernationDisabled: boolean | null;
  fastStartupDisabled: boolean | null;
  ntfsOptimizationsEnabled: boolean | null;
  detailedBsodEnabled: boolean | null;
  // (powerPlan moved to TweakSettings root — OsTweaks.powerPlan was unused dead code)
  /** Memory compression disabled (Disable-MMAgent -mc) */
  memoryCompressionDisabled: boolean | null;
  /** Foreground process priority boost (Win32PrioritySeparation=38) */
  win32PrioritySeparation: boolean | null;
  /** Desktop shell processes use high CPU and I/O priority at every user logon. */
  desktopShellPriorityEnabled: boolean | null;
  /** Service kill/hung app timeouts optimized for speed */
  serviceTimeoutsOptimized: boolean | null;
  /** Reserved storage disabled (ShippedWithReserves=0) */
  reservedStorageDisabled: boolean | null;
  /** Windows Automatic Maintenance and scheduled diagnostics execution disabled. */
  automaticMaintenanceDisabled: boolean | null;
  /** Win32 long-path support enabled. Requires compatible applications. */
  win32LongPathsEnabled: boolean | null;
  /** SMB client bandwidth throttling disabled. */
  smbBandwidthThrottlingDisabled: boolean | null;
}

export interface BootKernelTweaks {
  /** Intel TSX enabled for improved transaction performance */
  tsxEnabled: boolean | null;
  /** First sign-in animation disabled */
  firstLogonAnimationDisabled: boolean | null;
  /** Startup sound disabled */
  startupSoundDisabled: boolean | null;
  /** Automatic restart sign-on disabled */
  autoRestartSignonDisabled: boolean | null;
  /** Auto-reboot after BSOD disabled */
  autoRebootOnBsodDisabled: boolean | null;
  /** Crash dump set to small memory dump (64KB) */
  smallMemoryDumpEnabled: boolean | null;
}

export interface UiTweaks {
  classicContextMenu: boolean | null;
  fileExtensionsVisible: boolean | null;
  hiddenFilesVisible: boolean | null;
  galleryHomeRemoved: boolean | null;
  bingSearchDisabled: boolean | null;
  backgroundAppsDisabled: boolean | null;
  notificationsDisabled: boolean | null;
  endTaskOnTaskbar: boolean | null;
  /** Fixes slow media folder loading (FolderType=NotSpecified) */
  folderTypeDiscoveryDisabled: boolean | null;
  /** Removes " - Shortcut" text from shortcuts */
  shortcutSuffixRemoved: boolean | null;
  /** Disables AutoPlay + AutoRun on all drives */
  autoPlayDisabled: boolean | null;
  /** Always-on: Chat, Widgets, Meet Now, Task View, People, News hidden */
  taskbarDebloated: boolean | null;
  /** Always-on: Start menu recommendations disabled */
  startRecommendationsDisabled: boolean | null;
  /** Disables low disk space warnings */
  lowDiskCheckDisabled: boolean | null;
  /** Explorer opens to "This PC" instead of Quick Access */
  explorerOpensThisPc: boolean | null;
  /** Hides OneDrive/sync/ad notifications in Explorer */
  syncProviderNotificationsHidden: boolean | null;
  /** Disables transparency effects + minimize animation */
  transparencyDisabled: boolean | null;
  /** Shows full path in Explorer title bar */
  fullPathInTitleBar: boolean | null;

  // ── Granular UI ────────────────────────────────────────────────────
  /** Show "This PC" icon on desktop */
  desktopIconThisPc: boolean | null;
  /** Show Recycle Bin on desktop */
  desktopIconRecycleBin: boolean | null;
  /** Show user profile folder on desktop */
  desktopIconUserFiles: boolean | null;
  /** Show Network icon on desktop */
  desktopIconNetwork: boolean | null;
  /** Show Control Panel on desktop */
  desktopIconControlPanel: boolean | null;
  /** Shortcut-arrow overlay icon removed (Shell Icons\29 blanked) */
  shortcutArrowRemoved: boolean | null;
  /** Snap-assist flyout (window snap suggestions) disabled */
  snapAssistFlyoutDisabled: boolean | null;
  /** File Explorer compact-mode rows */
  explorerCompactMode: boolean | null;
  /** File-selection checkboxes in Explorer (AutoCheckSelect) */
  explorerCheckboxesEnabled: boolean | null;
  /** Window shake-to-minimise disabled (DisallowShaking=1) */
  windowShakeDisabled: boolean | null;
  /** Show seconds in taskbar clock (ShowSecondsInSystemClock=1) */
  clockSecondsVisible: boolean | null;
}

// ── Network ──────────────────────────────────────────────────────────

export interface NetworkSettings {
  dns: DnsSettings;
  hosts: HostsSettings;
  firewall: FirewallSettings;
  vpnKillSwitch?: VpnKillSwitchSettings;
  /** Rogue-AP detector policy and its device-local trusted-network baseline. */
  wifiGuard?: WifiGuardSettings;
}

export interface WifiGuardBaselineEntry {
  /** Stored locally only; never included in a Fleet alert. */
  ssid: string;
  /** BSSIDs observed/trusted for this SSID. */
  bssids: string[];
  /** Internal authentication-strength score retained for downgrade detection. */
  bestAuthStrength: number;
}

export interface WifiGuardSettings {
  /** Re-arm automatically after WinCommander starts. Paid Pro detector. */
  enabled?: boolean | null;
  /** How long a newly empty baseline observes before alerting (5 min–7 days). */
  learningWindowSecs?: number | null;
  /** UTC deadline for an in-progress learning window; keeps learning honest across restarts. */
  learningUntil?: string | null;
  /** netsh association check cadence (5–300 seconds). */
  pollIntervalSecs?: number | null;
  /** Repeat-alert cooldown for one SSID/BSSID pair (30–3600 seconds). */
  alertDebounceSecs?: number | null;
  /** Device-local baseline; SSIDs/BSSIDs are not sent to Fleet. */
  baseline?: WifiGuardBaselineEntry[] | null;
  /** Forward only a coarse rogue-AP alarm class to Fleet. */
  reportToFleet?: boolean | null;
}

export interface VpnKillSwitchSettings {
  /** When true, a VPN tunnel drop cuts all internet via the internet kill switch. */
  armed?: boolean | null;
  /** "auto" (default) | "tailscale" | "protonvpn" */
  provider?: string | null;
  pollIntervalSecs?: number | null;
}

export interface DnsSettings {
  provider: string | null;
  ipv4Preference: boolean | null;
  swissFirewallConfig: SwissFirewallConfig | null;
  controlDFilterSlug: string | null;
  /** Anti-censorship: when true, outbound plaintext DNS (port 53) is
   *  firewall-blocked so all lookups are forced through the encrypted DoH
   *  resolver. Requires Encrypted DNS to be on. */
  censorshipProtection: boolean | null;
}

export interface SwissFirewallConfig {
  dohId: string | null;
  deviceName: string | null;
}

export interface HostsSettings {
  enabledBlocklists: string[];
}

export interface FirewallSettings {
  lockdownMode: boolean | null;
  managedRules: ManagedFirewallRule[];
}

export interface ManagedFirewallRule {
  name: string;
  direction: 'Inbound' | 'Outbound';
  action: 'Block' | 'Allow';
  protocol: 'TCP' | 'UDP' | 'Any';
  port: string | null;
  enabled: boolean;
}

// ── Identity ─────────────────────────────────────────────────────────

export interface IdentitySettings {
  branding: BrandingSettings;
  stealthModeEnabled: boolean | null;
  /** When true or null (default), Server Apps is hidden from the sidebar. */
  hideServerApps: boolean | null;
  /** When true, hide WinCommander from Start Menu and Installed Apps where possible. */
  hideWinCommander: boolean | null;
  /** Hotkey to peek at WinCommander while hidden. Default "Ctrl+Alt+W". Only active when hideWinCommander is true. */
  hideWinCommanderHotkey: string | null;
  /** When true, the Flows panel is visible in the sidebar. Default: hidden.
   *  Flows is beta — reserved for the upcoming multi-WinCommander
   *  admin orchestration. Toggle on from Settings to opt in early. */
  flowsEnabled: boolean | null;
  /** When true, the Investigator panel is visible in the sidebar. Default: hidden.
   *  Investigator is a specialist cleanup mode — not for general use.
   *  Toggle on from Settings to enable. Requires an advanced-tools licence in prod. */
  advancedToolsEnabled: boolean | null;
  /** When true, the Dashboard Risk Matrix view is available. Pro-gated Settings switch. */
  riskMatrixEnabled: boolean | null;
  /** When true, the Dashboard More Products view is available. Set from the
   *  Help & Setup start step or the Settings "Product Showcase" switch. Not
   *  Pro-gated — the flag controls availability. */
  moreProductsEnabled: boolean | null;
  /** List of backend app IDs that the user wants to hide. */
  hideBackendAppsList?: string[];
}

export interface BrandingSettings {
  companyName: string | null;
  productName: string | null;
  pcName: string | null;
  manufacturer: string | null;
  supportUrl: string | null;
}

// ── App Management ───────────────────────────────────────────────────
// LEARNING: AppManagementSettings lives in BOTH ideal and current SystemState.
// - ideal.apps → admin intent (requiredApps, blockedApps, autoUpdate policy)
// - current.apps → actual PC state (edgeRemoved, onedriveRemoved) + inventory snapshot
// The `inventory` field is ONLY meaningful in `current` — holds persisted winget scan.
// Drift detection compares ideal.apps.requiredApps vs current.apps.inventory.manifestApps.

export interface AppManagementSettings {
  requiredApps: string[];
  blockedApps: string[];
  edgeRemoved: boolean | null;
  onedriveRemoved: boolean | null;
  /** Microsoft Teams removed */
  teamsRemoved: boolean | null;
  /** List of removed APPX package family names */
  removedAppx: string[];
  /** List of deprovisioned apps (prevents reinstall via updates) */
  deprovisionedAppx: string[];

  // ── Admin Policy Fields (meaningful in ideal only) ──
  /** Should this PC auto-update all apps? */
  autoUpdate?: boolean | null;
  /** Only auto-update manifest apps (not random installed software)? */
  autoUpdateManifestOnly?: boolean | null;
  /** Lock specific apps to a version: { "Python.Python.3.12": "3.12.0" } */
  pinnedVersions?: Record<string, string>;
  /** How often to re-scan installed apps (minutes). Default 60. */
  scanIntervalMinutes?: number | null;

  // ── Inventory Snapshot (meaningful in current only) ──
  /** Persisted scan results from Get-AppInventory */
  inventory?: AppInventorySnapshot | null;
}

/** Point-in-time snapshot of all apps on this PC.
 * Written to current.apps.inventory by the scan engine.
 * Sent to admin server via heartbeat (summary + pendingUpdates for lightweight payload). */
export interface AppInventorySnapshot {
  /** When this scan was performed (ISO 8601) */
  lastScanAt: string;
  /** How long the scan took in ms */
  scanDurationMs?: number | null;

  /** Apps from the WinCommander manifest catalog */
  manifestApps: ManifestAppEntry[];

  /** Apps installed but NOT in manifest */
  otherApps: OtherAppEntry[];

  /** Flat list of all apps with pending updates */
  pendingUpdates: PendingUpdateEntry[];

  /** Critical dependency status */
  essentials: EssentialAppsStatus;

  /** Pre-computed counts for admin dashboard */
  summary: AppInventorySummary;
}

export interface ManifestAppEntry {
  id: string;
  name: string;
  /** App description from the manifest catalog (for UI display) */
  description: string;
  category: string;
  installed: boolean;
  installedVersion: string | null;
  latestVersion: string | null;
  updateAvailable: boolean;
  iconData?: string | null;
}

export interface OtherAppEntry {
  id: string;
  name: string | null;
  installedVersion: string | null;
  latestVersion: string | null;
  updateAvailable: boolean;
  iconData?: string | null;
}

export interface PendingUpdateEntry {
  id: string;
  name: string | null;
  installedVersion: string | null;
  latestVersion: string | null;
  /** Package source (e.g. "winget", "msstore") */
  source: string | null;
  inManifest: boolean;
  iconData?: string | null;
}

export interface EssentialAppsStatus {
  meshVpn: MeshVPNEssentialInfo;
  productivityEngine: ProductivityEssentialInfo;
  winget: EssentialAppInfo;
}

export interface EssentialAppInfo {
  installed: boolean;
  version: string | null;
}

export interface MeshVPNEssentialInfo {
  installed: boolean;
  version: string | null;
  connected: boolean | null;
}

export interface ProductivityEssentialInfo {
  installed: boolean;
  running: boolean | null;
}

export interface AppInventorySummary {
  totalInstalled: number;
  manifestInstalled: number;
  manifestTotal: number;
  manifestMissing: number;
  otherInstalled: number;
  updatesAvailable: number;
  essentialsOk: boolean;
}

// ── Productivity ─────────────────────────────────────────────────────

export interface ProductivitySettings {
  trackerEnabled: boolean | null;
  productivityEngineStealthEnabled: boolean | null;
  excludeAfk: boolean | null;
  defaultRange: string | null;
}
// ── Admin Policy ─────────────────────────────────────────────────────

export interface PolicySettings {
  syncMode: 'standalone' | 'managed';
  adminServerUrl: string | null;
  mergeStrategy: 'merge' | 'overwrite';
  lockedPaths: string[];
  lastSyncedAt: string | null;
  masterConfigVersion: number | null;
  organization: string | null;
  /** Base64 Ed25519 fleet signing key pinned at connect (P2). */
  fleetSigningKey?: string | null;
  /** Enrollment-locked by org policy (P5) — in-app Disconnect is refused. */
  managed?: boolean;
  /** Admin pin for `VaultSettings.mountScope`, set locally by the single
   *  administrator on an unmanaged (`syncMode: "standalone"`) install —
   *  read directly by `MountScopeSelector.tsx`. Distinct from the generic
   *  Fleet `lockedPaths` mechanism, which only ever gets populated by a
   *  connected admin server (`apply_admin_config_cmd`): a standalone box has
   *  no server to push that, yet still has exactly one administrator who
   *  needs to stop other signed-in users from switching a shared vault to
   *  per-user scope. Settings are machine-wide (see AGENTS.md), so one write
   *  here pins the effective scope for every user's session. `null`/absent
   *  = not pinned, users choose freely. */
  pinnedMountScope?: "auto" | "machine" | "per-user" | null;
}

// ── Server Apps ──────────────────────────────────────────────────────

export interface ServerAppsSettings {
  apps: ServerAppConfig[];
}

export interface ServerAppConfig {
  id: string;
  name: string;
  url: string;
  icon: string;
  /** Optional CSS injected into the webview to whitelabel the app (hide logos, rename headers, etc.) */
  customCss?: string;
}

// ═══════════════════════════════════════════════════════════════════════
// DEVICE IDENTITY (returned by get_device_identity)
// ═══════════════════════════════════════════════════════════════════════

export interface DeviceIdentity {
  deviceId: string;
  appVersion: string;
  lastSeenAt: string;
  createdAt: string;
  settingsHash: string;
  settingsVersion: number;
  syncMode: string;
  organization: string | null;
  masterConfigVersion: number | null;
}

// ═══════════════════════════════════════════════════════════════════════
// SETTINGS PATCH — Deep partial type for patch_settings
// ═══════════════════════════════════════════════════════════════════════

export type DeepPartial<T> = {
  [P in keyof T]?: T[P] extends (infer U)[]
  ? U[]
  : T[P] extends object
  ? DeepPartial<T[P]>
  : T[P];
};

export type SettingsPatch = DeepPartial<AppSettings>;

// ═══════════════════════════════════════════════════════════════════════
// DRIFT REPORT — Ideal vs Current comparison
// ═══════════════════════════════════════════════════════════════════════

export interface DriftItem {
  path: string;
  idealValue: unknown;
  currentValue: unknown;
  /** PowerShell command that would fix this drift (null if no reverse map) */
  command: string | null;
}
