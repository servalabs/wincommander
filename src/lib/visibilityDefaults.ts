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
//   • Productivity + Server Apps + Flows → hidden ALWAYS by default
//     (Secret Settings is hidden-until-revealed via its own 5×-click gate, so
//      it is not listed here.) Flows is the Pro automation surface — revealed
//      from the Secret Settings visibility table.
//   • When Borrowed Mode is active, only dashboard / tweaks / apps / system-identity
//     remain visible; all other panels are hidden.
//   • Right-sidebar: only Search + AI Advisor remain when borrowed; all
//     destructive/sensitive quick-actions are hidden.

// Panels hidden while Borrowed Mode is active. Everything except the four
// "safe" panels (dashboard, tweaks, apps, system-identity).
import type { PanelId } from "../types/panels";

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
  "secret",
] satisfies PanelId[];

export const DEFAULT_ALWAYS_PANELS = ["productivity", "server-apps", "flows"] satisfies PanelId[];

// Right-sidebar surfaces hidden when borrowed. ai-advisor and search are
// intentionally absent — they stay visible while borrowed.
export const DEFAULT_BORROWED_EXTRAS: string[] = [
  "action:dismount",
  "action:delete",
  "action:scrubMeta",
  "action:lockdown",
  "popup-alerts",
  "desktop-alerts",
  "engines-section",
];
