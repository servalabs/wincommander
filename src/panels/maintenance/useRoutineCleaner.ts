import { useCallback, useMemo } from "react";
import useBackend, { type RoutineCleanerCategory, type RoutineCleanerCleanResult, type RoutineCleanerScan } from "../../hooks/useBackend";
import { getRecommendedItemIds, getScanAfterClean, getSelectedItems, ROUTINE_CLEANER_CATEGORIES, toggleRoutineCategorySelection } from "./routineCleanerHelpers";
import { recordMaintenanceOperation } from "../../hooks/useMaintenanceHistory";
import { useMaintenanceSessionState } from "./maintenanceSessionState";

type RoutineCleanerOperation = "idle" | "scanning" | "cleaning";

export function useRoutineCleaner(initialCategories: RoutineCleanerCategory[] = ROUTINE_CLEANER_CATEGORIES.map((category) => category.id)) {
  const { routineCleanerScan, routineCleanerClean, routineCleanerCancel } = useBackend();
  const sessionKey = `routine-cleaner.${[...initialCategories].sort().join("-")}`;
  const [categories, setCategories] = useMaintenanceSessionState<RoutineCleanerCategory[]>(`${sessionKey}.categories`, initialCategories);
  const [scan, setScan] = useMaintenanceSessionState<RoutineCleanerScan | null>(`${sessionKey}.scan`, null);
  const [selectedIds, setSelectedIds] = useMaintenanceSessionState<Set<string>>(`${sessionKey}.selected`, new Set());
  const [operation, setOperation] = useMaintenanceSessionState<RoutineCleanerOperation>(`${sessionKey}.operation`, "idle");
  const [result, setResult] = useMaintenanceSessionState<RoutineCleanerCleanResult | null>(`${sessionKey}.result`, null);
  const [error, setError] = useMaintenanceSessionState<string | null>(`${sessionKey}.error`, null);

  const selectedItems = useMemo(
    () => getSelectedItems(scan?.items ?? [], selectedIds),
    [scan?.items, selectedIds],
  );

  const setCategory = useCallback((category: RoutineCleanerCategory) => {
    if (operation !== "idle") return;
    setCategories((current) => current.includes(category)
      ? current.filter((item) => item !== category)
      : [...current, category]);
  }, [operation, setCategories]);

  const scanSelected = useCallback(async () => {
    if (!categories.length || operation !== "idle") return;
    setOperation("scanning");
    setSelectedIds(new Set());
    setScan(null);
    setError(null);
    setResult(null);
    try {
      const nextScan = await routineCleanerScan(categories);
      setScan(nextScan);
      setSelectedIds(new Set(getRecommendedItemIds(nextScan.items)));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setOperation("idle");
    }
  }, [categories, operation, routineCleanerScan, setError, setOperation, setResult, setScan, setSelectedIds]);

  const cleanSelected = useCallback(async () => {
    if (!selectedItems.length || operation !== "idle") return;
    setOperation("cleaning");
    setError(null);
    setResult(null);
    try {
      const cleanResult = await routineCleanerClean(selectedItems.map((item) => item.id));
      setResult(cleanResult);
      recordMaintenanceOperation({ tool: "cleanup", title: "Application cache cleanup", outcome: cleanResult.cancelled ? "cancelled" : cleanResult.errors.length ? "partial" : "completed", affectedItems: cleanResult.itemsCleaned, bytesRecovered: cleanResult.bytesRecovered, detail: "Logical data removed; free space not measured." });
      if (cleanResult.cancelled) { setScan(null); setSelectedIds(new Set()); return; }
      const failedIds = new Set(cleanResult.errors.map((item) => item.id));
      const cleanedIds = selectedItems.map((item) => item.id).filter((id) => !failedIds.has(id));
      setScan((current) => current ? getScanAfterClean(current, cleanedIds) : current);
      setSelectedIds(failedIds);
    } catch (reason) {
      recordMaintenanceOperation({ tool: "cleanup", title: "Application cache cleanup", outcome: "failed" });
      setScan(null);
      setSelectedIds(new Set());
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setOperation("idle");
    }
  }, [operation, routineCleanerClean, selectedItems, setError, setOperation, setResult, setScan, setSelectedIds]);

  const cancel = useCallback(async () => {
    try {
      await routineCleanerCancel();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [routineCleanerCancel, setError]);

  const toggleItem = useCallback((id: string) => {
    if (operation !== "idle" || !scan?.items.some((item) => item.id === id)) return;
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, [operation, scan?.items, setSelectedIds]);

  const selectRecommended = useCallback(() => {
    if (operation !== "idle") return;
    setSelectedIds(new Set(getRecommendedItemIds(scan?.items ?? [])));
  }, [operation, scan?.items, setSelectedIds]);

  return {
    categories,
    scan,
    selectedIds,
    selectedItems,
    operation,
    result,
    error,
    setCategory,
    scanSelected,
    cleanSelected,
    cancel,
    toggleItem,
    toggleCategorySelection: (category: RoutineCleanerCategory) => {
      if (operation === "idle") setSelectedIds((current) => toggleRoutineCategorySelection(scan?.items ?? [], current, category));
    },
    selectRecommended,
    clearSelection: () => { if (operation === "idle") setSelectedIds(new Set()); },
  };
}
