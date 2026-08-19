import { describe, expect, test } from "bun:test";
import type { Rule } from "../types/generated/fleet";
import {
  localClipboardGuardPolicy,
  saveLocalClipboardRules,
} from "./useClipboardGuardRules";

function localRule(): Rule {
  return {
    id: "0e8f1a2b-3c4d-5e6f-7a8b-9c0d1e2f3a4b",
    revision: 1,
    name: "Local invoice marker",
    enabled: true,
    priority: 100,
    matcher: { kind: "phrase", params: { value: "INTERNAL", case_sensitive: false } },
    severity: "warn",
    actions: ["notify_user"],
    cooldownSeconds: 30,
    snoozable: true,
    locked: false,
  };
}

describe("local clipboard rule persistence", () => {
  test("uses the Tauri camel-case policy argument", () => {
    expect(localClipboardGuardPolicy([localRule()])).toEqual({
      policyVersion: 0,
      rules: [localRule()],
    });
  });

  test("sends local rules only to the dedicated per-user save command", async () => {
    const calls: string[] = [];
    await saveLocalClipboardRules(
      [localRule()],
      async (policy) => {
        calls.push(`save:${policy.policyVersion}:${policy.rules.length}`);
      },
    );
    expect(calls).toEqual(["save:0:1"]);
  });

  test("propagates a typed backend validation failure", async () => {
    try {
      await saveLocalClipboardRules(
        [localRule()],
        async () => { throw new Error("invalid regular expression"); },
      );
      throw new Error("expected the backend validation failure");
    } catch (error) {
      expect(error).toBeInstanceOf(Error);
      expect((error as Error).message).toBe("invalid regular expression");
    }
  });
});
