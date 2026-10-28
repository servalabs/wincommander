import { expect, test } from "bun:test";
import { createCleanupScheduleReadGate } from "./cleanupScheduleReadGate";

test("a late older task read cannot replace or verify a newer snapshot", async () => {
  const gate = createCleanupScheduleReadGate();
  let firstResolve!: (value: string) => void;
  let secondResolve!: (value: string) => void;
  const first = new Promise<string>(resolve => { firstResolve = resolve; });
  const second = new Promise<string>(resolve => { secondResolve = resolve; });
  const accept = async (pending: Promise<string>) => {
    const ticket = gate.begin();
    const value = await pending;
    return gate.isCurrent(ticket) ? value : null;
  };
  const older = accept(first);
  const newer = accept(second);
  secondResolve("fresh");
  expect(await newer).toBe("fresh");
  firstResolve("stale");
  expect(await older).toBeNull();
});
