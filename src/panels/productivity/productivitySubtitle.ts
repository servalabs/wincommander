export type FleetConnectionState = "checking" | "connected" | "disconnected" | "unverified";

export function productivitySubtitle(fleetEnabled: boolean, connection: FleetConnectionState): string {
  if (!fleetEnabled) return "Data stays on this device — this device is not fleet-enrolled, so nothing here is uploaded.";
  if (connection === "connected") {
    return "This device is fleet-enrolled: app names, window titles, URLs, file paths, and activity are reported to the fleet.";
  }
  if (connection === "disconnected") {
    return "Fleet is enabled but this device is not currently connected. Activity may be reported when the connection is restored.";
  }
  return "Fleet status could not be verified. Treat the activity shown here as potentially reported to the fleet.";
}
