// Fleet enrollment is device-owned: the Free/Pro agent stores its runtime
// session in the shared service context, while UI settings are user-owned.
// Never infer a device link from the latter when an authoritative status read
// is available.

export interface FleetLinkStatus {
  connected: boolean;
  deviceId: string;
  serverUrl: string;
  lastEnrollAt: string | null;
  lastError: string | null;
  retrying: boolean;
  pendingApproval?: boolean;
}

export type FleetLinkState = "linked" | "pending" | "reconnecting" | "offline" | "error" | "not_linked";

export function fleetLinkState(status: FleetLinkStatus | null): FleetLinkState {
  if (!status) return "not_linked";
  // The agent has already authenticated this check-in. It is more current
  // than a stale per-user app.fleet setting (or the absence of one).
  if (status.connected) return status.pendingApproval ? "pending" : "linked";

  const hasDeviceIdentity = status.deviceId.trim().length > 0 && status.serverUrl.trim().length > 0;
  if (status.lastError) return "error";
  if (status.retrying && hasDeviceIdentity) return "reconnecting";
  return hasDeviceIdentity ? "offline" : "not_linked";
}

export function fleetLinkLabel(state: FleetLinkState): string {
  switch (state) {
    case "linked": return "Linked";
    case "pending": return "Awaiting approval";
    case "reconnecting": return "Reconnecting…";
    case "offline": return "Offline";
    case "error": return "Connection error";
    case "not_linked": return "Not linked";
  }
}
