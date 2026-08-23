export const STARTUP_MILESTONE_EVENT = "wincommander:startup-milestone";

export const STARTUP_JOB_IDS = [
  "settings-cache",
  "system-probe",
  "startup-status",
  "pro-install-status",
  "pro-manifest",
  "pro-hash",
  "defender-status",
  "dependencies",
  "mesh-status",
  "app-inventory",
  "panel-preload",
  "disk-cleanup-preload",
  "search-preload",
] as const;

export type StartupJobId = (typeof STARTUP_JOB_IDS)[number];
export type StartupMilestone =
  "queued" | "started" | "completed" | "timed-out" | "cancelled" | "failed";

export const STARTUP_PHASE_IDS = [
  "process_start",
  "native_setup_entered",
  "main_window_show_requested",
  "webview_dom_ready",
  "settings_cache_hydrated",
  "dashboard_first_visible",
  "dashboard_interactive",
  "protection_required_ready",
  "protection_not_required",
  "protection_failed",
  "fresh_system_probe_complete",
  "background_idle",
] as const;

export type StartupPhaseId = (typeof STARTUP_PHASE_IDS)[number];

export interface StartupMilestoneEvent {
  job: StartupJobId;
  milestone: StartupMilestone;
  durationMs: number;
}

export interface StartupNativeReporter {
  report(event: StartupMilestoneEvent): Promise<void>;
}

type NativeInvoke = <T>(
  command: "report_startup_milestone",
  args: {
    job: StartupJobId;
    milestone: StartupMilestone;
    durationMs: number;
  },
) => Promise<T>;

const MAX_REPORTED_DURATION_MS = 60 * 60 * 1_000;

function clampDuration(durationMs: number): number {
  if (!Number.isFinite(durationMs)) return 0;
  return Math.max(
    0,
    Math.min(Math.round(durationMs), MAX_REPORTED_DURATION_MS),
  );
}

/**
 * The payload is deliberately enum-only. Startup reporting must never turn a
 * frontend error string or a caller-provided command into native input.
 */
export function createTauriStartupReporter(
  invoke: NativeInvoke,
): StartupNativeReporter {
  return {
    async report(event) {
      await invoke("report_startup_milestone", {
        job: event.job,
        milestone: event.milestone,
        durationMs: clampDuration(event.durationMs),
      });
    },
  };
}

export function dispatchStartupMilestone(event: StartupMilestoneEvent): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<StartupMilestoneEvent>(STARTUP_MILESTONE_EVENT, {
      detail: { ...event, durationMs: clampDuration(event.durationMs) },
    }),
  );
}
