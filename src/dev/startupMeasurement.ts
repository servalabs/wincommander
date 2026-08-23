export const STARTUP_SCENARIOS = ["warm", "cold", "first-install", "offline", "downloads-50k"] as const;
export type StartupScenario = (typeof STARTUP_SCENARIOS)[number];

export const REQUIRED_STARTUP_PHASES = [
  "process_start", "native_setup_entered", "main_window_show_requested",
  "webview_dom_ready", "settings_cache_hydrated", "dashboard_first_visible",
  "dashboard_interactive", "fresh_system_probe_complete", "background_idle",
] as const;
export type StartupPhase = (typeof REQUIRED_STARTUP_PHASES)[number];

export interface StartupSample {
  scenario: StartupScenario;
  elapsedMs: Partial<Record<StartupPhase, number>>;
}

export interface StartupSummary {
  scenario: StartupScenario;
  samples: number;
  phases: Partial<Record<StartupPhase, { p50: number; p95: number; max: number }>>;
}

function percentile(values: number[], percentileValue: number): number {
  const index = Math.ceil(percentileValue * values.length) - 1;
  return values[Math.max(0, Math.min(index, values.length - 1))];
}

export function validateStartupSample(sample: StartupSample): string[] {
  const failures: string[] = [];
  let previous = -1;
  for (const phase of REQUIRED_STARTUP_PHASES) {
    const elapsed = sample.elapsedMs[phase];
    if (elapsed === undefined) continue;
    if (!Number.isFinite(elapsed) || elapsed < 0) failures.push(`${phase}: invalid elapsed time`);
    if (elapsed < previous) failures.push(`${phase}: out of order`);
    previous = elapsed;
  }
  return failures;
}

export function summarizeStartupSamples(samples: StartupSample[]): StartupSummary[] {
  return STARTUP_SCENARIOS.map((scenario) => {
    const scenarioSamples = samples.filter((sample) => sample.scenario === scenario);
    const phases: StartupSummary["phases"] = {};
    for (const phase of REQUIRED_STARTUP_PHASES) {
      const values = scenarioSamples.map((sample) => sample.elapsedMs[phase])
        .filter((value): value is number => typeof value === "number")
        .sort((left, right) => left - right);
      if (values.length) phases[phase] = { p50: percentile(values, 0.5), p95: percentile(values, 0.95), max: values.at(-1)! };
    }
    return { scenario, samples: scenarioSamples.length, phases };
  }).filter((summary) => summary.samples > 0);
}
