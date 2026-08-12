import { useCallback, useEffect, useRef } from "react";
import { useBackend, type DriverMaintenanceInventory, type StartupImpactScan } from "../../hooks/useBackend";
import { useMaintenanceSessionState } from "./maintenanceSessionState";

export function useStartupDrivers() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [startup, setStartup] = useMaintenanceSessionState<StartupImpactScan | undefined>("startup-drivers.startup", undefined);
  const [drivers, setDrivers] = useMaintenanceSessionState<DriverMaintenanceInventory | undefined>("startup-drivers.drivers", undefined);
  const [busy, setBusy] = useMaintenanceSessionState("startup-drivers.busy", false);
  const [error, setError] = useMaintenanceSessionState<string | undefined>("startup-drivers.error", undefined);
  const initialScanRequested = useRef(false);

  const scanChecks = useCallback(async () => {
    setBusy(true); setError(undefined);
    try {
      const [startupScan, driverScan] = await Promise.all([
        backendRef.current.startupImpactScan(),
        backendRef.current.driverMaintenanceInventory(),
      ]);
      if (!Array.isArray(startupScan?.entries) || !Array.isArray(driverScan?.drivers)) {
        throw new Error("Startup or driver scan returned an invalid response.");
      }
      setStartup(startupScan);
      setDrivers(driverScan);
    } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [setBusy, setDrivers, setError, setStartup]);

  useEffect(() => {
    if ((!startup || !drivers) && !initialScanRequested.current) {
      initialScanRequested.current = true;
      void scanChecks();
    }
  }, [drivers, scanChecks, startup]);

  const openUpdates = useCallback(async () => {
    setError(undefined);
    try { await backendRef.current.driverUpdateSeam(); }
    catch (cause) { setError(String(cause)); }
  }, [setError]);

  return { startup, drivers, busy, error, scanChecks, openUpdates };
}
