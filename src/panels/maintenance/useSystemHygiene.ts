import { useCallback, useEffect, useRef } from "react";
import {
  useBackend,
  type EnvironmentScan,
  type ShortcutScan,
  type UninstallLeftoverScan,
} from "../../hooks/useBackend";
import { useMaintenanceSessionState } from "./maintenanceSessionState";

export type SystemHygieneTool = "shortcuts" | "environment" | "leftovers";

export function useSystemHygiene() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [tool, setTool] = useMaintenanceSessionState<SystemHygieneTool>("system-hygiene.tool", "shortcuts");
  const [shortcuts, setShortcuts] = useMaintenanceSessionState<ShortcutScan | undefined>("system-hygiene.shortcuts", undefined);
  const [environment, setEnvironment] = useMaintenanceSessionState<EnvironmentScan | undefined>("system-hygiene.environment", undefined);
  const [leftovers, setLeftovers] = useMaintenanceSessionState<UninstallLeftoverScan | undefined>("system-hygiene.leftovers", undefined);
  const [selected, setSelected] = useMaintenanceSessionState<Set<string>>("system-hygiene.selected", new Set());
  const [busy, setBusy] = useMaintenanceSessionState("system-hygiene.busy", false);
  const [error, setError] = useMaintenanceSessionState<string | undefined>("system-hygiene.error", undefined);
  const [summary, setSummary] = useMaintenanceSessionState<string | undefined>("system-hygiene.summary", undefined);
  const [hasPreScanned, setHasPreScanned] = useMaintenanceSessionState("system-hygiene.pre-scanned", false);

  const scan = useCallback(async () => {
    setBusy(true);
    setError(undefined);
    setSummary(undefined);
    setSelected(new Set());
    try {
      if (tool === "shortcuts") setShortcuts(await backendRef.current.shortcutCleanerScan());
      else if (tool === "environment") setEnvironment(await backendRef.current.environmentCleanerScan());
      else setLeftovers(await backendRef.current.uninstallLeftoversScan());
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }, [setBusy, setEnvironment, setError, setLeftovers, setSelected, setShortcuts, setSummary, tool]);

  // Preload every hygiene review once. The three tabs are cached snapshots;
  // changing tabs only changes what is shown, never launches background work.
  useEffect(() => {
    if (hasPreScanned || busy) return;
    setHasPreScanned(true);
    setBusy(true);
    setError(undefined);
    setSummary(undefined);
    Promise.all([
      backendRef.current.shortcutCleanerScan(),
      backendRef.current.environmentCleanerScan(),
      backendRef.current.uninstallLeftoversScan(),
    ])
      .then(([shortcutScan, environmentScan, leftoverScan]) => {
        setShortcuts(shortcutScan);
        setEnvironment(environmentScan);
        setLeftovers(leftoverScan);
      })
      .catch((cause) => setError(String(cause)))
      .finally(() => setBusy(false));
  }, [busy, hasPreScanned, setBusy, setEnvironment, setError, setHasPreScanned, setLeftovers, setShortcuts, setSummary]);

  const apply = useCallback(async () => {
    if (!selected.size) return;
    setBusy(true);
    setError(undefined);
    try {
      const ids = [...selected];
      if (tool === "shortcuts") {
        const result = await backendRef.current.shortcutCleanerRemove(ids);
        setSummary(`Removed ${result.removed} broken shortcuts; ${result.errors.length} errors.`);
        setShortcuts((current) => current && ({ ...current, shortcuts: current.shortcuts.filter((item) => !selected.has(item.id)) }));
      } else if (tool === "environment") {
        const result = await backendRef.current.environmentCleanerRepair(ids);
        setSummary(`Repaired ${result.repaired} environment entries; ${result.backupLocations.length} backups retained.`);
        setEnvironment((current) => current && ({ ...current, entries: current.entries.filter((item) => !selected.has(item.id)) }));
      } else {
        const result = await backendRef.current.uninstallLeftoversRemove(ids);
        setSummary(`Removed ${result.removed} leftover folders and recovered ${result.bytesRecovered.toLocaleString()} bytes.`);
        setLeftovers((current) => current && ({ ...current, entries: current.entries.filter((item) => !selected.has(item.id)) }));
      }
      setSelected(new Set());
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }, [selected, setBusy, setEnvironment, setError, setLeftovers, setSelected, setShortcuts, setSummary, tool]);

  const cancel = useCallback(async () => {
    if (tool === "shortcuts") await backendRef.current.shortcutCleanerCancel();
    if (tool === "leftovers") await backendRef.current.uninstallLeftoversCancel();
  }, [tool]);

  const select = useCallback((id: string) => setSelected((current) => {
    const next = new Set(current);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  }), [setSelected]);

  const changeTool = useCallback((next: SystemHygieneTool) => {
    setTool(next);
    setSelected(new Set());
    setError(undefined);
    setSummary(undefined);
  }, [setError, setSelected, setSummary, setTool]);

  return { tool, shortcuts, environment, leftovers, selected, busy, error, summary, scan, apply, cancel, select, changeTool };
}
