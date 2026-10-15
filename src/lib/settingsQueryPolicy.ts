import { QueryObserver, type QueryClient } from "@tanstack/react-query";
import type { AppSettings } from "../types/settings";
import type { AuthMode } from "../context/AuthModeContext";
import { DECOY_APP_SETTINGS } from "./decoyFakeData";

export const settingsKeys = {
  all: ["settings"] as const,
  detail: () => ["settings", "detail"] as const,
  decoy: () => ["settings", "detail", "decoy"] as const,
};

export function settingsQueryOptions(mode: AuthMode, read: () => Promise<AppSettings>) {
  return {
    queryKey: mode === "decoy" ? settingsKeys.decoy() : settingsKeys.detail(),
    // Refetch bypasses Query's enabled flag; it must never read real settings in decoy mode.
    queryFn: mode === "decoy" ? async () => DECOY_APP_SETTINGS : read,
    enabled: mode !== "decoy",
    select: (settings: AppSettings): AppSettings | undefined => mode === "decoy" ? undefined : settings,
  };
}
// A passive observer retains the cache without installing a competing reader.
export function createSettingsCacheObserver(client: QueryClient, mode: AuthMode) {
  return new QueryObserver<AppSettings, Error, AppSettings | undefined>(client, {
    queryKey: mode === "decoy" ? settingsKeys.decoy() : settingsKeys.detail(),
    enabled: false,
    select: settings => mode === "decoy" ? undefined : settings,
  });
}
