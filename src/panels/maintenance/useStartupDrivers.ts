import { useCallback, useRef } from "react";
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

  const scanStartup = useCallback(async () => {
    setBusy(true); setError(undefined);
    try { setStartup(await backendRef.current.startupImpactScan()); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [setBusy, setError, setStartup]);

  const scanDrivers = useCallback(async () => {
    setBusy(true); setError(undefined);
    try { setDrivers(await backendRef.current.driverMaintenanceInventory()); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [setBusy, setDrivers, setError]);

  const openUpdates = useCallback(async () => {
    setError(undefined);
    try { await backendRef.current.driverUpdateSeam(); }
    catch (cause) { setError(String(cause)); }
  }, [setError]);

  return { startup, drivers, busy, error, scanStartup, scanDrivers, openUpdates };
}
