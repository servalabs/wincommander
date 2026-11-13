import { describe, expect, it } from "bun:test";
import { isConfirmedAmcacheDeferredReceipt } from "./cleanupAmcacheReceipt";
import { getCleanupManualConfirmation } from "./cleanupManualPolicy";

describe("App Launch Cache pending deletion receipt", () => {
  it("accepts only a confirmed positive number of scheduled files", () => {
    expect(isConfirmedAmcacheDeferredReceipt({ ok: true, stdout: "ok: bootScheduled=3" })).toBe(true);
    for (const receipt of [
      { ok: true, stdout: "ok: bootScheduled=0" },
      { ok: false, stdout: "ok: bootScheduled=3" },
      { ok: true, stdout: "fail: bootScheduled=3; some deletions may already be scheduled" },
      { ok: true, stdout: "ok: bootScheduled=3\nother output" },
      { ok: true, stdout: "ok: bootScheduled=999999999999999999999" },
      null,
    ]) expect(isConfirmedAmcacheDeferredReceipt(receipt)).toBe(false);
  });
  it("explains the restart requirement before the destructive action", () => {
    const confirmation = getCleanupManualConfirmation({ id: "amcache", label: "App Launch Cache" }, "clear");
    expect(confirmation.description).toContain("next restart");
    expect(confirmation.description).toContain("not removed now");
    expect(confirmation.description).toContain("scan again after restarting");
  });
});
