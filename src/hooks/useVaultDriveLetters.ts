import { useCallback, useMemo, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppSettings } from "@/types/settings";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useAuthMode } from "../context/AuthModeContext";
import { createSettingsCacheObserver, settingsKeys } from "../lib/settingsQueryPolicy";
import useBackend from "./useBackend";
import { selectableDriveLetters } from "@/lib/vaultOperationFeedback";
import { waitForMountOptions } from "../panels/vault/mountOperationProgress";

const EMPTY_LETTERS: string[] = [];

// AppContext owns background reads; opening a picker only subscribes to this cache.
export default function useVaultDriveLetters(preload = false, includeSavedReservations = true) {
  const { mode } = useAuthMode();
  const queryClient = useQueryClient();
  const settingsObserver = useMemo(() => createSettingsCacheObserver(queryClient, mode), [queryClient, mode]);
  const subscribeSettings = useCallback((notify: () => void) => settingsObserver.subscribe(notify), [settingsObserver]);
  const readSettings = useCallback(() => settingsObserver.getCurrentResult().data, [settingsObserver]);
  const settings = useSyncExternalStore(subscribeSettings, readSettings, readSettings);
  const settingsUnavailable = includeSavedReservations && !settings;
  const savedLetters = settings?.app.vault.quickMountSlots?.map(slot => slot.driveLetter) ?? EMPTY_LETTERS;
  const { getAvailableDriveLetters } = useBackend();
  const query = useQuery({
    queryKey: ["vault", "available-drive-letters", mode],
    queryFn: async () => {
      const result = await waitForMountOptions(getAvailableDriveLetters());
      if (!result.success || !result.data) throw new Error("vault_drive_letters_unavailable");
      return selectableDriveLetters(result.data.letters);
    },
    enabled: preload && mode !== "decoy",
    staleTime: 30_000,
    refetchInterval: preload && mode !== "decoy" ? 30_000 : false,
    refetchIntervalInBackground: false,
    refetchOnMount: preload ? "always" : false,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const { refetch } = query;
  const refresh = useCallback(async () => {
    if (mode === "decoy") return null;
    try {
      const current = await invoke<AppSettings>("get_settings", { refreshPersonal: true });
      queryClient.setQueryData(settingsKeys.detail(), current);
      const result = await refetch({ cancelRefetch: false });
      if (result.isError || !result.data) return null;
      return selectableDriveLetters(result.data, includeSavedReservations ? current.app.vault.quickMountSlots?.map(slot => slot.driveLetter) : []);
    } catch { return null; }
  }, [mode, refetch, queryClient, includeSavedReservations]);
  return {
    letters: mode === "decoy" || query.isError || settingsUnavailable ? EMPTY_LETTERS : selectableDriveLetters(query.data ?? EMPTY_LETTERS, includeSavedReservations ? savedLetters : []),
    loading: mode !== "decoy" && (query.isPending || query.isFetching),
    unavailable: mode === "decoy" || query.isError || settingsUnavailable,
    refresh,
  };
}
