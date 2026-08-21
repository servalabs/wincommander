import { describe, expect, test } from "bun:test";
import { buildRadarReport, shouldProbeBrowserHardening } from "./radarScan";

function makeSettings(
  modules: Record<string, boolean>,
  current: Record<string, unknown> = {},
  persona?: "casual" | "secure",
) {
  return {
    app: {
      capabilities: [],
      experienceLevel: "standard",
      modules,
      privacyCleanEnabled: false,
      ...(persona ? { persona } : {}),
    },
    ideal: {},
    current,
  } as any;
}

describe("radar scan filtering", () => {
  test("does not surface findings for disabled modules", () => {
    const report = buildRadarReport({
      appSettings: makeSettings({
        privacy: false,
        network: false,
        tweaks: false,
        cleanup: false,
      }),
      networkBlocklistStatus: { applied: [] },
      browserHardening: [{ Name: "Chrome", Hardened: false }],
    });

    expect(report.findings).toEqual([]);
  });

  test("does not probe browser hardening unless privacy is enabled", () => {
    expect(shouldProbeBrowserHardening(makeSettings({ privacy: false }))).toBe(false);
    expect(shouldProbeBrowserHardening(makeSettings({ privacy: true }))).toBe(true);
  });

  test("marks recommendation-only toggle findings as drift when ideal and current differ", () => {
    const settings = makeSettings(
      { privacy: true, network: false, tweaks: false, cleanup: false },
      { privacy: { appCapabilities: { webcam: "Allow" } } },
    );
    settings.ideal = {
      privacy: { appCapabilities: { webcam: "Deny" } },
    };

    const report = buildRadarReport({
      appSettings: settings,
      networkBlocklistStatus: { applied: [] },
    });

    const finding = report.findings.find((f) => f.id === "cap-webcam");
    expect(finding?.drift).toBe(true);
    expect(finding?.targetChecked).toBe(true);
  });

  test("surfaces safe recommended defaults on a fresh install where current.* is null", () => {
    // A fresh install serializes every current.* field as null (not undefined,
    // not {}), which the radar previously skipped — so recommended tweaks never
    // showed until the system probe ran. `telemetry` is a safeDefault toggle.
    const settings = makeSettings(
      { privacy: true, network: false, tweaks: false, cleanup: false },
      { privacy: { telemetry: { windowsDisabled: null } } },
    );

    const report = buildRadarReport({
      appSettings: settings,
      networkBlocklistStatus: { applied: [] },
    });

    const finding = report.findings.find((f) => f.id === "telemetry");
    expect(finding?.id).toBe("telemetry");
    expect(finding?.drift).toBe(false);
  });

  test("casual persona does not get the secure-only Quick Access / clipboard nudges", () => {
    const settings = makeSettings(
      { privacy: true, network: false, tweaks: false, cleanup: false },
      {
        privacy: {
          clipboard: { historyDisabled: null },
          tracking: { quickAccessRecentDisabled: null, quickAccessFrequentDisabled: null, searchHistoryDisabled: null },
        },
      },
      "casual",
    );

    const report = buildRadarReport({ appSettings: settings, networkBlocklistStatus: { applied: [] } });
    const ids = report.findings.map((f) => f.id);

    expect(ids).not.toContain("clipboardHistory");
    expect(ids).not.toContain("hideQuickAccessRecent");
    expect(ids).not.toContain("hideQuickAccessFrequent");
    expect(ids).not.toContain("disableSearchHistory");
  });

  test("secure persona recommends Clipboard History disable, hide-recent, hide-frequent, disable-search", () => {
    const settings = makeSettings(
      { privacy: true, network: false, tweaks: false, cleanup: false },
      {
        privacy: {
          clipboard: { historyDisabled: null },
          tracking: { quickAccessRecentDisabled: null, quickAccessFrequentDisabled: null, searchHistoryDisabled: null },
        },
      },
      "secure",
    );

    const report = buildRadarReport({ appSettings: settings, networkBlocklistStatus: { applied: [] } });
    const ids = report.findings.map((f) => f.id);

    expect(ids).toContain("clipboardHistory");
    expect(ids).toContain("hideQuickAccessRecent");
    expect(ids).toContain("hideQuickAccessFrequent");
    expect(ids).toContain("disableSearchHistory");
  });

  test("Windows Server findings only surface when isServerSku is true", () => {
    const clientSettings = makeSettings(
      { privacy: false, network: false, tweaks: true, cleanup: false },
      { tweaks: { server: { isServerSku: false, wdigestBlocked: false, smb1Disabled: false } } },
    );
    const clientReport = buildRadarReport({ appSettings: clientSettings, networkBlocklistStatus: { applied: [] } });
    expect(clientReport.findings.some((f) => f.id === "serverWdigest")).toBe(false);
    expect(clientReport.findings.some((f) => f.id === "serverSmb1")).toBe(false);
    expect(clientReport.findings.some((f) => f.id === "serverHideLastUser")).toBe(false);

    const serverSettings = makeSettings(
      { privacy: false, network: false, tweaks: true, cleanup: false },
      { tweaks: { server: { isServerSku: true, wdigestBlocked: false, smb1Disabled: false } } },
    );
    const serverReport = buildRadarReport({ appSettings: serverSettings, networkBlocklistStatus: { applied: [] } });
    expect(serverReport.findings.some((f) => f.id === "serverWdigest")).toBe(true);
    expect(serverReport.findings.some((f) => f.id === "serverSmb1")).toBe(true);
    expect(serverReport.findings.some((f) => f.id === "serverHideLastUser")).toBe(true);

    // reducesSecurity toggles are never radar-eligible, on Server or client.
    expect(serverReport.findings.some((f) => f.id === "serverCtrlAltDel")).toBe(false);
    expect(serverReport.findings.some((f) => f.id === "serverIeEsc")).toBe(false);
  });

  test("surfaces stale Windows storage cleanup when the cleanup module is enabled", () => {
    const settings = makeSettings(
      { privacy: false, network: false, tweaks: false, cleanup: true },
    );
    settings.app.firstRunComplete = true;

    const report = buildRadarReport({ appSettings: settings });

    expect(report.findings.some((finding) => finding.id === "disk-cleanup")).toBe(true);
  });
});
