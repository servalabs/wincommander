export type StartupStaggerStage = "dependencies" | "mesh" | "inventory";

export interface StartupStaggerStep {
  stage: StartupStaggerStage;
  delayMs: number;
  runWhenIdle?: boolean;
}

export const STARTUP_STAGGER_PLAN: readonly StartupStaggerStep[] = [
  { stage: "dependencies", delayMs: 1_500 },
  { stage: "mesh", delayMs: 4_000 },
  { stage: "inventory", delayMs: 8_000, runWhenIdle: true },
] as const;

export function getStartupStaggerStep(stage: StartupStaggerStage): StartupStaggerStep {
  const step = STARTUP_STAGGER_PLAN.find((entry) => entry.stage === stage);
  if (!step) {
    throw new Error(`Unknown startup stagger stage: ${stage}`);
  }
  return step;
}
