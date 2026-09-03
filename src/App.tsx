import {
  lazy,
  Suspense,
  useEffect,
  useState,
  useCallback,
  useRef,
  useMemo,
  type ComponentType,
  type LazyExoticComponent,
} from "react";
// `emit` was only used by the removed `flow-key-press` bridge.
// KeySequenceTrigger now consumes events directly from the system-wide
// keyboard hook in `services::keyboard_hook`.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { motion, MotionConfig } from "framer-motion";
import { showError, showInfo, showWarning } from "./utils/toast";
import { reportSettingsWriteFailure } from "./lib/settingsWriteRecovery";
import useMotionPreference, { MotionPreferenceProvider } from "./hooks/useMotionPreference";
import useLowPerformanceMode from "./hooks/useLowPerformanceMode";
import { panelVariants, panelTransition } from "./components/shared/motion";
import DependencyGate from "./components/DependencyGate";
import SplashScreen from "./components/SplashScreen";
import { shouldSkipStartupSplash } from "./lib/startupMode";
import AppShell from "./components/AppShell";
import { SearchProvider } from "./context/SearchContext";
import BackgroundPollers from "./components/BackgroundPollers";
import CalculatorGate from "./components/startup/CalculatorGate";
import { AuthModeProvider, useAuthMode, type AuthMode } from "./context/AuthModeContext";
import ShredConfirmationDialog from "./components/ShredConfirmationDialog";
import UsbHidApprovalDialog from "./components/shared/UsbHidApprovalDialog";
import RdpIdleWarningDialog from "./components/RdpIdleWarningDialog";
import useRdpIdleDisconnect from "./hooks/useRdpIdleDisconnect";
import useRdpIncomingDismount from "./hooks/useRdpIncomingDismount";
import useRdpIncomingIdleSignout from "./hooks/useRdpIncomingIdleSignout";
import usePasteMonitor, {
  resolveCategories,
  DEFAULT_PASTE_MONITOR_CRYPTO_SWAP_ENABLED,
  DEFAULT_PASTE_MONITOR_AUTO_CLEAR_ENABLED,
  DEFAULT_PASTE_MONITOR_AUTO_CLEAR_SECONDS,
  DEFAULT_PASTE_MONITOR_AUTO_CLEAR_ON_LOCK,
} from "./hooks/usePasteMonitor";
import useDecoyMonitor from "./hooks/useDecoyMonitor";
import useRansomwareMonitor, {
  DEFAULT_RANSOMWARE_THRESHOLD,
  DEFAULT_RANSOMWARE_WINDOW_SECONDS,
  DEFAULT_RANSOMWARE_ALERT_COOLDOWN_SECONDS,
  DEFAULT_RANSOMWARE_ATTRIBUTION_MIN_FILES,
  DEFAULT_RANSOMWARE_ACTION,
} from "./hooks/useRansomwareMonitor";
import useRemoteAccessMonitor from "./hooks/useRemoteAccessMonitor";
import useWifiGuardMonitor, {
  DEFAULT_WIFI_GUARD_ALERT_DEBOUNCE_SECS,
  DEFAULT_WIFI_GUARD_LEARNING_WINDOW_SECS,
  DEFAULT_WIFI_GUARD_POLL_INTERVAL_SECS,
} from "./hooks/useWifiGuardMonitor";
import useAuthAnomalyMonitor, {
  DEFAULT_AUTH_ALERT_DEBOUNCE_SECS,
  DEFAULT_AUTH_FAILED_BURST_THRESHOLD,
  DEFAULT_AUTH_FAILED_BURST_WINDOW_SECS,
  DEFAULT_AUTH_WORK_END_HOUR,
  DEFAULT_AUTH_WORK_DAYS,
  DEFAULT_AUTH_WORK_START_HOUR,
} from "./hooks/useAuthAnomalyMonitor";
import useAcquisitionWatch from "./hooks/useAcquisitionWatch";
import useFleetEpoch from "./hooks/useFleetEpoch";
import useLockdownWords from "./hooks/useLockdownWords";
import useDistressPhrases from "./hooks/useDistressPhrases";
import useEntitlements from "./hooks/useEntitlements";
import { useInvalidateLicense, useLicenseQuery } from "./hooks/queries/useLicenseQuery";
import LicenseGate from "./components/LicenseGate";
import LicenseCelebrationListener from "./components/shared/LicenseCelebrationListener";
import InstallProDialog from "./components/InstallProDialog";
import useProInstall, { isProVersionCompatible, getCachedFreeVersion } from "./hooks/useProInstall";
import { AppProvider, useAppState } from "./context/AppContext";
import { LiveMetricsProvider } from "./context/LiveMetricsContext";
import { TaskStatusProvider } from "./context/TaskStatusContext";
import { OperationOverlay } from "./context/OperationContext";
import { useStartupSound } from "./hooks/useStartupSound";
import { useActivePanelPoller } from "./hooks/useActivePanelPoller";
import { useNetworkTrafficListener } from "./hooks/useNetworkTraffic";
import { useHasActiveWork } from "./hooks/useHasActiveWork";
import PanelSkeleton from "./components/shared/PanelSkeleton";
import FleetSkeleton from "./components/shared/FleetSkeleton";
import PanelErrorBoundary from "./components/PanelErrorBoundary";
import AppToaster from "./components/AppToaster";
import FlowActivityLogger from "./components/FlowActivityLogger";
import GuideHost from "./components/guide/GuideHost";
import UpdateFlowDialog from "./components/UpdateFlowDialog";
import { startUpdaterListener, useUpdater } from "./hooks/updaterStore";
import useAutomaticUpdate from "./hooks/useAutomaticUpdate";
import { PANEL_MANIFESTS, type PanelId } from "./types/panels";
import type { WifiGuardBaselineEntry } from "./types/settings";
import { getModuleForPanel, isModuleEnabled } from "./types/modules";
import { setSoundEnabled } from "./utils/sound";
import { importPanelWithRetry } from "./lib/panelLoading";
import { PanelPrefetchQueue, scheduleWhenIdle } from "./lib/panelPrefetch";
import { isTourActive } from "./lib/tourActive";
import { reportStartupPhase } from "./hooks/startupTrace";
import {
  createStartupProtectionReadiness,
  type StartupProtectionOperation,
  type StartupProtectionReadiness,
} from "./lib/startupProtectionReadiness";
import DashboardPanel from "./panels/dashboard";

// INACTIVITY TIMER: After this amount of no mouse or keyboard activity, 
// we pause all active panel polling to save resources on Rust sysinfo calls,
// smartctl, and any PS-backed panel refreshes.
// Polling resumes on any activity.

const IDLE_PAUSE_MS = 3 * 60 * 1000;

function compareVersionStrings(a: string | null | undefined, b: string | null | undefined): number | null {
  const parse = (value: string | null | undefined): number[] | null => {
    const match = (value ?? "").trim().replace(/^v/i, "").match(/\d+(?:\.\d+){0,3}/);
    if (!match) return null;
    return match[0].split(".").map((part) => Number.parseInt(part, 10));
  };
  const left = parse(a);
  const right = parse(b);
  if (!left || !right) return null;
  const len = Math.max(left.length, right.length);
  for (let i = 0; i < len; i++) {
    const diff = (left[i] ?? 0) - (right[i] ?? 0);
    if (diff !== 0) return diff > 0 ? 1 : -1;
  }
  return 0;
}

// ═══════════════════════════════════════════════════════════════════════
// MANIFEST-DRIVEN PANEL LOADING
// ═══════════════════════════════════════════════════════════════════════
//
// OLD WAY: 12 import functions + 12 lazy() calls + switch-case + preload array
// NEW WAY: One loop over PANEL_MANIFESTS → auto-generates everything
//
// HOW IT WORKS:
//   1. PANEL_MANIFESTS (in types/panels.ts) declares each panel's importFn
//   2. We build a Record<PanelId, LazyComponent> from those manifests here
//   3. renderPanel() just does a lookup instead of a switch-case
//   4. preloadAllPanels() iterates the same manifests
//
// TO ADD A NEW PANEL: Add one entry to PANEL_MANIFESTS. Done.

// React discards hook/memo state when a component suspends during its initial
// mount. Lazy wrappers therefore must live outside PanelRoute; creating one in
// useMemo inside the suspending component causes an endless skeleton loop.
// recoveryGeneration deliberately selects a fresh cached wrapper after Retry.
const LAZY_PANEL_CACHE = new Map<string, LazyExoticComponent<ComponentType>>();

function getLazyPanel(
  panelId: PanelId,
  recoveryGeneration: number,
): LazyExoticComponent<ComponentType> | null {
  const manifest = PANEL_MANIFESTS.find((candidate) => candidate.id === panelId)
    ?? PANEL_MANIFESTS.find((candidate) => candidate.id === "dashboard");
  if (!manifest) return null;

  const cacheKey = `${manifest.id}:${recoveryGeneration}`;
  const cached = LAZY_PANEL_CACHE.get(cacheKey);
  if (cached) return cached;

  const component = lazy(importPanelWithRetry(manifest.importFn));
  LAZY_PANEL_CACHE.set(cacheKey, component);
  return component;
}

function PanelRoute({
  panelId,
  recoveryGeneration,
}: {
  panelId: PanelId;
  recoveryGeneration: number;
}) {
  const manifest = PANEL_MANIFESTS.find((candidate) => candidate.id === panelId)
    ?? PANEL_MANIFESTS.find((candidate) => candidate.id === "dashboard");
  // KT: Dashboard is the initial, always-visible surface. Keeping it out of the
  // lazy-import path prevents a Vite module fetch from replacing the app's
  // main view with a panel error while the remaining panels prefetch.
  if (manifest?.id === "dashboard") return <DashboardPanel />;
  const LazyPanel = getLazyPanel(panelId, recoveryGeneration);
  if (!manifest || !LazyPanel) return null;
  if (manifest.requiresDependency) {
    return (
      <DependencyGate panelId={manifest.id}>
        <LazyPanel />
      </DependencyGate>
    );
  }
  return <LazyPanel />;
}

function AppContent() {
  // Chromium's stock menu exposes developer tooling in packaged builds. Keep
  // it available to the dev server, but suppress the browser menu in releases.
  useEffect(() => {
    if (import.meta.env.DEV) return;
    const suppressBrowserContextMenu = (event: MouseEvent) => event.preventDefault();
    window.addEventListener("contextmenu", suppressBrowserContextMenu);
    return () => window.removeEventListener("contextmenu", suppressBrowserContextMenu);
  }, []);
  const [activePanel, setActivePanel] = useState<PanelId>(() => {
    if (!import.meta.env.DEV) return "dashboard";
    const requested = new URLSearchParams(window.location.search).get("panel") as PanelId | null;
    return requested && PANEL_MANIFESTS.some((panel) => panel.id === requested) ? requested : "dashboard";
  });
  const [panelRecoveryGeneration, setPanelRecoveryGeneration] = useState(0);
  const [splashDone, setSplashDone] = useState(() =>
    shouldSkipStartupSplash(import.meta.env.DEV, window.location.pathname),
  );
  const { playStartupSound } = useStartupSound();
  const [hiddenPanelsUnlocked, setHiddenPanelsUnlocked] = useState(false);
  const [shredPaths, setShredPaths] = useState<string[]>([]);
  const [isShredDialogOpen, setIsShredDialogOpen] = useState(false);
  const { hasPaid, canUpdatePro, isLoading: entitlementLoading } = useEntitlements();

  const canUseDevTools = entitlementLoading || hasPaid;
  // Free-side signal for the combined UpdateFlowDialog auto-trigger below.
  const updaterSnapshot = useUpdater();

  // Resolved motion preference: OS reduce-motion ∪ wc-no-motion toggle ∪ low-profile
  // posture. Drives <MotionConfig> so EVERY framer-motion surface honors it at once.
  const motionPref = useMotionPreference();

  // Low Performance Mode. Resolved once here and threaded into both consumers so
  // animations and polling can never disagree about whether this machine is
  // constrained.
  const lowPerformance = useLowPerformanceMode();

  // KT: We grab the full appState object so manifest-driven hover prefetch
  // can look up refreshKey dynamically, plus we destructure specific fields
  // that are used directly in this component. Declared here (before
  // lockHiddenPanels) so the borrowed-panel redirect can read lockedPanelIds.
  const appState = useAppState();
  const { productivityStatus, appSettings, patchAppSettings, startupComplete, startupDataState, runStartupJob } = appState;
  const panelPrefetchRef = useRef<PanelPrefetchQueue | null>(null);
  const automaticUpdatesEnabled = appSettings?.app?.autoUpdate ?? true;
  useAutomaticUpdate(automaticUpdatesEnabled, canUpdatePro);

  const lockHiddenPanels = useCallback(() => {
    setHiddenPanelsUnlocked(false);
    // Always land on the dashboard when Borrowed Mode engages (owner request) —
    // never leave the borrower on whatever panel was last open. Previously this
    // only redirected when the active panel was itself borrow-hidden, so a
    // non-hidden panel would stay visible after locking.
    setActivePanel('dashboard');
  }, []);

  // Pro install dialog -- triggered by the global `pro-install-open`
  // event from LicenseGate (after activation succeeds + Pro not on disk)
  // and from LicenseQuickPanel's "Install Pro" button. Centralised here
  // so any future surface (paid-cmd error toast, etc) just fires the
  // event without having to mount its own copy.
  //
  // The event detail may carry `{ isStartupNag: true }` when the
  // auto-prompt triggers it. In that case, "Not now" persists the
  // dismissed flag so the nag doesn't fire on the next startup.
  const [proInstallOpen, setProInstallOpen] = useState(false);
  const proInstallIsStartupNagRef = useRef(false);
  useEffect(() => {
    const handler = (e: Event) => {
      if (!hasPaid) {
        window.dispatchEvent(new CustomEvent("license-gate-open", { detail: { tab: "buy" } }));
        return;
      }
      const detail = (e as CustomEvent<{ isStartupNag?: boolean } | undefined>).detail;
      proInstallIsStartupNagRef.current = detail?.isStartupNag === true;
      setProInstallOpen(true);
    };
    window.addEventListener("pro-install-open", handler);
    return () => window.removeEventListener("pro-install-open", handler);
  }, [hasPaid]);

  // Close app-level dialogs when the Pro activation celebration fires so the
  // confetti overlay is fully visible.
  useEffect(() => {
    const handler = () => setProInstallOpen(false);
    window.addEventListener("commander-dismiss-dialogs", handler);
    return () => window.removeEventListener("commander-dismiss-dialogs", handler);
  }, []);

  // Sidebar's Shred button fires `open-shred-dialog` to open the in-app
  // shred dialog with no targets — the user picks files or folders via
  // the dialog's own Add files / Add folder buttons. We can't use the OS
  // picker for this entry-point because Windows native pickers can't mix
  // files and folders in one selection.
  useEffect(() => {
    const handler = () => {
      setShredPaths([]);
      setIsShredDialogOpen(true);
    };
    window.addEventListener("open-shred-dialog", handler);
    return () => window.removeEventListener("open-shred-dialog", handler);
  }, []);

  useEffect(() => {
    const unlockHandler = () => setHiddenPanelsUnlocked(true);
    window.addEventListener("hidden-panels-unlock", unlockHandler);
    window.addEventListener("hidden-panels-lock", lockHiddenPanels);
    return () => {
      window.removeEventListener("hidden-panels-unlock", unlockHandler);
      window.removeEventListener("hidden-panels-lock", lockHiddenPanels);
    };
  }, [lockHiddenPanels]);

  useEffect(() => {
    let unlistenUnlock: (() => void) | undefined;
    let unlistenLock: (() => void) | undefined;
    let unlistenShred: (() => void) | undefined;

    listen("hidden-panels-unlock", () => {
      window.dispatchEvent(new Event("hidden-panels-unlock"));
    }).then((fn) => { unlistenUnlock = fn; }).catch(() => {});
    listen("hidden-panels-lock", () => {
      window.dispatchEvent(new Event("hidden-panels-lock"));
    }).then((fn) => { unlistenLock = fn; }).catch(() => {});
    // Shred can also be triggered from the search-overlay window's context
    // menu (see useSearchResultContextMenu.ts) — that window's own
    // window.dispatchEvent never reaches this one, so it also emits this
    // Tauri cross-window event, rebroadcast here as the same local event
    // the useEffect above already listens for.
    listen("open-shred-dialog", () => {
      window.dispatchEvent(new Event("open-shred-dialog"));
    }).then((fn) => { unlistenShred = fn; }).catch(() => {});

    return () => {
      unlistenUnlock?.();
      unlistenLock?.();
      unlistenShred?.();
    };
  }, []);

  // Global "licence changed" -> invalidate the React Query cache so every
  // useEntitlements consumer (LockedToggle / TierGate / Paywall / sidebar
  // status pill) sees the new state immediately, instead of waiting for
  // the 30s staleTime to expire. Without this, after activation the
  // user's UI stayed gated for 10-15 seconds while the cache went stale
  // on its own. The activate / refresh / deactivate / start-trial
  // call-sites all dispatch `license-updated` already.
  const invalidateLicense = useInvalidateLicense();
  useEffect(() => {
    const handler = () => invalidateLicense();
    window.addEventListener("license-updated", handler);
    return () => window.removeEventListener("license-updated", handler);
  }, [invalidateLicense]);
  const [isIdlePaused, setIsIdlePaused] = useState(false);
  const idleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const { data: licenseStatus, isLoading: isLicenseLoading } = useLicenseQuery();
  const licenseChecked = !isLicenseLoading;

  // Auto-open the combined UpdateFlowDialog at startup, in either of two
  // cases: (1) the Free background scheduler already found/staged an update
  // (updater.phase), or (2) the licence is valid/trial and Pro either isn't
  // installed yet or is behind the manifest. Ref guards make each case fire at
  // most once per running process — the dialog itself then runs Free first
  // and, only if paid, continues straight into Pro in the same popup (see
  // UpdateFlowDialog / useUpdateFlow). This replaces the old separate
  // RestartPrompt toast + Pro-only startup nag with one surface.
  //
  // KT (announce-once): this effect only evaluates at actual app launch (this
  // component mounts fresh on cold start AND on the relaunch after installing
  // — the window HIDES rather than closing, so a tray/hotkey reveal of the
  // SAME running process never remounts it and never re-runs this effect).
  // The ref guards alone would still re-announce the same pending version on
  // every later launch of the same still-not-installed update, so the
  // detected version (see freeKey/proKey below) is also compared against the
  // persisted `lastAnnouncedUpdateVersion` and only opens the dialog — and
  // re-persists — when it's actually different.
  //
  // KT: If the user previously dismissed the Pro-not-installed nag with "Not
  // now" we skip that PASSIVE trigger (proInstallPromptDismissed=true) — but
  // a Free update becoming available still opens the dialog regardless (Free
  // updates aren't gated by that flag), and it'll just skip the Pro leg for
  // an unpaid/dismissed user the same way the old startup nag would have.
  //
  // KT (manifest source): reads the SAME shared useProInstall() store the
  // Settings "Versions & Updates" card and the combined dialog's Pro step
  // use, instead of firing its own separate fetch_pro_manifest invoke
  // against a hardcoded /pro/latest.json. Previously this effect duplicated
  // the manifest fetch entirely and bypassed useProInstall's version-pinned
  // "tested pairs" path (/pro/v<freeVersion>/latest.json, falling back to
  // /pro/latest.json) — two independent sources of truth for "is Pro up to
  // date" that could silently disagree once the version-pinned path actually
  // resolves to something (it was defeated by a manifest-filename bug on the
  // publish side until that was fixed).
  const proInstallPromptDismissed = appSettings?.app?.proInstallPromptDismissed ?? false;
  // Version key (e.g. "free:3.0.10") the auto-open effect below last announced.
  // Compared against the freshly-detected key so a still-pending update (same
  // version) evaluated again at a later launch doesn't re-announce itself —
  // see the effect for how this is kept in sync with reality.
  const lastAnnouncedUpdateVersion = appSettings?.app?.lastAnnouncedUpdateVersion ?? null;
  const hasStartupProEntitlement = !!licenseStatus && (
    (licenseStatus.licensed && licenseStatus.valid) || licenseStatus.trial_active === true
  );
  const shouldProbeProPrompt =
    !automaticUpdatesEnabled &&
    licenseChecked &&
    hasStartupProEntitlement &&
    canUpdatePro &&
    !import.meta.env.DEV &&
    !proInstallPromptDismissed;
  const proInstall = useProInstall({
    status: shouldProbeProPrompt,
    manifest: shouldProbeProPrompt,
    defender: false,
  });
  const [updateFlowOpen, setUpdateFlowOpen] = useState(false);
  const updateFlowIsStartupNagRef = useRef(false);
  const updateFlowAutoPromptedRef = useRef(false);
  const freeUpdatePromptedRef = useRef(false);
  useEffect(() => {
    if (automaticUpdatesEnabled) return;
    // Free side: the background scheduler has something to do. This has its OWN
    // guard (checked ABOVE the Pro guard) because the Rust updater has a ~30s
    // initial delay before its first check, while the Pro status/manifest probe
    // resolves in a fraction of a second and latches updateFlowAutoPromptedRef
    // (e.g. "Pro is up-to-date"). With a single shared guard, that early Pro
    // latch permanently starved the Free popup the moment a Free update later
    // became available — the "no update popup on launch" bug.
    if (
      !freeUpdatePromptedRef.current &&
      (updaterSnapshot.phase === "staged" || updaterSnapshot.phase === "available" || updaterSnapshot.phase === "ready")
    ) {
      freeUpdatePromptedRef.current = true;
      // Key the "already announced" check to the actual version so a NEW
      // release still auto-opens even though the last one was already shown.
      const freeKey = updaterSnapshot.version ? `free:${updaterSnapshot.version}` : null;
      if (!freeKey || freeKey !== lastAnnouncedUpdateVersion) {
        updateFlowAutoPromptedRef.current = true;
        updateFlowIsStartupNagRef.current = false;
        if (freeKey) patchAppSettings({ app: { lastAnnouncedUpdateVersion: freeKey } }).catch(reportSettingsWriteFailure);
        setUpdateFlowOpen(true);
        return;
      }
      // Already announced this exact version (this launch or an earlier one)
      // — fall through to the Pro check instead of reopening the same dialog.
    }
    // Pro side: same detection the old standalone nag used, fired at most once.
    if (updateFlowAutoPromptedRef.current) return;
    if (!licenseChecked || !licenseStatus) return;
    if (!hasStartupProEntitlement) return;
    if (!canUpdatePro) {
      updateFlowAutoPromptedRef.current = true;
      return;
    }
    // Dev builds must run only against the local repo-built Pro binary. Do not
    // fetch the manifest or prompt update/repair from the public manifest.
    if (import.meta.env.DEV) {
      updateFlowAutoPromptedRef.current = true;
      return;
    }
    // Skip passive startup nag when the user already dismissed it once.
    if (proInstallPromptDismissed) {
      updateFlowAutoPromptedRef.current = true;
      return;
    }
    // Wait for the shared store's install-status probe to land before
    // deciding anything (it fetches on mount from useProInstall() itself,
    // possibly already in flight via the always-mounted UpdateFlowDialog).
    const s = proInstall.status;
    if (!s) return;
    const manifest = proInstall.manifest;
    if (!s.installed) {
      // Not yet installed: wait for the manifest and apply the same version-
      // compatibility gate used by the already-installed update path below.
      // This prevents a 3.0.9 Free from being pushed a 3.0.10 Pro binary.
      if (!manifest) {
        if (proInstall.manifestError) updateFlowAutoPromptedRef.current = true;
        return;
      }
      if (!isProVersionCompatible(manifest.version, getCachedFreeVersion())) {
        console.warn(
          `[ProAutoInstall] Manifest Pro ${manifest.version} is ahead of Free ` +
          `${getCachedFreeVersion() ?? "unknown"}; skipping startup prompt.`,
        );
        updateFlowAutoPromptedRef.current = true;
        return;
      }
      const proKey = `pro:${manifest.version}`;
      if (proKey === lastAnnouncedUpdateVersion) {
        updateFlowAutoPromptedRef.current = true;
        return;
      }
      updateFlowAutoPromptedRef.current = true;
      updateFlowIsStartupNagRef.current = true;
      patchAppSettings({ app: { lastAnnouncedUpdateVersion: proKey } }).catch(reportSettingsWriteFailure);
      setUpdateFlowOpen(true);
      return;
    }
    // Already installed: if the manifest advertises a newer or
    // unverifiable Pro EXE, surface the same combined dialog the
    // Settings card uses. Keep the consent/repair path visible instead
    // of silently overwriting the sidecar in the background.
    if (!s.local_sha256) { updateFlowAutoPromptedRef.current = true; return; }
    if (!manifest) {
      if (proInstall.manifestError) updateFlowAutoPromptedRef.current = true;
      return;
    }
    if (manifest.sha256.toLowerCase() === s.local_sha256.toLowerCase()) {
      updateFlowAutoPromptedRef.current = true;
      return;
    }
    const versionOrder = compareVersionStrings(manifest.version, s.local_version);
    if (versionOrder !== null && versionOrder !== 1) {
      console.warn(
        `[ProAutoUpdate] Pro hash differs but manifest is not newer ` +
        `(local=${s.local_version ?? "unknown"}, manifest=${manifest.version}); skipping automatic prompt.`,
      );
      updateFlowAutoPromptedRef.current = true;
      return;
    }
    // KT: Don't auto-force a Pro that's ahead of the running Free
    // major.minor (e.g. a 3.0.9 Free must not be pushed a 3.0.10 Pro).
    // User can update manually via Settings.
    if (!isProVersionCompatible(manifest.version, getCachedFreeVersion())) {
      console.warn(
        `[ProAutoUpdate] Manifest Pro ${manifest.version} is ahead of Free ` +
        `${getCachedFreeVersion() ?? "unknown"}; skipping startup prompt.`,
      );
      updateFlowAutoPromptedRef.current = true;
      return;
    }
    const proKey = `pro:${manifest.version}`;
    if (proKey === lastAnnouncedUpdateVersion) {
      updateFlowAutoPromptedRef.current = true;
      return;
    }
    console.log(`[ProAutoUpdate] Prompting Pro update/repair ${s.local_version ?? "unknown"} -> ${manifest.version}`);
    updateFlowAutoPromptedRef.current = true;
    updateFlowIsStartupNagRef.current = false;
    patchAppSettings({ app: { lastAnnouncedUpdateVersion: proKey } }).catch(reportSettingsWriteFailure);
    setUpdateFlowOpen(true);
  }, [
    updaterSnapshot.phase,
    updaterSnapshot.version,
    licenseChecked,
    licenseStatus,
    hasStartupProEntitlement,
    canUpdatePro,
    proInstallPromptDismissed,
    proInstall.status,
    proInstall.manifest,
    proInstall.manifestError,
    lastAnnouncedUpdateVersion,
    patchAppSettings,
    automaticUpdatesEnabled,
  ]);

  // RDP idle disconnect (Idle Session Monitor) -- runs globally so the
  // warning dialog appears on any panel. Paid feature: gated by
  // useEntitlements so a free user with the toggle persisted from a
  // prior trial doesn't keep getting auto-disconnected.
  const rdpIdleEnabledIdeal = appSettings?.ideal?.privacy?.tracking?.rdpIdleDisconnectEnabled ?? false;
  const rdpIdleEnabled   = hasPaid && rdpIdleEnabledIdeal;
  const rdpIdleTimeout   = appSettings?.ideal?.privacy?.tracking?.rdpIdleDisconnectTimeout ?? 120;
  const rdpIdleWarningSeconds = Math.max(5, appSettings?.ideal?.privacy?.tracking?.rdpIdleWarningSeconds ?? 5);
  const rdpClearCache    = appSettings?.ideal?.privacy?.tracking?.rdpClearCacheOnDisconnect ?? true;
  const rdpRemoveCreds   = appSettings?.ideal?.privacy?.tracking?.rdpRemoveCredsOnDisconnect ?? false;
  const rdpDismountVaultsCfg = appSettings?.ideal?.privacy?.tracking?.rdpDismountVaultsOnDisconnect ?? false;
  const rdpIncomingDismountCfg = appSettings?.ideal?.tweaks?.rdp?.incomingDismountOnEmpty ?? false;
  const rdpIncomingSignOffOnDisconnectCfg = appSettings?.ideal?.tweaks?.rdp?.incomingSignOffOnDisconnect ?? false;
  const rdpIdleDisabledReason = !rdpIdleEnabledIdeal
    ? "toggle off"
    : entitlementLoading
      ? "waiting for entitlement check"
      : !hasPaid
        ? "paid entitlement missing"
        : "disabled";
  const rdpIdle = useRdpIdleDisconnect(
    rdpIdleEnabled,
    rdpIdleTimeout,
    rdpIdleWarningSeconds,
    rdpClearCache,
    rdpRemoveCreds,
    false,
    rdpDismountVaultsCfg,
    rdpIdleDisabledReason,
  );
  // RDP Incoming idle sign-out — app-side enforcement. Watches each incoming
  // session's idle time and signs it off at the configured threshold (the
  // Group Policy set on enable only enforces at ~1-min resolution).
  const rdpIncomingIdleEnabledCfg = appSettings?.ideal?.tweaks?.rdp?.incomingIdleTimeoutEnabled ?? false;
  const rdpIncomingIdleSeconds = appSettings?.ideal?.tweaks?.rdp?.incomingIdleTimeoutSeconds
    ?? ((appSettings?.ideal?.tweaks?.rdp?.incomingIdleTimeoutMinutes ?? 15) * 60);
  useRdpIncomingDismount(
    hasPaid && rdpIncomingIdleEnabledCfg && (rdpIncomingDismountCfg || rdpIncomingSignOffOnDisconnectCfg),
    rdpIncomingDismountCfg,
    rdpIncomingSignOffOnDisconnectCfg,
  );
  useRdpIncomingIdleSignout(hasPaid && rdpIncomingIdleEnabledCfg, rdpIncomingIdleSeconds, rdpIncomingDismountCfg);

  useEffect(() => {
    console.log("[RdpIdle] Gate", {
      enabled: rdpIdleEnabled,
      toggle: rdpIdleEnabledIdeal,
      hasPaid,
      entitlementLoading,
      timeoutSeconds: rdpIdleTimeout,
      reason: rdpIdleEnabled ? "started" : rdpIdleDisabledReason,
    });
  }, [rdpIdleEnabled, rdpIdleEnabledIdeal, hasPaid, entitlementLoading, rdpIdleTimeout, rdpIdleDisabledReason]);

  // This launch-only aggregate deliberately follows real privileged rearm
  // completions below. Settings hydration, listeners, and inactive controls
  // do not make a protection "ready".
  const startupProtectionReadinessRef = useRef<StartupProtectionReadiness | null>(null);
  if (!startupProtectionReadinessRef.current) {
    startupProtectionReadinessRef.current = createStartupProtectionReadiness(reportStartupPhase);
  }
  const startupProtectionConfiguredRef = useRef(false);
  const reportStartupProtectionRearm = useCallback((operation: StartupProtectionOperation, succeeded: boolean) => {
    startupProtectionReadinessRef.current?.report(operation, succeeded);
  }, []);
  const configuredStartupProtectionOperations = useMemo<readonly StartupProtectionOperation[] | null>(() => {
    if (!appSettings || entitlementLoading) return null;
    const privacy = appSettings.ideal?.privacy;
    const usb = privacy?.usbSecurity;
    const paidUsbEnabled = hasPaid && (
      usb?.hidGuardEnabled === true
      || usb?.meteringEnabled === true
      || usb?.autoSandboxEnabled === true
      || usb?.hidApprovalGateEnabled === true
    );
    return [
      ...(hasPaid && privacy?.decoyMonitor?.enabled === true ? ["decoy-monitor" as const] : []),
      ...(privacy?.ransomwareMonitor?.enabled === true ? ["ransomware-monitor" as const] : []),
      ...(hasPaid && privacy?.remoteAccessMonitor?.enabled === true ? ["remote-access-monitor" as const] : []),
      ...(usb && (usb.monitorEnabled === true || paidUsbEnabled) ? ["usb-security" as const] : []),
      ...(hasPaid && appSettings.ideal?.network?.wifiGuard?.enabled === true ? ["wifi-guard" as const] : []),
      ...(hasPaid && privacy?.authAnomalyMonitor?.enabled === true ? ["auth-anomaly-monitor" as const] : []),
      ...(hasPaid && privacy?.screenCapture?.detectionEnabled === true ? ["screen-capture-watch" as const] : []),
    ];
  }, [appSettings, entitlementLoading, hasPaid]);
  useEffect(() => {
    if (startupProtectionConfiguredRef.current || !configuredStartupProtectionOperations) return;
    startupProtectionConfiguredRef.current = true;
    startupProtectionReadinessRef.current?.configure(configuredStartupProtectionOperations);
  }, [configuredStartupProtectionOperations]);

  // F-1 Paste monitor — clipboard credential watcher driven off the
  // privacy.clipboard settings group. Free feature; the Rust side is
  // idempotent so a duplicate start is harmless. Toast handler for the
  // `paste-monitor-detected` event lives in BackgroundPollers.tsx
  // alongside the other global Tauri listeners.
  const pasteMonitorEnabled = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorEnabled ?? false;
  const pasteMonitorCategories = resolveCategories(
    appSettings?.ideal?.privacy?.clipboard?.pasteMonitorCategories,
  );
  const pasteMonitorCryptoSwap = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorCryptoSwapEnabled
    ?? DEFAULT_PASTE_MONITOR_CRYPTO_SWAP_ENABLED;
  const pasteMonitorAutoClearEnabled = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorAutoClearEnabled
    ?? DEFAULT_PASTE_MONITOR_AUTO_CLEAR_ENABLED;
  const pasteMonitorAutoClearSeconds = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorAutoClearSeconds
    ?? DEFAULT_PASTE_MONITOR_AUTO_CLEAR_SECONDS;
  const pasteMonitorAutoClearOnLock = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorAutoClearOnLock
    ?? DEFAULT_PASTE_MONITOR_AUTO_CLEAR_ON_LOCK;
  usePasteMonitor(
    pasteMonitorEnabled,
    pasteMonitorCategories,
    pasteMonitorCryptoSwap,
    pasteMonitorAutoClearEnabled,
    pasteMonitorAutoClearSeconds,
    pasteMonitorAutoClearOnLock,
  );

  // F-2 Decoy file monitor — filesystem honeypots driven off
  // privacy.decoyMonitor settings. Reconciliation hook handles
  // start/stop + per-path enroll/remove.
  const decoyEnabled = appSettings?.ideal?.privacy?.decoyMonitor?.enabled ?? false;
  const decoyEnrolledPaths = appSettings?.ideal?.privacy?.decoyMonitor?.enrolledPaths ?? [];
  const decoyReadAuditEnabled = appSettings?.ideal?.privacy?.decoyMonitor?.readAuditEnabled ?? false;
  const decoyFleetAlertEnabled = appSettings?.ideal?.privacy?.decoyMonitor?.fleetAlertEnabled ?? false;
  // Filesystem decoys are an organisation-facing tripwire. Do not leave a
  // persisted trial setting armed after the licence expires.
  useDecoyMonitor(hasPaid && decoyEnabled,
    !hasPaid && decoyEnabled,
    decoyEnrolledPaths,
    decoyReadAuditEnabled,
    decoyFleetAlertEnabled,
    entitlementLoading,
    hasPaid && decoyEnabled ? reportStartupProtectionRearm : undefined,
  );

  // F-3 Anti-ransomware monitor — mass-modify detector over user-content
  // dirs. Hook syncs threshold/window first then start/stop watcher.
  const ransomwareEnabled = appSettings?.ideal?.privacy?.ransomwareMonitor?.enabled ?? false;
  const ransomwareThreshold = appSettings?.ideal?.privacy?.ransomwareMonitor?.threshold ?? DEFAULT_RANSOMWARE_THRESHOLD;
  const ransomwareWindowSeconds = appSettings?.ideal?.privacy?.ransomwareMonitor?.windowSeconds ?? DEFAULT_RANSOMWARE_WINDOW_SECONDS;
  const ransomwareAlertCooldownSeconds = appSettings?.ideal?.privacy?.ransomwareMonitor?.alertCooldownSeconds ?? DEFAULT_RANSOMWARE_ALERT_COOLDOWN_SECONDS;
  const ransomwareAttributionMinFiles = appSettings?.ideal?.privacy?.ransomwareMonitor?.attributionMinFiles ?? DEFAULT_RANSOMWARE_ATTRIBUTION_MIN_FILES;
  const ransomwareCustomDirs = appSettings?.ideal?.privacy?.ransomwareMonitor?.customWatchDirs ?? [];
  const ransomwareAction = appSettings?.ideal?.privacy?.ransomwareMonitor?.action ?? DEFAULT_RANSOMWARE_ACTION;
  useRansomwareMonitor(
    ransomwareEnabled,
    hasPaid,
    ransomwareThreshold,
    ransomwareWindowSeconds,
    ransomwareAlertCooldownSeconds,
    ransomwareAttributionMinFiles,
    ransomwareCustomDirs,
    ransomwareAction,
    ransomwareEnabled ? reportStartupProtectionRearm : undefined,
  );

  // #4 Remote-access monitor — paid Pro-sidecar detector. Hook pushes
  // per-tool overrides then start/stop; Pro + Free wrapper enforce paid.
  const remoteAccessEnabled = appSettings?.ideal?.privacy?.remoteAccessMonitor?.enabled ?? false;
  const remoteAccessTools = appSettings?.ideal?.privacy?.remoteAccessMonitor?.tools ?? null;
  useRemoteAccessMonitor(
    remoteAccessEnabled,
    remoteAccessTools,
    hasPaid && remoteAccessEnabled ? reportStartupProtectionRearm : undefined,
  );

  // Free owns only the basic attach timeline. Pro owns HID timing intelligence
  // and automatic isolation. Reconcile them separately so a Free user keeps
  // their simple timeline while an expired entitlement can still deactivate
  // already-running paid monitors.
  const usbSecurity = appSettings?.ideal?.privacy?.usbSecurity;
  const usbMonitorEnabled = usbSecurity?.monitorEnabled === true;
  const usbHidGuardEnabled = usbSecurity?.hidGuardEnabled === true;
  const usbMeteringEnabled = usbSecurity?.meteringEnabled === true;
  const usbAutoSandboxEnabled = usbSecurity?.autoSandboxEnabled === true;
  const usbHidApprovalGateEnabled = usbSecurity?.hidApprovalGateEnabled === true;
  const usbHidApprovalTtlSecs = usbSecurity?.hidApprovalTtlSecs;
  const usbSecurityConfigured = usbSecurity !== undefined;
  const usbRearmFailureRef = useRef<string | null>(null);
  useEffect(() => {
    // A missing legacy setting must not leave a previously armed Pro monitor
    // alive after entitlement expiry. Wait while entitlement is unresolved, but
    // otherwise run the deactivation path even on an untouched settings file.
    if (!usbSecurityConfigured && (entitlementLoading || hasPaid)) return;
    let cancelled = false;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let attempt = 0;
    const paidMonitorDesired = usbHidGuardEnabled
      || usbMeteringEnabled
      || usbAutoSandboxEnabled
      || usbHidApprovalGateEnabled;
    const basicMonitorDesired = usbMonitorEnabled || (hasPaid && paidMonitorDesired);
    const reconcile = async () => {
      try {
        await invoke(basicMonitorDesired ? "start_usb_monitor" : "stop_usb_monitor");

        if (entitlementLoading) {
          usbRearmFailureRef.current = null;
          return;
        }
        if (hasPaid) {
          // Pro's ProgramData state is canonical. Legacy AppSettings are used
          // only if that Pro state does not exist yet, so a stale RDS process
          // cannot overwrite a policy changed in another session.
          await invoke("reconcile_usb_guard", {
            legacy: {
              monitorEnabled: usbMonitorEnabled,
              hidGuardEnabled: usbHidGuardEnabled,
              meteringEnabled: usbMeteringEnabled,
              autoSandboxEnabled: usbAutoSandboxEnabled,
              hidApprovalGateEnabled: usbHidApprovalGateEnabled,
              hidApprovalTtlSecs: usbHidApprovalTtlSecs,
            },
          });
        } else {
          const pro = await invoke<{ installed?: boolean }>("get_pro_install_status");
          if (pro?.installed) {
            // Fixed deactivation-only feature IDs remain available after an
            // entitlement expires. Run them even on a fresh app process so a
            // persisted or orphaned Pro policy cannot remain armed.
            await invoke("stop_usb_autosandbox");
            await invoke("stop_usb_metering");
            await invoke("stop_usb_hid_guard");
            await invoke("stop_usb_hid_approval_gate");
          }
        }
        if (cancelled) return;
        usbRearmFailureRef.current = null;
        if (basicMonitorDesired || (hasPaid && paidMonitorDesired)) {
          reportStartupProtectionRearm("usb-security", true);
        }
      } catch (err) {
        if (cancelled) return;
        const message = String(err);
        if (usbRearmFailureRef.current !== message) {
          usbRearmFailureRef.current = message;
          showWarning("USB security monitors could not fully re-arm. WinCommander will retry automatically.", 12_000);
        }
        attempt += 1;
        if (attempt < 3) {
          retryTimer = setTimeout(() => { void reconcile(); }, attempt * 5_000);
        } else if (basicMonitorDesired || (hasPaid && paidMonitorDesired)) {
          reportStartupProtectionRearm("usb-security", false);
        }
      }
    };
    void reconcile();
    return () => {
      cancelled = true;
      if (retryTimer) clearTimeout(retryTimer);
    };
  }, [
    entitlementLoading,
    hasPaid,
    usbSecurityConfigured,
    usbMonitorEnabled,
    usbHidGuardEnabled,
    usbMeteringEnabled,
    usbAutoSandboxEnabled,
    usbHidApprovalGateEnabled,
    usbHidApprovalTtlSecs,
    reportStartupProtectionRearm,
  ]);

  // Wi-Fi Guard retains trusted SSID/BSSID observations in the local settings
  // file, then rehydrates/re-arms the Pro detector after an app restart. Its
  // baseline never travels in a Fleet event.
  const wifiGuard = appSettings?.ideal?.network?.wifiGuard;
  const wifiGuardEnabled = wifiGuard?.enabled ?? false;
  const wifiGuardLearningWindowSecs = wifiGuard?.learningWindowSecs
    ?? DEFAULT_WIFI_GUARD_LEARNING_WINDOW_SECS;
  const wifiGuardLearningUntil = wifiGuard?.learningUntil ?? null;
  const wifiGuardPollIntervalSecs = wifiGuard?.pollIntervalSecs
    ?? DEFAULT_WIFI_GUARD_POLL_INTERVAL_SECS;
  const wifiGuardAlertDebounceSecs = wifiGuard?.alertDebounceSecs
    ?? DEFAULT_WIFI_GUARD_ALERT_DEBOUNCE_SECS;
  const wifiGuardBaseline = wifiGuard?.baseline ?? [];
  const persistWifiGuardBaseline = useCallback((baseline: WifiGuardBaselineEntry[], learningUntil: string | null) => {
    patchAppSettings({ ideal: { network: { wifiGuard: { baseline, learningUntil } } } }).catch(reportSettingsWriteFailure);
  }, [patchAppSettings]);
  useWifiGuardMonitor(
    wifiGuardEnabled,
    {
      learningWindowSecs: wifiGuardLearningWindowSecs,
      learningUntil: wifiGuardLearningUntil,
      pollIntervalSecs: wifiGuardPollIntervalSecs,
      alertDebounceSecs: wifiGuardAlertDebounceSecs,
    },
    wifiGuardBaseline,
    persistWifiGuardBaseline,
    hasPaid && wifiGuardEnabled ? reportStartupProtectionRearm : undefined,
  );

  const authAnomaly = appSettings?.ideal?.privacy?.authAnomalyMonitor;
  const authAnomalyEnabled = authAnomaly?.enabled ?? false;
  const authAnomalyWorkDays = authAnomaly?.workDays;
  const authAnomalyPolicy = useMemo(() => ({
    failedBurstThreshold: authAnomaly?.failedBurstThreshold ?? DEFAULT_AUTH_FAILED_BURST_THRESHOLD,
    failedBurstWindowSecs: authAnomaly?.failedBurstWindowSecs ?? DEFAULT_AUTH_FAILED_BURST_WINDOW_SECS,
    workStartHour: authAnomaly?.workStartHour ?? DEFAULT_AUTH_WORK_START_HOUR,
    workEndHour: authAnomaly?.workEndHour ?? DEFAULT_AUTH_WORK_END_HOUR,
    workDays: authAnomalyWorkDays ?? [...DEFAULT_AUTH_WORK_DAYS],
    timeBasis: authAnomaly?.timeBasis ?? "local",
    detectRdp: authAnomaly?.detectRdp ?? true,
    detectNewAccounts: authAnomaly?.detectNewAccounts ?? true,
    detectOffHours: authAnomaly?.detectOffHours ?? true,
    alertDebounceSecs: authAnomaly?.alertDebounceSecs ?? DEFAULT_AUTH_ALERT_DEBOUNCE_SECS,
    reportToFleet: authAnomaly?.reportToFleet ?? true,
  }), [
    authAnomaly?.failedBurstThreshold,
    authAnomaly?.failedBurstWindowSecs,
    authAnomaly?.workStartHour,
    authAnomaly?.workEndHour,
    authAnomalyWorkDays,
    authAnomaly?.timeBasis,
    authAnomaly?.detectRdp,
    authAnomaly?.detectNewAccounts,
    authAnomaly?.detectOffHours,
    authAnomaly?.alertDebounceSecs,
    authAnomaly?.reportToFleet,
  ]);
  useAuthAnomalyMonitor(
    authAnomalyEnabled,
    authAnomalyPolicy,
    hasPaid,
    appSettings?.ideal?.security?.requireAllDeviceAlertsInFleet === true,
    hasPaid && authAnomalyEnabled ? reportStartupProtectionRearm : undefined,
  );

  // Anti-Acquisition Defenses: continuous WARN-mode watcher — polls the
  // existing read-only Scan-AcquisitionThreats detector; warns only, no
  // auto-lockdown. Paid.
  const acquisitionWatchEnabled = appSettings?.ideal?.privacy?.acquisitionWatchEnabled ?? false;
  useAcquisitionWatch(acquisitionWatchEnabled, hasPaid);

  // Fleet epoch poller — applies any pending signed policy epoch from the Pro
  // sidecar and invalidates the settings cache so panels refresh immediately.
  const fleetEnabled = appSettings?.app?.fleet?.enabled ?? false;
  useFleetEpoch(fleetEnabled);

  // #5 WDA_EXCLUDEFROMCAPTURE is per-HWND runtime state — it does not survive a
  // restart, so re-apply it from the persisted preference. Apply the CURRENT
  // value in BOTH directions (not just `true`, not just once) so a change
  // pushed by the fleet (a ConfigEpoch that sets ideal.privacy.screenCapture.
  // protectWindow) takes effect on the running app immediately — enabling AND
  // disabling — rather than only on the next restart. Best-effort: swallow
  // errors (unlicensed / pre-19041) so startup never blocks.
  useEffect(() => {
    const desired = appSettings?.ideal?.privacy?.screenCapture?.protectWindow;
    if (desired === undefined) return;
    void invoke("set_capture_protection", { enabled: desired === true }).catch(() => {});
  }, [appSettings?.ideal?.privacy?.screenCapture?.protectWindow]);

  // Detection is also sidecar runtime state, so a saved ON preference must
  // re-arm after every app/sidecar restart. The panel toggle remains the
  // interactive control; this reconciler makes persistence truthful.
  const screenCaptureRearmFailureRef = useRef<string | null>(null);
  useEffect(() => {
    if (entitlementLoading) return;
    const desired = appSettings?.ideal?.privacy?.screenCapture?.detectionEnabled;
    if (desired === undefined) return;
    if (!hasPaid && desired === true) return;
    let cancelled = false;
    const command = desired === true
      ? "start_screen_capture_watch"
      : "stop_screen_capture_watch";
    void invoke(command)
      .then(() => {
        if (cancelled) return;
        screenCaptureRearmFailureRef.current = null;
        if (desired === true) reportStartupProtectionRearm("screen-capture-watch", true);
      })
      .catch((err) => {
        if (cancelled) return;
        if (desired !== true) return;
        reportStartupProtectionRearm("screen-capture-watch", false);
        const message = String(err);
        if (screenCaptureRearmFailureRef.current === message) return;
        screenCaptureRearmFailureRef.current = message;
        showWarning("Screen-capture detection could not re-arm. Open Privacy → Screen capture to retry.", 12_000);
      });
    return () => { cancelled = true; };
  }, [appSettings?.ideal?.privacy?.screenCapture?.detectionEnabled, entitlementLoading, hasPaid, reportStartupProtectionRearm]);

  // F-5 Coercion code-phrase trigger — paid. The hook gates on hasPaid
  // and the Rust start command also enforces require_paid.
  const coercionEnabled = appSettings?.ideal?.privacy?.coercionPhrase?.enabled ?? false;
  const coercionPhrasesList = appSettings?.ideal?.privacy?.coercionPhrase?.phrases ?? [];
  useLockdownWords(coercionEnabled, coercionPhrasesList, hasPaid);
  // Distress phrase sync — keeps DISTRESS_REGISTERED in Rust up to date.
  // Pass appSettings directly; the hook itself skips syncing while in decoy
  // mode (appSettings is null there), so the phrases armed before the switch
  // stay live instead of being wiped to [].
  useDistressPhrases(appSettings, hasPaid);

  // Distress phrase keyboard-hook (method C) — fire decoy/destroy from
  // a system-wide typed phrase while the app is already running.
  const { setMode: setAuthMode } = useAuthMode();
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    import("@tauri-apps/api/event").then(({ listen }) => {
      listen<{ mode: string }>("distress-phrase-fired", (ev) => {
        if (ev.payload.mode === "decoy") {
          setAuthMode("decoy");
        } else if (ev.payload.mode === "reboot_usb") {
          // F6 reboot-to-USB wipe: the Rust distress handler already spawned
          // the orchestrator in-process (see shortcut_actions.rs).
          // Nothing for the frontend to do except acknowledge — the orchestrator
          // runs gate + stage-1 + stage-2 and reboots or logs the error itself.
          // We do NOT invoke a separate Tauri command here to keep the
          // orchestrator unreachable as a free-standing IPC endpoint (SAFETY rule 3).
        }
      }).then((fn) => { unlisten = fn; });
    });
    return () => { unlisten?.(); };
  }, [setAuthMode]);

  // KT: Active-panel polling — replaces the old "poll everything always" approach.
  // Only the currently viewed panel gets periodic refreshes (10s interval).
  // See useActivePanelPoller.ts for full behavior.
  //
  // `paused` is load-bearing, not decorative: the 10s panel refresh spawns a COLD
  // powershell.exe per tick (backend.rs build_powershell_command — no runspace reuse,
  // no cache), and the hook has honoured a `paused` flag since it was written. It was
  // simply never passed, so idle sessions kept spawning PowerShell every 10s while the
  // UI displayed the "Resource pause mode active due to inactivity" overlay. That is
  // wasteful on a desktop and multiplies per session on a multi-user server, where
  // several idle-but-logged-in RDP sessions each pay it forever.
  //
  // Low Performance Mode pauses the same polling permanently rather than only when
  // idle — that PowerShell spawn is the dominant cost on a constrained or heavily
  // shared box, so switching off animations alone would not deliver what the user
  // asked for when they enabled it.
  useActivePanelPoller({ activePanel, paused: isIdlePaused || lowPerformance.active });
  useNetworkTrafficListener();

  // Pause must NOT engage while a task is running (app install, applying
  // toggles, self-destruct, etc). hasActiveWork combines TaskStatusContext
  // running operations with in-flight run_backend_script invokes; either
  // signal blocks pause arming and immediately un-pauses if work starts
  // while the app is already idle-paused.
  const hasActiveWork = useHasActiveWork();

  const armIdleTimer = useCallback(() => {
    if (idleTimerRef.current) {
      clearTimeout(idleTimerRef.current);
    }
    if (hasActiveWork) return; // never arm while busy
    idleTimerRef.current = setTimeout(() => {
      setIsIdlePaused(true);
    }, IDLE_PAUSE_MS);
  }, [hasActiveWork]);

  // If work begins while we're already paused, resume immediately so the
  // user-triggered task isn't running behind the "Resource pause" overlay.
  useEffect(() => {
    if (hasActiveWork && isIdlePaused) {
      setIsIdlePaused(false);
    }
  }, [hasActiveWork, isIdlePaused]);

  useEffect(() => {
    const onActivity = () => {
      if (isIdlePaused) {
        setIsIdlePaused(false);
      }
      armIdleTimer();
    };

    const activityEvents: Array<keyof WindowEventMap> = [
      "mousemove",
      "mousedown",
      "keydown",
      "wheel",
      "touchstart",
      "pointerdown",
    ];

    activityEvents.forEach((eventName) => {
      window.addEventListener(eventName, onActivity, { passive: true });
    });

    armIdleTimer();

    return () => {
      activityEvents.forEach((eventName) => {
        window.removeEventListener(eventName, onActivity as EventListener);
      });
      if (idleTimerRef.current) {
        clearTimeout(idleTimerRef.current);
      }
    };
  }, [armIdleTimer, isIdlePaused]);

  // Removed 2026-05: the `flow-key-press` Tauri-event bridge was the
  // KeySequenceTrigger's only data source. It had two crippling bugs:
  //   1. Only fired when WC had foreground focus (the keydown listener
  //      was on the WebView's window, not system-wide).
  //   2. F12 was eaten by the WebView's DevTools shortcut before this
  //      handler ever ran — so the pre-shipped Contingency system flow
  //      (F12 ×3) never fired even with WC focused.
  // Both fixed by routing KeySequenceTrigger through the system-wide
  // `services::keyboard_hook` Rust service. Leaving this listener in
  // would now double-count every keypress when WC has focus
  // (frontend + system hook both emit). Removed entirely.

  // Satisfy lint — log when productivity tracking is active
  useEffect(() => {
    if (productivityStatus?.running) {
      console.log("[AppContext] Productivity tracking active");
    }
  }, [productivityStatus?.running]);

  // Keep the native splash over every launch until the startup readiness gate
  // completes. Cached settings are useful, but they are not proof that the
  // dashboard, capability state, and startup services are ready to use.
  const isLoading = !splashDone;

  useEffect(() => {
    if (isLoading || appSettings === null) return;
    reportStartupPhase("dashboard_first_visible");
    const frame = window.requestAnimationFrame(() => {
      reportStartupPhase("dashboard_interactive");
    });
    return () => window.cancelAnimationFrame(frame);
  }, [appSettings, isLoading]);

  // Restore last active panel from settings.json — DISABLED
  // The user wants the app to always start on 'System Info' (Dashboard).
  /*
  const hasSeededPanel = useRef(false);
  useEffect(() => {
    if (!hasSeededPanel.current && appSettings?.app?.lastPanel) {
      const restored = appSettings.app.lastPanel as PanelId;
      const moduleId = getModuleForPanel(restored);
      if (!moduleId || isModuleEnabled(appSettings?.app?.modules, moduleId)) {
        setActivePanel(restored);
      }
      hasSeededPanel.current = true;
    }
  }, [appSettings?.app?.lastPanel, appSettings?.app?.modules]);
  */

  useEffect(() => {
    const moduleId = getModuleForPanel(activePanel);
    if (moduleId && !isModuleEnabled(appSettings?.app?.modules, moduleId)) {
      setActivePanel('dashboard');
    }
  }, [activePanel, appSettings?.app?.modules]);

  const handleSplashComplete = useCallback(() => {
    setSplashDone(true);
  }, []);

  const preloadDiskCleanup = useCallback((priority: "background" | "idle") => {
    void runStartupJob({
      id: "disk-cleanup-preload",
      priority,
      cost: "expensive",
      timeoutMs: 60_000,
      run: async (signal) => {
        if (signal.aborted) return;
        const module = await import("./components/tweaks/managers/DiskCleanupGranular");
        if (signal.aborted) return;
        await module.fetchDiskCleanupScan();
      },
    });
  }, [runStartupJob]);

  useEffect(() => {
    if (isLoading) return;
    const queue = new PanelPrefetchQueue(1);
    panelPrefetchRef.current = queue;
    const queueWhenIdle = (signal: AbortSignal, items: ReadonlyArray<{ id: string; load: () => Promise<unknown> }>) =>
      new Promise<void>((resolve) => {
        const cancel = scheduleWhenIdle(() => {
          if (!signal.aborted) queue.enqueueIdle(items);
          resolve();
        });
        signal.addEventListener("abort", () => {
          cancel();
          resolve();
        }, { once: true });
      });

    const primaryPanels = PANEL_MANIFESTS
      .filter((panel) => panel.navTier === "primary" && panel.id !== "dashboard" && panel.id !== "search-files")
      .map((panel) => ({ id: panel.id, load: panel.importFn }));
    const searchPanel = PANEL_MANIFESTS.find((panel) => panel.id === "search-files");
    void runStartupJob({
      id: "panel-preload",
      priority: "idle",
      cost: "light",
      timeoutMs: 15_000,
      run: (signal) => queueWhenIdle(signal, primaryPanels),
    });
    if (searchPanel) {
      void runStartupJob({
        id: "search-preload",
        priority: "idle",
        cost: "light",
        timeoutMs: 15_000,
        run: (signal) => queueWhenIdle(signal, [{ id: searchPanel.id, load: searchPanel.importFn }]),
      });
    }
    // Do not reserve the coordinator's single expensive lane while merely
    // waiting for browser idle. The job enters that lane only once this timer
    // actually runs, after critical probes have had a chance to start.
    const cancelDiskIdle = scheduleWhenIdle(() => preloadDiskCleanup("idle"));
    return () => {
      cancelDiskIdle();
      queue.dispose();
      if (panelPrefetchRef.current === queue) panelPrefetchRef.current = null;
    };
  }, [isLoading, preloadDiskCleanup, runStartupJob]);

  // A flow's NotifyAction ("Show a notification") emits `flow-notify`; surface it
  // as a real toast + bell entry. Without this a NotifyAction fired but showed
  // nothing (e.g. a gaze→notify rule looked dead).
  useEffect(() => {
    const unlisten = listen<{ message?: string; severity?: string }>("flow-notify", (event) => {
      const message = event.payload?.message?.trim();
      if (!message) return;
      const severity = event.payload?.severity ?? "info";
      if (severity === "danger") void showError(message, 8_000);
      else if (severity === "warning") void showWarning(message, 7_000);
      else void showInfo(message, 5_000);
    });
    return () => {
      unlisten.then((dispose) => dispose()).catch(() => {});
    };
  }, []);

  const handlePanelChange = useCallback((panel: PanelId) => {
    if (panel === "flows" && !canUseDevTools) {
      window.dispatchEvent(new CustomEvent("license-gate-open", { detail: { tab: "buy", featureLabel: "Automation" } }));
      return;
    }
    const moduleId = getModuleForPanel(panel);
    // A running tour navigates itself and must be allowed through: System
    // Cleanup's module is off by default for the Casual persona, so the Scan
    // All step's `navigate-panel` was dropped here and its anchor never
    // mounted, breaking the tour from that step on (2026-07-26 fix). Only
    // navigation is relaxed — the module stays off.
    if (moduleId && !isModuleEnabled(appSettings?.app?.modules, moduleId) && !isTourActive()) {
      return;
    }
    // Sidebar Dashboard always returns to the dashboard overview rather than
    // leaving a previously selected Risk Matrix/Product showcase on screen.
    if (panel === "dashboard") {
      window.sessionStorage.setItem("wincommander.dashboard-view", "dashboard");
      window.dispatchEvent(new CustomEvent("dashboard-view", { detail: "dashboard" }));
    }
    if (panel === activePanel) {
      window.dispatchEvent(new Event("panel-scroll-top"));
      return;
    }
    panelPrefetchRef.current?.keepRelevant(panel);
    if (panel === "maintenance" || panel === "cleanup") {
      preloadDiskCleanup("background");
    }
    setActivePanel(panel);
    // KT: Persist to settings.json so next session resumes on the same panel
    patchAppSettings({ app: { lastPanel: panel } }).catch(reportSettingsWriteFailure);
  }, [activePanel, patchAppSettings, appSettings?.app?.modules, canUseDevTools, preloadDiskCleanup]);

  useEffect(() => {
    if (activePanel === "flows" && !canUseDevTools) setActivePanel("dashboard");
  }, [activePanel, canUseDevTools]);

  // Global custom event listener for navigating via search dropdowns
  useEffect(() => {
    const handleNavigation = (e: CustomEvent<PanelId>) => {
      handlePanelChange(e.detail);
    };
    window.addEventListener('navigate-panel', handleNavigation as EventListener);
    return () => window.removeEventListener('navigate-panel', handleNavigation as EventListener);
  }, [handlePanelChange]);

  // The native second-Ctrl+Space path sends this only after the overlay has
  // acknowledged its current query.  Keeping this listener at App level is
  // essential: the normal main window does not render EverythingSearchBar,
  // so a listener inside that component silently dropped the native event.
  useEffect(() => {
    const unlisten = listen<string>("open-search-files-panel", (event) => {
      const query = typeof event.payload === "string" ? event.payload.trim() : "";
      if (query) {
        window.localStorage.setItem("wincommander.search-files-query", query);
      }
      handlePanelChange("search-files");
      if (query) {
        window.dispatchEvent(new CustomEvent("search-files-query-handoff", { detail: { query } }));
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [handlePanelChange]);

  // Title-bar shortcuts select a specific dashboard destination. Persist the
  // choice before navigation so it survives the Dashboard component mounting.
  useEffect(() => {
    const handleDashboardViewNavigation = (event: CustomEvent<"dashboard" | "risk" | "products">) => {
      const view = event.detail;
      if (view !== "dashboard" && view !== "risk" && view !== "products") return;
      window.sessionStorage.setItem("wincommander.dashboard-view", view);
      if (activePanel === "dashboard") {
        window.dispatchEvent(new CustomEvent("dashboard-view", { detail: view }));
        return;
      }
      setActivePanel("dashboard");
      patchAppSettings({ app: { lastPanel: "dashboard" } }).catch(reportSettingsWriteFailure);
    };
    window.addEventListener("navigate-dashboard-view", handleDashboardViewNavigation as EventListener);
    return () => window.removeEventListener("navigate-dashboard-view", handleDashboardViewNavigation as EventListener);
  }, [activePanel, patchAppSettings]);

  // KT: Sidebar hover prefetch — silently pre-fetches data when user hovers a nav item.
  // 300ms debounce (handled in Sidebar) prevents spam on quick mouse passes.
  // Manifest-driven: reads refreshKey from each panel's manifest.
  const handlePanelHover = useCallback((panel: PanelId) => {
    if (isIdlePaused) return;
    const manifest = PANEL_MANIFESTS.find((m) => m.id === panel);
    if (!manifest) return;
    void runStartupJob({
      id: "panel-preload",
      priority: "background",
      cost: "light",
      timeoutMs: 10_000,
      run: async (signal) => {
        if (!signal.aborted) panelPrefetchRef.current?.enqueueIntent(manifest.id, manifest.importFn);
      },
    });
    if (panel === "maintenance" || panel === "cleanup") {
      preloadDiskCleanup("background");
    }
    if (manifest.refreshKey) {
      // Look up the refresh function on the AppContext by its key.
      const refreshFn = (appState as unknown as Record<string, unknown>)[manifest.refreshKey];
      if (typeof refreshFn === "function") void (refreshFn as (force: boolean) => unknown)(true);
    }
  }, [appState, isIdlePaused, preloadDiskCleanup, runStartupJob]);
  const handleShredRequest = useCallback((payload: string | string[]) => {
    const incoming = Array.isArray(payload) ? payload : [payload];
    setShredPaths(prev => {
      // If dialog is NOT open, we start a fresh list.
      // If it IS open (e.g. from a previous batch of rapid context-menu clicks), we append.
      if (!isShredDialogOpen) return incoming;
      const existing = new Set(prev);
      return [...prev, ...incoming.filter(p => !existing.has(p))];
    });
    setIsShredDialogOpen(true);
  }, [isShredDialogOpen]);

  // Play startup sound when everything is ready
  useEffect(() => {
    if (!isLoading) playStartupSound();
  }, [isLoading, playStartupSound]);

  // Attach the self-update event listener once. The Rust scheduler
  // (updater.rs) owns the cadence; this just reflects emitted phases so the
  // Dashboard banner + the combined UpdateFlowDialog's auto-trigger react.
  // Idempotent.
  useEffect(() => {
    startUpdaterListener();
  }, []);



  useEffect(() => {
    setSoundEnabled(appSettings?.app?.sounds?.enabled ?? true);
  }, [appSettings?.app?.sounds?.enabled]);

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      // Dev builds run from target/debug and need the Vite dev server, so a
      // Run-key entry pointing at the dev exe can't start standalone — and it
      // would clobber the installed app's autostart path with a broken one
      // (this is exactly why silent startup "stopped working" while testing).
      // Only register autostart from a packaged/installed build.
      if (import.meta.env.DEV) {
        console.info("[Autostart] dev build — skipping autostart registration");
        return;
      }
      try {
        // Machine-wide logon Scheduled Task (replaces the per-user HKCU Run
        // key): fires for any user's logon in their interactive session and
        // elevates without a UAC prompt for admins — required because the app
        // is requireAdministrator. Idempotent and re-points at the current exe,
        // so a moved portable exe re-binds on next launch; it also clears the
        // legacy Run-key value so we don't double-register.
        if (!cancelled) {
          await invoke("ensure_autostart_task");
        }
      } catch (error) {
        console.warn("[Autostart] Failed to enforce startup launch:", error);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <MotionConfig reducedMotion={motionPref === "reduced" ? "always" : "user"}>
    <SearchProvider>
    <>
      {isLoading && <SplashScreen onComplete={handleSplashComplete} isAppReady={startupComplete} />}

      <div
        className="app-container"
        data-startup-data-state={startupDataState}
        aria-busy={startupDataState === 'loading' || startupDataState === 'refreshing'}
        style={{ display: isLoading ? 'none' : 'flex', flexDirection: 'column', height: '100vh', width: '100vw', overflow: 'hidden' }}
      >
        <AppShell
          activePanel={activePanel}
          onPanelChange={handlePanelChange}
          onPanelHover={handlePanelHover}
          showUnlockedPanels={hiddenPanelsUnlocked}
        >
          {/* KT: Removed AnimatePresence mode="wait" — it forced the old panel to
               complete its exit animation BEFORE the new panel could mount, causing
               a visible "stuck on old panel" delay (150ms exit + chunk load + 150ms enter).
               Now we just do a quick fade-in on the new panel with no exit animation.
               Suspense fallback kept as safety net but shouldn't trigger after preload. */}
          <PanelErrorBoundary
            key={`${activePanel}:${panelRecoveryGeneration}`}
            panelId={activePanel}
            onRetry={() => setPanelRecoveryGeneration((generation) => generation + 1)}
          >
          <Suspense fallback={activePanel === "fleet" ? <FleetSkeleton /> : <PanelSkeleton />}>
            {/* Panel enter: crossfade + rise from the motion SSOT. No exit/
                AnimatePresence by design (see KT note above) — only initial+
                animate apply on the keyed remount. */}
            <motion.div
              key={activePanel}
              variants={panelVariants}
              initial="initial"
              animate="animate"
              transition={panelTransition}
              style={{ height: '100%' }}
            >
              {isIdlePaused ? (
                <div className="h-full w-full flex items-center justify-center px-6">
                  <div className="font-mono text-[11px] tracking-[1px] uppercase text-[var(--color-text-muted)] border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-4 py-3">
                    Resource pause mode active due to inactivity. Move mouse or press any key to resume.
                  </div>
                </div>
              ) : (
                <PanelRoute
                  panelId={activePanel}
                  recoveryGeneration={panelRecoveryGeneration}
                />
              )}
            </motion.div>
          </Suspense>
          </PanelErrorBoundary>
        </AppShell>
      </div>

      {/* Background pollers — no UI, purely side effects */}
      <BackgroundPollers
        onShredRequest={handleShredRequest}
        onPanelChange={handlePanelChange}
      />
      <UsbHidApprovalDialog />

      <RdpIdleWarningDialog
        isOpen={rdpIdle.warningActive}
        secondsLeft={rdpIdle.warningSecondsLeft}
        idleSeconds={rdpIdle.secondsSinceActivity}
        timeoutSeconds={rdpIdleTimeout}
        warningWindowSeconds={rdpIdleWarningSeconds}
        onDismiss={rdpIdle.snooze}
      />

      <ShredConfirmationDialog
        isOpen={isShredDialogOpen}
        paths={shredPaths}
        onClose={() => setIsShredDialogOpen(false)}
      />

      {/* License Gate — self-contained: verification, activation, gating */}
      <LicenseGate />

      {/* Global celebration listener — every activation path dispatches
          `license-activated-celebration`; this renders the burst + toast. */}
      <LicenseCelebrationListener />

      {/* Pro sidecar install dialog — opened via the `pro-install-open`
          window event by any surface that wants to prompt the user to
          download the Pro EXE (LicenseGate post-activation auto-trigger,
          LicenseQuickPanel sidebar button).
          onNotNow: only fires from the startup-nag path; persists the
          dismissed flag so the nag won't repeat on subsequent launches. */}
      <InstallProDialog
        isOpen={proInstallOpen}
        onClose={() => setProInstallOpen(false)}
        onNotNow={proInstallIsStartupNagRef.current
          ? () => {
              setProInstallOpen(false);
              patchAppSettings({ app: { proInstallPromptDismissed: true } }).catch(reportSettingsWriteFailure);
            }
          : undefined}
      />

      {/* Task status moved into NotificationsMenu (title-bar bell) — the
          floating bottom-right card is gone; in-flight operations now live
          inside the popover so the title bar is the single status surface. */}
      {/* Settings-aware toast host — position/duration from app.notifications */}
      <AppToaster />
      {/* Flow/automation activity → DevTools console (detectors + engine decisions). */}
      <FlowActivityLogger />
      {/* In-app guide: first-launch setup + spotlight tour (sole help/onboarding surface). */}
      <GuideHost />
      {/* Universal Operation Overlay — multi-step task list (self-destruct, volume create, shred, etc.) */}
      <OperationOverlay />
      {/* Combined update flow — Free check/install, then (if paid) Pro
          check/install in the same dialog, one restart prompt at the end.
          Auto-opens per the effect above; onNotNow only applies to the
          Pro-not-installed startup nag so "Not now" there persists the
          dismissed flag exactly like the old standalone nag did. */}
      <UpdateFlowDialog
        isOpen={updateFlowOpen}
        onClose={() => setUpdateFlowOpen(false)}
        hasPaid={hasPaid}
        canUpdatePro={canUpdatePro}
        // The auto-trigger above only opens this dialog when a Free/Pro update
        // or Pro install is actually pending, so an open dialog always has an
        // update available — this drives the paid-user upfront confirm.
        updateAvailable={updateFlowOpen}
        onNotNow={updateFlowIsStartupNagRef.current
          ? () => {
              setUpdateFlowOpen(false);
              patchAppSettings({ app: { proInstallPromptDismissed: true } }).catch(reportSettingsWriteFailure);
            }
          : undefined}
      />
    </>
    </SearchProvider>
    </MotionConfig>
  );
}

function StartupAuthGate({ children }: { children: React.ReactNode }) {
  const [showCalc, setShowCalc]       = useState(false);
  const { setMode } = useAuthMode();

  useEffect(() => {
    let mounted = true;
    const handleCalculatorLockEngaged = () => {
      setShowCalc(true);
    };
    window.addEventListener("wincommander:calculator-lock-engaged", handleCalculatorLockEngaged);

    // Backend signal emitted by enter_calculator_mode on EVERY lock entry path
    // (dashboard Lock button, lock keyword, tray, peek hotkey, relaunch). The
    // backend resizes the window to calc dimensions; this shows the matching
    // calc UI so the window is never a calc-sized blank real app.
    let unlistenCalc: (() => void) | undefined;
    listen("calculator-mode-entered", () => {
      if (mounted) setShowCalc(true);
    }).then((un) => {
      if (mounted) unlistenCalc = un;
      else un();
    }).catch(() => {});

    // Truthful panic: the destruct cascade emits an aggregate summary. If any
    // step failed (notably PRO_NOT_INSTALLED when Pro isn't installed/licensed),
    // tell the user the machine was NOT fully wiped instead of looking "done".
    let unlistenDestruct: (() => void) | undefined;
    listen<{ total: number; failed: number; proMissing: number; complete: boolean; licenseWarning?: string | null }>(
      "destruct-summary",
      (e) => {
        const p = e.payload;
        if (p.licenseWarning) {
          void showError(p.licenseWarning, 60000);
        }
        if (!p.complete) {
          void showError(
            `Wipe incomplete: ${p.failed} of ${p.total} step(s) failed` +
              (p.proMissing > 0 ? ` — ${p.proMissing} require WinCommander Pro` : "") +
              ". Data may still be recoverable.",
            60000,
          );
        }
      },
    ).then((un) => { unlistenDestruct = un; }).catch(() => {});

    invoke<boolean>("startup_pin_is_configured")
      .then((configured) => {
        if (!mounted) return;
        if (configured) {
          // Native setup owns the window transition before its first paint;
          // React only replaces the webview content with the calculator.
          setShowCalc(true);
        } else {
          setMode("real");
        }
      })
      .catch(() => {
        if (mounted) setMode("real");
      });

    return () => {
      mounted = false;
      window.removeEventListener("wincommander:calculator-lock-engaged", handleCalculatorLockEngaged);
      unlistenCalc?.();
      unlistenDestruct?.();
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleAuth = useCallback((mode: AuthMode) => {
    // setMode (AuthModeContext) arms/disarms the backend DECOY_MODE backstop
    // for every decoy transition, so no explicit set_decoy_mode call is needed
    // here — the calculator gate, distress phrase, and command palette all go
    // through the same context setter.
    setMode(mode);
    setShowCalc(false);
  }, [setMode]);

  if (showCalc) {
    return <CalculatorGate onAuth={handleAuth} />;
  }
  // Always render children while the PIN check runs — AppContent already
  // hides the dashboard behind SplashScreen, so a blank native window is
  // worse than mounting early. Calculator still replaces the tree above.
  return <>{children}</>;
}

function App() {
  return (
    <AuthModeProvider>
      <LiveMetricsProvider>
        <AppProvider>
          <MotionPreferenceProvider>
            <TaskStatusProvider>
              <StartupAuthGate>
                <AppContent />
              </StartupAuthGate>
            </TaskStatusProvider>
          </MotionPreferenceProvider>
        </AppProvider>
      </LiveMetricsProvider>
    </AuthModeProvider>
  );
}

export default App;
