// Safe, pure projection from a terminal diagnostic event to the in-app bell.
// This module must not accept raw error strings, user labels, or event context.
import type { NotifKind, NotifSeverity } from "./notificationStore";

export interface DiagnosticNotificationInput {
  feature: string;
  action: string;
  outcome: "started" | "progress" | "succeeded" | "failed" | "degraded" | "recovered" | "cancelled" | "timed_out";
  privacyClass: "public" | "local_sensitive" | "restricted";
}

export interface DiagnosticBellProjection {
  severity: NotifSeverity;
  kind: NotifKind;
  message: string;
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

/**
 * Converts only a terminal outcome into short bell text. Restricted records
 * receive a generic label, and unknown caller-controlled tokens never render.
 */
export function diagnosticBellProjection(input: DiagnosticNotificationInput): DiagnosticBellProjection | undefined {
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
