import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const card = fs.readFileSync(path.join(root, "src/panels/privacy/RdpRedirectionCard.tsx"), "utf8");
const backend = fs.readFileSync(path.join(root, "src-tauri/commander-free/src/rdp_redirection.rs"), "utf8");
const client = fs.readFileSync(path.join(root, "src/hooks/rdpRedirectionClient.ts"), "utf8");
const machineSettings = fs.readFileSync(path.join(root, "src-tauri/commander-free/src/machine_settings.rs"), "utf8");

describe("Windows Server RDP resource redirection", () => {
  it("is hidden on non-server Windows editions", () => {
    expect(card).toContain('systemInfo?.osName?.toLowerCase().includes("server") === true');
    expect(card).toContain("if (!osLooksLikeServer) return null");
    expect(card).toContain("next.isWindowsServer ? next : null");
  });

  it("uses the existing privileged machine-setting seam", () => {
    expect(client).toContain('invoke<T>("apply_machine_setting"');
    expect(card).toContain("../../hooks/rdpRedirectionClient");
    expect(client).toContain('setting: "rdp_redirection"');
    expect(machineSettings).toContain('Some("rdp_redirection")');
    expect(machineSettings).toContain("rdp_redirection::handle(value)");
  });

  it("supports every native resource requested by the server rollout", () => {
    for (const capability of [
      "smart_cards",
      "drives",
      "clipboard",
      "printers",
      "audio_playback",
      "microphone",
      "pnp_devices",
      "camera",
      "webauthn",
    ]) {
      expect(backend).toContain(`\"${capability}\"`);
    }
  });

  it("keeps generic RemoteFX USB passthrough disabled", () => {
    expect(backend).toContain("Remove-ItemProperty -Path $usb -Name fUsbRedirectionEnableMode");
    expect(card).toContain("Generic RemoteFX USB passthrough stays disabled");
    const profile = backend.match(/fn client_profile\(\)[\s\S]*?\n}/)?.[0] ?? "";
    expect(profile).toContain("redirectsmartcards");
    expect(profile.toLowerCase()).not.toContain("usbdevicestoredirect");
  });

  it("generates the required native RDP client flags", () => {
    for (const flag of [
      "redirectsmartcards:i:1",
      "redirectclipboard:i:1",
      "redirectprinters:i:1",
      "drivestoredirect:s:*",
      "audiomode:i:0",
      "audiocapturemode:i:1",
      "camerastoredirect:s:*",
      "redirectwebauthn:i:1",
    ]) {
      expect(backend).toContain(flag);
    }
  });

  it("re-checks Windows Server and administrator rights before writes", () => {
    expect(backend).toContain("This control is only available on Windows Server.");
    expect(backend).toContain("Administrator rights are required to change machine-wide RDP redirection policy.");
  });
});
