import { useCallback, useEffect, useRef, useState } from "react";
import useBackend from "@/hooks/useBackend";
import { useAppState } from "@/context/AppContext";
import { requireHostsChangeReceipt, type ConnectionCheck, type HostsUndoStatus } from "@/lib/hostsProtection";
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from "@/lib/machineScopeElevation";
import { showSuccess, showWarning } from "@/utils/toast";

export function useConnectionCheck() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const { systemInfo, refreshNetwork } = useAppState();
  const [result, setResult] = useState<ConnectionCheck | null>(null);
  const [undo, setUndo] = useState<HostsUndoStatus | undefined>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const locked = useRef(false);
  const needsElevation = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);

  const refreshUndo = useCallback(async () => {
    const status = await backendRef.current.getBlocklistStatus();
    setUndo(status.success ? status.data?.undo : undefined);
  }, []);

  useEffect(() => { void refreshUndo().catch(() => setUndo(undefined)); }, [refreshUndo]);

  const inspect = useCallback(async () => {
    if (locked.current) return;
    locked.current = true;
    setBusy(true); setError(null); setResult(null);
    try {
      const response = await backendRef.current.testBlocklistConnection();
      if (!response.success || !response.data?.checks?.length) throw new Error(response.error || "The connection check could not complete.");
      setResult(response.data);
    } catch (cause) { setError(cause instanceof Error ? cause.message : "The connection check failed."); }
    finally {
      await refreshUndo().catch(() => setUndo(undefined));
      locked.current = false; setBusy(false);
    }
  }, [refreshUndo]);

  const undoLastChange = useCallback(async () => {
    if (needsElevation) { setError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
    if (locked.current) return;
    locked.current = true;
    setBusy(true); setError(null); setResult(null);
    try {
      const receipt = requireHostsChangeReceipt(await backendRef.current.undoHostsBlocklistChange());
      void showSuccess("Last WinCom blocking change undone and verified.");
      if (receipt.dnsWarning) void showWarning(receipt.dnsWarning);
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Undo failed."); }
    finally {
      await refreshUndo().catch(() => setUndo(undefined));
      void refreshNetwork(true, true);
      locked.current = false; setBusy(false);
    }
  }, [needsElevation, refreshNetwork, refreshUndo]);

  return { result, undo, busy, error, needsElevation, inspect, undoLastChange };
}
