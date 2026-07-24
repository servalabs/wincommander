// src/panels/apps/useDebloatInventory.ts
import { useCallback, useEffect, useMemo, useState } from "react";
import useBackend from "../../hooks/useBackend";
import { useAppState } from "../../context/AppContext";
import { DebloatItem } from "./types";
import { isSystemCritical, getFriendlyName, getCategoryForStoreId, RECOMMENDED_IDS } from "./debloatLists";

export interface UseDebloatInventoryResult {
  items: DebloatItem[];
  loading: boolean;
  errors: { store?: string; programs?: string };
  bcuInstalled: boolean;
  bcuInstalling: boolean;
  rescan: () => void;
  installBcu: () => Promise<void>;
}

export function useDebloatInventory(): UseDebloatInventoryResult {
  const { appSettings, refreshSettings } = useAppState();
  const {
    getInstalledAppxInventory, removeAppxByName,
    testBcuInstalled, installBcu: installBcuCmd,
    getBcuApplicationList, bcuQuietUninstallSingle, bcuLoudUninstallSingle,
    testEdgeInstalled, testOneDriveInstalled, getTeamsStatus,
    removeEdge, removeOneDrive, removeTeams, removeCopilotAIComponents,
  } = useBackend();

  const [storeItems, setStoreItems] = useState<DebloatItem[]>([]);
  const [programItems, setProgramItems] = useState<DebloatItem[]>([]);
  const [windowsItems, setWindowsItems] = useState<DebloatItem[]>([]);
  const [storeLoading, setStoreLoading] = useState(true);
  const [programsLoading, setProgramsLoading] = useState(true);
  const [storeError, setStoreError] = useState<string | undefined>();
  const [programsError, setProgramsError] = useState<string | undefined>();
  const [bcuInstalled, setBcuInstalled] = useState(false);
  const [bcuInstalling, setBcuInstalling] = useState(false);
  const [removedWindowsExtraIds, setRemovedWindowsExtraIds] = useState<Set<string>>(new Set());
  const [scanTrigger, setScanTrigger] = useState(0);

  // Store apps scan
  useEffect(() => {
    let cancelled = false;
    setStoreLoading(true);
    setStoreError(undefined);
    (async () => {
      try {
        const res = await getInstalledAppxInventory();
        if (cancelled) return;
        const rawList: any[] = Array.isArray(res.data)
          ? (res.data as any[])
          : ((res.data as any)?.apps ?? []);
        if (res.success) {
          const normalized = rawList
            .map((a: any) => ({ name: (a.name ?? a.Name ?? "") as string }))
            .filter(a => a.name && !isSystemCritical(a.name));
          setStoreItems(normalized.map(a => ({
            id: a.name,
            label: getFriendlyName(a.name),
            source: "store" as const,
            category: getCategoryForStoreId(a.name),
            recommended: RECOMMENDED_IDS.has(a.name),
            remove: async () => {
              const r = await removeAppxByName(a.name);
              if (!r.success) return { success: false, error: r.error };
              const status = (r.data as any)?.status;
              if (status === "removed" || status === "not_found") return { success: true };
              return {
                success: false,
                error: (r.data as any)?.message ?? `Removal did not complete for ${a.name}`,
              };
            },
          })));
        } else {
          setStoreError(res.error ?? "Scan failed");
        }
      } catch (e) {
        if (!cancelled) setStoreError(e instanceof Error ? e.message : String(e));
      } finally {
        if (!cancelled) setStoreLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [scanTrigger]); // eslint-disable-line react-hooks/exhaustive-deps

  // BCU check + programs scan
  useEffect(() => {
    let cancelled = false;
    setProgramsLoading(true);
    setProgramsError(undefined);
    (async () => {
      try {
        const testRes = await testBcuInstalled();
        const installed = testRes.data?.installed ?? false;
        if (cancelled) return;
        setBcuInstalled(installed);
        if (!installed) { setProgramsLoading(false); return; }
        const res = await getBcuApplicationList();
        if (cancelled) return;
        if (res.success && res.data) {
          setProgramItems(
            res.data.apps
              .filter(a => a.uninstallerKind !== "StoreApp")
              .map(a => ({
                id: a.displayName,
                label: a.displayName,
                source: "program" as const,
                category: "Programs",
                sizeKB: a.estimatedSizeKB,
                recommended: false,
                remove: async () => {
                  const r = a.canQuietUninstall
                    ? await bcuQuietUninstallSingle(a.displayName)
                    : await bcuLoudUninstallSingle(a.displayName);
                  return { success: r.success, error: r.error };
                },
              }))
          );
        } else {
          setProgramsError(res.error ?? "Scan failed");
        }
      } catch (e) {
        if (!cancelled) setProgramsError(e instanceof Error ? e.message : String(e));
      } finally {
        if (!cancelled) setProgramsLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [scanTrigger]); // eslint-disable-line react-hooks/exhaustive-deps

  // Windows extras (Edge, OneDrive, Teams, Copilot AI)
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const [edgeRes, odRes, teamsRes] = await Promise.all([
        testEdgeInstalled(),
        testOneDriveInstalled(),
        getTeamsStatus(),
      ]);
      if (cancelled) return;
      const items: DebloatItem[] = [];
      if (edgeRes.data?.installed) {
        items.push({
          id: "edge", label: "Microsoft Edge", source: "windows", category: "Windows extras",
          recommended: true, riskNote: "Removes browser from Windows Sandbox",
          remove: async () => { const r = await removeEdge(); return { success: r.success, error: r.error }; },
        });
      }
      if (odRes.data?.installed) {
        items.push({
          id: "onedrive", label: "OneDrive", source: "windows", category: "Windows extras",
          recommended: true, riskNote: "Disables OneDrive sync; data not deleted",
          remove: async () => { const r = await removeOneDrive(); return { success: r.success, error: r.error }; },
        });
      }
      if (teamsRes.data?.installed) {
        items.push({
          id: "teams", label: "Microsoft Teams", source: "windows", category: "Windows extras",
          recommended: true,
          remove: async () => { const r = await removeTeams(); return { success: r.success, error: r.error }; },
        });
      }
      const copilotRemoved = appSettings?.current?.tweaks?.security?.copilotAiRemoved === true
        || removedWindowsExtraIds.has("copilot-ai");
      if (!copilotRemoved) {
        items.push({
          id: "copilot-ai", label: "Copilot AI Components", source: "windows", category: "Windows extras",
          recommended: true,
          remove: async () => {
            const r = await removeCopilotAIComponents();
            if (r.success) {
              setRemovedWindowsExtraIds(prev => new Set([...prev, "copilot-ai"]));
              setWindowsItems(prev => prev.filter(item => item.id !== "copilot-ai"));
              await refreshSettings();
            }
            return { success: r.success, error: r.error };
          },
        });
      }
      setWindowsItems(items);
    })();
    return () => { cancelled = true; };
  }, [scanTrigger, appSettings?.current?.tweaks?.security?.copilotAiRemoved, removedWindowsExtraIds]); // eslint-disable-line react-hooks/exhaustive-deps

  const items = useMemo(
    () => [...storeItems, ...programItems, ...windowsItems],
    [storeItems, programItems, windowsItems]
  );

  const rescan = useCallback(() => setScanTrigger(t => t + 1), []);

  const installBcu = useCallback(async () => {
    setBcuInstalling(true);
    try {
      const res = await installBcuCmd();
      if (res.success) {
        setBcuInstalled(true);
        rescan();
      }
    } finally {
      setBcuInstalling(false);
    }
  }, [installBcuCmd, rescan]);

  return {
    items,
    loading: storeLoading || programsLoading,
    errors: { store: storeError, programs: programsError },
    bcuInstalled,
    bcuInstalling,
    rescan,
    installBcu,
  };
}
