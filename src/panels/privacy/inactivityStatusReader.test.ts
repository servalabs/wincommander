import { expect, test } from "bun:test";
import { createInactivityStatusReader } from "./inactivityStatusReader";

test("a hung status read becomes unknown without accumulating reads or accepting the late result", async () => {
  let complete!: (value: string) => void;
  let calls = 0;
  const reader = createInactivityStatusReader(() => {
    calls++;
    return calls === 1 ? new Promise<string>((resolve) => { complete = resolve; }) : Promise.resolve("fresh");
  }, 10);
  const timedOut = await reader().catch((error: unknown) => error);
  expect(timedOut instanceof Error && timedOut.message.includes("timed out")).toBe(true);
  const stillPending = await reader().catch((error: unknown) => error);
  expect(stillPending instanceof Error && stillPending.message.includes("still pending")).toBe(true);
  expect(calls).toBe(1);
  complete("stale");
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(await reader()).toBe("fresh");
  expect(calls).toBe(2);
});
