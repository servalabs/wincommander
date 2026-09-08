// Central routing policy for structured diagnostic events.
//
// This is deliberately a pure function: a feature supplies structured state,
// never message text. The encrypted diagnostic record remains the source of
// truth; every delivery surface is derived here after persistence.
import type { NotifKind, NotifSeverity } from "./notificationStore";

export interface DiagnosticNotificationInput {
  feature: string;
  action: string;
  outcome: "started" | "progress" | "succeeded" | "failed" | "degraded" | "recovered" | "cancelled" | "timed_out";
  privacyClass: "public" | "local_sensitive" | "restricted";
  severity?: "debug" | "info" | "warn" | "error" | "critical";
}

export interface DiagnosticBellProjection {
  severity: NotifSeverity;
  kind: NotifKind;
  message: string;
}

/** Windows is opt-in for the small set of explicitly eligible critical states. */
export type WindowsDiagnosticRoute =
  | { mode: "disabled" }
  | { mode: "generic_critical"; message: "WinCommander protection needs attention" };

/** A Fleet sender may use category/outcome only after its own managed-device gate. */
export type FleetDiagnosticRoute = "withhold_local_detail" | "managed_outcome_only";

export interface DiagnosticNotificationRoute {
  bell?: DiagnosticBellProjection;
  windows: WindowsDiagnosticRoute;
  fleet: FleetDiagnosticRoute;
}

const FEATURE_LABELS: Record<string, string> = {
  vault: "Vault", privacy_shield: "Privacy Shield", rdp: "Remote Desktop", fleet: "Fleet management", flow: "Flow",
  ransomware: "Ransomware protection", clipboard: "Clipboard protection", paste: "Clipboard protection",
  decoy: "Decoy protection", usb: "USB protection", vpn: "VPN protection", driver_health: "Driver health",
  remote_access: "Remote-access protection", network: "Network protection",
};

const ACTION_LABELS: Record<string, string> = {
  create: "setup", mount: "mount", dismount: "dismount", unlock: "unlock", start: "start", stop: "stop",
  apply_policy: "policy update", listener: "listener check", firewall: "firewall update",
  session_monitoring: "session monitoring", run: "run", command: "command",
};

const SECURITY_FEATURES = new Set([
  "ransomware", "clipboard", "paste", "decoy", "usb", "vpn", "driver_health", "remote_access", "network",
]);

// Windows notifications are intentionally exceptional. They do not identify a
// vault, process, path, camera, RDP session, clipboard, or person.
const WINDOWS_GENERIC_CRITICAL_FEATURES = new Set(["ransomware", "decoy", "remote_access", "network"]);

function bellProjection(input: DiagnosticNotificationInput): DiagnosticBellProjection | undefined {
  if (input.outcome !== "failed" && input.outcome !== "timed_out" && input.outcome !== "degraded" && input.outcome !== "recovered") return undefined;

  const restricted = input.privacyClass === "restricted";
  const feature = restricted ? "A protected operation" : (FEATURE_LABELS[input.feature] ?? "A protected operation");
  const action = restricted ? "" : (ACTION_LABELS[input.action] ?? "operation");
  const kind: NotifKind = SECURITY_FEATURES.has(input.feature) ? "alert" : "notification";
  const operation = action ? `${feature} ${action}` : feature;

  if (input.outcome === "recovered") return { severity: "info", kind, message: `${feature} recovered` };
  if (input.outcome === "degraded") return { severity: "warn", kind, message: `${operation} needs attention` };
  return { severity: "danger", kind, message: `${operation} failed` };
}

/**
 * The only policy used to route a durable diagnostic event. It intentionally
 * returns no local details for Windows or Fleet; their transport adapters must
 * use these values and must not invent their own text.
 */
export function routeDiagnosticNotification(input: DiagnosticNotificationInput): DiagnosticNotificationRoute {
  const bell = bellProjection(input);
  const windows: WindowsDiagnosticRoute = input.privacyClass !== "restricted"
    && input.severity === "critical"
    && input.outcome !== "recovered"
    && WINDOWS_GENERIC_CRITICAL_FEATURES.has(input.feature)
    ? { mode: "generic_critical", message: "WinCommander protection needs attention" }
    : { mode: "disabled" };

  // Fleet transport is intentionally outside the desktop notification path.
  // Sensitive local context, error text, and diagnostic details never leave it.
  const fleet: FleetDiagnosticRoute = input.privacyClass === "public"
    ? "managed_outcome_only"
    : "withhold_local_detail";

  return { bell, windows, fleet };
}

/** Compatibility export for existing callers/tests; new callers use the route. */
export function diagnosticBellProjection(input: DiagnosticNotificationInput): DiagnosticBellProjection | undefined {
  return routeDiagnosticNotification(input).bell;
}
