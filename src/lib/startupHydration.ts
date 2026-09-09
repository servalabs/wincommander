import { waitForSoftTimeout } from "./softTimeout";

/** Bound the UI wait and reject late hydration without claiming native cancellation. */
export async function hydrateWithinBudget<T>(
  load: (signal: AbortSignal) => Promise<T | null>,
  timeoutMs = 8_000,
): Promise<T | null> {
  const controller = new AbortController();
  try {
    const result = await waitForSoftTimeout(load(controller.signal), timeoutMs);
    return result.status === "completed" ? result.value : null;
  } catch {
    return null;
  } finally {
    controller.abort();
  }
}
