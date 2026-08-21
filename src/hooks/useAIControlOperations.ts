import { useCallback, useRef, useState } from "react";
import { runOperation } from "../context/OperationContext";
import { showSuccess } from "../utils/toast";
import useBackend, { type AIControlOperationId, type AIControlStatus } from "./useBackend";

export type AIControlMode = "apply" | "revert";

export function useAIControlOperations() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [busyOperation, setBusyOperation] = useState<AIControlOperationId>();
  const [status, setStatus] = useState<AIControlStatus>();
  const [error, setError] = useState<string>();

  const refresh = useCallback(async () => {
    setError(undefined);
    const response = await backendRef.current.getAIControlStatus();
    if (!response.success || !response.data) {
      setError(response.error || "Could not read Windows AI component status.");
      return;
    }
    setStatus(response.data);
  }, []);

  const run = useCallback(async (
    operation: AIControlOperationId,
    label: string,
    mode: AIControlMode = "apply",
  ) => {
    setBusyOperation(operation);
    setError(undefined);
    try {
      await runOperation(label, [{
        label: `${label}…`,
        fn: async () => {
          const response = await backendRef.current.runAIControlOperation(operation, mode, true);
          if (!response.success) throw new Error(response.error || `${label} failed.`);
          return response.data;
        },
      }], { mode: "sequential", accent: "neutral" });
      showSuccess(`${label} completed.`);
      await refresh();
      return true;
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      return false;
    } finally {
      setBusyOperation(undefined);
    }
  }, [refresh]);

  return { busyOperation, status, error, refresh, run };
}
