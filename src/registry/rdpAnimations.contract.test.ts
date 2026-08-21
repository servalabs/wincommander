import { describe, expect, test } from "bun:test";
import { TWEAKS_TOGGLES } from "./tweaks.toggles";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const read = (path: string) => Bun.file(path).text();

describe("persistent RDP animations", () => {
  test("is a reversible Windows Server toggle", () => {
    const toggle = TWEAKS_TOGGLES.find((entry) => entry.id === "serverPersistentRdpAnimations");

    expect(toggle).toMatchObject({
      tier: "free",
      needsAdmin: true,
      irreversible: false,
      section: "server",
      settingsPath: "ideal.tweaks.server.persistentRdpAnimations",
      currentPath: "current.tweaks.server.persistentRdpAnimations",
      enableCmd: "Enable-PersistentRdpAnimations",
      disableCmd: "Disable-PersistentRdpAnimations",
      statusCmd: "Get-PersistentRdpAnimationsStatus",
    });
  });

  test("covers profiles, RDP session overrides, policy, triggers, and live APIs", async () => {
    const script = await read("src-tauri/commander-free/scripts/modules/tweaks/server.ps1");

    expect(script).toContain("VisualFXSetting' -Value 2");
    expect(script).toContain("MinAnimate' -Value '1'");
    expect(script).toContain("TaskbarAnimations' -Value 1");
    expect(script).toContain("Users\\Default\\NTUSER.DAT");
    expect(script).toContain("DisallowAnimations' -Value 0");
    expect(script).toContain("New-ScheduledTaskTrigger -AtLogOn");
    expect(script).toContain("MSFT_TaskSessionStateChangeTrigger");
    expect(script).toContain("$remoteTrigger.StateChange = 3");
    expect(script).toContain("$remoteTrigger.Delay = 'PT2S'");
    expect(script).toContain("'*S-1-5-32-545:(RX)'");
    expect(script).toContain("SystemParametersInfo(0x0049");
    expect(script).toContain("@(0x1043, 0x1003, 0x1013, 0x1017, 0x1019)");
  });

  test("routes both states and persists verified current state", async () => {
    const [backend, settings, bridge] = await Promise.all([
      read("src-tauri/commander-free/src/backend.rs"),
      read("src-tauri/commander-free/src/settings.rs"),
      read("src-tauri/commander-free/scripts/core/settings-bridge.ps1"),
    ]);

    for (const command of ["Enable-PersistentRdpAnimations", "Disable-PersistentRdpAnimations", "Get-PersistentRdpAnimationsStatus"]) {
      expect(backend).toContain(`"${command}" => Some("tweaks/server")`);
    }
    expect(backend).toContain('"persistentRdpAnimations": true');
    expect(backend).toContain('"persistentRdpAnimations": false');
    expect(settings).toContain('("tweaks.server.persistentRdpAnimations", true)');
    expect(settings).toContain('("tweaks.server.persistentRdpAnimations", false)');
    expect(bridge).toContain("persistentRdpAnimations = [bool](");
  });
});
