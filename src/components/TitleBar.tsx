import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import { useAppState } from "../context/AppContext";
import { useEffect, useRef, useState, useCallback, type MouseEvent } from "react";
import { useSovereigntyScore } from "../hooks/useSovereigntyScore";
import { getDisplayBranding } from "../lib/branding";
import useProInstall from "../hooks/useProInstall";
import useEntitlements from "../hooks/useEntitlements";
import { useTheme } from "../context/ThemeContext";
import { logo as brandLogo } from "@/assets";
import { Icon } from "./ui/icon";
import { Badge } from "./ui/badge";
import NotificationsMenu from "./NotificationsMenu";
import { TITLEBAR_ICON_BTN } from "./ui/titleBarButtonClass";
import { setNotificationsHidden, setPopupAlertsSuppressed } from "../lib/notificationStore";
import useBorrowedActive from "../hooks/useBorrowedActive";
import { useAuthMode } from "../context/AuthModeContext";
import { allTopics } from "../content/guide";
import { tourIdForPanel } from "../lib/tour";
import { PANEL_MANIFESTS, type PanelId } from "../types/panels";

const appWindow = (window as any).__TAURI_INTERNALS__ ? getCurrentWindow() : null;

/** Open the global ⌘K command palette (handled by GlobalCommandPalette). */
function openPalette() {
  window.dispatchEvent(new CustomEvent("open-command-palette"));
}

type DashboardView = "dashboard" | "risk" | "products";

function openDashboardView(view: DashboardView) {
  window.dispatchEvent(new CustomEvent("navigate-dashboard-view", { detail: view }));
}

interface TitleBarProps {
  activePanel: PanelId;
}

function TitleBar({ activePanel }: TitleBarProps) {
  const { appSettings } = useAppState();
  const { theme, setTheme } = useTheme();
  const [version, setVersion] = useState("");
  const [dashboardView, setDashboardView] = useState<DashboardView>(() => {
    const stored = window.sessionStorage.getItem("wincommander.dashboard-view");
    return stored === "risk" || stored === "products" ? stored : "dashboard";
  });
  const score = useSovereigntyScore();
  const branding = getDisplayBranding(appSettings);

  useEffect(() => {
    const syncDashboardView = (event: Event) => {
      const view = (event as CustomEvent<DashboardView>).detail;
      if (view === "dashboard" || view === "risk" || view === "products") setDashboardView(view);
    };
    window.addEventListener("dashboard-view", syncDashboardView);
    return () => window.removeEventListener("dashboard-view", syncDashboardView);
  }, []);
  const { isInstalled: proInstalled } = useProInstall();
  const { hasPaid } = useEntitlements();
  const edition = hasPaid && proInstalled ? 'PRO' : 'FREE';

  const { mode: authMode } = useAuthMode();
  const decoyEnabled = appSettings?.app?.decoyMode?.enabled === true;
  const decoyName = appSettings?.app?.decoyMode?.displayName?.trim() || 'WinCommander';

  // Calculator-disguise: when the calc lock is armed, the OS window title must
  // stay "Calculator" (taskbar / Alt-Tab) even after the user unlocks into the
  // full app — matching the calc icon exit_calculator_mode keeps. The in-app
  // title bar below still renders the REAL branding; only the OS-visible title
  // is disguised. Without this the title bar effect re-pushed "WinCommander Pro"
  // over the "Calculator" title, leaving the broken calc-icon + WinCommander-name
  // taskbar state.
  const startupPin = appSettings?.ideal?.privacy?.startupPin;
  const calcLockArmed = !!startupPin?.realHash && startupPin?.enabled !== false;

  // Push the edition label to OS-visible surfaces (uninstall registry / Start
  // menu / window title).
  // KT: skip the effect entirely in decoy mode — the cover name was already set
  // by exit_calculator_mode before the UI mounted; overwriting it would drop deniability.
  const lastPushedLabelRef = useRef<string | null>(null);
  useEffect(() => {
    if (authMode === "decoy") return;
    const desired = decoyEnabled
      ? decoyName
      : (edition === 'PRO' ? 'WinCommander Pro' : 'WinCommander');
    // OS-visible title: "Calculator" while the calc lock is armed (disguise),
    // the real label otherwise.
    const osTitle = calcLockArmed && !decoyEnabled ? 'Calculator' : desired;
    if (lastPushedLabelRef.current === osTitle) return;
    lastPushedLabelRef.current = osTitle;
    // Only rebrand the uninstall/Start-menu surfaces for the real label — never
    // churn them to "Calculator". The disguise is the taskbar title + icon.
    if (!decoyEnabled && !calcLockArmed) {
      invoke('set_app_display_label', { label: desired }).catch((err) => {
        console.warn('[DisplayLabel] update failed:', err);
      });
    }
    appWindow?.setTitle(osTitle).catch((err) => {
      console.warn('[DisplayLabel] setTitle failed:', err);
    });
  }, [authMode, edition, decoyEnabled, decoyName, calcLockArmed]);

  useEffect(() => {
    getVersion().then(setVersion);
  }, []);

  // Track whether panels are currently locked (mirrors App.tsx + GlobalCommandPalette).
  const [panelsUnlocked, setPanelsUnlocked] = useState(false);
  useEffect(() => {
    const onUnlock = () => setPanelsUnlocked(true);
    const onLock = () => setPanelsUnlocked(false);
    window.addEventListener("hidden-panels-unlock", onUnlock);
    window.addEventListener("hidden-panels-lock", onLock);
    return () => {
      window.removeEventListener("hidden-panels-unlock", onUnlock);
      window.removeEventListener("hidden-panels-lock", onLock);
    };
  }, []);
  const panelsLocked = !panelsUnlocked && (appSettings?.app?.lockedPanelIds?.length ?? 0) > 0;
  const borrowedActive = useBorrowedActive();
  const borrowedHidden = appSettings?.app?.borrowedHidden ?? [];
  // Alerts + Processes share ONE visibility toggle (still the "notif-bell"
  // key/hideNotificationBell field — this was one bell before the
  // Alerts/Processes icon split, and splitting the icon didn't change the
  // owner's ask for a single hide switch). Hidden when: the standalone
  // "always" toggle is set, OR the legacy mute-while-locked flag fires, OR
  // Borrowed Mode is active and "notif-bell" is in the borrowedHidden set.
  const hideNotificationIcons =
    appSettings?.app?.hideNotificationBell === true ||
    (panelsLocked && appSettings?.app?.muteNotificationsWhenLocked === true) ||
    (borrowedActive && borrowedHidden.includes("notif-bell"));
  // Mirror the hidden state into the notification store so toast popups stay
  // suppressed while Alerts is hidden (toast helpers can't read AppContext).
  useEffect(() => {
    setNotificationsHidden(hideNotificationIcons);
  }, [hideNotificationIcons]);

  // Mirror popup-alerts borrowed suppression so toast helpers (non-React) see it.
  const popupAlertsBorrowed = borrowedActive && borrowedHidden.includes("popup-alerts");
  useEffect(() => {
    setPopupAlertsSuppressed(popupAlertsBorrowed);
  }, [popupAlertsBorrowed]);

  // 5-click brand trigger → reveal Secret Settings for this session and open it.
  // Uses a button (not a drag child) so WebView2 always delivers the clicks;
  // a pure div under a data-tauri-drag-region header was unreliable on Windows.
  const brandClickCount = useRef(0);
  const brandClickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const handleBrandClick = useCallback((e: MouseEvent) => {
    // Never reveal Secret Settings while in decoy auth mode.
    // decoyEnabled (from appSettings) is null in decoy mode, so we check
    // authMode directly — it stays accurate regardless of settings state.
    if (authMode === "decoy") return;
    e.preventDefault();
    e.stopPropagation();
    brandClickCount.current += 1;
    if (brandClickTimer.current) clearTimeout(brandClickTimer.current);
    if (brandClickCount.current >= 5) {
      brandClickCount.current = 0;
      window.dispatchEvent(new CustomEvent("secret-settings-reveal"));
      // Open immediately — revealing only the sidebar entry made the gesture
      // look like a no-op when the user didn't notice the new System row.
      window.dispatchEvent(new CustomEvent("navigate-panel", { detail: "secret" }));
    } else {
      // 2s between clicks is more forgiving than 1s for deliberate multi-clicks.
      brandClickTimer.current = setTimeout(() => { brandClickCount.current = 0; }, 2000);
    }
  }, [authMode]);

  // Minimize is a real minimize to the taskbar (owner request) — NOT the
  // close → hide-and-lock path. The window stays in the taskbar and restores
  // to where it was.
  const handleMinimize = () => appWindow?.minimize();
  const handleMaximize = () => appWindow?.toggleMaximize();
  const handleClose = () => appWindow?.close();

  // Single tour entry point, one click, no menu — starts the full tour when
  // on Dashboard (tourIdForPanel resolves "dashboard" straight to
  // "tour-dashboard", the full walkthrough) or just the current panel's own
  // tour otherwise. Adapts silently off activePanel; there's no dropdown
  // asking the user to choose (2026-07-20 fix — this used to be a
  // Popover offering "take the full tour" vs. the panel tour as two options).
  const contextualTourId = tourIdForPanel(allTopics(), activePanel);
  const contextualTourLabel = contextualTourId === "welcome"
    ? "Replay tour"
    : `${PANEL_MANIFESTS.find((m) => m.id === activePanel)?.label ?? ""} tour`;

  return (
    <header
      className="relative z-40 flex h-[52px] shrink-0 items-center gap-2 border-b border-[var(--border)] bg-[color-mix(in_srgb,var(--surface)_72%,transparent)] px-3 backdrop-blur-[14px]"
      data-tauri-drag-region
    >
      {/* ── Brand — 5× click reveals + opens Secret Settings for this session ── */}
      <button
        type="button"
        className="flex cursor-pointer select-none items-center gap-2 rounded-[var(--r-sm)] border-0 bg-transparent p-1 -ml-1 text-left"
        data-tauri-drag-region={false}
        onClick={handleBrandClick}
        onMouseDown={(e) => e.stopPropagation()}
        aria-label={authMode === "decoy" ? undefined : "WinCommander"}
      >
        <img src={brandLogo} alt="" className="h-5 w-5 pointer-events-none" draggable={false} />
        <span className="font-[family-name:var(--font-display)] text-[13px] font-semibold tracking-[0.13em] text-[var(--text)] pointer-events-none">
          {/* KT: in decoy mode render empty — real branding blows cover to a coerced examiner.
               Normal mode (incl. non-decoy borrowed mode) keeps the real product name. */}
          {authMode === "decoy" ? '' : (appSettings ? branding.titleLabel : '')}
        </span>
        {version && (
          <span className="rounded-[var(--r-sm)] border border-[var(--border)] px-1.5 py-0.5 font-[family-name:var(--font-mono)] text-[10px] text-[var(--text-mute)] pointer-events-none">
            v{version}
          </span>
        )}
        {edition === 'PRO' && !decoyEnabled && authMode !== 'decoy' && <Badge tone="accent">PRO</Badge>}
      </button>

      <div className="flex-1" data-tauri-drag-region />

      {/* ── ⌘K unified search / command palette trigger — centered ── */}
      <button
        onClick={openPalette}
        data-tauri-drag-region={false}
        data-tour="search"
        className={`absolute top-1/2 z-10 flex -translate-x-1/2 -translate-y-1/2 items-center gap-2 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-1.5 text-[var(--text-mute)] transition-colors duration-150 hover:border-[var(--border-strong)] hover:text-[var(--text-dim)] ${activePanel === "dashboard" ? "left-[calc(50%-160px)]" : "left-1/2"}`}
        title="Search settings, files & actions"
      >
        <Icon icon="search" size={14} />
        <span className="text-[12.5px]">Search settings &amp; tools</span>
        <span className="ml-2 rounded-[var(--r-sm)] border border-[var(--border)] bg-[var(--surface-3)] px-1.5 py-0.5 font-[family-name:var(--font-mono)] text-[10px] text-[var(--text-mute)]">⌘K</span>
      </button>

      {activePanel === "dashboard" && authMode !== "decoy" && (
        <div
          className="absolute left-[calc(50%+10px)] top-1/2 z-10 flex -translate-y-1/2 items-center gap-1"
          data-tauri-drag-region={false}
          aria-label="Dashboard views"
        >
          <button type="button" onClick={() => { setDashboardView("risk"); openDashboardView("risk"); }} aria-pressed={dashboardView === "risk"} className={`flex items-center gap-1 rounded-[var(--r-sm)] px-2 py-1.5 text-[11px] font-semibold transition-colors ${dashboardView === "risk" ? "bg-[var(--accent)] text-[var(--accent-contrast)]" : "text-[var(--text-mute)] hover:bg-[var(--surface-3)] hover:text-[var(--text)]"}`} title="Risk Matrix">
            <Icon icon="shield" size={13} /><span>Risk Matrix</span>
          </button>
          <button type="button" onClick={() => { setDashboardView("products"); openDashboardView("products"); }} aria-pressed={dashboardView === "products"} className={`flex items-center gap-1 rounded-[var(--r-sm)] px-2 py-1.5 text-[11px] font-semibold transition-colors ${dashboardView === "products" ? "bg-[var(--accent)] text-[var(--accent-contrast)]" : "text-[var(--text-mute)] hover:bg-[var(--surface-3)] hover:text-[var(--text)]"}`} title="More Products">
            <Icon icon="clean" size={13} /><span>More Products</span>
          </button>
        </div>
      )}

      {/* ── Posture pill ── */}
      <div
        className="flex items-center gap-2 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-1"
        title={`System health ${score.total}%`}
        role="status"
        aria-label={`System health ${score.total}%`}
        data-tour="health"
             >
        <span
          className="h-2 w-2 rounded-full"
          style={{ background: score.color, boxShadow: `0 0 0 3px color-mix(in srgb, ${score.color} 25%, transparent)` }}
        />
        <span className="font-[family-name:var(--font-mono)] text-[10.5px] uppercase tracking-wider text-[var(--text-mute)]">Health</span>
        <span className="font-[family-name:var(--font-display)] text-[13px] font-semibold" style={{ color: score.color }}>{score.total}%</span>
      </div>

      {/* ── Theme toggle — direct dark / light switch ──
          The active mode button gets a solid accent fill so the current theme
          is unmistakable at a glance; the inactive button stays muted. */}
      <div className="flex items-center gap-0.5 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-0.5">
        <button
          onClick={() => setTheme("dark")}
          aria-pressed={theme === "dark"}
          className={`grid place-items-center w-7 h-7 rounded-[var(--r-sm)] transition-colors duration-150 ${theme === "dark" ? "bg-[var(--accent)] text-[var(--accent-contrast)] shadow-[0_0_0_1px_var(--accent)]" : "text-[var(--text-mute)] hover:text-[var(--text)] hover:bg-[var(--surface-3)]"}`}
          title="Dark mode (Anduril)"
        >
          <Icon icon="moon" size={13} />
        </button>
        <button
          onClick={() => setTheme("light")}
          aria-pressed={theme === "light"}
          className={`grid place-items-center w-7 h-7 rounded-[var(--r-sm)] transition-colors duration-150 ${theme === "light" ? "bg-[var(--accent)] text-[var(--accent-contrast)] shadow-[0_0_0_1px_var(--accent)]" : "text-[var(--text-mute)] hover:text-[var(--text)] hover:bg-[var(--surface-3)]"}`}
          title="Light mode (Daylight)"
        >
          <Icon icon="sun" size={13} />
        </button>
      </div>

      {/* A single local inbox combines process activity and local alerts.
          Fleet alerts remain in the Fleet console. */}
      {!hideNotificationIcons && (
        <NotificationsMenu />
      )}
      {/* ── Take the tour — single title-bar entry point for onboarding;
          also the tour's own anchor for its "find help later" stop ── */}
      <button
        onClick={() => window.dispatchEvent(new CustomEvent("start-tour", { detail: { tourId: contextualTourId } }))}
        className={TITLEBAR_ICON_BTN}
        title={contextualTourLabel}
        data-tour="help"
        data-tauri-drag-region={false}
      >
        <Icon icon="help" size={15} />
      </button>

      {/* ── Window controls ── */}
      <div className="ml-1 flex items-center gap-0.5" data-tauri-drag-region={false}>
        <button onClick={handleMinimize} className={TITLEBAR_ICON_BTN} title="Minimize"><Icon icon="minus" size={14} /></button>
        <button onClick={handleMaximize} className={TITLEBAR_ICON_BTN} title="Maximize"><Icon icon="square" size={12} /></button>
        <button
          onClick={handleClose}
          className="grid h-8 w-8 place-items-center rounded-[var(--r-sm)] text-[var(--text-mute)] transition-colors duration-150 hover:bg-[var(--danger)] hover:text-white"
          title="Close"
        >
          <Icon icon="cross" size={14} />
        </button>
      </div>
    </header>
  );
}

export default TitleBar;
