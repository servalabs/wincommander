import { describe, expect, test } from "bun:test";
import { resolveEffectiveMountScope } from "./mountScope";

describe("mount scope resolution", () => {
  test("requires an explicit choice before exposing a vault machine-wide", () => {
    expect(resolveEffectiveMountScope("auto", "Windows 11 Pro")).toBe("per-user");
    expect(resolveEffectiveMountScope("auto", null)).toBe("per-user");
    expect(resolveEffectiveMountScope("machine", "Windows Server 2025")).toBe("machine");
  });
});
