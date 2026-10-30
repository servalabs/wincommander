import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppConfirm } from "../components/shared/AppConfirmDialog";
import { bluetoothControlIssue, canChangeBluetooth, describeBluetoothFailure, parseBluetoothControlStatus, type BluetoothControlStatus } from "../lib/bluetoothControl";
import { MACHINE_SCOPE_ELEVATION_MESSAGE } from "../lib/machineScopeElevation";
import { showError, showWarning } from "../utils/toast";
import { requestDestructiveCapability } from "./destructiveAuthz";

// Dashboard cards unmount when the user changes pages. Keep the last known
// policy result for the next mount so the control does not flash a temporary
// loading message or change width while the background confirmation runs.
// The cached result is display-only until the fresh request below verifies it.
let cachedStatus: BluetoothControlStatus | null = null;

export default function useBluetoothControl(isAdmin: boolean | null | undefined) {
  const confirm = useAppConfirm();
  const [status, setStatus] = useState<BluetoothControlStatus | null>(() => cachedStatus);
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(cachedStatus === null);
  const [verified, setVerified] = useState(false);
  const mounted = useRef(false);
  const operating = useRef(false);
  const reading = useRef(false);
  const epoch = useRef(0);
  const lastIssue = useRef<string | null>(null);

  const reportStatus = useCallback((next: BluetoothControlStatus | null, failure?: unknown) => {
    const issue = bluetoothControlIssue(next);
    if (issue && issue.key !== lastIssue.current) {
      void showWarning(failure === undefined ? issue.message : describeBluetoothFailure(failure, issue.message), undefined, { kind: "notification" }).catch(() => undefined);
    }
    lastIssue.current = issue?.key ?? null;
    cachedStatus = next;
    setStatus(next);
  }, []);

  const refresh = useCallback(async (): Promise<void> => {
    if (reading.current || operating.current) return;
    reading.current = true;
    const requestEpoch = epoch.current;
    if (mounted.current) setRefreshing(true);
    try {
      const next = parseBluetoothControlStatus(await invoke<unknown>("bluetooth_control_status"));
      if (mounted.current && epoch.current === requestEpoch) {
        reportStatus(next);
        setVerified(true);
      }
    } catch (error) {
      if (mounted.current && epoch.current === requestEpoch) {
        // A previous result may remain visible while this read is retried, but
        // it must not authorise a policy change until Windows confirms it.
        if (cachedStatus) {
          if (lastIssue.current !== "unavailable") {
            void showWarning(describeBluetoothFailure(error), undefined, { kind: "notification" }).catch(() => undefined);
          }
          lastIssue.current = "unavailable";
          setStatus(cachedStatus);
        } else {
          reportStatus(null, error);
        }
        setVerified(false);
      }
    } finally {
      reading.current = false;
      if (mounted.current && epoch.current === requestEpoch) setRefreshing(false);
      else if (mounted.current && !operating.current) void refresh();
    }
  }, [reportStatus]);

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
    setRefreshing(false);
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
      if (mounted.current) {
        reportStatus(next);
        setVerified(true);
      }
    } catch (error) {
      if (mounted.current) {
        setStatus(cachedStatus);
        setVerified(false);
        lastIssue.current = "unavailable";
        void showError(describeBluetoothFailure(error), undefined, { kind: "notification" }).catch(() => undefined);
      }
    } finally {
      operating.current = false;
      if (mounted.current) { setBusy(false); void refresh(); }
    }
  }, [confirm, isAdmin, refresh, reportStatus, status]);

  return { status, busy, refreshing, verified, change };
}
