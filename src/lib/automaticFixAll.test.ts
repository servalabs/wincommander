import { describe, expect, test } from "bun:test";
import {
  automaticFixAllCandidates,
  automaticFixAllFingerprint,
} from "./automaticFixAll";
import type { ScanFinding } from "../components/startup/WizardAnimations";

const finding = (id: string, partial: Partial<ScanFinding> = {}): ScanFinding => ({
  id,
  category: "privacy",
  label: id,
  impact: "",
  severity: "info",
  ...partial,
});

describe("automatic Fix All", () => {
  test("applies only safe, non-ignored recommendations", () => {
    const candidates = automaticFixAllCandidates([
      finding("safe", { safeDefault: true }),
      finding("ignored", { safeDefault: true }),
      finding("manual", { safeDefault: false }),
    ], ["ignored"]);

    expect(candidates.map((item) => item.id)).toEqual(["safe"]);
  });

  test("keeps Fix All manual when a person has a drift setting", () => {
    const candidates = automaticFixAllCandidates([
      finding("safe", { safeDefault: true }),
      finding("user-choice", { safeDefault: true, drift: true, targetChecked: true }),
    ], []);

    expect(candidates).toEqual([]);
  });

  test("uses a stable fingerprint regardless of finding order", () => {
    const first = [finding("b"), finding("a", { targetChecked: true })];
    expect(automaticFixAllFingerprint(first)).toBe(
      automaticFixAllFingerprint([...first].reverse()),
    );
  });
});
