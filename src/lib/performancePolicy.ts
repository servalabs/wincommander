export type StartupStaggerStage = "dependencies" | "mesh" | "inventory";

export interface StartupStaggerStep {
  stage: StartupStaggerStage;
  delayMs: number;
  runWhenIdle?: boolean;
}

export const STARTUP_STAGGER_PLAN: readonly StartupStaggerStep[] = [
  { stage: "dependencies", delayMs: 1_500 },
  { stage: "mesh", delayMs: 4_000 },
  // The installed-app inventory carries hundreds of records and icon payloads.
  // It must never compete with the first-use experience of the desktop app.
  // Start it only after two minutes, then only when Chromium reports an idle
  // window. Opening the Apps panel or using its Refresh action still requests
  // the data immediately.
  { stage: "inventory", delayMs: 120_000, runWhenIdle: true },
] as const;

export function getStartupStaggerStep(stage: StartupStaggerStage): StartupStaggerStep {
  const step = STARTUP_STAGGER_PLAN.find((entry) => entry.stage === stage);
  if (!step) {
    throw new Error(`Unknown startup stagger stage: ${stage}`);
  }
  return step;
}
