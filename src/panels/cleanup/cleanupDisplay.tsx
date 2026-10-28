import { createContext, useContext, useState, type ReactNode } from "react";

export type CleanupDisplayMode = "simple" | "advanced";
const DISPLAY_KEY = "wincommander.cleanup.display.v1";
const CleanupDisplayContext = createContext<{ mode: CleanupDisplayMode; select: (mode: CleanupDisplayMode) => void } | undefined>(undefined);

export function parseCleanupDisplayPreference(value: string | null): CleanupDisplayMode | null {
  return value === "simple" || value === "advanced" ? value : null;
}

function readPreference(): CleanupDisplayMode | null {
  try { return parseCleanupDisplayPreference(window.localStorage.getItem(DISPLAY_KEY)); }
  catch { return null; }
}

/** This preference changes presentation only; it never writes app capabilities or cleanup settings. */
export function CleanupDisplayProvider({ defaultMode, children }: { defaultMode: CleanupDisplayMode; children: ReactNode }) {
  const [preference, setPreference] = useState<CleanupDisplayMode | null>(readPreference);
  const mode = preference ?? defaultMode;
  const select = (next: CleanupDisplayMode) => {
    setPreference(next);
    try { window.localStorage.setItem(DISPLAY_KEY, next); } catch { /* Keep the choice for this session. */ }
  };
  return (
    <CleanupDisplayContext.Provider value={{ mode, select }}>{children}</CleanupDisplayContext.Provider>
  );
}

export function useCleanupDisplay(): CleanupDisplayMode | undefined {
  return useContext(CleanupDisplayContext)?.mode;
}

export function CleanupDisplayPicker() {
  const value = useContext(CleanupDisplayContext);
  if (!value) return null;
  return <div className="cleanup-display-choice" role="group" aria-label="System Cleanup display">
    <div><strong>Choose your view</strong><span>Both views use the same scan and cleanup settings. A schedule repeats a clear only after you choose an interval.</span></div>
    <div className="cleanup-display-choice__buttons">
      <button type="button" aria-pressed={value.mode === "simple"} onClick={() => value.select("simple")}>Simple</button>
      <button type="button" aria-pressed={value.mode === "advanced"} onClick={() => value.select("advanced")}>Advanced</button>
    </div>
  </div>;
}
