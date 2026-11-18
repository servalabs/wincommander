import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppConfirm } from "../components/shared/AppConfirmDialog";
import { canChangeBluetooth, describeBluetoothFailure, parseBluetoothControlStatus, type BluetoothControlStatus } from "../lib/bluetoothControl";
import { MACHINE_SCOPE_ELEVATION_MESSAGE } from "../lib/machineScopeElevation";
import { showError } from "../utils/toast";
import { requestDestructiveCapability } from "./destructiveAuthz";

export default function useBluetoothControl(isAdmin: boolean | null | undefined) {
  const confirm = useAppConfirm();
  const [status, setStatus] = useState<BluetoothControlStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const mounted = useRef(false);
  const operating = useRef(false);
  const reading = useRef(false);
  const epoch = useRef(0);
  const refresh = useCallback(async (): Promise<void> => {
    if (reading.current || operating.current) return;
    reading.current = true;
    const requestEpoch = epoch.current;
    try {
      const next = parseBluetoothControlStatus(await invoke<unknown>("bluetooth_control_status"));
      if (mounted.current && epoch.current === requestEpoch) {
        setStatus(next);
      }
    } catch {
      if (mounted.current && epoch.current === requestEpoch) {
        // Invalidate the last result while the background poll retries.
        setStatus(null);
      }
    } finally {
      reading.current = false;
      if (mounted.current && epoch.current !== requestEpoch && !operating.current) void refresh();
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    const onFocus = () => { if (!document.hidden) void refresh(); };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onFocus);
    const timer = window.setInterval(onFocus, 2_000);
    return () => {
      mounted.current = false;
      epoch.current += 1;
      window.clearInterval(timer);
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onFocus);
    };
  }, [refresh]);

  const change = useCallback(async (blocked: boolean) => {
    if (operating.current) return;
    if (!canChangeBluetooth(status, isAdmin)) {
      void showError(isAdmin !== true ? MACHINE_SCOPE_ELEVATION_MESSAGE : "Bluetooth policy cannot be changed until its current state and local service are verified. WinCommander will check again automatically.", undefined, { kind: "notification" }).catch(() => undefined);
      return;
    }
    operating.current = true;
    epoch.current += 1;
    setBusy(true);
    try {
      if (blocked && !await confirm({
        title: "Block Bluetooth by policy for this PC?",
        description: "Apply the Windows Bluetooth restriction to every account on this PC. Bluetooth keyboards, mice, headphones and other connected devices may stop working when Windows applies the policy. Connect a wired keyboard and mouse or use built-in controls before continuing. Existing pairings are preserved.",
        confirmLabel: "Apply Bluetooth block",
        cancelLabel: "Keep current setting",
      })) return;
      if (!mounted.current) return;
      let capabilityToken: string;
      try {
        capabilityToken = await requestDestructiveCapability({ command: "bluetooth_control_set", blocked });
      } catch (error) {
        if (error === "confirmation failed") return;
        throw error;
      }
      const next = parseBluetoothControlStatus(await invoke<unknown>("bluetooth_control_set", { blocked, capabilityToken }));
      if (mounted.current) setStatus(next);
    } catch (error) {
      if (mounted.current) {
        setStatus(null);
        void showError(describeBluetoothFailure(error), undefined, { kind: "notification" }).catch(() => undefined);
      }
    } finally {
      operating.current = false;
      if (mounted.current) { setBusy(false); void refresh(); }
    }
  }, [confirm, isAdmin, refresh, status]);

  return { status, busy, change };
}
