// src/components/guide/GuideHost.tsx
//
// Single mount point for the guide runtime. Renders the spotlight tour (only
// while a tour is active) — the sole help/onboarding surface (the help
// center article system was removed). On an unresolved first launch,
// auto-starts the full onboarding tour instead of the old Setup Wizard
// (removed) — non-dismissable until it has been completed once. Listens for
// the `start-tour` window event so manual replays (title bar "?", dashboard
// "Take the tour") work any time.

import { useCallback, useEffect, useRef, useState } from "react";
import { useAppState } from "../../context/AppContext";
import { reportSettingsWriteFailure } from "../../lib/settingsWriteRecovery";
import SpotlightTour from "./SpotlightTour";
import { GUIDE_TOPICS } from "../../content/guide";
import { resolveTourSteps } from "../../lib/tour";
import type { TourStep } from "../../content/guide/types";
import { getDensityForSettings } from "../../lib/personaMigration";
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from "../../lib/machineScopeElevation";
import { setTourActive } from "../../lib/tourActive";
import useBraveInstalled from "../../hooks/useBraveInstalled";
import useBorrowedActive from "../../hooks/useBorrowedActive";
import useVisibility from "../../hooks/useVisibility";
import { DEFAULT_BORROWED_EXTRAS } from "../../lib/visibilityDefaults";
import { Button } from "../ui/bp";
import { CompatDialog, CompatDialogBody, CompatDialogFooter } from "../ui/compat-dialog";

// The full onboarding sequence — Dashboard's hero moments (Fix all, Scrub,
// Lockdown, quick toggles) continuing straight through Privacy Settings,
// Secure Storage, and Packages & Apps. See the tour-dashboard membership
// comments in content/guide/topics.ts.
const FIRST_RUN_TOUR_ID = "tour-dashboard";

export default function GuideHost() {
  const { appSettings, startupComplete, systemInfo, patchAppSettings } = useAppState();
  const [steps, setSteps] = useState<TourStep[] | null>(null);
  // True only for the auto-started first-run tour, and only until it has
  // been completed once — SpotlightTour suppresses its own X/Escape while
  // this holds, so a fresh install can't dismiss its way out of the one
  // mandatory walkthrough. Every other invocation (manual replay, or a
  // fresh-install run after hasSeenMandatoryTour is already true) stays
  // fully cancellable.
  const [mandatory, setMandatory] = useState(false);
  const [selfDestructConsentOpen, setSelfDestructConsentOpen] = useState(false);
  const [savingSelfDestructConsent, setSavingSelfDestructConsent] = useState(false);
  const firstRunTourRef = useRef(false);
  const tourAutoStartedRef = useRef(false);

  // Density drives how many stops run (Expert sees fewer). In decoy mode
  // appSettings is null → getDensityForSettings falls back to the calmer
  // Guided set.
  const density = getDensityForSettings(appSettings);
  // Gates the conditional "install Brave" stop — resolveTourSteps drops it
  // once Brave is already on the machine.
  const braveInstalled = useBraveInstalled();
  const borrowedActive = useBorrowedActive();
  const borrowedHidden = appSettings?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS;
  const visibility = useVisibility();
  const tourHidden =
    appSettings?.app?.hideTour === true
    || (borrowedActive && borrowedHidden.includes("tour"));
  const scrubMetadataVisible =
    visibility.isVisible({ capability: ["privacy"] })
    && !appSettings?.app?.hiddenSidebarActions?.includes("scrubMeta")
    && !(borrowedActive && borrowedHidden.includes("action:scrubMeta"));
  const lockdownVisible =
    appSettings?.ideal?.privacy?.selfDestruct?.enabled === true
    && !appSettings?.app?.hiddenSidebarActions?.includes("lockdown")
    && !(borrowedActive && borrowedHidden.includes("action:lockdown"));
  const lockdownEnableBlocked = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);

  // Manual tour starts (title bar "?", dashboard "Take the tour", deep
  // links) — always dismissable.
  useEffect(() => {
    const onStart = (e: Event) => {
      if (tourHidden) return;
      const tourId = (e as CustomEvent<{ tourId?: string }>).detail?.tourId ?? "welcome";
      const resolved = resolveTourSteps(GUIDE_TOPICS, tourId, density, { braveInstalled, lockdownVisible, scrubMetadataVisible });
      if (resolved.length > 0) {
        firstRunTourRef.current = false;
        setMandatory(false);
        setSteps(resolved);
      }
    };
    window.addEventListener("start-tour", onStart as EventListener);
    return () => window.removeEventListener("start-tour", onStart as EventListener);
  }, [density, braveInstalled, lockdownVisible, scrubMetadataVisible, tourHidden]);

  // First launch auto-starts the full spotlight tour instead of the removed
  // Setup Wizard. Closing it does not re-open it during the same app session.
  useEffect(() => {
    const isLoading = !startupComplete;
    const hasSettings = appSettings != null;
    const firstRunComplete = appSettings?.app?.firstRunComplete === true;
    const shouldStart = !tourHidden && !isLoading && hasSettings && !firstRunComplete && !tourAutoStartedRef.current;
    if (!shouldStart) return;
    tourAutoStartedRef.current = true;
    const timer = window.setTimeout(() => {
      const resolved = resolveTourSteps(GUIDE_TOPICS, FIRST_RUN_TOUR_ID, density, { braveInstalled, lockdownVisible, scrubMetadataVisible });
      if (resolved.length === 0) return;
      firstRunTourRef.current = true;
      setMandatory(appSettings?.app?.hasSeenMandatoryTour !== true);
      setSteps(resolved);
    }, 500);
    return () => window.clearTimeout(timer);
  }, [appSettings, startupComplete, density, braveInstalled, lockdownVisible, scrubMetadataVisible, tourHidden]);

  useEffect(() => {
    if (!tourHidden) return;
    setSteps(null);
    setMandatory(false);
    firstRunTourRef.current = false;
  }, [tourHidden]);

  // Publish "a tour is running" for the surfaces that hide a step's anchor
  // outside Expert density or behind a disabled module — see lib/tourActive.ts.
  // Driven off `steps` (not the start/close callbacks) so an unmount mid-tour
  // still clears it.
  const tourRunning = steps !== null && steps.length > 0;
  useEffect(() => {
    setTourActive(tourRunning);
    return () => setTourActive(false);
  }, [tourRunning]);

  const handleSelfDestructConsentClose = useCallback(() => {
    if (!savingSelfDestructConsent) setSelfDestructConsentOpen(false);
  }, [savingSelfDestructConsent]);

  const handleEnableSelfDestruct = useCallback(async () => {
    if (savingSelfDestructConsent || lockdownEnableBlocked) return;
    setSavingSelfDestructConsent(true);
    try {
      await patchAppSettings({ ideal: { privacy: { selfDestruct: { enabled: true } } } } as any);
      setSelfDestructConsentOpen(false);
    } catch (error) {
      reportSettingsWriteFailure(error);
    } finally {
      setSavingSelfDestructConsent(false);
    }
  }, [lockdownEnableBlocked, patchAppSettings, savingSelfDestructConsent]);

  const handleClose = useCallback((completed: boolean) => {
    // useTour can invoke onClose from a state updater. Consume the marker
    // synchronously so React Strict Mode or a duplicate completion callback
    // cannot replay the opt-in prompt or settings write.
    const completedFirstRun = firstRunTourRef.current && completed;
    firstRunTourRef.current = false;
    if (completedFirstRun) {
      void patchAppSettings({ app: { firstRunComplete: true, hasSeenMandatoryTour: true } }).catch(reportSettingsWriteFailure);
      window.dispatchEvent(new CustomEvent("navigate-panel", { detail: "dashboard" }));
      if (appSettings?.ideal?.privacy?.selfDestruct?.enabled !== true) {
        setSelfDestructConsentOpen(true);
      }
    }
    setSteps(null);
    setMandatory(false);
  }, [appSettings?.ideal?.privacy?.selfDestruct?.enabled, patchAppSettings]);

  return (
    <>
      {steps && steps.length > 0 && (
        <SpotlightTour steps={steps} onClose={handleClose} dismissable={!mandatory} />
      )}
      <CompatDialog
        isOpen={selfDestructConsentOpen}
        onClose={handleSelfDestructConsentClose}
        title="Enable Lockdown?"
        icon="warning-sign"
        className="w-[min(32rem,calc(100vw-2rem))]"
        canEscapeKeyClose={!savingSelfDestructConsent}
        canOutsideClickClose={!savingSelfDestructConsent}
        isCloseButtonShown={!savingSelfDestructConsent}
      >
        <CompatDialogBody className="space-y-3">
          <p className="text-sm leading-6 text-[var(--text-dim)]">
            Lockdown is the emergency action on the right side of the window. Enabling it also arms any Lockdown triggers you have configured, which may run when their conditions are met. This prompt will not press the Lockdown button for you.
          </p>
          {lockdownEnableBlocked && (
            <p role="status" className="text-sm leading-6 text-[var(--warn)]">
              {MACHINE_SCOPE_ELEVATION_MESSAGE}
            </p>
          )}
        </CompatDialogBody>
        <CompatDialogFooter className="flex-wrap">
          <Button small minimal disabled={savingSelfDestructConsent} onClick={handleSelfDestructConsentClose}>
            Leave Lockdown off
          </Button>
          <Button small intent="danger" disabled={lockdownEnableBlocked} loading={savingSelfDestructConsent} onClick={() => void handleEnableSelfDestruct()}>
            Enable Lockdown
          </Button>
        </CompatDialogFooter>
      </CompatDialog>
    </>
  );
}
