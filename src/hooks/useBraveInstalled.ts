// src/hooks/useBraveInstalled.ts
//
// Whether Brave is present in the current app inventory's manifest apps.
// Extracted from the old setup-wizard scan (useSetupGuide.ts, removed) so a
// tour/help step can gate on it without reviving the wizard.

import { useAppState } from "../context/AppContext";

export default function useBraveInstalled(): boolean {
  const { appInventory } = useAppState();
  return appInventory?.manifestApps?.some((app) => app.id === "Brave.Brave" && app.installed) === true;
}
