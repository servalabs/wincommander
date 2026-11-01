import { createContext, useContext } from "react";
import type { AppSettings, SettingsPatch } from "../types/settings";

export type SettingsUpdater = SettingsPatch | ((latest: AppSettings | null) => SettingsPatch);

export interface SettingsWriter {
  read: () => Promise<AppSettings>;
  patch: (patch: SettingsUpdater) => Promise<AppSettings>;
  adopt: (patch: SettingsPatch) => Promise<AppSettings>;
  replace: (settings: AppSettings) => Promise<AppSettings>;
  updateCurrent: (probe: Record<string, unknown>) => Promise<AppSettings>;
  import: (json: string) => Promise<AppSettings>;
}

export const SettingsWriteContext = createContext<SettingsWriter | null>(null);

export function useSettingsWriter(): SettingsWriter {
  const writer = useContext(SettingsWriteContext);
  if (!writer) throw new Error("Settings writes require an AppProvider");
  return writer;
}
