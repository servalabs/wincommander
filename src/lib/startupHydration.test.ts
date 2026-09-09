import { expect, test } from "bun:test";
import { hydrateWithinBudget } from "./startupHydration";

test("failed settings read settles without making startup ready", async () => {
  expect(await hydrateWithinBudget(async () => { throw new Error("unavailable"); })).toBeNull();
});

test("hung settings read expires and late consumer is aborted", async () => {
  let signal!: AbortSignal;
  let resolve!: (value: string) => void;
  const pending = new Promise<string>(done => { resolve = done; });
  const result = await hydrateWithinBudget(active => { signal = active; return pending; }, 5);
  expect(result).toBeNull();
  expect(signal.aborted).toBe(true);
  resolve("late settings");
});

test("successful retry returns settings within its budget", async () => {
  expect(await hydrateWithinBudget(async () => ({ loaded: true }))).toEqual({ loaded: true });
});
