import { Bluetooth, BluetoothOff } from "lucide-react";
import { useAppState } from "../../context/AppContext";
import useBluetoothControl from "../../hooks/useBluetoothControl";
import { bluetoothControlDescription, bluetoothControlLabel, canChangeBluetooth } from "../../lib/bluetoothControl";
import { MACHINE_SCOPE_ELEVATION_MESSAGE } from "../../lib/machineScopeElevation";

export default function BluetoothControl() {
  const { systemInfo } = useAppState();
  const { status, busy, refreshing, verified, change } = useBluetoothControl(systemInfo?.isAdmin);
  // A retained result makes page-to-page navigation visually stable, but it
  // never enables a mutation until the new dashboard mount rechecks Windows.
  const canChange = verified && canChangeBluetooth(status, systemInfo?.isAdmin);
  const nextBlocked = !status?.blocked;
  const description = systemInfo?.isAdmin !== true
    ? MACHINE_SCOPE_ELEVATION_MESSAGE
    : refreshing && !verified
      ? "Checking the current Bluetooth policy. The control remains unavailable until Windows confirms it."
      : bluetoothControlDescription(status);
  // Keep this paired dashboard control the same stable width as Internet.
  // The full state remains in the accessible title; unresolved policy states
  // are not presented as a misleading ON/OFF value.
  const label = !busy && (status?.state === "allowed" || status?.state === "blocked")
    ? bluetoothControlLabel(status)
    : "Bluetooth";

  return (
    <button
      type="button"
      className={`killswitch-toggle bluetooth-control focus-visible:outline-2 focus-visible:outline-offset-2 ${status?.state === "blocked" ? "cut" : status?.state === "allowed" ? "live" : ""}`}
      disabled={busy || !canChange}
      aria-busy={busy || refreshing}
      title={canChange ? `${description} ${nextBlocked ? "Click to apply the Bluetooth block policy for all Windows accounts." : "Click to release WinCommander’s Bluetooth policy restriction."}` : description}
      onClick={() => void change(nextBlocked)}
    >
      {status?.state === "blocked" ? <BluetoothOff size={13} aria-hidden="true" /> : <Bluetooth size={13} aria-hidden="true" />}
      {label}
    </button>
  );
}
