import { isPackageBackedDependency } from "../../lib/packageOperationLock";

export interface DashboardFixStep {
  id: string;
  label: string;
  fn: () => Promise<unknown>;
}

export function isDashboardPackageFinding(id: string): boolean {
  return id.startsWith("app-update:") || (
    id.startsWith("dependency:") && isPackageBackedDependency(id.slice("dependency:".length))
  );
}

function isInstaller(id: string): boolean {
  return id.startsWith("dependency:") || id.startsWith("app-update:");
}

function isExclusive(id: string): boolean {
  return id === "disk-cleanup" || id === "services-profile";
}

function isUpdateDialog(id: string): boolean {
  return id === "wincommander-update" || id === "pro-sidecar";
}

export function dashboardFixDependencies(steps: readonly DashboardFixStep[]): number[][] {
  return steps.map((step, index) => steps.flatMap((other, otherIndex) => {
    if (index === otherIndex) return [];
    if (other.id === "dependency:winget" && step.id !== "dependency:winget" && isDashboardPackageFinding(step.id)) return [otherIndex];
    if (isUpdateDialog(step.id)) {
      return !isUpdateDialog(other.id) || otherIndex < index ? [otherIndex] : [];
    }
    if (isExclusive(step.id)) {
      return !isUpdateDialog(other.id) && (!isExclusive(other.id) || otherIndex < index) ? [otherIndex] : [];
    }
    // Installers can replace browser preferences; apply hardening afterwards.
    if (step.id.startsWith("browser-hardening:") && isInstaller(other.id)) return [otherIndex];
    return [];
  }));
}

/** Dependencies wait for attempts; each command must recheck its own capabilities. */
export async function runDashboardFixes(
  steps: readonly DashboardFixStep[],
  execute: (step: DashboardFixStep, index: number) => Promise<unknown>,
): Promise<void> {
  const dependencies = dashboardFixDependencies(steps);
  const pending = new Set(steps.map((_, index) => index));
  const settled = new Set<number>();
  const running = new Map<number, Promise<void>>();
  let installerRunning = false;
  while (pending.size || running.size) {
    for (const index of pending) {
      if (running.size >= 3) break;
      const step = steps[index];
      const installer = isInstaller(step.id);
      if (installer && installerRunning) continue;
      if (!dependencies[index].every(dependency => settled.has(dependency))) continue;
      pending.delete(index);
      if (installer) installerRunning = true;
      const execution = Promise.resolve().then(async () => { await execute(step, index); }).catch(() => undefined).finally(() => {
        if (installer) installerRunning = false;
        settled.add(index);
        running.delete(index);
      });
      running.set(index, execution);
    }
    if (running.size) await Promise.race(running.values());
  }
}
