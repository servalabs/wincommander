export interface BluetoothControlStatus {
  blocked: boolean;
  state: "allowed" | "blocked" | "no_adapter" | "partial" | "recovery_required" | "externally_blocked";
  adapterCount: number;
  ownedAdapterCount: number;
  canChange: boolean;
  message: string;
  monitoring: boolean;
}

const STATES = new Set(["allowed", "blocked", "no_adapter", "partial", "recovery_required", "externally_blocked"]);

export function parseBluetoothControlStatus(value: unknown): BluetoothControlStatus {
  if (!value || typeof value !== "object") throw new Error("Bluetooth status unavailable");
  const status = value as Record<string, unknown>;
  if (typeof status.blocked !== "boolean" || typeof status.state !== "string" || !STATES.has(status.state)
    || typeof status.canChange !== "boolean" || typeof status.monitoring !== "boolean"
    || typeof status.message !== "string" || status.message.length > 1024
    || !Number.isInteger(status.adapterCount) || (status.adapterCount as number) < 0 || (status.adapterCount as number) > 128
    || !Number.isInteger(status.ownedAdapterCount) || (status.ownedAdapterCount as number) < 0 || (status.ownedAdapterCount as number) > 128
    || ((status.state === "blocked" || status.state === "partial") && !status.blocked)
    || ((status.state === "blocked" || status.state === "partial") && status.adapterCount === 0)
    || ((status.state === "allowed" || status.state === "externally_blocked") && status.blocked)
    || (status.state === "allowed" && (status.adapterCount === 0 || status.ownedAdapterCount !== 0))
    || (status.state === "no_adapter" && status.adapterCount !== 0)) {
    throw new Error("Bluetooth status unavailable");
  }
  return status as unknown as BluetoothControlStatus;
}

export function bluetoothControlLabel(status: BluetoothControlStatus | null): string {
  if (!status) return "Bluetooth unknown";
  switch (status.state) {
    case "allowed": return "Bluetooth allowed";
    case "blocked": return "Bluetooth blocked";
    case "no_adapter": return status.blocked ? "Bluetooth block saved" : "No Bluetooth adapter";
    case "externally_blocked": return "Bluetooth externally blocked";
    case "partial": return "Bluetooth partly blocked";
    case "recovery_required": return "Bluetooth needs recovery";
  }
}

export function bluetoothControlDescription(status: BluetoothControlStatus | null): string {
  if (!status) return "Bluetooth state could not be verified. WinCommander will retry automatically.";
  if (!status.monitoring && status.blocked) return "Bluetooth protection needs its local service. Wait for the service, or allow Bluetooth to release WinCommander’s restriction.";
  if (!status.monitoring) return "Bluetooth protection needs its local service before a new block can be enabled. WinCommander will check again automatically.";
  switch (status.state) {
    case "allowed": return "Allowed for this PC. Windows or another policy may still keep Bluetooth off.";
    case "blocked": return "Blocked for all Windows accounts on this PC. Paired devices are remembered. New adapters are checked in the background.";
    case "no_adapter": return status.blocked ? "No adapter detected. The block is saved; new adapters are checked in the background." : "No Bluetooth adapter detected on this PC.";
    case "externally_blocked": return "Windows or another policy blocks Bluetooth. WinCommander will not override it.";
    case "partial": return "Some Bluetooth adapters could not be blocked. WinCommander will keep checking. Check the adapter in Windows Device Manager if this persists, or allow Bluetooth to release the saved block.";
    case "recovery_required": return "Reconnect removed adapters and retry Allow. For an interrupted change, check the adapter in Windows Device Manager; WinCommander will not enable an unverified adapter.";
  }
}

export function bluetoothControlIssue(status: BluetoothControlStatus | null): { key: string; message: string } | null {
  if (!status) return { key: "unavailable", message: bluetoothControlDescription(null) };
  if (!status.monitoring) return { key: `service:${status.blocked}`, message: bluetoothControlDescription(status) };
  if (status.state === "partial" || status.state === "recovery_required" || status.state === "externally_blocked") {
    return { key: status.state, message: bluetoothControlDescription(status) };
  }
  return null;
}

export function canChangeBluetooth(status: BluetoothControlStatus | null, isAdmin: boolean | null | undefined): boolean {
  return isAdmin === true && status?.canChange === true;
}

export function describeBluetoothFailure(error: unknown, fallback = "Windows did not confirm the Bluetooth change. WinCommander will check its current state automatically before another action. No result is assumed."): string {
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  const code = message.match(/\[(BT_[A-Z_]+)\]/)?.[1];
  switch (code) {
    case "BT_ACCESS_DENIED": return "Windows denied the Bluetooth adapter change. Run WinCommander as administrator and check Windows device policy.";
    case "BT_DEVICE_CHANGED": return "The Bluetooth adapter changed or disconnected. WinCommander is checking its current state automatically.";
    case "BT_RESTART_REQUIRED": return "Check the Bluetooth adapter in Windows Device Manager. Windows reported that this adapter change requires a restart; no completed change is assumed.";
    case "BT_DISABLE_REJECTED": return "Windows rejected blocking the Bluetooth adapter. Check the adapter in Windows Device Manager; WinCommander will recheck automatically.";
    case "BT_ENABLE_REJECTED": return "Windows rejected restoring the Bluetooth adapter. Check the adapter in Windows Device Manager and review Windows device policy.";
    case "BT_TRANSITION_TIMEOUT": return "Windows did not finish the Bluetooth adapter transition in time. WinCommander will recheck automatically. No success is assumed.";
    case "BT_STATE_UNVERIFIED": return "WinCommander could not verify protected Bluetooth recovery state. Ask your administrator to check the adapter in Windows Device Manager before another change.";
    case "BT_OPERATION_BUSY": return "Another Bluetooth operation is already running. WinCommander will check its current state automatically.";
    case "BT_SERVICE_UNAVAILABLE": return "The local Bluetooth service connection could not be verified. WinCommander will check again automatically. If this persists, check that the matching WinCommander service is running.";
    default: return fallback;
  }
}
