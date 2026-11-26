import { useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useAuthMode } from "../context/AuthModeContext";
import { afterVaultMutation, FLEET_VAULTS_CHANGED_EVENT } from "../lib/vaultChangeEvents";
import { cleanVaultPath, readVaultDisplayNames, type VaultDisplayName, type VaultRenameResult } from "../lib/vaultRename";

const EMPTY_NAMES: VaultDisplayName[] = [];
const vaultNamesKey = (mode: string) => ["vault", "display-names", mode] as const;

/** Names are shown next to paths only; a failed read just shows none. */
export default function useVaultNames() {
  const { mode } = useAuthMode();
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: vaultNamesKey(mode),
    enabled: mode !== "decoy",
    queryFn: async () => readVaultDisplayNames(await invoke<unknown>("vault_display_names")),
  });
  useEffect(() => {
    const refresh = () => void queryClient.invalidateQueries({ queryKey: vaultNamesKey(mode) });
    window.addEventListener(FLEET_VAULTS_CHANGED_EVENT, refresh);
    return () => window.removeEventListener(FLEET_VAULTS_CHANGED_EVENT, refresh);
  }, [mode, queryClient]);
  const renameVault = useCallback(
    (containerPath: string, name: string) =>
      afterVaultMutation(() => invoke<VaultRenameResult>("vault_rename", { containerPath: cleanVaultPath(containerPath), name: name.trim() })),
    [],
  );
  return { names: mode === "decoy" ? EMPTY_NAMES : query.data ?? EMPTY_NAMES, renameVault };
}
