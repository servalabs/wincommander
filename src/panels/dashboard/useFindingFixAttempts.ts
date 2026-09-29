import { useCallback, useState } from "react";
import type { ScanFinding } from "@/components/startup/WizardAnimations";
import { dashboardFixFailure, type FindingFixAttempt } from "./fixVerification";

export function useFindingFixAttempts() {
  const [fixAttempts, setFixAttempts] = useState<Record<string, FindingFixAttempt>>({});
  const trackFindingFix = useCallback(async <T,>(finding: ScanFinding, operation: () => Promise<T>): Promise<T> => {
    setFixAttempts(current => ({ ...current, [finding.id]: { finding, error: null } }));
    try {
      const result = await operation();
      setFixAttempts(current => { const next = { ...current }; delete next[finding.id]; return next; });
      return result;
    } catch (error) {
      const message = dashboardFixFailure(error);
      setFixAttempts(current => ({ ...current, [finding.id]: { finding, error: message } }));
      throw new Error(message);
    }
  }, []);
  return { fixAttempts, trackFindingFix };
}
