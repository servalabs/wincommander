// Typed Fleet connection-status IPC wrapper.

import { invoke } from "@tauri-apps/api/core";

export interface FleetConnectionStatus {
  connected: boolean;
}

export const getFleetStatus = (): Promise<FleetConnectionStatus> =>
  invoke<FleetConnectionStatus>("fleet_status");
