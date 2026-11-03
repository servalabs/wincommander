import { expect, test } from "bun:test";
import { createLocalClipboardRule } from "./LocalClipboardRules";
import { describeFinding, localDetectorOverride, resolveDetectorRule } from "./contentDetectorModel";

test("Fleet catalog overrides remain authoritative over a local disabled detector", () => {
  const preset = createLocalClipboardRule(new Set());
  const local = { ...preset, enabled: false };
  const managed = { ...preset, enabled: true, locked: true, revision: 4 };
  expect(resolveDetectorRule(preset, [local], [managed])).toEqual(managed);
  expect(resolveDetectorRule(preset, [local], [])).toEqual(local);
});

test("local catalog changes cannot inherit Fleet reporting actions", () => {
  const rule = createLocalClipboardRule(new Set());
  const next = localDetectorOverride({ ...rule, locked: true, actions: ["notify_user", "report_fleet", "alert_admin"] }, { threshold: 80 });
  expect(next.actions).toEqual(["notify_user"]);
  expect(next.threshold).toBe(80);
  expect(next.revision).toBe(rule.revision + 1);
  expect(next.locked).toBe(false);
});

test("why-it-matched uses fixed categories and reasons without detector or policy text", () => {
  const rule = createLocalClipboardRule(new Set());
  const text = describeFinding({ ruleId: rule.id, ruleRevision: 1, detectorId: "do-not-render-policy-value",
    category: "payment_card", score: 90, count: 2, reasons: [{ kind: "checksum_valid" }, { kind: "context_word" }], severity: "warn", actions: [] });
  expect(text).toBe("payment card (2): checksum valid, context word");
  expect(text).not.toContain("do-not-render-policy-value");
});
