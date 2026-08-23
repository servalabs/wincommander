import { invoke } from "@tauri-apps/api/core";
import type { StartupPhaseId } from "../events/startup";

const reportedPhases = new Set<StartupPhaseId>();

/** Reports each value-free launch phase once per WebView process. */
export function reportStartupPhase(phase: StartupPhaseId): void {
  if (reportedPhases.has(phase)) return;
  reportedPhases.add(phase);
  void invoke("report_startup_phase", { phase }).catch(() => {
    // Startup diagnostics must never delay or break the product surface.
  });
}
