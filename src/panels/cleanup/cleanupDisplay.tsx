import { createContext, useContext, type ReactNode } from "react";
import type { ThreatPersona } from "../../types/settings";

export type CleanupDisplayMode = "simple" | "advanced";

/** The existing persona determines presentation only. It grants no capability. */
export function cleanupModeForPersona(persona: ThreatPersona): CleanupDisplayMode {
  return persona === "secure" ? "advanced" : "simple";
}

const CleanupDisplayContext = createContext<CleanupDisplayMode | undefined>(undefined);

export function CleanupDisplayProvider({ persona, children }: { persona: ThreatPersona; children: ReactNode }) {
  return <CleanupDisplayContext.Provider value={cleanupModeForPersona(persona)}>{children}</CleanupDisplayContext.Provider>;
}

export function useCleanupDisplay(): CleanupDisplayMode | undefined {
  return useContext(CleanupDisplayContext);
}
