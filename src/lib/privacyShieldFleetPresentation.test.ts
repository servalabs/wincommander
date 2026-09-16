import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Privacy Shield Fleet presentation contract", () => {
  test("uses applied detector mode and authoritative Fleet state", async () => {
    const [shield, card] = await Promise.all([
      Bun.file("src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1").text(),
      Bun.file("src/panels/privacy/PrivacyShieldCard.tsx").text(),
    ]);

    expect(shield).toContain("activeMode");
    expect(shield).toContain("--blur-(?:gaze|faces|phone)");
    expect(card).toContain("activeShieldMode");
    expect(card).toContain("presentedShieldMode");
    expect(card).toContain("resolveFleetPrivacyShieldControl");
  });
});
