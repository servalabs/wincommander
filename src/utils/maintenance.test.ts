import { describe, expect, test } from "bun:test";
import { getMaintenanceFailureMessage } from "./maintenance";

describe("maintenance failure reporting", () => {
  test("rejects a failed native action receipt", () => {
    expect(getMaintenanceFailureMessage("System repair", { success: true, data: { ok: false, message: "Repair failed" } })).toBe("Repair failed");
  });
  test("returns backend and PowerShell errors to the caller", () => {
    expect(getMaintenanceFailureMessage("Apply Service Profile", {
      success: false,
      error: "Administrator privileges required.",
    })).toBe("Administrator privileges required.");

    expect(getMaintenanceFailureMessage("Apply Service Profile", {
      success: true,
      data: { error: true, message: "Could not update service startup type." },
    })).toBe("Could not update service startup type.");
  });

  test("summarizes per-service failures from a successful backend envelope", () => {
    expect(getMaintenanceFailureMessage("Apply Service Profile", {
      success: true,
      data: {
        status: "done",
        manual: { failed: [{ name: "AppMgmt", error: "Access is denied." }] },
        disable: { failed: 0 },
      },
    })).toBe("Apply Service Profile completed with errors: 1 manual change failed: AppMgmt (Access is denied.)");
  });

  test("accepts clean profiles and empty maintenance responses", () => {
    expect(getMaintenanceFailureMessage("Apply Service Profile", {
      success: true,
      data: {
        status: "done",
        manual: { touched: 2, alreadyOK: 1, failed: [] },
        disable: { touched: 1, failed: [] },
      },
    })).toBe(null);
    expect(getMaintenanceFailureMessage("Apply Service Profile", null)).toBe(null);
  });
});
