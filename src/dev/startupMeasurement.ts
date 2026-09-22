import { STARTUP_PHASE_IDS, type StartupPhaseId } from "../events/startup";

export const STARTUP_SCENARIOS = ["warm", "cold", "first-install", "offline", "downloads-50k"] as const;
export type StartupScenario = (typeof STARTUP_SCENARIOS)[number];

export const REQUIRED_STARTUP_PHASES = [
  "process_start", "native_setup_entered", "main_window_show_requested",
  "webview_dom_ready", "settings_cache_hydrated", "dashboard_first_visible",
  "dashboard_interactive", "fresh_system_probe_complete", "background_idle",
] as const satisfies readonly StartupPhaseId[];
export type StartupPhase = StartupPhaseId;

export const PROTECTION_STARTUP_PHASES = [
  "protection_required_ready", "protection_not_required", "protection_failed",
] as const satisfies readonly StartupPhaseId[];

export interface StartupSample {
  scenario: StartupScenario;
  elapsedMs: Partial<Record<StartupPhase, number>>;
}

export interface StartupSummary {
  scenario: StartupScenario;
  samples: number;
  phases: Partial<Record<StartupPhase, { samples: number; p50: number; p95: number; max: number }>>;
}

function percentile(values: number[], percentileValue: number): number {
  const index = Math.ceil(percentileValue * values.length) - 1;
  return values[Math.max(0, Math.min(index, values.length - 1))];
}

export function validateStartupSample(sample: unknown): string[] {
  if (typeof sample !== "object" || sample === null || Array.isArray(sample)) return ["sample: expected an object"];
  const candidate = sample as Record<string, unknown>;
  const failures: string[] = [];
  if (!STARTUP_SCENARIOS.some((scenario) => scenario === candidate.scenario)) failures.push("scenario: unknown value");
  if (typeof candidate.elapsedMs !== "object" || candidate.elapsedMs === null || Array.isArray(candidate.elapsedMs)) {
    return [...failures, "elapsedMs: expected a timing object"];
  }
  const timings = candidate.elapsedMs as Record<string, unknown>;
  if (Object.keys(timings).length === 0) failures.push("elapsedMs: no timings");
  for (const [phase, elapsed] of Object.entries(timings)) {
    if (!STARTUP_PHASE_IDS.some((known) => known === phase)) {
      failures.push("elapsedMs: unknown phase");
    } else if (typeof elapsed !== "number" || !Number.isFinite(elapsed) || elapsed < 0) {
      failures.push(`${phase}: invalid elapsed time`);
    }
  }
  if (PROTECTION_STARTUP_PHASES.filter((phase) => Object.hasOwn(timings, phase)).length > 1) {
    failures.push("protection: conflicting outcomes");
  }
  let previous = -1;
  for (const phase of REQUIRED_STARTUP_PHASES) {
    const elapsed = timings[phase];
    if (typeof elapsed !== "number" || !Number.isFinite(elapsed) || elapsed < 0) continue;
    if (elapsed < previous) failures.push(`${phase}: out of order`);
    previous = elapsed;
  }
  return failures;
}

export function summarizeStartupSamples(samples: StartupSample[]): StartupSummary[] {
  for (const [index, sample] of samples.entries()) {
    const failures = validateStartupSample(sample);
    if (failures.length) throw new Error(`Invalid startup sample ${index}: ${failures.join("; ")}`);
  }
  return STARTUP_SCENARIOS.map((scenario) => {
    const scenarioSamples = samples.filter((sample) => sample.scenario === scenario);
    const phases: StartupSummary["phases"] = {};
    for (const phase of STARTUP_PHASE_IDS) {
      const values = scenarioSamples.map((sample) => sample.elapsedMs[phase])
        .filter((value): value is number => typeof value === "number")
        .sort((left, right) => left - right);
      if (values.length) phases[phase] = { samples: values.length, p50: percentile(values, 0.5), p95: percentile(values, 0.95), max: values.at(-1)! };
    }
    return { scenario, samples: scenarioSamples.length, phases };
  }).filter((summary) => summary.samples > 0);
}
