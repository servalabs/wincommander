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
    // When no usable camera exists, the complete trigger/autostart control
    // surface must disappear rather than leaving inert privacy controls.
    expect(card).toContain("{cameraAvailable !== false && (");
    expect(pollers).toContain("const res = await startPrivacyShield");
    expect(pollers).toContain("if (res.success)");
    expect(pollers).toContain('update_tray_shield_label", { running: false');
  });

  test("fleet policy stop retains ownership until the local process really stops", async () => {
    const pollers = await read("src/components/BackgroundPollers.tsx");

    expect(pollers).toContain("const fleetPrivacySupervisor");
    expect(pollers).toContain("if (!fleetPrivacySupervisor) return;");
    expect(pollers).toContain("const stopOwnedSession = async () =>");
    expect(pollers).toContain("if (!stopped.success || stopped.data?.success !== true) return false;");
    expect(pollers).toContain('await report("disabled_by_policy");');
  });

  test("Fleet policy does not lock a shield session the local user started", async () => {
    const card = await read("src/panels/privacy/PrivacyShieldCard.tsx");

    expect(card).toContain("const fleetPolicyManaged");
    expect(card).toContain("const fleetShieldSessionLocked");
    expect(card).toContain("Privacy Shield was started by Fleet and can only be stopped by a Fleet administrator.");
    expect(card).toContain("!privacyShieldRunning && fleetPolicyManaged");
  });

  test("Fleet attention alerts require the enabled signed Fleet policy", async () => {
    const backend = await read("src-tauri/commander-free/src/backend.rs");

    expect(backend).toContain("fn fleet_privacy_event_is_enabled(");
    expect(backend).toContain("shield.fleet_managed != Some(true)");
    expect(backend).toContain("shield.fleet_monitoring_enabled != Some(true)");
    expect(backend).toContain('"look_away" | "no_face" | "multiple_faces" | "secondary_device"');
    expect(backend).toContain("if allow_fleet_privacy_alert(gaze_kind).await {");
  });
});
