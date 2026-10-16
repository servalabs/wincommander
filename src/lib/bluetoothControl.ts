export interface BluetoothControlStatus {
  schemaVersion: 2;
  blocked: boolean;
  state: "allowed" | "blocked" | "pending" | "unsupported" | "externally_managed" | "legacy_recovery_required";
  canChange: boolean;
  message: string;
}

const STATES = new Set(["allowed", "blocked", "pending", "unsupported", "externally_managed", "legacy_recovery_required"]);
const FIELDS = new Set(["schemaVersion", "blocked", "state", "canChange", "message"]);
const LOCKED_STATES = new Set(["unsupported", "externally_managed", "legacy_recovery_required"]);

export function parseBluetoothControlStatus(value: unknown): BluetoothControlStatus {
  if (!value || typeof value !== "object") throw new Error("Bluetooth policy status unavailable");
  const status = value as Record<string, unknown>;
  if (status.schemaVersion !== 2 || Object.keys(status).some((key) => !FIELDS.has(key))
    || typeof status.blocked !== "boolean" || typeof status.state !== "string" || !STATES.has(status.state)
    || typeof status.canChange !== "boolean" || typeof status.message !== "string" || status.message.length > 1024
    || (status.state === "blocked" && !status.blocked)
    || (status.state === "allowed" && status.blocked)
    || (LOCKED_STATES.has(status.state) && status.canChange)) {
    throw new Error("Bluetooth policy status unavailable");
  }
  return status as unknown as BluetoothControlStatus;
}

export function bluetoothControlLabel(status: BluetoothControlStatus | null): string {
  if (!status) return "Bluetooth policy unknown";
  switch (status.state) {
    case "allowed": return "Bluetooth policy allowed";
    case "blocked": return "Bluetooth policy blocked";
    case "pending": return "Bluetooth policy pending";
    case "unsupported": return "Bluetooth policy unsupported";
    case "externally_managed": return "Bluetooth policy managed";
    case "legacy_recovery_required": return "Bluetooth needs review";
  }
}

export function bluetoothControlDescription(status: BluetoothControlStatus | null): string {
  if (!status) return "Bluetooth policy state could not be verified. WinCommander will retry automatically.";
  switch (status.state) {
    case "allowed": return "WinCommander’s Bluetooth policy restriction is released. Windows radio settings and other restrictions still apply.";
    case "blocked": return "Windows confirms the Bluetooth policy block for every account on this PC. Existing pairings are preserved.";
    case "pending": return "Windows has not confirmed the requested Bluetooth policy state. WinCommander will keep checking automatically.";
    case "unsupported": return "Bluetooth policy control is unsupported on this Windows installation. Ask your administrator to check Windows policy support.";
    case "externally_managed": return "Another Windows or organisation policy controls Bluetooth. Ask your administrator to review it.";
    case "legacy_recovery_required": return "A previous device-based Bluetooth restriction needs administrator review in Windows Device Manager before policy control can continue.";
  }
}

export function bluetoothControlIssue(status: BluetoothControlStatus | null): { key: string; message: string } | null {
  if (!status) return { key: "unavailable", message: bluetoothControlDescription(null) };
  if (status.state !== "allowed" && status.state !== "blocked") {
    return { key: status.state, message: bluetoothControlDescription(status) };
  }
  return null;
}

export function canChangeBluetooth(status: BluetoothControlStatus | null, isAdmin: boolean | null | undefined): boolean {
  return isAdmin === true && status?.canChange === true && !LOCKED_STATES.has(status.state);
}

export function describeBluetoothFailure(error: unknown, fallback = "Windows did not confirm the Bluetooth policy change. WinCommander will check its current state automatically before another action. No result is assumed."): string {
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  const code = message.match(/\[(BT_[A-Z_]+)\]/)?.[1];
  switch (code) {
    case "BT_POLICY_UNSUPPORTED": return "Bluetooth policy control is unsupported on this Windows installation. Ask your administrator to check Windows policy support.";
    case "BT_POLICY_MANAGED": return "Another Windows or organisation policy controls Bluetooth. Ask your administrator to review it.";
    case "BT_POLICY_PENDING": return "Windows has not confirmed the Bluetooth policy change. WinCommander will keep checking automatically.";
    case "BT_POLICY_REJECTED": return "Windows rejected the Bluetooth policy change. Check Windows policy support and organisation restrictions before retrying.";
    case "BT_POLICY_UNVERIFIED": return "WinCommander could not verify the Windows Bluetooth policy. It will keep checking; ask your administrator to review Windows policy if this persists.";
    case "BT_STATE_UNVERIFIED": return "WinCommander could not verify protected Bluetooth policy state. Ask your administrator to review it before another change.";
    case "BT_OPERATION_BUSY": return "Another Bluetooth policy operation is already running. WinCommander will check its current state automatically.";
    case "BT_ACCESS_DENIED": return "Windows denied the Bluetooth policy change. Run WinCommander as administrator and check organisation policy.";
    case "BT_LEGACY_RECOVERY": return "A previous device-based Bluetooth restriction needs administrator review in Windows Device Manager. Policy control cannot restore that earlier device change.";
    case "BT_SERVICE_UNAVAILABLE": return "The local service could not confirm Bluetooth policy state. WinCommander will check again automatically. If this persists, check that the matching WinCommander service is running.";
    default: return fallback;
  }
}
