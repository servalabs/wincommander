// src/lib/visibilityDefaults.ts
//
// Default panel / feature visibility, applied ONLY when the user has never
// configured it (the stored setting is undefined/null). An explicit array —
// even an empty one — means the user set it and overrides these.
//
// Kept in ONE place so the sidebar, the Secret Settings table, the command
// palette, and every borrowed-mode consumer agree on the starting state.
//
// Semantics requested by the owner:
//   • A new installation starts with every ordinary panel visible. The user
//     can still hide a panel explicitly in Secret Settings. (Secret Settings
//     itself remains hidden-until-revealed via its own 5×-click gate.)
//   • When Borrowed Mode is active, only dashboard / tweaks / apps / system-identity
//     remain visible; all other panels are hidden.
//   • Borrowed Mode also conceals the dashboard's alternate views, all
//     notification surfaces, and the sidebar's persona/interface controls.
//   • Right-sidebar: only Search remains when borrowed; all other quick
//     actions are hidden.

// Panels hidden while Borrowed Mode is active. Everything except the four
// "safe" panels (dashboard, tweaks, apps, system-identity).
import type { PanelId } from "../types/panels";

// "secret" is intentionally absent — it is gated solely by the title-bar
// 5× brand-click reveal, never by Borrowed Mode (see Sidebar.tsx).
export const DEFAULT_BORROWED_PANELS = [
  "privacy",
  "network",
  "cleanup",
  "vault",
  "private-mesh",
  "server-apps",
  "productivity",
  "flows",
  "fleet",
] satisfies PanelId[];

// An explicit persisted array, including an empty one, is always respected.
// This is only the unconfigured first-run fallback.
export const DEFAULT_ALWAYS_PANELS = [] satisfies PanelId[];

// Extra surfaces hidden when borrowed. This covers the dashboard's alternate
// views, every notification surface, and the sidebar's persona/interface
// controls as well as the sensitive quick actions.
export const DEFAULT_BORROWED_EXTRAS: string[] = [
  "risk-matrix",
  "more-products",
  "notif-bell",
  "popup-alerts",
  "desktop-alerts",
  "sidebar-preferences",
  "action:ai-advisor",
  "action:dismount",
  "action:delete",
  "action:scrubMeta",
  "action:lockdown",
  "engines-section",
];

// AI Advisor starts hidden everywhere. An explicit empty persisted list still
// means the person using the PC chose to show it.
export const DEFAULT_ALWAYS_HIDDEN_SIDEBAR_ACTIONS = ["ai-advisor"];
