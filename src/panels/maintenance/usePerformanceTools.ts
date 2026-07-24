import { useCallback, useRef } from "react";
import { useBackend, type GameModePreview, type PerformanceSnapshot } from "../../hooks/useBackend";
import { useMaintenanceSessionState } from "./maintenanceSessionState";

export function usePerformanceTools() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [snapshot, setSnapshot] = useMaintenanceSessionState<PerformanceSnapshot | undefined>("performance.snapshot", undefined);
  const [gameMode, setGameMode] = useMaintenanceSessionState<GameModePreview | undefined>("performance.game-mode", undefined);
  const [busy, setBusy] = useMaintenanceSessionState("performance.busy", false);
  const [error, setError] = useMaintenanceSessionState<string | undefined>("performance.error", undefined);

  const refresh = useCallback(async () => {
    setBusy(true); setError(undefined);
    try {
      const [performance, profile] = await Promise.all([
        backendRef.current.getPerformanceSnapshot(),
        backendRef.current.gameModePreview(),
      ]);
      setSnapshot(performance); setGameMode(profile);
    } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [setBusy, setError, setGameMode, setSnapshot]);

  const apply = useCallback(async () => {
    setBusy(true); setError(undefined);
    try { setGameMode((await backendRef.current.gameModeApply()).preview); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [setBusy, setError, setGameMode]);

  const restore = useCallback(async () => {
    setBusy(true); setError(undefined);
    try { setGameMode((await backendRef.current.gameModeRestore()).preview); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [setBusy, setError, setGameMode]);

  return { snapshot, gameMode, busy, error, refresh, apply, restore };
}
