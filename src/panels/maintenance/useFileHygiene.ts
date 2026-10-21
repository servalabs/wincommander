import { useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  useBackend,
  type DuplicateRemoveResult,
  type DuplicateScan,
  type EmptyFolderRemoveResult,
  type EmptyFolderScan,
} from "../../hooks/useBackend";
import { recordMaintenanceOperation } from "../../hooks/useMaintenanceHistory";
import { getRetainedFileId, selectDuplicateCopies, type DuplicateKeepPolicy } from "../../lib/maintenanceSelection";
import { useMaintenanceSessionState } from "./maintenanceSessionState";

type FileTool = "duplicates" | "empty";

export function useFileHygiene() {
  const backend = useBackend();
  const [keepPolicy, setKeepPolicy] = useMaintenanceSessionState<DuplicateKeepPolicy>("file-hygiene.keep-policy", "first");
  const [retainedFiles, setRetainedFiles] = useMaintenanceSessionState<Record<string, string>>("file-hygiene.retained-files", {});
  const [roots, setRoots] = useMaintenanceSessionState<string[]>("file-hygiene.roots", []);
  const [tool, setTool] = useMaintenanceSessionState<FileTool>("file-hygiene.tool", "duplicates");
  const [duplicateScan, setDuplicateScan] = useMaintenanceSessionState<DuplicateScan | undefined>("file-hygiene.duplicate-scan", undefined);
  const [emptyScan, setEmptyScan] = useMaintenanceSessionState<EmptyFolderScan | undefined>("file-hygiene.empty-scan", undefined);
  const [selected, setSelected] = useMaintenanceSessionState<Set<string>>("file-hygiene.selected", new Set());
  const [result, setResult] = useMaintenanceSessionState<DuplicateRemoveResult | EmptyFolderRemoveResult | undefined>("file-hygiene.result", undefined);
  const [busy, setBusy] = useMaintenanceSessionState("file-hygiene.busy", false);
  const [error, setError] = useMaintenanceSessionState<string | undefined>("file-hygiene.error", undefined);

  const chooseRoots = useCallback(async () => {
    if (busy) return;
    const chosen = await open({ directory: true, multiple: true, title: "Choose folders to inspect" });
    if (!chosen) return;
    const picked = Array.isArray(chosen) ? chosen : [chosen];
    setRoots((current) => [...new Set([...current, ...picked])]);
    setDuplicateScan(undefined);
    setEmptyScan(undefined);
    setSelected(new Set());
  }, [busy, setDuplicateScan, setEmptyScan, setRoots, setSelected]);

  const removeRoot = useCallback((root: string) => {
    if (busy) return;
    setRoots((current) => current.filter((existing) => existing !== root));
    setDuplicateScan(undefined);
    setEmptyScan(undefined);
    setSelected(new Set());
  }, [busy, setDuplicateScan, setEmptyScan, setRoots, setSelected]);

  const scan = useCallback(async () => {
    if (busy || !roots.length) return;
    setBusy(true);
    setError(undefined);
    setResult(undefined);
    setSelected(new Set());
    setRetainedFiles({});
    if (tool === "duplicates") setDuplicateScan(undefined);
    else setEmptyScan(undefined);
    try {
      if (tool === "duplicates") setDuplicateScan(await backend.duplicateFinderScan(roots));
      else setEmptyScan(await backend.emptyFolderCleanerScan(roots));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }, [backend, busy, roots, setBusy, setDuplicateScan, setEmptyScan, setError, setResult, setSelected, setRetainedFiles, tool]);

  const remove = useCallback(async () => {
    if (busy || !selected.size) return;
    setBusy(true);
    setError(undefined);
    setResult(undefined);
    let hasReturnedResult = false;
    try {
      const ids = [...selected];
      const next = tool === "duplicates"
        ? await backend.duplicateFinderRemove(ids)
        : await backend.emptyFolderCleanerRemove(ids);
      setResult(next);
      hasReturnedResult = true;
      recordMaintenanceOperation({ tool: tool === "duplicates" ? "duplicates" : "empty-folders", title: tool === "duplicates" ? "Duplicate file removal" : "Empty folder removal", outcome: next.cancelled ? "cancelled" : next.errors.length ? "partial" : "completed", affectedItems: "filesRemoved" in next ? next.filesRemoved : next.foldersRemoved, bytesRecovered: "bytesRecovered" in next ? next.bytesRecovered : undefined });
      // Removal results don't identify successful IDs; only a new scan can reconcile failures.
      setDuplicateScan(undefined);
      setEmptyScan(undefined);
      setSelected(new Set());
      if (tool === "duplicates") setDuplicateScan(await backend.duplicateFinderScan(roots));
      else setEmptyScan(await backend.emptyFolderCleanerScan(roots));
    } catch (cause) {
      if (!hasReturnedResult) recordMaintenanceOperation({ tool: tool === "duplicates" ? "duplicates" : "empty-folders", title: tool === "duplicates" ? "Duplicate file removal" : "Empty folder removal", outcome: "failed" });
      setSelected(new Set());
      setError(hasReturnedResult ? `Removal finished, but refreshing the scan failed: ${String(cause)}` : String(cause));
    } finally {
      setBusy(false);
    }
  }, [backend, busy, roots, selected, setBusy, setDuplicateScan, setEmptyScan, setError, setResult, setSelected, tool]);

  const cancel = useCallback(async () => {
    try {
      if (tool === "duplicates") await backend.duplicateFinderCancel();
      else await backend.emptyFolderCleanerCancel();
    } catch (cause) { setError(String(cause)); }
  }, [backend, setError, tool]);

  const select = useCallback((id: string) => {
    if (busy) return;
    if (tool === "duplicates" && !duplicateScan?.groups.some((group) => group.files.some((file) => file.id === id) && getRetainedFileId(group, keepPolicy, retainedFiles) !== id)) return;
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, [busy, duplicateScan, keepPolicy, retainedFiles, setSelected, tool]);

  const changeTool = useCallback((next: FileTool) => {
    if (busy) return;
    setTool(next);
    setSelected(new Set());
    setResult(undefined);
    setError(undefined);
  }, [busy, setError, setResult, setSelected, setTool]);

  return {
    roots, tool, duplicateScan, emptyScan, selected, result, busy, error,
    chooseRoots, removeRoot, scan, remove, cancel, select, changeTool, keepPolicy,
    retainedFileId: (group: NonNullable<DuplicateScan>["groups"][number]) => getRetainedFileId(group, keepPolicy, retainedFiles),
    keepFile: (groupId: string, fileId: string) => {
      if (busy) return;
      setRetainedFiles((current) => ({ ...current, [groupId]: fileId }));
      setSelected((current) => { const next = new Set(current); next.delete(fileId); return next; });
    },
    changeKeepPolicy: (next: DuplicateKeepPolicy) => { if (!busy) { setKeepPolicy(next); setSelected(new Set()); } },
    smartSelect: () => { if (!busy) setSelected(tool === "duplicates" ? selectDuplicateCopies(duplicateScan?.groups ?? [], keepPolicy, retainedFiles) : new Set(emptyScan?.folders.map((folder) => folder.id) ?? [])); },
    clearSelection: () => { if (!busy) setSelected(new Set()); },
  };
}
