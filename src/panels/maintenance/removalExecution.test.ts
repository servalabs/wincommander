import { describe, expect, test } from "bun:test";
import { executeRemoval } from "./removalExecution";

describe("removal report lifecycle", () => {
  test("returns a partial result, invalidates once and does not rescan", async () => {
    const calls: string[] = [];
    const result = await executeRemoval(async () => {
      calls.push("remove");
      return { filesRemoved: 1, errors: ["unavailable"] };
    }, () => calls.push("invalidate"));
    expect(result.filesRemoved).toBe(1);
    expect(result.errors).toEqual(["unavailable"]);
    expect(calls).toEqual(["remove", "invalidate"]);
  });

  test("an unknown mutation outcome still invalidates the old report", async () => {
    const calls: string[] = [];
    let message = "";
    try {
      await executeRemoval(async () => {
        calls.push("remove");
        throw new Error("connection lost");
      }, () => calls.push("invalidate"));
    } catch (error) { message = String(error); }
    expect(message).toContain("connection lost");
    expect(calls).toEqual(["remove", "invalidate"]);
  });
});
