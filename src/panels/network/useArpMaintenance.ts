import { useCallback, useEffect, useRef, useState } from "react";
import { useBackend, type ArpClearResult, type ArpScan } from "../../hooks/useBackend";
import { ARP_SCAN_TTL_MS, classifyArpError, type ErrorAdvice } from "../../lib/arpDiagnostics";

/** How often the age readout re-evaluates. The server-side snapshot expires at
 *  ARP_SCAN_TTL_MS, so the UI has to notice on its own — the user gets no event. */
const AGE_TICK_MS = 15_000;

export function useArpMaintenance() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [scan, setScan] = useState<ArpScan>();
  const [scannedAt, setScannedAt] = useState<number>();
  const [result, setResult] = useState<ArpClearResult>();
  const [inspecting, setInspecting] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [error, setError] = useState<ErrorAdvice>();
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!scannedAt) return;
    const id = setInterval(() => setNow(Date.now()), AGE_TICK_MS);
    return () => clearInterval(id);
  }, [scannedAt]);

  const inspect = useCallback(async () => {
    setInspecting(true);
    setError(undefined);
    setResult(undefined);
    try {
      setScan(await backendRef.current.arpCacheScan());
      setScannedAt(Date.now());
      setNow(Date.now());
    } catch (cause) {
      setError(classifyArpError(String(cause)));
    } finally {
      setInspecting(false);
    }
  }, []);

  const clear = useCallback(async () => {
    if (!scan) return;
    setClearing(true);
    setError(undefined);
    try {
      setResult(await backendRef.current.arpCacheClear(scan.scanId));
      setScan(await backendRef.current.arpCacheScan());
      setScannedAt(Date.now());
      setNow(Date.now());
    } catch (cause) {
      const raw = String(cause);
      setError(classifyArpError(raw));
      // An expired scan_id can never be accepted again. Drop the snapshot so
      // the card falls back to its pre-scan state instead of leaving a dead
      // Clear button pointed at a selection the server has forgotten.
      if (raw.toLowerCase().includes("expired or is invalid")) {
        setScan(undefined);
        setScannedAt(undefined);
        setResult(undefined);
      }
    } finally {
      setClearing(false);
    }
  }, [scan]);

  const ageMs = scannedAt ? Math.max(0, now - scannedAt) : 0;

  return {
    scan,
    scannedAt,
    ageMs,
    isStale: scannedAt != null && ageMs > ARP_SCAN_TTL_MS,
    result,
    error,
    inspecting,
    clearing,
    busy: inspecting || clearing,
    inspect,
    clear,
  };
}
