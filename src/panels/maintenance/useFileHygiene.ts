import { useCallback, useEffect, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  useBackend,
  type DuplicateRemoveResult,
  type DuplicateScan,
  type DuplicateProgress,
  type EmptyFolderRemoveResult,
  type EmptyFolderScan,
} from "../../hooks/useBackend";
import { recordMaintenanceOperation } from "../../hooks/useMaintenanceHistory";
import { getRetainedFileId, selectDuplicateCopies, type DuplicateKeepPolicy } from "../../lib/maintenanceSelection";
import { useMaintenanceSessionState } from "./maintenanceSessionState";

type FileTool = "duplicates" | "empty";
const DUPLICATE_SIZE_CHOICES = [0, 200 * 1024, 1024 * 1024, 10 * 1024 * 1024, 100 * 1024 * 1024] as const;

export function useFileHygiene() {
  const backend = useBackend();
  const requestProgress = backend.duplicateFinderProgress;
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
  const [scopeMode, setScopeMode] = useMaintenanceSessionState<"local" | "folders">("file-hygiene.scope", "local");
  const [progress, setProgress] = useMaintenanceSessionState<DuplicateProgress | undefined>("file-hygiene.progress", undefined);
  const [minimumSizeBytes, setMinimumSizeBytes] = useMaintenanceSessionState<number>("file-hygiene.minimum-size-bytes", 200 * 1024);
  const enteredTool = useRef<FileTool | undefined>(undefined);

  const chooseRoots = useCallback(async () => {
    if (busy) return;
    const chosen = await open({ directory: true, multiple: true, title: "Choose folders to inspect" });
    if (!chosen) return;
    const picked = Array.isArray(chosen) ? chosen : [chosen];
    setRoots((current) => [...new Set([...current, ...picked])]);
    setScopeMode("folders");
    setDuplicateScan(undefined);
    setEmptyScan(undefined);
    setSelected(new Set());
  }, [busy, setDuplicateScan, setEmptyScan, setRoots, setSelected, setScopeMode]);

  const removeRoot = useCallback((root: string) => {
    if (busy) return;
    setRoots((current) => current.filter((existing) => existing !== root));
    setDuplicateScan(undefined);
    setEmptyScan(undefined);
    setSelected(new Set());
  }, [busy, setDuplicateScan, setEmptyScan, setRoots, setSelected]);

  const scan = useCallback(async (requestedMode?: "local" | "folders") => {
    const mode = requestedMode ?? scopeMode;
    if (busy || ((tool === "empty" || mode === "folders") && !roots.length)) return;
    setBusy(true);
    setError(undefined);
    setResult(undefined);
    setSelected(new Set());
    setRetainedFiles({});
    setProgress(undefined);
    if (tool === "duplicates") setScopeMode(mode);
    if (tool === "duplicates") setDuplicateScan(undefined);
    else setEmptyScan(undefined);
    try {
      if (tool === "duplicates") setDuplicateScan(await (mode === "local" ? backend.duplicateFinderScanLocal(minimumSizeBytes) : backend.duplicateFinderScan(roots, minimumSizeBytes)));
      else setEmptyScan(await backend.emptyFolderCleanerScan(roots));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }, [backend, busy, minimumSizeBytes, roots, scopeMode, setBusy, setDuplicateScan, setEmptyScan, setError, setResult, setSelected, setRetainedFiles, setProgress, setScopeMode, tool]);

  useEffect(() => {
    if (enteredTool.current === tool) return;
    enteredTool.current = tool;
    // This store lives for the desktop-app session, while Maintenance tabs
    // mount and unmount as people navigate. Start one useful local scan on
    // the first visit after app launch, then keep the completed report in
    // place until the person explicitly asks to scan again. Re-entering the
    // tab must never discard a report or restart an expensive drive walk.
    if (tool === "duplicates" && !busy && !duplicateScan) void scan("local");
  }, [busy, duplicateScan, scan, tool]);

  useEffect(() => {
    if (!busy || tool !== "duplicates" || duplicateScan) return;
    let disposed = false;
    let timer: number | undefined;
    const poll = async () => {
      try { const next = await requestProgress(); if (!disposed) setProgress(next); }
      catch (cause) { if (!disposed) setError(`Scan progress unavailable: ${String(cause)}`); }
      if (!disposed) timer = window.setTimeout(() => void poll(), 600);
    };
    void poll();
    return () => { disposed = true; window.clearTimeout(timer); };
  }, [requestProgress, busy, duplicateScan, setError, setProgress, tool]);

  const loadGroups = useCallback(async (offset: number) => {
    if (busy || !duplicateScan) return;
    setBusy(true); setError(undefined); setSelected(new Set()); setRetainedFiles({});
    try { setDuplicateScan(await backend.duplicateFinderPage(duplicateScan.scanId, offset)); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [backend, busy, duplicateScan, setBusy, setDuplicateScan, setError, setSelected, setRetainedFiles]);

  const loadMembers = useCallback(async (groupId: string, offset: number) => {
    if (busy || !duplicateScan) return;
    setBusy(true); setError(undefined); setSelected(new Set()); setRetainedFiles({});
    try {
      const group = await backend.duplicateFinderGroupPage(duplicateScan.scanId, groupId, offset);
      setDuplicateScan((current) => current ? { ...current, groups: current.groups.map((entry) => entry.id === group.id ? group : entry) } : current);
    } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  }, [backend, busy, duplicateScan, setBusy, setDuplicateScan, setError, setSelected, setRetainedFiles]);

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
      recordMaintenanceOperation({ tool: tool === "duplicates" ? "duplicates" : "empty-folders", title: tool === "duplicates" ? "Duplicate file removal" : "Empty folder removal", outcome: next.cancelled ? "cancelled" : next.errors.length ? "partial" : "completed", affectedItems: "filesRemoved" in next ? next.filesRemoved : next.foldersRemoved, bytesRecovered: "bytesRecovered" in next ? next.bytesRecovered : undefined, detail: "bytesRecovered" in next ? "Logical file bytes removed; free space not measured." : undefined });
      // Removal results don't identify successful IDs; only a new scan can reconcile failures.
      setDuplicateScan(undefined);
      setEmptyScan(undefined);
      setSelected(new Set());
      if (tool === "duplicates") setDuplicateScan(await (scopeMode === "local" ? backend.duplicateFinderScanLocal(minimumSizeBytes) : backend.duplicateFinderScan(roots, minimumSizeBytes)));
      else setEmptyScan(await backend.emptyFolderCleanerScan(roots));
    } catch (cause) {
      if (!hasReturnedResult) recordMaintenanceOperation({ tool: tool === "duplicates" ? "duplicates" : "empty-folders", title: tool === "duplicates" ? "Duplicate file removal" : "Empty folder removal", outcome: "failed" });
      setSelected(new Set());
      setError(hasReturnedResult ? `Removal finished, but refreshing the scan failed: ${String(cause)}` : String(cause));
    } finally {
      setBusy(false);
    }
  }, [backend, busy, minimumSizeBytes, roots, scopeMode, selected, setBusy, setDuplicateScan, setEmptyScan, setError, setResult, setSelected, tool]);

  const cancel = useCallback(async () => {
    try {
      if (tool === "duplicates") await backend.duplicateFinderCancel();
      else await backend.emptyFolderCleanerCancel();
    } catch (cause) { setError(String(cause)); }
  }, [backend, setError, tool]);

  const openDuplicate = useCallback(async (fileId: string) => {
    if (busy || !duplicateScan) return;
    try { await backend.duplicateFinderOpen(duplicateScan.scanId, fileId); }
    catch (cause) { setError(String(cause)); }
  }, [backend, busy, duplicateScan, setError]);

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
    roots, tool, duplicateScan, emptyScan, selected, result, busy, error, scopeMode, progress, minimumSizeBytes, loadGroups, loadMembers,
    chooseRoots, removeRoot, scan, remove, cancel, openDuplicate, select, changeTool, keepPolicy,
    retainedFileId: (group: NonNullable<DuplicateScan>["groups"][number]) => getRetainedFileId(group, keepPolicy, retainedFiles),
    keepFile: (groupId: string, fileId: string) => {
      if (busy) return;
      setRetainedFiles((current) => ({ ...current, [groupId]: fileId }));
      setSelected((current) => { const next = new Set(current); next.delete(fileId); return next; });
    },
    changeKeepPolicy: (next: DuplicateKeepPolicy) => { if (!busy) { setKeepPolicy(next); setSelected(new Set()); } },
    changeMinimumSize: (next: number) => { if (!busy && DUPLICATE_SIZE_CHOICES.includes(next as typeof DUPLICATE_SIZE_CHOICES[number])) { setMinimumSizeBytes(next); setDuplicateScan(undefined); setSelected(new Set()); } },
    smartSelect: () => { if (!busy) setSelected(tool === "duplicates" ? selectDuplicateCopies(duplicateScan?.groups ?? [], keepPolicy, retainedFiles) : new Set(emptyScan?.folders.map((folder) => folder.id) ?? [])); },
    clearSelection: () => { if (!busy) setSelected(new Set()); },
  };
}
