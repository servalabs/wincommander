import { useCallback, useEffect, useRef, useState } from "react";
import useBackend, { type AIControlStatus } from "./useBackend";

export type ClassicWindowsAppsCheckState = "checking" | "ready" | "failed";

export function useClassicWindowsAppsStatus() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [status, setStatus] = useState<AIControlStatus>();
  const [checkState, setCheckState] = useState<ClassicWindowsAppsCheckState>("checking");
  const [checkError, setCheckError] = useState<string>();
  const [lastCheckedAt, setLastCheckedAt] = useState<Date>();

  const check = useCallback(async () => {
    setCheckState("checking");
    setCheckError(undefined);
    const checkedAt = new Date();
    setLastCheckedAt(checkedAt);

    try {
      const response = await backendRef.current.getAIControlStatus();
      if (!response.success || !response.data) {
        setStatus(undefined);
        setCheckState("failed");
        setCheckError(response.error || "WinCommander could not check these apps.");
        return false;
      }

      setStatus(response.data);
      setCheckState("ready");
      return true;
    } catch (cause) {
      setStatus(undefined);
      setCheckState("failed");
      setCheckError(cause instanceof Error ? cause.message : "WinCommander could not check these apps.");
      return false;
    }
  }, []);

  useEffect(() => {
    void check();
  }, [check]);

  return { check, checkError, checkState, lastCheckedAt, status };
}
