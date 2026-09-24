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

  test("persists a remove or delete before refreshing and handles Windows path spelling", async () => {
    const decoy = await Bun.file("src/panels/privacy/DecoyMonitorSection.tsx").text();

    expect(decoy).toContain('await onPatchDecoy({ enrolledPaths: enrolledPaths.filter((p) => !sameDecoyPath(p, path)) })');
    expect(decoy).toContain('left.replaceAll("/", "\\\\").toLocaleLowerCase()');
    expect(decoy).not.toContain("enrolledPaths.filter(p => p !== path)");
  });

  test("removes externally deleted files from the card and limits disk deletion to standard decoys", async () => {
    const decoy = await Bun.file("src/panels/privacy/DecoyMonitorSection.tsx").text();

    expect(decoy).toContain("const visibleDecoys = decoys.filter((decoy) => decoy.exists)");
    expect(decoy).toContain("setInterval(refreshDecoys, expanded ? 5_000 : 30_000)");
    expect(decoy).toContain("Only WinCommander’s standard decoys can be deleted here");
    expect(decoy).toContain("{d.standard && (");
    expect(decoy).toContain("standard?: boolean;");
  });

  test("states the Fleet decoy-incident attribution boundary", async () => {
    const [decoy, intro] = await Promise.all([
      Bun.file("src/panels/privacy/DecoyMonitorSection.tsx").text(),
      Bun.file("src/panels/privacy/MonitorIntros.tsx").text(),
    ]);

    expect(decoy).toContain("The SID stays on this PC.");
    expect(intro).toContain("Fleet receives the decoy path plus the Windows account, domain, and app");
    expect(intro).toContain("The SID stays on this PC.");
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
