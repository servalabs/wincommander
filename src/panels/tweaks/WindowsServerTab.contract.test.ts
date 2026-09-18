import { describe, expect, it } from "vitest";
import fs from "node:fs";

describe("Windows Server tab", () => {
  it("keeps server-only settings and RDP redirection in Windows Settings", async () => {
    const tweaks = fs.readFileSync("src/panels/tweaks/index.tsx", "utf8");
    const privacy = fs.readFileSync("src/panels/privacy/index.tsx", "utf8");

    expect(tweaks).toContain('value="windows-server"');
    expect(tweaks).toContain('<RdpRedirectionCard />');
    expect(tweaks).toContain('section={TWEAKS_SECTIONS[8]}');
    expect(privacy).not.toContain('<RdpRedirectionCard />');
  });
});
