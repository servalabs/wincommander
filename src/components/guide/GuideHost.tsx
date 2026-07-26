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
import SpotlightTour from "./SpotlightTour";
import { GUIDE_TOPICS } from "../../content/guide";
import { resolveTourSteps } from "../../lib/tour";
import type { TourStep } from "../../content/guide/types";
import { getDensityForSettings } from "../../lib/personaMigration";
import { setTourActive } from "../../lib/tourActive";
import useBraveInstalled from "../../hooks/useBraveInstalled";

// The full onboarding sequence — Dashboard's hero moments (Fix all, Scrub,
// Lockdown, quick toggles) continuing straight through Privacy Settings,
// Secure Storage, and Packages & Apps. See the tour-dashboard membership
// comments in content/guide/topics.ts.
const FIRST_RUN_TOUR_ID = "tour-dashboard";

export default function GuideHost() {
  const { appSettings, startupComplete, patchAppSettings } = useAppState();
  const [steps, setSteps] = useState<TourStep[] | null>(null);
  // True only for the auto-started first-run tour, and only until it has
  // been completed once — SpotlightTour suppresses its own X/Escape while
  // this holds, so a fresh install can't dismiss its way out of the one
  // mandatory walkthrough. Every other invocation (manual replay, or a
  // fresh-install run after hasSeenMandatoryTour is already true) stays
  // fully cancellable.
  const [mandatory, setMandatory] = useState(false);
  const tourAutoStartedRef = useRef(false);

  // Density drives how many stops run (Expert sees fewer). In decoy mode
  // appSettings is null → getDensityForSettings falls back to the calmer
  // Guided set.
  const density = getDensityForSettings(appSettings);
  // Gates the conditional "install Brave" stop — resolveTourSteps drops it
  // once Brave is already on the machine.
  const braveInstalled = useBraveInstalled();

  // Manual tour starts (title bar "?", dashboard "Take the tour", deep
  // links) — always dismissable.
  useEffect(() => {
    const onStart = (e: Event) => {
      const tourId = (e as CustomEvent<{ tourId?: string }>).detail?.tourId ?? "welcome";
      const resolved = resolveTourSteps(GUIDE_TOPICS, tourId, density, { braveInstalled });
      if (resolved.length > 0) {
        setMandatory(false);
        setSteps(resolved);
      }
    };
    window.addEventListener("start-tour", onStart as EventListener);
    return () => window.removeEventListener("start-tour", onStart as EventListener);
  }, [density, braveInstalled]);

  // First launch auto-starts the full spotlight tour instead of the removed
  // Setup Wizard. Closing it does not re-open it during the same app session.
  useEffect(() => {
    const isLoading = !startupComplete;
    const hasSettings = appSettings != null;
    const firstRunComplete = appSettings?.app?.firstRunComplete === true;
    const shouldStart = !isLoading && hasSettings && !firstRunComplete && !tourAutoStartedRef.current;
    if (!shouldStart) return;
    tourAutoStartedRef.current = true;
    const timer = window.setTimeout(() => {
      const resolved = resolveTourSteps(GUIDE_TOPICS, FIRST_RUN_TOUR_ID, density, { braveInstalled });
      if (resolved.length === 0) return;
      setMandatory(appSettings?.app?.hasSeenMandatoryTour !== true);
      setSteps(resolved);
    }, 500);
    return () => window.clearTimeout(timer);
  }, [appSettings, startupComplete, density, braveInstalled]);

  // Publish "a tour is running" for the surfaces that hide a step's anchor
  // outside Expert density or behind a disabled module — see lib/tourActive.ts.
  // Driven off `steps` (not the start/close callbacks) so an unmount mid-tour
  // still clears it.
  const tourRunning = steps !== null && steps.length > 0;
  useEffect(() => {
    setTourActive(tourRunning);
    return () => setTourActive(false);
  }, [tourRunning]);

  const handleClose = useCallback((completed: boolean) => {
    // The mandatory run can only reach onClose via natural completion (its
    // X/Escape are suppressed) — persisting here both marks the tour seen
    // (for future dismissability) and resolves first-run, since the old
    // Setup Wizard was the only other thing that used to set it.
    if (mandatory && completed) {
      void patchAppSettings({ app: { firstRunComplete: true, hasSeenMandatoryTour: true } });
    }
    setSteps(null);
    setMandatory(false);
  }, [mandatory, patchAppSettings]);

  return (
    <>
      {steps && steps.length > 0 && (
        <SpotlightTour steps={steps} onClose={handleClose} dismissable={!mandatory} />
      )}
    </>
  );
}
