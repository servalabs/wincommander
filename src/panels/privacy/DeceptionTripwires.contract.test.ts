import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Deception & Tripwires presentation boundaries", () => {
  test("provides a reusable grouped presentation and the Privacy Monitor renders it once", async () => {
    const [grouped, index] = await Promise.all([
      Bun.file("src/panels/privacy/DeceptionTripwiresSection.tsx").text(),
      Bun.file("src/panels/privacy/index.tsx").text(),
    ]);

    expect(grouped).toContain("Deception &amp; Tripwires");
    expect(grouped).toContain("<DecoyMonitorSection");
    expect(grouped).toContain("<CanaryTokensSection");
    expect(index).toContain('import DeceptionTripwiresSection from "./DeceptionTripwiresSection"');
    expect(index).toContain("<DeceptionTripwiresSection");
    expect(index).not.toContain('import DecoyMonitorSection from "./DecoyMonitorSection"');
    expect(index).not.toContain('import CanaryTokensSection from "./CanaryTokensSection"');
  });

  test("keeps Canary explicitly local-only and uses the public listener contract", async () => {
    const canary = await Bun.file("src/panels/privacy/CanaryTokensSection.tsx").text();

    expect(canary).toContain("Current canaries use 127.0.0.1");
    expect(canary).toContain("another PC will not notify this PC");
    expect(canary).toContain("does not provide an internet-facing callback service");
    expect(canary).toContain("httpPort: port");
    expect(canary).not.toContain("beacons\n          home");
  });

  test("keeps educational detail behind accessible info controls", async () => {
    const [canary, decoy] = await Promise.all([
      Bun.file("src/panels/privacy/CanaryTokensSection.tsx").text(),
      Bun.file("src/panels/privacy/DecoyMonitorSection.tsx").text(),
    ]);

    expect(canary).toContain('aria-label="About Canary link and document detection"');
    expect(canary).toContain("aria-expanded={showHelp}");
    expect(decoy).toContain('aria-label="How decoy file monitoring works"');
    expect(decoy).toContain("aria-expanded={showIntro}");
  });

  test("makes record removal and artifact deletion boundaries explicit", async () => {
    const [canary, decoy] = await Promise.all([
      Bun.file("src/panels/privacy/CanaryTokensSection.tsx").text(),
      Bun.file("src/panels/privacy/DecoyMonitorSection.tsx").text(),
    ]);

    expect(canary).toContain("This removes its monitoring record. The generated artifact remains");
    expect(canary).toContain("Generated canary artifacts are not affected");
    expect(decoy).toContain("This removes only the monitoring record. The file remains on disk.");
    expect(decoy).toContain("This removes the actual file from disk, not only the watch entry.");
    expect(decoy).toContain("This removes only the recent decoy-file event history.");
  });

  test("states the Fleet-safe metadata boundary", async () => {
    const [decoy, intro] = await Promise.all([
      Bun.file("src/panels/privacy/DecoyMonitorSection.tsx").text(),
      Bun.file("src/panels/privacy/MonitorIntros.tsx").text(),
    ]);

    expect(decoy).toContain("Local path and user details stay on this PC.");
    expect(intro).toContain("paths, usernames, domains, SIDs, and process details stay local");
  });

  test("preserves paid gates for both deception mechanisms", async () => {
    const [canary, decoy, canaryBridge, decoyBridge] = await Promise.all([
      Bun.file("src/panels/privacy/CanaryTokensSection.tsx").text(),
      Bun.file("src/panels/privacy/DecoyMonitorSection.tsx").text(),
      Bun.file("src-tauri/commander-free/src/canary_tokens.rs").text(),
      Bun.file("src-tauri/commander-free/src/file_monitor.rs").text(),
    ]);

    expect(canary).toContain('tier="paid"');
    expect(decoy).toContain('tier="paid"');
    expect(canaryBridge).toContain('require_paid("canary tokens")');
    expect(decoyBridge).toContain('require_paid("Decoy File Monitor")');
  });
});
