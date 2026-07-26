// src/types/panels.ts
//
// ═══════════════════════════════════════════════════════════════════════
// PanelManifest — Declares everything about a panel in ONE place
// ═══════════════════════════════════════════════════════════════════════
//
// PROBLEM THIS SOLVES:
// Adding a new panel used to require editing 5 separate files:
//   1. App.tsx (lazy import + PanelId type + switch case + preload array)
//   2. Sidebar.tsx (navItems array + searchKeywords record)
//   3. useActivePanelPoller.ts (refresh mapping)
//   4. Panel component itself
//
// NOW, you only create ONE manifest + the panel component. Everything
// else auto-generates from the manifest.
//
// ANALOGY: Think of PanelManifest like a "passport" for each panel.
// Every service (sidebar, routing, polling, prefetch) reads the passport
// instead of maintaining its own list of who's allowed in.
//
// HOW TO ADD A NEW PANEL:
//   1. Create your panel component: src/panels/my-panel/index.tsx
//   2. Add a manifest to PANEL_MANIFESTS below
//   3. Done. The sidebar, router, poller, and prefetch all pick it up.

import type { IconName } from "@/components/ui/bp";
import { isVisible, type Visibility, type VisibilityCtx } from "../lib/visibility";

export type PanelId =
  | "dashboard"
  | "privacy"
  | "network"
  | "tweaks"
  | "apps"
  | "vault"
  | "private-mesh"
  | "system-identity"
  | "productivity"
  | "fleet"
  | "server-apps"
  | "search-files"
  | "maintenance"
  | "cleanup"
  | "sidecar"
  | "advisor"
  | "secret"
  | "flows"
  | "dev";

export interface PanelManifest {
  /** Unique panel identifier — used for routing, persistence, and state keys */
  id: PanelId;

  /** Display label in the sidebar */
  label: string;

  /** Blueprint icon name for the sidebar */
  icon: IconName;

  /**
   * Lazy import function for code-splitting.
   * Example: () => import("../panels/dashboard")
   *
   * This is both used by React.lazy AND by preloadAllPanels().
   */
  importFn: () => Promise<{ default: React.ComponentType }>;

  /**
   * Search keywords for the sidebar search bar.
   * Lets users find this panel by typing related terms.
   */
  searchKeywords?: string[];

  /**
   * If true, panel is wrapped in DependencyGate component.
   * DependencyGate checks if required software
   * is installed before showing the panel.
   */
  requiresDependency?: boolean;

  /**
   * Refresh function key from AppContext — called on hover prefetch
   * and by the active panel poller (10s interval).
   * Maps to a function on the AppContext like "refreshPrivacy".
   *
   * If undefined, no automatic refresh occurs for this panel.
   */
  refreshKey?: string;

  /**
   * Panel order in the sidebar. Lower numbers appear first.
   * Gaps are fine (10, 20, 30...) — makes inserting new panels easy.
   */
  order: number;

  /** IA tier used by the redesigned sidebar. Covert panels never render in the rail. */
  navTier?: "primary" | "capability" | "role" | "hidden" | "covert";

  /** New resolver descriptor. Kept optional while legacy panel code migrates. */
  visibility?: Visibility;
}

// ═══════════════════════════════════════════════════════════════════════
// THE MANIFEST — Single source of truth for all 12 panels
// ═══════════════════════════════════════════════════════════════════════
//
// ORDER is controlled by the `order` field. The sidebar renders panels
// in ascending order. Use gaps (10, 20, 30...) so new panels can be
// inserted between existing ones without renumbering everything.

export const PANEL_MANIFESTS: PanelManifest[] = [
  {
    id: "dashboard",
    label: "Dashboard",
    icon: "dashboard",
    importFn: () => import("../panels/dashboard"),
    navTier: "primary",
    searchKeywords: ["overview", "status", "risk", "map"],
    refreshKey: "refreshDashboard",
    order: 10,
  },
  {
    id: "privacy",
    label: "Privacy Settings",
    icon: "shield",
    importFn: () => import("../panels/privacy"),
    navTier: "primary",
    searchKeywords: ["telemetry", "cleanup", "passwords", "ai", "shield", "cam", "mic", "capabilities"],
    refreshKey: "refreshPrivacy",
    order: 27,
  },
  {
    id: "network",
    label: "Network Control",
    icon: "globe-network",
    importFn: () => import("../panels/network"),
    navTier: "primary",
    searchKeywords: ["dns", "firewall", "hosts", "block", "ip", "wifi", "wlan"],
    refreshKey: "refreshNetwork",
    order: 50,
  },
  {
    id: "tweaks",
    label: "Windows Settings",
    icon: "time",
    importFn: () => import("../panels/tweaks"),
    navTier: "primary",
    searchKeywords: ["power", "explorer", "mouse", "keyboard", "ads", "bing", "update", "defender", "taskbar", "end task", "debloat", "context menu"],
    refreshKey: "refreshHardening",
    order: 20,
  },
  {
    id: "apps",
    label: "Packages & Apps",
    icon: "applications",
    importFn: () => import("../panels/apps"),
    navTier: "primary",
    searchKeywords: ["install", "winget", "update", "upgrade", "uninstall", "browser"],
    requiresDependency: true,
    order: 60,
  },
  {
    id: "vault",
    label: "Secure Storage",
    icon: "lock",
    importFn: () => import("../panels/vault"),
    navTier: "capability",
    searchKeywords: ["encrypt", "mount", "container", "secure"],
    refreshKey: "refreshVault",
    order: 110,
  },
  {
    id: "private-mesh",
    label: "Private Network",
    icon: "ip-address",
    importFn: () => import("../panels/mesh"),
    navTier: "capability",
    visibility: { capability: ["network"] },
    searchKeywords: ["vpn", "mesh", "device", "share", "relay"],
    refreshKey: "refreshMesh",
    requiresDependency: true,
    order: 120,
  },
  {
    // Productivity / Activity Watch. Governed by the Secret Settings visibility
    // table like every other panel (defaults to "Always" hidden via
    // visibilityDefaults) — no longer hardwired-hidden via navTier.
    id: "productivity",
    label: "Productivity",
    icon: "timeline-events",
    importFn: () => import("../panels/productivity"),
    navTier: "capability",
    searchKeywords: ["activity", "track", "focus", "time", "summary", "analytics", "tempo"],
    refreshKey: "refreshProductivity",
    requiresDependency: true,
    order: 80,
  },
  {
    id: "server-apps",
    label: "Saved Sites",
    icon: "cloud",
    importFn: () => import("../panels/server-apps"),
    navTier: "capability",
    visibility: { capability: ["network"] },
    searchKeywords: ["gallery", "immich", "cloud", "nextcloud", "firewall", "home", "assistant", "pdf", "sync"],
    order: 90,
  },
  {
    id: "search-files",
    label: "Search Files",
    icon: "search",
    importFn: () => import("../panels/search-files"),
    // Moved off the left rail 2026-06-09 (owner request): reached from the
    // right sidebar launcher instead. `hidden` keeps it routable via
    // navigate-panel without listing it in the left nav.
    navTier: "hidden",
    visibility: { minDensity: "expert" },
    searchKeywords: ["everything", "find", "locate", "search", "file", "folder", "documents", "instant"],
    requiresDependency: true,
    order: 100,
  },
  {
    id: "maintenance",
    label: "Maintenance",
    icon: "wrench",
    importFn: () => import("../panels/maintenance"),
    navTier: "primary",
    searchKeywords: [
      "cache", "cleaner", "temporary files", "browser cache", "shader cache", "sqlite", "routine",
      "disk cleanup", "reclaim space", "recycle bin", "windows.old", "duplicates", "disk space analyzer",
      "repair", "sfc", "dism", "windows update repair", "defrag", "trim", "shortcuts", "path", "leftovers",
      "registry", "context menu", "malware", "defender", "startup", "driver", "performance", "processes",
    ],
    order: 31,
  },
  {
    id: "cleanup",
    label: "System Cleanup",
    icon: "clean",
    importFn: () => import("../panels/cleanup"),
    navTier: "primary",
    searchKeywords: ["privacy-clean", "shellbags", "usb", "bluetooth", "resource usage", "event logs", "secure-delete", "erase", "traces", "amcache", "prefetch", "lockdown"],
    order: 30,
  },
  {
    // Flows v2 (2026-07): rebuilt as a Pro-backed engine. The pure rule core
    // (flow-core) + runtime live in the commander-pro sidecar; this panel is a
    // thin client over the Free `flow_bridge` commands. Replaces the shelved,
    // broken local node-graph engine. Paid feature — the panel self-gates.
    id: "flows",
    label: "Flows",
    icon: "data-lineage",
    importFn: () => import("../panels/flows"),
    navTier: "capability",
    searchKeywords: ["flow", "automation", "trigger", "action", "rule", "if this then that", "gaze", "telemetry", "usb", "signal"],
    order: 110,
  },
  {
    // #10 AI Security Advisor — local Ollama LLM. Gated by
    // DependencyGate(localLlm) + the paid Llm-Analyze Pro handler.
    id: "advisor",
    label: "AI Advisor",
    icon: "predictive-analysis",
    importFn: () => import("../panels/advisor"),
    // Moved to the right-sidebar launcher 2026-06-09 (owner). `hidden` keeps it
    // routable via navigate-panel without listing it in the left rail.
    navTier: "hidden",
    visibility: { minDensity: "expert" },
    searchKeywords: ["ai", "llm", "ollama", "explain", "harden", "anomaly", "advisor", "security"],
    requiresDependency: true,
    order: 116,
  },
  {
    // Fleet — ENROLL this device into a self-hosted WinCommander Fleet server.
    // Enroll-only (Tauri `fleet_connect`); the fleet ADMIN console now lives in
    // the web app served by the fleet-server itself (fleet-server/console),
    // reached at the server's own origin, not inside WinCommander.
    id: "fleet",
    label: "Fleet",
    icon: "office",
    importFn: () => import("../panels/fleet"),
    navTier: "capability",
    searchKeywords: [
      "fleet", "enroll", "join", "enterprise", "central", "management",
      "device", "mdm", "remote", "org", "server", "connect",
    ],
    order: 124,
  },
  {
    // Secret Settings — single home for hidden/decoy/concealment features
    // (Calculator Mode, app/panel visibility, Borrowed-PC mode, sidebar-action
    // visibility). Added 2026-06-12 when the owner consolidated these out of
    // the Privacy and Settings panels.
    id: "secret",
    label: "Secret Settings",
    icon: "eye-off",
    importFn: () => import("../panels/secret"),
    navTier: "primary",
    searchKeywords: ["secret", "hidden", "decoy", "conceal", "calculator", "pin", "borrowed", "visibility", "stealth", "cover"],
    order: 125,
  },
  {
    // ID stays `system-identity` for persisted settings and import paths.
    // The redesign promotes this surface to the Settings rail entry. It
    // still owns branding, sidebar lock, product showcase, licence, and cover controls.
    id: "system-identity",
    label: "Settings",
    icon: "cog",
    importFn: () => import("../panels/identity"),
    navTier: "primary",
    searchKeywords: ["settings", "developer", "debug", "activation", "branding", "oem", "windows", "office", "identity"],
    order: 130,
  },
  {
    // DEV panel — only shown when is_dev_build() returns true.
    // navTier "hidden" keeps it routable but off the rail; the panel
    // mounts a useEffect that calls is_dev_build() and emits a
    // "navigate-panel:dev" gate if the binary is a release build.
    // In the sidebar, DevPanelLink wires a conditional entry.
    id: "dev",
    label: "Dev Tools",
    icon: "code",
    importFn: () => import("../panels/dev"),
    navTier: "hidden",
    searchKeywords: ["dev", "debug", "test", "simulate", "reset", "consent", "entitlement", "log"],
    order: 999,
  },
];

// ── Helper: get sorted manifests ─────────────────────────────────────

/** Returns manifests sorted by their `order` field */
export function getSortedManifests(): PanelManifest[] {
  return [...PANEL_MANIFESTS].sort((a, b) => a.order - b.order);
}

/** Primary cover IA entries shown before capability and role surfaces. */
export function getPrimaryManifests(): PanelManifest[] {
  return getSortedManifests().filter((manifest) => manifest.navTier === "primary");
}

/** Rail-visible manifests. Covert and disabled surfaces stay routable but unlisted. */
export function getSidebarManifests(ctx?: VisibilityCtx): PanelManifest[] {
  return getSortedManifests().filter(
    (manifest) =>
      manifest.navTier !== "hidden" &&
      manifest.navTier !== "covert" &&
      (!ctx || isVisible(manifest.visibility, ctx)),
  );
}

/** Find a manifest by panel ID */
export function getManifestById(id: PanelId): PanelManifest | undefined {
  return PANEL_MANIFESTS.find((m) => m.id === id);
}

// ── V2 sidebar grouping (Monitor / Protect / Secure / System + footer) ───
// The redesigned sidebar renders nav items under group headers. Groups are
// derived from panel id (one place) rather than stored on every manifest.

export type NavGroup = "monitor" | "protect" | "secure" | "system" | "footer";

/** Ordered groups + their display labels (footer label is blank — no header). */
export const NAV_GROUP_ORDER: { id: NavGroup; label: string }[] = [
  { id: "monitor", label: "Monitor" },
  { id: "protect", label: "Protect" },
  { id: "secure", label: "Secure" },
  { id: "system", label: "System" },
  { id: "footer", label: "" },
];

const NAV_GROUP_BY_ID: Partial<Record<PanelId, NavGroup>> = {
  dashboard: "monitor",
  privacy: "protect",
  network: "protect",
  "private-mesh": "protect",
  cleanup: "protect",
  maintenance: "system",
  vault: "secure",
  "server-apps": "secure",
  sidecar: "secure",
  tweaks: "system",
  apps: "system",
  fleet: "system",
  secret: "system",
  flows: "system",
  "system-identity": "footer",
  // advisor + search-files are rail-only (not listed); productivity/dev hidden.
  // flows is hidden-by-default (DEFAULT_ALWAYS_PANELS) and revealed from the
  // Secret Settings visibility table; when shown it sits in the System group.
  dev: "system",
};

/** Which group a panel belongs to (defaults to "system"). */
export function navGroupFor(id: PanelId): NavGroup {
  return NAV_GROUP_BY_ID[id] ?? "system";
}

/** Rail-visible manifests bucketed into ordered groups; empty groups dropped. */
export function getGroupedSidebarManifests(
  ctx?: VisibilityCtx,
): { group: NavGroup; label: string; items: PanelManifest[] }[] {
  const visible = getSidebarManifests(ctx);
  return NAV_GROUP_ORDER.map((g) => ({
    group: g.id,
    label: g.label,
    items: visible.filter((m) => navGroupFor(m.id) === g.id),
  })).filter((g) => g.items.length > 0);
}
