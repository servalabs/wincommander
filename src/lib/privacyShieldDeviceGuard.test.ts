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
    expect(shield).toContain("blockedByPolicy");
    expect(shield).toContain("Camera is blocked by Windows policy:");
    expect(shield).toContain("HKCU AllowCamera=0");
    expect(shield).toContain("HKLM AllowCamera=0");
    expect(shield).toContain("HKCU LetAppsAccessCamera=2");
    expect(shield).toContain("HKLM LetAppsAccessCamera=2");
    expect(shield).toContain("policyBlockers");
  });

  test("the camera capability toggle requires elevation for its policy writes", async () => {
    const toggles = await read("src/registry/capabilities.toggles.ts");
    const webcam = toggles.slice(toggles.indexOf('id: "cap-webcam"'), toggles.indexOf('id: "cap-microphone"'));

    expect(webcam).toContain("needsAdmin: true");
  });

  test("capability changes fail when Windows still reports the opposite effective access", async () => {
    const freeModule = await read("src-tauri/commander-free/scripts/modules/privacy/telemetry.ps1");
    const sharedModule = await read("src-tauri/wincmd-shared/scripts/capability-access.ps1");

    for (const source of [freeModule, sharedModule]) {
      expect(source).toContain("$effective = Get-AppCapabilityAccessStatus -Capability $Capability");
      expect(source).toContain("$effectiveAccess -ne $Access");
      expect(source).toContain("The policy change did not take effect.");
      expect(source).not.toContain("catch {}\n        }\n\n        @{ status = \"updated\"; capability = $Capability");
    }
  });

  test("an attentive camera result clears the Privacy Shield overlay", async () => {
    const shield = await read("src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1");

    expect(shield).toContain("should_blur = (not is_clear) and (");
    expect(shield).toContain("self.overlay.update_state(is_clear or not should_blur, reason)");
    expect(shield).not.toContain("should_blur = is_clear or (");
  });

  test("a managed detector exits with its WinCommander owner, not its transient PowerShell launcher", async () => {
    const shield = await read("src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1");
    const backend = await read("src-tauri/commander-free/src/backend.rs");
    const sidecar = await read("src-tauri/commander-free/src/sidecar.rs");

    expect(shield).not.toContain("--parent-pid");
    expect(shield).toContain("--owner-pid");
    expect(shield).toContain("owner_process_is_alive");
    expect(shield).toContain("Shield owner exited; stopping managed detector");
    expect(backend).toContain('cmd.env("WINCMD_SHIELD_OWNER_PID", std::process::id().to_string())');
    expect(sidecar).toContain('cmd.env("WINCMD_SHIELD_OWNER_PID", std::process::id().to_string())');
  });

  test("long-lived detector does not hold the backend command output pipe open", async () => {
    const shield = await read("src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1");

    expect(shield).toContain("$startInfo.RedirectStandardOutput = $true");
    expect(shield).toContain("$startInfo.RedirectStandardError = $true");
    expect(shield).toContain("$process.BeginOutputReadLine()");
    expect(shield).toContain("$process.BeginErrorReadLine()");
  });

  test("black camera frames fail startup before a false look-away blackout", async () => {
    const shield = await read("src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1");

    expect(shield).toContain("black_frames >= 16");
    expect(shield).toContain("Camera is delivering black frames");
    expect(shield).toContain("open its privacy shutter or close another camera app");
    expect(shield).toContain("Camera feed is black - open its privacy shutter or close another camera app.");
  });

  test("stop paths use the detector PID marker when command-line inspection is unavailable", async () => {
    const shield = await read("src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1");
    const backend = await read("src-tauri/commander-free/src/backend.rs");

    expect(shield).toContain("privacy_shield.pid");
    expect(shield).toContain("Get-PrivacyShieldPidFromMarker");
    expect(shield).toContain("Clear-PrivacyShieldPidMarker");
    expect(backend).toContain("privacy_shield.pid");
    expect(backend).toContain("Get-CimInstance -ClassName Win32_Process");
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

  test("Fleet receives unavailable-camera truth before an unmanaged stopped state", async () => {
    const pollers = await read("src/components/BackgroundPollers.tsx");
    const unavailable = pollers.indexOf('if (!running && status.success && status.data?.cameraAvailable === false)');
    const unmanaged = pollers.indexOf('if (!shieldControl.managed || !shieldControl.enabled)');

    expect(unavailable).toBeGreaterThan(-1);
    expect(unmanaged).toBeGreaterThan(unavailable);
    const capabilityBranch = pollers.slice(unavailable, unmanaged);
    expect(capabilityBranch).toContain('"windows_camera_policy_denied"');
    expect(capabilityBranch).toContain('"hardware_unavailable"');
    expect(capabilityBranch).toContain('"camera_status_unknown"');
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

  test("the event reader retains an initial look-away emitted during startup", async () => {
    const backend = await read("src-tauri/commander-free/src/backend.rs");

    expect(backend).toContain("let mut offset: u64 = 0;");
    expect(backend).toContain("initial native notification while the visual blur still worked");
  });
});
