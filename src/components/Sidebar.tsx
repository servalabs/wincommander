import { type PanelId, getSidebarManifests, NAV_GROUP_ORDER, navGroupFor } from "../types/panels";
import { lazy, Suspense, useEffect, useMemo, useState, useRef, useCallback } from "react";
import { useAuthMode } from "../context/AuthModeContext";
import { motion } from "framer-motion";
import { SPRING } from "./shared/motion";
import { useAppState } from "../context/AppContext";
import type { DependencyInfo } from "../hooks/useDependencies";
import { useSovereigntyScore } from "../hooks/useSovereigntyScore";
import LicenseQuickPanel from "./LicenseQuickPanel";
import ExperienceLevelSwitch from "./ExperienceLevelSwitch";
import PersonaSwitch from "./PersonaSwitch";
import { getModuleForPanel, getModuleDef, isModuleEnabled } from "../types/modules";
import type { ModuleConfig } from "../types/modules";
import { executeBackendCommand } from "../hooks/useBackend";
import { runOperation } from "../context/OperationContext";
import useVisibility from "../hooks/useVisibility";
import useBorrowedActive from "../hooks/useBorrowedActive";
import { DEFAULT_ALWAYS_PANELS, DEFAULT_BORROWED_PANELS } from "../lib/visibilityDefaults";
import { Icon, type IconName } from "./ui/icon";
import { Spinner } from "./ui/spinner";
import { invoke } from "@tauri-apps/api/core";
import './Sidebar.css';

// RDP management includes a full legacy dialog/form surface. The rail remains
// responsive while that infrequent control loads on demand.
const RdpQuickAction = lazy(() => import("./RdpQuickAction"));

// ── Dev build nav entry ────────────────────────────────────────────────
// Gated on is_dev_build() (Rust cfg!(debug_assertions)) — absent in release.
// Hidden-tier panel so it never appears via getSidebarManifests; we inject
// it explicitly here so the rail entry is visible only in debug binaries.
function useIsDevBuild() {
  const [isDev, setIsDev] = useState(false);
  useEffect(() => {
    invoke<boolean>("is_dev_build")
      .then(setIsDev)
      .catch(() => setIsDev(false));
  }, []);
  return isDev;
}

// ═════════════════════════════════════════════════════════════════════
// NAV ITEMS — auto-generated from PanelManifest, rendered in V2 groups
// (Monitor / Protect / Secure / System + footer) via getGroupedSidebarManifests.
// ═════════════════════════════════════════════════════════════════════

interface NavItem {
  id: PanelId;
  icon: IconName;
  label: string;
  searchKeywords: string[];
}

/** Convert PanelManifest[] into the NavItem[] used by the sidebar */
function buildNavItems(visibility: ReturnType<typeof useVisibility>): NavItem[] {
  return getSidebarManifests(visibility).map((m) => ({
    id: m.id,
    icon: m.icon as IconName,
    label: m.label,
    searchKeywords: m.searchKeywords ?? [],
  }));
}

// Default panel IDs hidden until the unlock keyword is typed.
// KT: FIRST-PAINT fallback used before settings load. Must match the backend's
// resolved default (settings.ts) — an empty [] let secret panels flash visible.

interface SidebarProps {
  activePanel: PanelId;
  onPanelChange: (panel: PanelId) => void;
  /** Called on intentional hover (300ms+) to silently pre-fetch panel data */
  onPanelHover?: (panel: PanelId) => void;
  showUnlockedPanels?: boolean;
}

export default function Sidebar({ activePanel, onPanelChange, onPanelHover, showUnlockedPanels }: SidebarProps) {
  const visibility = useVisibility();
  const score = useSovereigntyScore();
  const isDevBuild = useIsDevBuild();
  const { mode: authMode } = useAuthMode();
  const { appSettings, dependencyStatus, patchAppSettings, refreshDependencies } = useAppState();
  const [installingDeps, setInstallingDeps] = useState<Record<string, boolean>>({});
  const [secretSettingsRevealed, setSecretSettingsRevealed] = useState(false);

  const collapsed = appSettings?.app?.sidebarCollapsed ?? false;
  const toggleCollapsed = async () => {
    try {
      await patchAppSettings({ app: { sidebarCollapsed: !collapsed } });
    } catch {
      // Keep UI silent; sidebar state auto-refreshes from AppContext.
    }
  };

  // License quick-panel visibility (Borrowed Mode table). Hidden when the
  // standalone "Always" flag is set, or when Borrowed Mode is active and
  // "license-panel" is in the borrowed-hidden set.
  const borrowedActive = useBorrowedActive();
  const licenseHidden =
    appSettings?.app?.hideLicensePanel === true ||
    (borrowedActive && (appSettings?.app?.borrowedHidden ?? []).includes("license-panel"));
  const preferencesHidden =
    appSettings?.app?.hideSidebarPreferences === true ||
    (borrowedActive && (appSettings?.app?.borrowedHidden ?? []).includes("sidebar-preferences"));

  useEffect(() => {
    const revealSecretSettings = () => setSecretSettingsRevealed(true);
    const hideSecretSettings = () => setSecretSettingsRevealed(false);
    window.addEventListener("secret-settings-reveal", revealSecretSettings);
    window.addEventListener("hidden-panels-lock", hideSecretSettings);
    return () => {
      window.removeEventListener("secret-settings-reveal", revealSecretSettings);
      window.removeEventListener("hidden-panels-lock", hideSecretSettings);
    };
  }, []);

  const handleInstallDependency = useCallback(async (dep: DependencyInfo, e: React.MouseEvent | React.KeyboardEvent) => {
    e.stopPropagation();
    if (installingDeps[dep.id]) return;
    setInstallingDeps(s => ({ ...s, [dep.id]: true }));
    try {
      await runOperation(`Install ${dep.name}`, [
        {
          label: `Installing ${dep.name}`,
          fn: async () => {
            const res = await executeBackendCommand('Install-Dependency', { Id: dep.id });
            if (!res.success) throw new Error(res.error || 'Installation failed');
            if (res.data && (res.data as any).error) throw new Error((res.data as any).message || 'Installation failed');
          },
        },
        {
          label: 'Refreshing status',
          fn: async () => { await refreshDependencies(); },
        },
      ], { mode: 'sequential', accent: 'blue', failFast: true, autoDismissMs: 4000 });
    } finally {
      setInstallingDeps(s => {
        const next = { ...s };
        delete next[dep.id];
        return next;
      });
    }
  }, [installingDeps, refreshDependencies]);

  const modules: ModuleConfig = useMemo(
    () => appSettings?.app?.modules ?? {},
    [appSettings?.app?.modules]
  );
  const handleModuleToggle = async (moduleId: keyof ModuleConfig) => {
    const current = isModuleEnabled(modules, moduleId);
    try {
      await patchAppSettings({ app: { modules: { [moduleId]: !current } } });
    } catch {
      // Keep UI silent; sidebar state auto-refreshes from AppContext.
    }
  };

  // KT: render NO nav items until settings are ready so nothing flashes.
  // In decoy mode appSettings is intentionally null, but the sidebar nav
  // should still render (panels will appear empty/unconfigured, which is correct).
  const settingsReady = authMode === "decoy" || !!appSettings?.app;
  const navItems = useMemo(
    () => buildNavItems(visibility),
    [visibility],
  );
  const visibleNavItems = useMemo(() => {
    if (!settingsReady) return [];

    // Locked panels are only filtered while Borrowed Mode is active. On a
    // fresh install (no lockedPanelIds set) with borrow off, every panel is
    // visible — the defaults only apply when borrow is actually engaged.
    const lockedIds = (borrowedActive
      ? (appSettings?.app?.lockedPanelIds ?? DEFAULT_BORROWED_PANELS)
      : []) as string[];

    return navItems.filter(item => {
      // Secret Settings is governed ONLY by the title-bar 5× brand-click
      // reveal for this session — not Borrowed Mode, not permanently-hidden.
      // KT: secret used to sit in DEFAULT_BORROWED_PANELS, so once Borrowed
      // Mode was active the 5× click set secretSettingsRevealed=true but the
      // lockedIds filter still dropped the row. That made Secret Settings
      // unreachable (and Borrowed Mode un-exitable from the UI).
      if (item.id === "secret") return secretSettingsRevealed;

      // Server Apps, Productivity and Fleet are
      // now governed by the Secret Settings visibility table like every other
      // panel — no hardwired hide flags. Defaults (visibilityDefaults):
      // productivity + server-apps + flows start "Always" hidden.

      if (lockedIds.includes(item.id) && !showUnlockedPanels) return false;

      // Permanently hidden panels are always filtered, regardless of lock state.
      const permanentlyHidden = (appSettings?.app?.permanentlyHiddenPanels ?? DEFAULT_ALWAYS_PANELS) as string[];
      if (permanentlyHidden.includes(item.id)) return false;

      return true;
    });
  }, [
    borrowedActive,
    appSettings?.app?.lockedPanelIds,
    appSettings?.app?.permanentlyHiddenPanels,
    navItems,
    settingsReady,
    secretSettingsRevealed,
    showUnlockedPanels,
  ]);

  const orderedNavItems = useMemo(() => {
    return [...visibleNavItems].sort((a, b) => {
      const aMod = getModuleForPanel(a.id);
      const bMod = getModuleForPanel(b.id);
      const aOn = aMod ? isModuleEnabled(modules, aMod) : true;
      const bOn = bMod ? isModuleEnabled(modules, bMod) : true;
      if (aOn !== bOn) return aOn ? -1 : 1;
      return 0;
    });
  }, [visibleNavItems, modules]);

  // Bucket the (already filtered + ordered) nav items into V2 groups.
  const groupedNav = useMemo(() => {
    return NAV_GROUP_ORDER
      .map((g) => ({ ...g, items: orderedNavItems.filter((it) => navGroupFor(it.id) === g.id) }))
      .filter((g) => g.items.length > 0);
  }, [orderedNavItems]);

  const depMap = useMemo(() => {
    const m: Record<string, DependencyInfo> = {};
    if (dependencyStatus) {
      for (const dep of dependencyStatus) {
        if (dep.id === 'privacyShieldAI') continue;
        m[dep.panelId] = dep;
      }
    }
    return m;
  }, [dependencyStatus]);

  // ── Hover prefetch: 300ms debounce to avoid spam on quick mouse-overs ──
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleItemHover = useCallback((panelId: PanelId) => {
    if (panelId === activePanel || !onPanelHover) return;
    if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
    hoverTimerRef.current = setTimeout(() => {
      onPanelHover(panelId);
    }, 300);
  }, [activePanel, onPanelHover]);

  const handleItemLeave = useCallback(() => {
    if (hoverTimerRef.current) {
      clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
  }, []);

  const handlePanelClick = (panelId: PanelId) => {
    const moduleId = getModuleForPanel(panelId);
    if (moduleId && !isModuleEnabled(modules, moduleId)) return;
    onPanelChange(panelId);
    window.dispatchEvent(new CustomEvent('panel-change', { detail: panelId }));
  };

  const handleInlineModuleToggle = async (moduleId: keyof ModuleConfig, panelId: PanelId) => {
    const wasEnabled = isModuleEnabled(modules, moduleId);
    await handleModuleToggle(moduleId);
    if (wasEnabled && activePanel === panelId) {
      onPanelChange('dashboard');
    }
  };

  // ── Single nav-item renderer, reused across all groups ──
  const renderNavItem = (item: NavItem) => {
    const dep = depMap[item.id];
    const isMissing = dep && !dep.installed;
    const isActive = activePanel === item.id;
    const moduleId = getModuleForPanel(item.id);
    const moduleDef = moduleId ? getModuleDef(moduleId) : undefined;
    const moduleOn = moduleId ? isModuleEnabled(modules, moduleId) : true;
    return (
      <button
        key={item.id}
        data-tour={`nav-${item.id}`}
        className={`sidebar-item ${isActive ? "active" : ""} ${isMissing ? "sidebar-item--dep-missing" : ""} ${!moduleOn ? "sidebar-item--module-off" : ""}`}
        onClick={() => handlePanelClick(item.id)}
        onMouseEnter={() => { if (moduleOn) handleItemHover(item.id); }}
        onMouseLeave={handleItemLeave}
        title={
          !moduleOn
            ? `${moduleDef?.label ?? 'Module'} is disabled`
            : (isMissing ? `Requires ${dep?.name}` : undefined)
        }
      >
        {isActive && (
          <motion.div
            layoutId="sidebar-indicator"
            className="sidebar-active-bar"
            transition={SPRING.snappy}
          />
        )}
        <Icon icon={item.icon} size={17} className="item-icon" />
        <span className="sidebar-label">{item.label}</span>

        {moduleId && (
          <span
            role="button"
            tabIndex={0}
            className={`sidebar-power-btn ${moduleOn ? 'on' : 'off'}`}
            onClick={(e) => {
              e.stopPropagation();
              handleInlineModuleToggle(moduleId, item.id);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                e.stopPropagation();
                handleInlineModuleToggle(moduleId, item.id);
              }
            }}
            title={`Turn ${moduleDef?.label ?? 'Module'} ${moduleOn ? 'OFF' : 'ON'}`}
            aria-label={`Toggle ${moduleDef?.label ?? 'module'}`}
          >
            <Icon icon="power" size={12} />
          </span>
        )}

        {dep && !dep.installed && (
          installingDeps[dep.id] ? (
            <span
              className="dep-badge dep-install-spinner"
              title={`Installing ${dep.name}…`}
              aria-label={`Installing ${dep.name}`}
            >
              <Spinner size={10} />
            </span>
          ) : (
            <span
              role="button"
              tabIndex={0}
              className="dep-badge dep-install-btn"
              onClick={(e) => handleInstallDependency(dep, e)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  handleInstallDependency(dep, e);
                }
              }}
              title={`Install ${dep.name}`}
              aria-label={`Install ${dep.name}`}
            >
              <Icon icon="download" size={10} />
            </span>
          )
        )}
      </button>
    );
  };

  return (
    <nav className={`sidebar ${score.isArmed ? 'armed' : ''} ${collapsed ? 'collapsed' : ''}`} data-tour="sidebar">
      <div className="sidebar-header">
        <button
          className="sidebar-collapse-btn"
          onClick={toggleCollapsed}
          title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        >
          <Icon icon={collapsed ? "chevron-right" : "chevron-left"} size={14} />
        </button>
      </div>

      <div className="sidebar-nav">
        {groupedNav.map((group) => (
          <div className="sidebar-group" key={group.id}>
            {group.label && <div className="sidebar-group-label">{group.label}</div>}
            {group.items.map(renderNavItem)}
          </div>
        ))}
      </div>

      <div className="sidebar-footer">
        <Suspense fallback={null}>
          <RdpQuickAction isCollapsed={collapsed} />
        </Suspense>
        {!preferencesHidden && (
          <div className="preferences-row" data-tour="persona-density-switches">
            <ExperienceLevelSwitch compact />
            <PersonaSwitch compact />
          </div>
        )}
        {isDevBuild && (
          <button
            className={`sidebar-item sidebar-item--dev ${activePanel === "dev" ? "active" : ""}`}
            onClick={() => {
              onPanelChange("dev");
              window.dispatchEvent(new CustomEvent("panel-change", { detail: "dev" }));
            }}
            title="Dev Tools (debug build only)"
          >
            {activePanel === "dev" && (
              <motion.div
                layoutId="sidebar-indicator"
                className="sidebar-active-bar"
                transition={SPRING.snappy}
              />
            )}
            <Icon icon="code" size={17} className="item-icon" />
            <span className="sidebar-label">Dev Tools</span>
          </button>
        )}
        {!licenseHidden && <LicenseQuickPanel />}
      </div>
    </nav>
  );
}
