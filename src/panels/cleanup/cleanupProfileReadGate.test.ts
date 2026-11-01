import { expect, test } from "bun:test";
import { createCleanupProfileReadGate } from "./cleanupProfileReadGate";

test("switching accounts or starting a newer scan invalidates a late account result", () => {
  const gate = createCleanupProfileReadGate();
  const aliceScan = gate.begin();
  gate.begin(); // account switch
  expect(gate.isCurrent(aliceScan)).toBe(false);
  const bobScan = gate.begin();
  expect(gate.isCurrent(bobScan)).toBe(true);
  const newerBobScan = gate.begin();
  expect(gate.isCurrent(bobScan)).toBe(false);
  expect(gate.isCurrent(newerBobScan)).toBe(true);
});
