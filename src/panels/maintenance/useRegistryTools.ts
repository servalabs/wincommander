import { useCallback } from "react";
import {
  useBackend,
  type ExplorerContextResult,
  type ExplorerContextScan,
  type RegistryCleanerResult,
  type RegistryCleanerScan,
} from "../../hooks/useBackend";
import { useMaintenanceSessionState } from "./maintenanceSessionState";

type RegistryTool = "orphans" | "context";
type ContextAction = "disable" | "enable" | "remove";

export function useRegistryTools() {
  const backend = useBackend();
  const [tool, setTool] = useMaintenanceSessionState<RegistryTool>("registry-hygiene.tool", "orphans");
  const [registryScan, setRegistryScan] = useMaintenanceSessionState<RegistryCleanerScan | undefined>("registry-hygiene.registry-scan", undefined);
  const [contextScan, setContextScan] = useMaintenanceSessionState<ExplorerContextScan | undefined>("registry-hygiene.context-scan", undefined);
  const [selected, setSelected] = useMaintenanceSessionState<Set<string>>("registry-hygiene.selected", new Set());
  const [busy, setBusy] = useMaintenanceSessionState("registry-hygiene.busy", false);
  const [error, setError] = useMaintenanceSessionState<string | undefined>("registry-hygiene.error", undefined);
  const [result, setResult] = useMaintenanceSessionState<RegistryCleanerResult | ExplorerContextResult | undefined>("registry-hygiene.result", undefined);

  const scan = useCallback(async () => {
    setBusy(true);
    setError(undefined);
    setResult(undefined);
    setSelected(new Set());
    try {
      if (tool === "orphans") setRegistryScan(await backend.registryCleanerScan());
      else setContextScan(await backend.explorerContextMenuScan());
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }, [backend, setBusy, setContextScan, setError, setRegistryScan, setResult, setSelected, tool]);

  const mutate = useCallback(async (action: ContextAction = "remove") => {
    if (!selected.size) return;
    setBusy(true);
    setError(undefined);
    try {
      const ids = [...selected];
      const next = tool === "orphans"
        ? await backend.registryCleanerRemove(ids)
        : await backend.explorerContextMenuRemediate(action, ids);
      setResult(next);
      if (tool === "orphans") {
        setRegistryScan((current) => current && ({ ...current, entries: current.entries.filter((entry) => !selected.has(entry.id)) }));
      } else {
        setContextScan(undefined);
      }
      setSelected(new Set());
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }, [backend, selected, setBusy, setContextScan, setError, setRegistryScan, setResult, setSelected, tool]);

  const select = useCallback((id: string) => setSelected((current) => {
    const next = new Set(current);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  }), [setSelected]);

  const changeTool = useCallback((next: RegistryTool) => {
    setTool(next);
    setSelected(new Set());
    setError(undefined);
    setResult(undefined);
  }, [setError, setResult, setSelected, setTool]);

  return { tool, registryScan, contextScan, selected, busy, error, result, scan, mutate, select, changeTool };
}
