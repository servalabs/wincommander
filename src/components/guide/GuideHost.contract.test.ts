import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("first-run guide completion", () => {
  test("Lockdown is asked as the final step of the full tour before Dashboard completion", async () => {
    const host = await Bun.file("src/components/guide/GuideHost.tsx").text();
    const topics = await Bun.file("src/content/guide/topics.ts").text();
    const choice = await Bun.file("src/components/guide/LockdownTourChoice.tsx").text();
    const start = host.indexOf("const handleClose = useCallback((completed: boolean) => {");
    const end = host.indexOf("\n\n  return (", start);
    const completion = host.slice(start, end);
    const topicStart = topics.indexOf('id: "dashboard-tour-lockdown-choice"');
    const topicEnd = topics.indexOf("\n  },", topics.indexOf("tours: [{ id: \"tour-dashboard\", order: 90 }]", topicStart));
    const consentTopic = topics.slice(topicStart, topicEnd);

    expect(host).toContain("resolveTourSteps(GUIDE_TOPICS, FIRST_RUN_TOUR_ID, density, { braveInstalled, lockdownVisible, lockdownEnabled, scrubMetadataVisible })");
    expect(completion).toContain("const completedFirstRun = firstRunTourRef.current && completed");
    expect(completion).toContain("firstRunComplete: true, hasSeenMandatoryTour: true");
    expect(completion).toContain('new CustomEvent("navigate-panel", { detail: "dashboard" })');
    expect(completion).not.toContain("setSelfDestructConsentOpen");
    expect(host).not.toContain("CompatDialog");
    expect(consentTopic).toContain("component: LockdownTourChoice");
    expect(consentTopic).not.toContain("requiresAction");
    expect(consentTopic).toContain("order: 90");
    expect(choice).toContain("<Switch");
    expect(choice).toContain('aria-label="Enable Lockdown"');
    expect(choice).toContain("const persistedEnabled = appSettings?.ideal?.privacy?.selfDestruct?.enabled === true");
    expect(choice).toContain("Off by default.");
  });

  test("turning Lockdown on uses an explicit toggle, elevation, and a successful settings write", async () => {
    const choice = await Bun.file("src/components/guide/LockdownTourChoice.tsx").text();

    expect(choice).toContain("isPrivilegedWriteBlocked(true, systemInfo?.isAdmin)");
    expect(choice).toContain("disabled={saving || enableBlocked}");
    expect(choice).toContain("await patchAppSettings");
    expect(choice).toContain("reportSettingsWriteFailure(error)");
    expect(choice).toContain("onCheckedChange={(nextEnabled) => void setLockdownEnabled(nextEnabled)}");
    expect(choice).toContain("checked={enabled}");
  });

  test("keeps the Lockdown save pending across tour close until persisted settings catch up", async () => {
    const host = await Bun.file("src/components/guide/GuideHost.tsx").text();
    const tourState = await Bun.file("src/lib/tourActive.ts").text();
    const choice = await Bun.file("src/components/guide/LockdownTourChoice.tsx").text();

    expect(host).toContain("settleLockdownChoiceIfSaved(lockdownEnabled)");
    expect(tourState).not.toContain("shouldClearPending");
    expect(tourState).toContain("lockdownChoicePendingEnabled === persistedEnabled");
    expect(choice).toContain("closing the tour does not cancel the backend save");
  });

  test("the final choice points to a temporary footer preview and highlights the full rail", async () => {
    const topics = await Bun.file("src/content/guide/topics.ts").text();
    const sidebar = await Bun.file("src/components/RightSidebar.tsx").text();
    const choice = await Bun.file("src/components/guide/LockdownTourChoice.tsx").text();
    const spotlight = await Bun.file("src/components/guide/SpotlightTour.tsx").text();
    const spotlightCss = await Bun.file("src/components/guide/SpotlightTour.css").text();
    const tourStore = await Bun.file("src/lib/tourActive.ts").text();
    const topicStart = topics.indexOf('id: "dashboard-tour-lockdown-choice"');
    const topicEnd = topics.indexOf("\n  },", topics.indexOf("tours: [{ id: \"tour-dashboard\", order: 90 }]", topicStart));
    const choiceTopic = topics.slice(topicStart, topicEnd);

    expect(choiceTopic).toContain('[data-tour="right-sidebar-lockdown"], [data-tour="right-sidebar-lockdown-impression"]');
    expect(choiceTopic).toContain('secondaryAnchor: ".right-sidebar"');
    expect(choiceTopic).toContain('placement: "left"');
    expect(sidebar).toContain('activeTourStepId === "dashboard-tour-lockdown-choice"');
    expect(sidebar).toContain("const showLockdownImpression = lockdownChoiceTourActive && !lockdownVisibleInRail");
    expect(sidebar).toContain("const lockdownVisibleInRail = pendingLockdownEnabled ?? lockdownEnabled");
    expect(sidebar).toContain("disabled={needsElevation || pendingLockdownEnabled !== null}");
    expect(sidebar).toContain('data-tour="right-sidebar-lockdown-impression"');
    expect(choice).toContain("setLockdownChoicePendingEnabled(nextEnabled)");
    expect(choice).toContain("setLockdownChoicePendingEnabled(null)");
    expect(tourStore).toContain("export function settleLockdownChoiceIfSaved(persistedEnabled: boolean): void");
    expect(spotlight).toContain("setActiveTourStepId(step?.topicId ?? null)");
    expect(spotlight).toContain("spotlight-hero-modal--lockdown-choice");
    expect(spotlightCss).toContain("spotlight-root--lockdown-choice .spotlight-ring--lockdown-choice-rail");
    expect(spotlightCss).toContain("bottom: max(16px, 2vh)");
  });
});
