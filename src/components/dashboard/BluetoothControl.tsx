import { Bluetooth, BluetoothOff } from "lucide-react";
import { useAppState } from "../../context/AppContext";
import useBluetoothControl from "../../hooks/useBluetoothControl";
import { bluetoothControlDescription, bluetoothControlLabel, canChangeBluetooth } from "../../lib/bluetoothControl";
import { MACHINE_SCOPE_ELEVATION_MESSAGE } from "../../lib/machineScopeElevation";

export default function BluetoothControl() {
  const { systemInfo } = useAppState();
  const { status, busy, refreshing, change } = useBluetoothControl(systemInfo?.isAdmin);
  const canChange = canChangeBluetooth(status, systemInfo?.isAdmin);
  const nextBlocked = status?.state === "recovery_required" ? false : !status?.blocked;
  const description = systemInfo?.isAdmin !== true ? MACHINE_SCOPE_ELEVATION_MESSAGE : bluetoothControlDescription(status);
  const label = busy ? "Changing Bluetooth…" : refreshing && !status ? "Checking Bluetooth…" : bluetoothControlLabel(status);

  return (
    <button
      type="button"
      className={`killswitch-toggle focus-visible:outline-2 focus-visible:outline-offset-2 ${status?.state === "blocked" ? "cut" : status?.state === "allowed" ? "live" : ""}`}
      style={{ marginTop: 0, whiteSpace: "normal", padding: "6px 8px" }}
      disabled={busy || !canChange}
      aria-busy={busy}
      title={canChange ? `${description} ${nextBlocked ? "Click to block Bluetooth for all Windows accounts." : "Click to allow Bluetooth on this PC."}` : description}
      onClick={() => void change(nextBlocked)}
    >
      {status?.state === "blocked" ? <BluetoothOff size={13} aria-hidden="true" /> : <Bluetooth size={13} aria-hidden="true" />}
      {label}
    </button>
  );
}
