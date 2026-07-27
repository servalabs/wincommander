import { useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  useBackend,
  type DuplicateRemoveResult,
  type DuplicateScan,
  type EmptyFolderRemoveResult,
  type EmptyFolderScan,
} from "../../hooks/useBackend";
import { useMaintenanceSessionState } from "./maintenanceSessionState";

type FileTool = "duplicates" | "empty";

export function useFileHygiene() {
  const backend = useBackend();
  const [roots, setRoots] = useMaintenanceSessionState<string[]>("file-hygiene.roots", []);
  const [tool, setTool] = useMaintenanceSessionState<FileTool>("file-hygiene.tool", "duplicates");
  const [duplicateScan, setDuplicateScan] = useMaintenanceSessionState<DuplicateScan | undefined>("file-hygiene.duplicate-scan", undefined);
  const [emptyScan, setEmptyScan] = useMaintenanceSessionState<EmptyFolderScan | undefined>("file-hygiene.empty-scan", undefined);
  const [selected, setSelected] = useMaintenanceSessionState<Set<string>>("file-hygiene.selected", new Set());
  const [result, setResult] = useMaintenanceSessionState<DuplicateRemoveResult | EmptyFolderRemoveResult | undefined>("file-hygiene.result", undefined);
  const [busy, setBusy] = useMaintenanceSessionState("file-hygiene.busy", false);
  const [error, setError] = useMaintenanceSessionState<string | undefined>("file-hygiene.error", undefined);

  const chooseRoots = useCallback(async () => {
    const chosen = await open({ directory: true, multiple: true, title: "Choose folders to inspect" });
    if (!chosen) return;
    const picked = Array.isArray(chosen) ? chosen : [chosen];
    setRoots((current) => [...new Set([...current, ...picked])]);
    setDuplicateScan(undefined);
    setEmptyScan(undefined);
    setSelected(new Set());
  }, [setDuplicateScan, setEmptyScan, setRoots, setSelected]);

  const removeRoot = useCallback((root: string) => {
    setRoots((current) => current.filter((existing) => existing !== root));
    setDuplicateScan(undefined);
    setEmptyScan(undefined);
    setSelected(new Set());
  }, [setDuplicateScan, setEmptyScan, setRoots, setSelected]);

  const scan = useCallback(async () => {
    if (!roots.length) return;
    setBusy(true);
    setError(undefined);
    setResult(undefined);
    setSelected(new Set());
    try {
      if (tool === "duplicates") setDuplicateScan(await backend.duplicateFinderScan(roots));
      else setEmptyScan(await backend.emptyFolderCleanerScan(roots));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }, [backend, roots, setBusy, setDuplicateScan, setEmptyScan, setError, setResult, setSelected, tool]);

  const remove = useCallback(async () => {
    if (!selected.size) return;
    setBusy(true);
    setError(undefined);
    try {
      const ids = [...selected];
      const next = tool === "duplicates"
        ? await backend.duplicateFinderRemove(ids)
        : await backend.emptyFolderCleanerRemove(ids);
      setResult(next);
      if (tool === "duplicates") {
        setDuplicateScan((current) => current && ({
          ...current,
          groups: current.groups
            .map((group) => ({ ...group, files: group.files.filter((file) => !selected.has(file.id)) }))
            .filter((group) => group.files.length > 1),
        }));
      } else {
        setEmptyScan((current) => current && ({
          ...current,
          folders: current.folders.filter((folder) => !selected.has(folder.id)),
        }));
      }
      setSelected(new Set());
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }, [backend, selected, setBusy, setDuplicateScan, setEmptyScan, setError, setResult, setSelected, tool]);

  const cancel = useCallback(async () => {
    if (tool === "duplicates") await backend.duplicateFinderCancel();
    else await backend.emptyFolderCleanerCancel();
  }, [backend, tool]);

  const select = useCallback((id: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, [setSelected]);

  const changeTool = useCallback((next: FileTool) => {
    setTool(next);
    setSelected(new Set());
    setResult(undefined);
    setError(undefined);
  }, [setError, setResult, setSelected, setTool]);

  return {
    roots, tool, duplicateScan, emptyScan, selected, result, busy, error,
    chooseRoots, removeRoot, scan, remove, cancel, select, changeTool,
  };
}
