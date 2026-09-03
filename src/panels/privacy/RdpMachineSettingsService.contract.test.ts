import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("RDP machine settings use the typed service route", () => {
  test("the privacy card applies incoming RDP through the typed client", async () => {
    const card = await Bun.file("src/panels/privacy/RdpIdleCard.tsx").text();

    expect(card).toContain('import { applyMachineSetting } from "../../hooks/machineSettingsClient"');
    expect(card).toContain("await applyMachineSetting({");
    expect(card).toContain('setting: "rdp_incoming"');
    expect(card).toContain('kind: "rdp_incoming"');
    expect(card).not.toContain("Enable-RdpIncomingIdleTimeout");
    expect(card).not.toContain("Disable-RdpIncomingIdleTimeout");
  });

  test("the client and Tauri bridge retain both fixed RDP service settings", async () => {
    const [client, bridge] = await Promise.all([
      Bun.file("src/hooks/machineSettingsClient.ts").text(),
      Bun.file("src-tauri/commander-free/src/machine_settings.rs").text(),
    ]);

    expect(client).toContain('setting: "rdp_lock"');
    expect(client).toContain('invoke<MachineSettingObserved>("apply_machine_setting", { request })');
    expect(bridge).toContain("crate::svc_client::apply_machine_setting(request).await");
  });
});
