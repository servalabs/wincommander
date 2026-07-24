import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): {
    text(): Promise<string>;
  };
};

function read(path: string): Promise<string> {
  return Bun.file(path).text();
}

describe("privacy shield device guardrails", () => {
  test("backend reports camera availability before allowing shield start", async () => {
    const shield = await read("src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1");

    expect(shield).toContain("function Get-PrivacyShieldCameraAvailability");
    expect(shield).toContain("-Filter \"PNPClass='$className'\"");
    expect(shield).toContain("cameraAvailable");
    expect(shield).toContain("Privacy Shield requires a webcam");
  });

  test("start command does not optimistically persist active state", async () => {
    const backend = await read("src-tauri/commander-free/src/backend.rs");
    const optimisticStartPersist =
      /"Start-PrivacyShield"\s*=>\s*Some\(json!\(\{"privacy":\{"privacyShield":\{"shieldRunning": true\}\}\}\)\)/;
    const stoppedStatePersist =
      /"Stop-PrivacyShield"\s*=>\s*Some\(json!\(\{"privacy":\{"privacyShield":\{"shieldRunning": false\}\}\}\)\)/;

    expect(optimisticStartPersist.test(backend)).toBe(false);
    expect(stoppedStatePersist.test(backend)).toBe(true);
  });

  test("ui and tray start paths honor camera-unavailable failures", async () => {
    const card = await read("src/panels/privacy/PrivacyShieldCard.tsx");
    const pollers = await read("src/components/BackgroundPollers.tsx");

    expect(card).toContain("cameraAvailable === false");
    expect(card).toContain("Camera unavailable");
    expect(pollers).toContain("const res = await startPrivacyShield");
    expect(pollers).toContain("if (res.success)");
    expect(pollers).toContain('update_tray_shield_label", { running: false');
  });
});
