import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("first-run guide completion", () => {
  test("tour completion returns to Dashboard and offers the Lockdown opt-in", async () => {
    const source = await Bun.file("src/components/guide/GuideHost.tsx").text();
    const start = source.indexOf("const handleClose = useCallback((completed: boolean) => {");
    const end = source.indexOf("\n\n  return (", start);
    const completion = source.slice(start, end);

    expect(start).toBeGreaterThan(-1);
    expect(completion).toContain("const completedFirstRun = firstRunTourRef.current && completed");
    expect(completion).toContain("firstRunTourRef.current = false");
    expect(completion).toContain("if (completedFirstRun)");
    expect(completion).toContain("firstRunComplete: true, hasSeenMandatoryTour: true");
    expect(completion).toContain('new CustomEvent("navigate-panel", { detail: "dashboard" })');
    expect(completion).toContain("setSelfDestructConsentOpen(true)");
    expect(completion).not.toContain("selfDestruct: { enabled: true }");
    expect(completion).toContain("selfDestruct?.enabled !== true");
  });

  test("only the explicit, elevation-checked Lockdown choice enables the setting", async () => {
    const source = await Bun.file("src/components/guide/GuideHost.tsx").text();
    const enableStart = source.indexOf("const handleEnableSelfDestruct = useCallback");
    const closeStart = source.indexOf("const handleClose = useCallback", enableStart);
    const enable = source.slice(enableStart, closeStart);
    const close = source.slice(source.indexOf("const handleSelfDestructConsentClose ="), enableStart);
    const completion = source.slice(closeStart, source.indexOf("\n\n  return (", closeStart));
    const dialogStart = source.indexOf("<CompatDialog\n        isOpen={selfDestructConsentOpen}");
    const dialogEnd = source.indexOf("</CompatDialog>", dialogStart);
    const dialog = source.slice(dialogStart, dialogEnd);

    expect(enableStart).toBeGreaterThan(-1);
    expect(enable).toContain("selfDestruct: { enabled: true }");
    expect(enable).toContain("await patchAppSettings");
    expect(enable).toContain("if (savingSelfDestructConsent || lockdownEnableBlocked) return");
    expect(close).not.toContain("patchAppSettings");
    expect(close).not.toContain("selfDestruct: { enabled: true }");
    expect(close).toContain("setSelfDestructConsentOpen(false)");
    expect(dialog).toContain("onClose={handleSelfDestructConsentClose}");
    expect(dialog).toContain("onClick={handleSelfDestructConsentClose}");
    expect(dialog).toContain('title="Enable Lockdown?"');
    expect(dialog).toContain("Leave Lockdown off");
    expect(dialog).toContain("Enable Lockdown");
    expect(dialog).toContain("arms any Lockdown triggers you have configured, which may run when their conditions are met");
    expect(dialog).toContain("This prompt will not press the Lockdown button for you");
    expect(dialog).toContain("disabled={lockdownEnableBlocked}");
    expect(dialog).toContain("{MACHINE_SCOPE_ELEVATION_MESSAGE}");
    expect(completion).toContain('new CustomEvent("navigate-panel", { detail: "dashboard" })');
    expect(dialog).not.toContain("patchAppSettings");
    expect(source).toContain("firstRunTourRef.current = true");
    expect(source).toContain("firstRunTourRef.current = false");
  });
});
