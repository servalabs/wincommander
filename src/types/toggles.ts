// src/types/toggles.ts
//
// ═══════════════════════════════════════════════════════════════════════
// ToggleDef — The SINGLE SOURCE OF TRUTH for every setting in WinCommander.
// ═══════════════════════════════════════════════════════════════════════
//
// One object = one setting. From this single definition, the ENTIRE app
// auto-derives:
//   - Toggle UI (label, description, switch, risk badge)
//   - Backend wiring (enable/disable/status commands)
//   - Settings.json path for drift detection (ideal vs current)
//   - Privacy Score contribution
//   - Cleanup Score contribution
//   - Dashboard radar scan findings
//   - Help & Setup scan + auto-apply
//   - Feature visibility gating
//   - Fleet compliance weight
//
// BEFORE: This metadata was scattered across 6+ files with conflicting
// values. useSovereigntyScore had different point values than the registry,
// useDashboardRadar had its own SCAN_CHECKS array, setup had
// ANOTHER copy, the old visibility shim had its own level rules...
//
// AFTER: One object. No second opinions. No conflicting numbers.
//
// TO ADD A NEW TOGGLE: Add one object to the appropriate registry file.
// Score, radar, setup, visibility, fleet — all auto-derive. Zero wiring.

import type { IconName } from "@/components/ui/bp";
import type { ExperienceLevel } from "./settings";
import type { CapabilityBundle, Density } from "./persona";
import type { Visibility } from "../lib/visibility";

export interface ToggleModeText {
  label: string;
  description: string;
}

export type ToggleModeTextMap = Partial<Record<ExperienceLevel, ToggleModeText>>;

// ═══════════════════════════════════════════════════════════════════════
// TIER — Commercial classification (drives binary placement + paywall UX)
// ═══════════════════════════════════════════════════════════════════════
//
// "free" → ships in wincommander-free.exe ("WinCommander Free",
//          AGPL-3.0-or-later, AV-clean, no license required).
// "paid" → ships in wincommander-pro.exe ("WinCommander Pro", paid sidecar,
//          governed by the WinCommander EULA (private repo; source
//          on request), requires license JWT entitlement, AV may
//          flag because the sidecar contains Privacy Clean code).
//
// The four risk booleans below (needsAdmin / irreversible / reducesSecurity
// / defenderFlagged) are orthogonal to tier — they drive UX (UAC prompt,
// confirmation dialog with countdown, security-warning copy, AV-exclusion
// install gatewall). Only `tier` determines which binary the feature
// lives in.

export type Tier = "free" | "paid";

// ═══════════════════════════════════════════════════════════════════════
// TOGGLE DEFINITION — One object = one setting in the entire app
// ═══════════════════════════════════════════════════════════════════════

export interface ToggleDef {
  // ── Tier & Risk (REQUIRED — classify every toggle) ──────────────────
  // Five fields classifying the feature on two orthogonal axes:
  //   tier             → commercial: free vs paid (drives binary placement)
  //   needsAdmin       → requires UAC elevation
  //   irreversible     → cannot be undone; UI shows countdown confirm
  //   reducesSecurity  → weakens a Windows security feature; UI warns
  //   defenderFlagged  → AV will flag this code; MUST be tier="paid"
  //
  // Invariants enforced by CI (see plan §12):
  //   - irreversible: true    ⇒ needsAdmin: true
  //   - irreversible: true    ⇒ tier === "paid" (lives in sidecar)
  //   - defenderFlagged: true ⇒ tier === "paid" (must live in sidecar)
  //
  // Note: reducesSecurity is NOT bound to tier — single-registry security
  // tweaks (uac, vbs, smartscreen) are free even when reducesSecurity is
  // true. Risk is handled by the warning-dialog UX, not by paywall.
  //
  // No defaults — all five are required so authors decide explicitly.

  /** Commercial tier. "free" ships in the Free binary; "paid" ships in the Pro sidecar. */
  tier: Tier;

  /** Requires admin / UAC elevation to apply. Examples: registry writes
   *  to HKLM, group-policy registry paths, Set-Service, bcdedit. */
  needsAdmin: boolean;

  /** Cannot be undone once executed. Triggers warning dialog with a
   *  3-second countdown before the confirm button enables. Examples:
   *  shadow copy erases, free-space erases, bulk uninstalls, secure file shred. */
  irreversible: boolean;

  /** Disables or weakens a Windows security feature. Triggers a warning
   *  dialog explaining what protection is being removed. Examples: UAC,
   *  SmartScreen, VBS, Defender real-time, OOBE bypass. */
  reducesSecurity: boolean;

  /** Defender / SmartScreen / VirusTotal will flag this code. By
   *  invariant this means tier MUST be "paid" — defenderFlagged code
   *  cannot ship in the Free binary. Examples: vssadmin clears, cipher
   *  deep erase operations, Defender disablement, Recall DB erase, ETW autologger
   *  disable, secure file shredder, self-destruct. */
  defenderFlagged: boolean;

  // ── Identity ─────────────────────────────────────────────────────────
  /** Unique key within the app. Used as React key, loading map key, and
   *  cross-reference from radar/score/setup. Example: "telemetry" */
  id: string;

  /** Human-readable label shown in the toggle row (uppercase by convention).
   *  Example: "BLOCK TELEMETRY" */
  label: string;

  /** One-line explanation of what this toggle does.
   *  Example: "Disables Windows telemetry services and scheduled tasks" */
  description: string;

  /** Optional experience-level wording overrides kept alongside the card
   *  definition so behavior and copy live in one place. */
  modeText?: ToggleModeTextMap;

  /** What bad thing happens if this setting is OFF. Shown in radar scan
   *  findings and Help & Setup. Example: "Microsoft collects usage
   *  data, typing patterns, and app activity"
   *  Only required when radar: true. */
  impact?: string;

  /** Blueprint icon shown next to the label. Optional. */
  icon?: IconName;

  /** Keywords for search filtering. Optional. */
  keywords?: string[];

  // ── Categorization ───────────────────────────────────────────────────
  /** Which section header this toggle appears under.
   *  Must match a SectionDef.id. Example: "tracking", "ui", "security".
   *  Optional: radar-only entries omit it so they render in NO panel
   *  (they exist purely for the cleanup radar to detect, and are driven
   *  by their own cards via Tauri commands). */
  section?: string;

  /** Top-level domain — drives color tinting and CSS --color-domain-* vars.
   *  Also determines which panel this toggle appears in. */
  domain: "privacy" | "security" | "network" | "tweaks" | "identity";

  // ── Settings.json Paths (THE BACKBONE — SSOT for state) ──────────────
  /** Dot-path into settings.json for the IDEAL (desired) state.
   *  Written when user flips the toggle: "I want this to be true/false"
   *  Example: "ideal.privacy.telemetry.windowsDisabled" */
  settingsPath: string;

  /** Dot-path into settings.json for the CURRENT (probed) state.
   *  Read to determine what the OS actually reports right now.
   *  THIS IS THE SINGLE SOURCE OF TRUTH for toggle checked state.
   *  Example: "current.privacy.telemetry.windowsDisabled"
   *
   *  REQUIRED. Every toggle MUST have this.
   *  ToggleSection reads this path from appSettings via getByPath. */
  currentPath: string;

  // ── Backend Commands ─────────────────────────────────────────────────
  /** Backend command name to ENABLE the setting (make it show "Active").
   *  Sent via executeBackendCommand.
   *  Note: For "Disable X" toggles, enableCmd is "Disable-X"
   *  (confusing but consistent — "enable the privacy protection"). */
  enableCmd: string;

  /** Backend command name to DISABLE the setting (make it show "Standby"). */
  disableCmd: string;

  /** Probe command that checks the current OS state for this setting.
   *  Used during system probe to write to currentPath.
   *  Example: "Get-TelemetryStatus" */
  statusCmd?: string;

  /** For app capability toggles (webcam, microphone, etc.).
   *  Uses Set-AppCapabilityAccess instead of regular commands.
   *  Example: "webcam", "microphone", "broadFileSystemAccess" */
  capabilityKey?: string;

  /** Value in settings.json that means "checked / active".
   *  Default: truthy (any non-null, non-false, non-empty value).
   *  For app capabilities where settings stores "Deny"/"Allow" strings,
   *  set this to "Deny" so getByPath(…) === "Deny" → checked.
   *  Enables ToggleSection to compare against an exact value. */
  checkedWhen?: string;

  // ── UI Behavior ──────────────────────────────────────────────────────
  /** If true, shows "RESTART" badge — explorer/system restart after toggle */
  requiresRestart?: boolean;

  /** If true, the toggle is an action button (not a state toggle).
   *  Actions don't show a switch — they show a "RUN" / "OPEN" badge. */
  isAction?: boolean;

  /** For action toggles: which verb to show. Default: "run" */
  actionType?: "run" | "open";

  /** Visual severity — drives border tinting on the toggle row.
   *  Default: "none" (neutral). Use "danger" for destructive actions. */
  severity?: "none" | "primary" | "success" | "warning" | "danger";

  // ── Visibility & Gating ──────────────────────────────────────────────
  /** Minimum experience level to show this toggle.
   *  - "simple"   → everyone sees it (L1)
   *  - "standard" → standard + advanced users (L2)
   *  - "advanced" → power users only (L3)
   *  Default: "simple" */
  minExperience?: ExperienceLevel;

  /** New density gate. Replaces minExperience during the redesign
   *  migration; legacy rows still map through minExperience. */
  minDensity?: Density;

  /** Capability bundles required for this toggle. Omit for essentials. */
  capability?: CapabilityBundle[];

  /** If true, this toggle is safe for ALL users — won't break any functionality.
   *  Used in radar to show "Recommended" badge vs "Advanced" badge.
   *  Toggles without this flag default to advanced/optional. */
  safeDefault?: boolean;

  // ── Setup ────────────────────────────────────────────────────────────
  /** If true, this toggle is enabled in the ideal state during setup.
   *  Maps to the "ON" prefix in the doc level codes (ON L1, ON L2, etc.).
   *  Help & Setup uses: ALL_TOGGLES.filter(t => t.defaultOn) */
  defaultOn?: boolean;

  /** If true, this setting is forcefully applied during setup regardless
   *  of scan results. For things like Copilot removal, typing insights,
   *  recall snapshots — you don't check, you just do it.
   *  Setup uses: ALL_TOGGLES.filter(t => t.alwaysApplyOnFirstRun) */
  alwaysApplyOnFirstRun?: boolean;

  // ── Dashboard Radar ──────────────────────────────────────────────────
  /** If true, this toggle appears in the dashboard radar scan.
   *  When currentPath reads false/null, a finding is emitted.
   *  Replaces the hardcoded SCAN_CHECKS arrays in useDashboardRadar
   *  and Help & Setup. */
  radar?: boolean;

  /** If true, this radar finding only appears when Privacy Clean mode is enabled. */
  radarRequiresAntiCleanup?: boolean;

  /** If true, this toggle is excluded from the dashboard Needs Attention list
   *  (even when defaultOn/safeDefault would otherwise include it). */
  noNeedsAttention?: boolean;

  /** Radar finding category. Only used when radar: true.
   *  Determines grouping in the scan results. */
  radarCategory?: "privacy" | "performance" | "annoyance";

  /** Radar finding severity. Only used when radar: true.
   *  Drives visual priority in the scan animation. */
  radarSeverity?: "critical" | "warning" | "info";

  // ── Privacy Score ────────────────────────────────────────────────────
  //
  // Privacy Score (100 pts total) — measures how much data Microsoft and
  // third parties can collect from this machine. Shown to all users.

  /** How many Privacy Score points this toggle contributes when active.
   *  Set to 0 or omit to exclude from score. */
  privacyScore?: number;

  /** Which Privacy Score category this toggle counts toward.
   *  Determines where the points show up in the breakdown ring. */
  privacyScoreCategory?: "telemetry" | "surface" | "hardening" | "capabilities";

  // ── Cleanup Score ───────────────────────────────────────────────────
  //
  // Cleanup Score (100 pts max, toggle-state only) — measures how well
  // the machine is *configured* to prevent cleanup artifacts from forming.
  // Only shown when user selected cleanup/zero-trace mode in setup.
  //
  // RULE: Only set on persistent ON/OFF configuration toggles.
  // NEVER set on isAction: true entries.
  //
  // The logic: a score must reflect a durable machine state.
  //   ✓ "Disable Prefetch" — stays off permanently. Scoreable.
  //   ✗ "Clear Prefetch Files" — erases today, new files form tomorrow. Not scoreable.
  //   ✓ "Disable Hibernation" — no hiberfil.sys until re-enabled. Scoreable.
  //   ✗ "Clear Event Logs" — new events generate the moment you open Explorer. Not scoreable.

  /** How many Cleanup Score points this toggle contributes when active.
   *  Only set on persistent toggle state — never on one-shot actions. */
  cleanupScore?: number;

  /** Which Cleanup Score category this toggle counts toward.
   *  - "traces"       → prevents execution/launch fingerprints forming on disk
   *  - "memory"       → prevents memory data landing on disk (hiberfil, pagefile, crash dumps)
   *  - "behavior"     → prevents activity trail persisting across sessions
   *  - "surveillance" → prevents active observation / AI monitoring */
  cleanupScoreCategory?: "traces" | "memory" | "behavior" | "surveillance";

  // ── Dependencies & Conflicts ─────────────────────────────────────────
  /** IDs of toggles that cannot be active simultaneously with this one.
   *  When this toggle is enabled, any listed toggle that is currently
   *  active is automatically disabled. The relationship is checked
   *  symmetrically at runtime — if B lists A here, enabling A also
   *  disables B. Example: "performancePlan" conflicts with "powerSaving". */
  conflictsWith?: string[];

  // ── Extra input collection ───────────────────────────────────────────
  /** When true, turning this toggle ON opens a PIN-entry dialog first —
   *  the enable command isn't dispatched until the user supplies a PIN
   *  matching `pinPattern` (default: 6-20 ASCII digits). Cancelling the
   *  dialog leaves the toggle untouched. Turning the toggle OFF never
   *  prompts. Example: "bitlockerTpmPinEnforce" (Set-BitLockerTpmPin
   *  requires `Pin` when Enable is true). */
  requiresPinOnEnable?: boolean;

  /** Overrides the default 6-20-ASCII-digit PIN validation regex for
   *  `requiresPinOnEnable`. Must stay in sync with the backend guard. */
  pinPattern?: RegExp;

  /** Static params merged into BOTH the enable and disable command calls
   *  (e.g. { Drive: "C:" } for a per-volume BitLocker command). */
  extraCmdParams?: Record<string, string | number | boolean>;

  /** Set when `enableCmd` and `disableCmd` are the SAME backend command and
   *  direction is selected by a boolean param instead of the command name
   *  (e.g. Set-BitLockerTpmPin takes `Enable`). When set, ToggleSection
   *  always passes `{ [enableParamName]: checked }` alongside the command. */
  enableParamName?: string;

  // ── Fleet (future-proof) ─────────────────────────────────────────────
  /** Whether admin can lock this toggle via policy. Default: true.
   *  Almost all toggles are lockable; some UI tweaks might be user-only. */
  lockable?: boolean;

  /** Fleet compliance classification.
   *  - "required"    → device is non-compliant if this is off
   *  - "recommended" → shown as suggestion in compliance report
   *  - "optional"    → user preference, not flagged
   *  Default: "optional" */
  complianceWeight?: "required" | "recommended" | "optional";
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION DEFINITION — Groups toggles under a collapsible header
// ═══════════════════════════════════════════════════════════════════════

export interface SectionDef {
  /** Must match the `section` field on its ToggleDefs */
  id: string;
  /** Display title for the section header */
  title: string;
  /** Blueprint icon for the section header */
  icon?: IconName;
  /** Number of columns in the toggle grid. Default: 1 */
  columns?: 1 | 2 | 3 | 4;
}

// ═══════════════════════════════════════════════════════════════════════
// HELPER — Get a nested value from an object by dot-path
// ═══════════════════════════════════════════════════════════════════════
// Example: getByPath(settings, "current.privacy.telemetry.windowsDisabled")
// Returns the value at that path, or undefined if any segment is missing.

export function getByPath(obj: unknown, path: string): unknown {
  if (!obj || typeof obj !== "object") return undefined;
  const segments = path.split(".");
  let current: unknown = obj;
  for (const segment of segments) {
    if (current === null || current === undefined || typeof current !== "object") {
      return undefined;
    }
    current = (current as Record<string, unknown>)[segment];
  }
  return current;
}

// ═══════════════════════════════════════════════════════════════════════
// HELPER — Set a nested value in an object by dot-path (creates parents)
// ═══════════════════════════════════════════════════════════════════════
// Example: setByPath({}, "ideal.privacy.telemetry.windowsDisabled", true)
// Returns: { ideal: { privacy: { telemetry: { windowsDisabled: true } } } }

export function setByPath(obj: Record<string, unknown>, path: string, value: unknown): Record<string, unknown> {
  const segments = path.split(".");
  let current: Record<string, unknown> = obj;
  for (let i = 0; i < segments.length - 1; i++) {
    const key = segments[i];
    if (!(key in current) || typeof current[key] !== "object" || current[key] === null) {
      current[key] = {};
    }
    current = current[key] as Record<string, unknown>;
  }
  current[segments[segments.length - 1]] = value;
  return obj;
}

// ═══════════════════════════════════════════════════════════════════════
// HELPER — Build the params object for a toggle's enable/disable command
// ═══════════════════════════════════════════════════════════════════════
// Centralizes the merge order (static extraCmdParams < caller-supplied
// extraParams < the direction flag) so ToggleSection and its tests share
// one source of truth. Pure — no IPC, no state.

export function buildToggleCommandParams(
  toggle: Pick<ToggleDef, "extraCmdParams" | "enableParamName">,
  checked: boolean,
  extraParams?: Record<string, string | number | boolean>,
): Record<string, string | number | boolean> {
  const params: Record<string, string | number | boolean> = { ...toggle.extraCmdParams, ...extraParams };
  if (toggle.enableParamName) params[toggle.enableParamName] = checked;
  return params;
}

export function resolveToggleText(toggle: ToggleDef, level: ExperienceLevel): ToggleModeText {
  return (
    toggle.modeText?.[level] ??
    toggle.modeText?.standard ??
    toggle.modeText?.simple ??
    toggle.modeText?.advanced ??
    {
      label: toggle.label,
      description: toggle.description,
    }
  );
}

function resolveLegacyDensity(
  toggle: ToggleDef,
  profiles?: ReadonlySet<CapabilityBundle>,
): Density {
  if (toggle.minExperience === "advanced") {
    return "expert";
  }

  if (toggle.minExperience === "standard" && profiles && toggle.capability?.length) {
    return toggle.capability.some((capability) => profiles.has(capability))
      ? "guided"
      : "expert";
  }

  return "guided";
}

export function getToggleVisibility(
  toggle: ToggleDef,
  profiles?: ReadonlySet<CapabilityBundle>,
): Visibility {
  return {
    minDensity: toggle.minDensity ?? resolveLegacyDensity(toggle, profiles),
    capability: toggle.capability,
    tier: toggle.tier,
  };
}
