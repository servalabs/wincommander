import { expect, test } from "bun:test";
import { syncCalculatorShortcuts } from "./calculatorShortcuts";

test("reports shortcut failures even when the backend resolves normally", async () => {
  expect(await syncCalculatorShortcuts(async () => ({ success: false, error: "Access denied" }), true)).toBe("Access denied");
  expect(await syncCalculatorShortcuts(async () => ({ success: true, data: { status: "hidden", itemsChanged: 1, warnings: ["Shortcut unchanged"] } }), true)).toBe("Shortcut unchanged");
});

test("reports success only when shortcut operation succeeded without warnings", async () => {
  expect(await syncCalculatorShortcuts(async () => ({ success: true, data: { status: "hidden", itemsChanged: 1 } }), true)).toBeNull();
  expect(await syncCalculatorShortcuts(async () => { throw new Error("failed"); }, true)).toContain("failed");
});
