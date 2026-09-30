import { useCallback, useEffect, useMemo, useState } from "react";
import { clearLegacyAccessDirectory } from "./accessControlPolicy";
import { createAccessDirectoryController, type AccessDirectoryState } from "./accessDirectoryController";
import type { FleetAccessDirectory, VaultAccessDirectory, VaultSaveAccessDirectoryResponse } from "./accessControlTypes";

export function useFleetAccessDirectory(enabled: boolean, activeTab: string,
  read: () => Promise<VaultAccessDirectory>,
  write: (directory: VaultAccessDirectory) => Promise<VaultSaveAccessDirectoryResponse>) {
  const [state, setState] = useState<AccessDirectoryState>();
  const controller = useMemo(() => createAccessDirectoryController(read, write, setState), [read, write]);
  useEffect(() => {
    if (!enabled) { controller.invalidate(); return; }
    const refresh = () => { void controller.refresh(); };
    refresh();
    window.addEventListener("focus", refresh);
    return () => { controller.invalidate(); window.removeEventListener("focus", refresh); };
  }, [controller, enabled, activeTab]);
  const save = useCallback(async (candidate: FleetAccessDirectory) => {
    const saved = await controller.save(candidate);
    try { clearLegacyAccessDirectory(); } catch { /* The protected save is already verified; browser storage is not authoritative. */ }
    return saved;
  }, [controller]);
  return { ...(state ?? controller.getState()), update: controller.update, refresh: controller.refresh, save };
}
