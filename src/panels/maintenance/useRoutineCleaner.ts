import { useCallback, useMemo } from "react";
import useBackend, { type RoutineCleanerCategory, type RoutineCleanerCleanResult, type RoutineCleanerScan } from "../../hooks/useBackend";
import { getRecommendedItemIds, getScanAfterClean, getSelectedItems, ROUTINE_CLEANER_CATEGORIES } from "./routineCleanerHelpers";
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
    try {
      const cleanResult = await routineCleanerClean(selectedItems.map((item) => item.id));
      setResult(cleanResult);
      if (cleanResult.cancelled) return;
      const failedIds = new Set(cleanResult.errors.map((item) => item.id));
      const cleanedIds = selectedItems.map((item) => item.id).filter((id) => !failedIds.has(id));
      setScan((current) => current ? getScanAfterClean(current, cleanedIds) : current);
      setSelectedIds(failedIds);
    } catch (reason) {
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
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, [setSelectedIds]);

  const selectRecommended = useCallback(() => {
    setSelectedIds(new Set(getRecommendedItemIds(scan?.items ?? [])));
  }, [scan?.items, setSelectedIds]);

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
    selectRecommended,
  };
}
