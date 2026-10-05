import { useCallback, useEffect, useState } from "react";
import type { ScanFinding } from "@/components/startup/WizardAnimations";
import { dashboardFixFailure, type FindingFixAttempt } from "./fixVerification";
import { showError } from "@/utils/toast";
import { TOGGLE_REPAIR_VERIFIED_EVENT, type VerifiedToggleRepair } from "@/lib/autoHeal";

export function useFindingFixAttempts() {
  const [fixAttempts, setFixAttempts] = useState<Record<string, FindingFixAttempt>>({});
  useEffect(() => {
    const onVerified = (event: Event) => {
      const { toggleId, targetChecked } = (event as CustomEvent<VerifiedToggleRepair>).detail;
      setFixAttempts(current => {
        const next = { ...current };
        for (const id of [toggleId, `drift:${toggleId}`]) {
          const attempt = next[id];
          if (attempt?.error && (attempt.finding.targetChecked ?? !id.startsWith("drift:")) === targetChecked) delete next[id];
        }
        return next;
      });
    };
    window.addEventListener(TOGGLE_REPAIR_VERIFIED_EVENT, onVerified);
    return () => window.removeEventListener(TOGGLE_REPAIR_VERIFIED_EVENT, onVerified);
  }, []);
  const trackFindingFix = useCallback(async <T,>(finding: ScanFinding, operation: () => Promise<T>): Promise<T> => {
    setFixAttempts(current => ({ ...current, [finding.id]: { finding, error: null } }));
    try {
      const result = await operation();
      setFixAttempts(current => { const next = { ...current }; delete next[finding.id]; return next; });
      return result;
    } catch (error) {
      const message = dashboardFixFailure(error);
      setFixAttempts(current => ({ ...current, [finding.id]: { finding, error: message } }));
      void showError(`${finding.label}: ${message}`, undefined, { kind: "notification" });
      throw new Error(message);
    }
  }, []);
  return { fixAttempts, trackFindingFix };
}
