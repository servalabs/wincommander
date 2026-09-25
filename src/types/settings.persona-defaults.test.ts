import { describe, expect, test } from "bun:test";
import { getDensityForSettings } from "../lib/personaMigration";
import { getPersona } from "./settings";

describe("default interface and threat persona", () => {
  test("unset settings resolve to Guided and Casual", () => {
    const settings = { app: { experienceLevel: "standard" } } as any;

    expect(getDensityForSettings(settings)).toBe("guided");
    expect(getPersona(settings)).toBe("casual");
  });

  test("keeps an explicitly saved Secure choice", () => {
    expect(getPersona({ app: { persona: "secure" } })).toBe("secure");
  });
});
