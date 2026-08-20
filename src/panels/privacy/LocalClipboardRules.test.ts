import { describe, expect, test } from "bun:test";
import type { Rule } from "../../types/generated/fleet";
import {
  createLocalClipboardRule,
  editableMatcherValue,
  ensureLocalActions,
  setLocalAction,
  updateEditableMatcher,
  validateLocalClipboardRule,
} from "./LocalClipboardRules";

function validRule(): Rule {
  return {
    id: "0e8f1a2b-3c4d-5e6f-7a8b-9c0d1e2f3a4b",
    revision: 1,
    name: "Local invoice marker",
    enabled: true,
    priority: 100,
    matcher: { kind: "phrase", params: { value: "INTERNAL-INVOICE", case_sensitive: false } },
    severity: "warn",
    actions: ["notify_user"],
    cooldownSeconds: 30,
    snoozable: true,
    locked: false,
  };
}

describe("local clipboard rule contract", () => {
  test("repairs an actionless legacy rule to a visible local notification", () => {
    expect(ensureLocalActions([])).toEqual(["notify_user"]);
  });

  test("does not allow the last selected local action to be removed", () => {
    const rule = createLocalClipboardRule(new Set());
    expect(setLocalAction(rule, "notify_user", false).actions).toEqual(["notify_user"]);
  });

  test("rejects an ID collision with a Fleet rule", () => {
    const rule = validRule();
    const result = validateLocalClipboardRule(rule, new Set([rule.id]));
    expect(result).toEqual({ isValid: false, message: "That rule ID is already in use." });
  });

  test("rejects Fleet reporting actions from a local rule", () => {
    const rule = { ...validRule(), actions: ["notify_user", "report_fleet"] as Rule["actions"] };
    expect(validateLocalClipboardRule(rule, new Set()).message).toContain("cannot report to Fleet");
  });

  test("keeps local actions within device-only actions", () => {
    const withClear = setLocalAction(validRule(), "clear_clipboard", true);
    expect(withClear.actions).toEqual(["notify_user", "clear_clipboard"]);
    const withQuarantine = setLocalAction(withClear, "quarantine_clipboard", true);
    expect(withQuarantine.actions).toEqual(["notify_user", "clear_clipboard", "quarantine_clipboard"]);
    expect(setLocalAction(withQuarantine, "notify_user", false).actions).toEqual(["clear_clipboard", "quarantine_clipboard"]);
  });

  test("preserves case sensitivity while changing matcher type", () => {
    const matcher = { kind: "phrase", params: { value: "SECRET", case_sensitive: true } } as const;
    const regex = updateEditableMatcher(matcher, "regex", "SEC.*");
    expect(regex).toEqual({ kind: "regex", params: { pattern: "SEC.*", case_sensitive: true } });
    expect(editableMatcherValue(regex)).toBe("SEC.*");
  });

  test("new rule does not reuse an existing identifier", () => {
    const first = createLocalClipboardRule(new Set());
    const second = createLocalClipboardRule(new Set([first.id]));
    expect(second.id).not.toBe(first.id);
  });
});
