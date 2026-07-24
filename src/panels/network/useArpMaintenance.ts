import { useCallback, useRef, useState } from "react";
import { useBackend, type ArpClearResult, type ArpScan } from "../../hooks/useBackend";

export function useArpMaintenance() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [scan, setScan] = useState<ArpScan>();
  const [result, setResult] = useState<ArpClearResult>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const inspect = useCallback(async () => {
    setBusy(true); setError(undefined); setResult(undefined);
    try { setScan(await backendRef.current.arpCacheScan()); } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  }, []);
  const clear = useCallback(async () => {
    if (!scan) return;
    setBusy(true); setError(undefined);
    try { const next = await backendRef.current.arpCacheClear(scan.scanId); setResult(next); setScan(await backendRef.current.arpCacheScan()); } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  }, [scan]);
  return { scan, result, busy, error, inspect, clear };
}
