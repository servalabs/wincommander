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

  test("the camera toggle permits current-user consent without weakening machine policy", async () => {
    const toggles = await read("src/registry/capabilities.toggles.ts");
    const webcam = toggles.slice(toggles.indexOf('id: "cap-webcam"'), toggles.indexOf('id: "cap-microphone"'));

    expect(webcam).toContain("needsAdmin: false");
    const source = await read("src-tauri/commander-free/scripts/modules/privacy/telemetry.ps1");
    expect(source).toContain("if (-not (Test-IsAdmin)) { return Set-CurrentUserCapabilityAccess");
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

  test("sustained presence loss is independently debounced and never relabeled as look-away", async () => {
    const shield = await read("src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1");
    const backend = await read("src-tauri/commander-free/src/backend.rs");

    expect(shield).toContain("self._no_face_detected_streak = 0");
    expect(shield).toContain("self._face_recovery_streak = 0");
    expect(shield).toContain("self._presence_loss_active = False");
    expect(shield).toContain("self._no_face_detected_streak >= self.buffer_frames");
    expect(shield).toContain("self._face_recovery_streak >= self.buffer_frames");
    expect(shield).toContain("presence_lost_triggered = self._presence_loss_active");
    expect(shield).toContain('parts.append("PRESENCE LOST")');
    expect(shield).toContain("recovery_unknown = presence_unknown or raw_look_away");
    expect(shield).toContain("if recovery_unknown and is_clear:");
    expect(shield).not.toContain('parts.append("NO FACE")');
    expect(backend).toContain('classes.push("presence_lost")');
    expect(backend).toContain('"look_away" | "presence_lost" | "multiple_faces" | "phone_detected"');
    expect(backend).not.toContain('"look_away" | "no_face" | "multiple_faces" | "secondary_device"');
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
      /"Stop-PrivacyShield" if privacy_shield_stop_succeeded\(result\)\s*=>\s*\{\s*Some\(json!\(\{"privacy":\{"privacyShield":\{"shieldRunning": false\}\}\}\)\)/;

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

  test("native Fleet supervisor reports camera unavailability instead of a generic stopped state", async () => {
    const native = await read("src-tauri/commander-free/src/fleet_agent.rs");
    expect(native).toContain('fleet_shield_camera_issue(&local).unwrap_or("stopped")');
    expect(native).toContain('fleet_shield_camera_issue(&before).unwrap_or("stopped")');
    expect(native).toContain('Some("windows_camera_policy_denied")');
    expect(native).toContain('Some("hardware_unavailable")');
  });

  test("native Fleet stop retains ownership until stop is observed", async () => {
    const native = await read("src-tauri/commander-free/src/fleet_agent.rs");
    const stop = native.indexOf('"Privacy Shield stop was not accepted by the device"');
    const verified = native.indexOf('"Privacy Shield is still running after the stop request"', stop);
    const clearOwner = native.indexOf('"privacyShieldSessionOwned": false', verified);
    expect(stop).toBeGreaterThan(-1);
    expect(verified).toBeGreaterThan(stop);
    expect(clearOwner).toBeGreaterThan(verified);
  });

  test("Fleet policy does not lock a shield session the local user started", async () => {
    const card = await read("src/panels/privacy/PrivacyShieldCard.tsx");

    expect(card).toContain("const fleetPolicyManaged");
    expect(card).toContain("const fleetShieldSessionLocked");
    expect(card).toContain("Privacy Shield was started by Fleet and can only be stopped by a Fleet administrator.");
    expect(card).toContain("!privacyShieldRunning && localFleetControl.startLocked");
    expect(card).toContain("resolveLocalFleetPrivacyShieldControl({");
    expect(card).toContain("sessionOwned: fleetShieldSessionOwned");
  });

  test("a Fleet Stop releases the tray start action after fresh state read-back", async () => {
    const pollers = await read("src/components/BackgroundPollers.tsx");
    const trayStart = pollers.slice(
      pollers.indexOf('const unlistenTrayShield = listen("tray-shield-toggle-requested"'),
      pollers.indexOf('const unlistenFleetShieldDenied = listen')
    );

    expect(trayStart).toContain('invoke<typeof desiredState>("fleet_sync_shield_state")');
    expect(trayStart).toContain("resolveFleetPrivacyShieldControl({");
    expect(trayStart).toContain("resolveLocalFleetPrivacyShieldControl({");
    expect(trayStart).toContain("if (localControl.startLocked)");
    expect(trayStart).not.toContain("if (appSettingsRef.current?.ideal?.privacy?.privacyShield?.fleetManaged === true)");
  });

  test("Fleet attention alerts require the selected signed Fleet monitor", async () => {
    const backend = await read("src-tauri/commander-free/src/backend.rs");

    expect(backend).toContain("fn fleet_privacy_event_gate(");
    expect(backend).toContain("let fleet_monitoring_selected = shield.fleet_monitoring_enabled == Some(true)");
    expect(backend).toContain("!fleet_session_owned && !fleet_monitoring_selected");
    expect(backend).toContain("selected_fleet_monitor_reports_a_locally_started_shield");
    expect(backend).toContain('"look_away" | "presence_lost" | "multiple_faces" | "phone_detected"');
    expect(backend).toContain("match fleet_privacy_alert_mode(class).await {");
  });

  test("Fleet attention alerts wake the heartbeat owner even when detector IPC uses another Pro worker", async () => {
    const backend = await read("src-tauri/commander-free/src/backend.rs");
    const proTransport = await read("../wincommander-pro/commander-pro/src/fleet_agent/legacy_transport.rs");

    expect(backend).toContain('"record_privacy_shield_event"');
    expect(proTransport).toContain("const CHECKIN_REQUEST_EVENT_NAME");
    expect(proTransport).toContain("signal_cross_process_checkin_request();");
    expect(proTransport).toContain("take_cross_process_checkin_request()");
    expect(proTransport).toContain("wait_for_next_checkin().await;");
  });

  test("the event reader retains an initial look-away emitted during startup", async () => {
    const backend = await read("src-tauri/commander-free/src/backend.rs");

    expect(backend).toContain("let mut offset: u64 = 0;");
    expect(backend).toContain("initial native notification while the visual blur still worked");
  });
});
