import { describe, expect, test } from "bun:test";
import { STARTUP_FIELD_OWNERSHIP, STARTUP_TASK_INVENTORY } from "./startupInventory";

describe("startup ownership manifest", () => {
  test("assigns every public coordinator task one owner and one single-flight key", () => {
    expect(new Set(STARTUP_TASK_INVENTORY.map((task) => task.id)).size).toBe(STARTUP_TASK_INVENTORY.length);
    expect(STARTUP_TASK_INVENTORY.every((task) => task.owner && task.timeoutMs > 0 && task.singleFlight)).toBe(true);
    expect(STARTUP_FIELD_OWNERSHIP["current.apps.inventory"]).toBe("Get-AppInventory");
  });
});
