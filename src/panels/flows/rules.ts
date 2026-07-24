// rules.ts — the v2 flows data model (TypeScript mirror of flow_core::Rule).
//
// These shapes serialize to the exact JSON the Pro flow engine deserializes, so
// the field names / serde tags here MUST match `flow-core`'s schema.rs. The UI
// builds a Rule, the Free bridge stores it in `app.proFlows`, and Pro parses it.

export type RiskLevel = "low" | "medium" | "high";

export type GazeKind = "look_away" | "no_face" | "multiple_faces" | "secondary_device";

// ── Triggers (serde tag "type") ──────────────────────────────────────────
export type Trigger =
  | { type: "SettingChangedTrigger"; path: string; to?: unknown }
  | { type: "GazeTrigger"; kind?: GazeKind | null }
  | { type: "USBTrigger"; mode: "insert" | "remove"; deviceInstanceId?: string | null; deviceName?: string | null; delaySeconds?: number }
  | { type: "RansomwareMonitorTrigger" }
  | { type: "PasteMonitorTrigger"; pattern_contains?: string | null; severity?: string | null }
  | { type: "DecoyMonitorTrigger"; path_contains?: string | null }
  | { type: "WifiGuardTrigger"; ssid_contains?: string | null };

export type TriggerType = Trigger["type"];

// ── Conditions ───────────────────────────────────────────────────────────
export type Condition =
  | { type: "TimeCondition"; startHour: number; endHour: number }
  | { type: "SettingCondition"; path: string; operator: "==" | "!="; value: unknown }
  | { type: "BatteryCondition"; operator: "<" | "<=" | ">" | ">=" | "==" | "!="; percentage: number };

// ── Actions ──────────────────────────────────────────────────────────────
export type Action =
  | { type: "SetToggleAction"; toggleId: string; on: boolean }
  | { type: "CommandAction"; command: string; params?: Record<string, string> | null }
  | { type: "NotifyAction"; message: string; severity: string; duration?: number | null }
  | { type: "DelayAction"; seconds: number }
  | { type: "SignalAction"; targetRole: string; signalType: string }
  | { type: "LockdownAction"; shred_folders?: boolean };

export type ActionType = Action["type"];

export type RuleSource = { kind: "Local" } | { kind: "Fleet"; epochVersion: number };

export interface Rule {
  id: string;
  name: string;
  system?: boolean;
  enabled: boolean;
  triggers: Trigger[];
  conditions: Condition[];
  actions: Action[];
  notes?: string;
  tags?: string[];
  riskLevel?: RiskLevel;
  schemaVersion?: number;
  source?: RuleSource;
  locked?: boolean;
  debounceMs?: number | null;
}

// ── Catalogs used by the editor ───────────────────────────────────────────

/** Curated toggles for the SetToggle action (mirrors flow-core ToggleMap::builtin).
 *
 * `on = true` engages PROTECTION (see flow-core toggles.rs): for capability
 * toggles that means the resource is DENIED/blocked, not enabled. `onMeans` /
 * `offMeans` spell out the concrete effect of each switch position so the editor
 * never shows a bare "on/off" that reads backwards (e.g. Microphone switch "off"
 * looking like "mic disabled" when it actually ALLOWS the mic). */
export const TOGGLE_CATALOG: { id: string; label: string; onMeans: string; offMeans: string }[] = [
  { id: "telemetry", label: "Block Telemetry", onMeans: "telemetry blocked", offMeans: "telemetry allowed" },
  { id: "location", label: "Location Tracking", onMeans: "location off", offMeans: "location on" },
  { id: "activity-history", label: "Activity History", onMeans: "history off", offMeans: "history on" },
  { id: "advertising-id", label: "Advertising ID", onMeans: "ad ID off", offMeans: "ad ID on" },
  { id: "cap-webcam", label: "Camera", onMeans: "camera denied", offMeans: "camera allowed" },
  { id: "cap-microphone", label: "Microphone", onMeans: "mic denied", offMeans: "mic allowed" },
];

/** Common settings paths surfaced by name for the SettingChanged trigger. */
export const SETTING_PATH_CATALOG: { path: string; label: string; onValue: unknown; offValue: unknown }[] = [
  { path: "ideal.privacy.telemetry.windowsDisabled", label: "Telemetry protection", onValue: true, offValue: false },
  { path: "ideal.privacy.telemetry.locationTrackingDisabled", label: "Location protection", onValue: true, offValue: false },
  { path: "ideal.privacy.appCapabilities.webcam", label: "Camera capability", onValue: "Allow", offValue: "Deny" },
  { path: "ideal.privacy.appCapabilities.microphone", label: "Microphone capability", onValue: "Allow", offValue: "Deny" },
];

export const GAZE_KIND_LABELS: Record<GazeKind, string> = {
  look_away: "Looked away from screen",
  no_face: "No face detected (walked away)",
  multiple_faces: "Multiple faces (shoulder surfer)",
  secondary_device: "Phone / camera detected in frame",
};

export const TRIGGER_LABELS: Record<TriggerType, string> = {
  SettingChangedTrigger: "A setting changes",
  GazeTrigger: "Privacy Shield detects gaze",
  USBTrigger: "USB device inserted / removed",
  RansomwareMonitorTrigger: "Ransomware detected",
  PasteMonitorTrigger: "Clipboard secret detected",
  DecoyMonitorTrigger: "Decoy file accessed",
  WifiGuardTrigger: "Rogue Wi-Fi detected",
};

export const ACTION_LABELS: Record<ActionType, string> = {
  SetToggleAction: "Flip a protection toggle",
  CommandAction: "Run an app command",
  NotifyAction: "Show a notification",
  DelayAction: "Wait",
  SignalAction: "Send a contingency signal",
  LockdownAction: "Trigger lockdown (self-destruct)",
};

// ── Factories ──────────────────────────────────────────────────────────────

let idCounter = 0;
export function newRuleId(): string {
  idCounter += 1;
  // Deterministic-ish local id; the backend does not require a UUID.
  return `rule-${Date.now().toString(36)}-${idCounter}`;
}

export function emptyRule(): Rule {
  return {
    id: newRuleId(),
    name: "New flow",
    enabled: false,
    triggers: [{ type: "GazeTrigger", kind: "look_away" }],
    conditions: [],
    actions: [{ type: "NotifyAction", message: "Flow fired", severity: "info" }],
    notes: "",
    tags: [],
    riskLevel: "low",
    schemaVersion: 2,
    source: { kind: "Local" },
    locked: false,
  };
}

export function defaultTrigger(type: TriggerType): Trigger {
  switch (type) {
    case "SettingChangedTrigger":
      return { type, path: SETTING_PATH_CATALOG[0].path, to: SETTING_PATH_CATALOG[0].onValue };
    case "GazeTrigger":
      return { type, kind: "look_away" };
    case "USBTrigger":
      return { type, mode: "remove" };
    case "PasteMonitorTrigger":
      return { type };
    case "DecoyMonitorTrigger":
      return { type };
    case "WifiGuardTrigger":
      return { type };
    case "RansomwareMonitorTrigger":
      return { type };
  }
}

export function defaultAction(type: ActionType): Action {
  switch (type) {
    case "SetToggleAction":
      return { type, toggleId: "location", on: true };
    case "CommandAction":
      return { type, command: "" };
    case "NotifyAction":
      return { type, message: "Flow fired", severity: "info" };
    case "DelayAction":
      return { type, seconds: 5 };
    case "SignalAction":
      return { type, targetRole: "admins", signalType: "distress" };
    case "LockdownAction":
      return { type, shred_folders: false };
  }
}

// ── Human-readable summaries (for the rule cards) ───────────────────────────

export function triggerSummary(t: Trigger): string {
  switch (t.type) {
    case "SettingChangedTrigger": {
      const known = SETTING_PATH_CATALOG.find((s) => s.path === t.path);
      const name = known?.label ?? t.path;
      return `${name} changes`;
    }
    case "GazeTrigger":
      return t.kind ? GAZE_KIND_LABELS[t.kind] : "any gaze event";
    case "USBTrigger":
      return `USB ${t.mode === "insert" ? "inserted" : "removed"}`;
    default:
      return TRIGGER_LABELS[t.type];
  }
}

export function actionSummary(a: Action): string {
  switch (a.type) {
    case "SetToggleAction": {
      const known = TOGGLE_CATALOG.find((tg) => tg.id === a.toggleId);
      // Show the concrete effect ("mic denied") not a backwards "enable/disable"
      // — `on` engages protection, which DENIES capability toggles.
      if (known) return a.on ? known.onMeans : known.offMeans;
      return `${a.on ? "engage" : "release"} ${a.toggleId}`;
    }
    case "CommandAction":
      return a.command || "(no command)";
    case "NotifyAction":
      return `notify "${a.message}"`;
    case "DelayAction":
      return `wait ${a.seconds}s`;
    case "SignalAction":
      return `signal ${a.targetRole}`;
    case "LockdownAction":
      return "trigger lockdown";
  }
}

export function ruleSummary(rule: Rule): { when: string; then: string } {
  const when = rule.triggers.map(triggerSummary).join(" or ") || "(no trigger)";
  const then = rule.actions.map(actionSummary).join(", then ") || "(no action)";
  return { when, then };
}

export function isFleetLocked(rule: Rule): boolean {
  return rule.locked === true || rule.source?.kind === "Fleet";
}

// ── Templates (the two showcase rules, one-click add) ───────────────────────

export interface RuleTemplate {
  key: string;
  name: string;
  blurb: string;
  build(): Rule;
}

export const RULE_TEMPLATES: RuleTemplate[] = [
  {
    key: "telemetry-location",
    name: "Telemetry on → Location off",
    blurb: "When telemetry protection is switched off (telemetry becomes active), immediately turn location tracking off.",
    build(): Rule {
      return {
        ...emptyRule(),
        id: newRuleId(),
        name: "Telemetry on → Location off",
        triggers: [{ type: "SettingChangedTrigger", path: "ideal.privacy.telemetry.windowsDisabled", to: false }],
        conditions: [],
        actions: [{ type: "SetToggleAction", toggleId: "location", on: true }],
        notes: "Showcase rule: telemetry becomes active → enforce location-off.",
      };
    },
  },
  {
    key: "gaze-camera",
    name: "Gaze detected → Camera off",
    blurb: "When Privacy Shield sees you look away (or a shoulder-surfer), deny camera access.",
    build(): Rule {
      return {
        ...emptyRule(),
        id: newRuleId(),
        name: "Gaze detected → Camera off",
        triggers: [{ type: "GazeTrigger", kind: "look_away" }],
        conditions: [],
        actions: [{ type: "SetToggleAction", toggleId: "cap-webcam", on: true }],
        notes: "Showcase rule: gaze/look-away → camera denied.",
      };
    },
  },
];
