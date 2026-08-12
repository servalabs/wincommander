// src/lib/badgeCount.ts
//
// Pure badge-count/colour math for the title-bar Alerts + Processes icons
// (see AlertsMenu.tsx / ProcessesMenu.tsx). Extracted out of the components
// so this logic — which domain contributes to which badge, and which colour
// wins — is unit-testable without rendering React/Radix (Popover needs a
// real DOM portal + effects, unnecessary ceremony for "what number, what
// colour"). Each icon's badge counts ONLY its own domain; see notifKind()
// in notificationStore.ts for how notifications route to Alerts.

import { notifKind, type AppNotification } from "./notificationStore";

export interface BadgeState {
  count: number;
  color: string;
}

/** Splits the raw notificationStore list into AlertsMenu's two sections. */
export function splitNotificationsByKind(
  notifs: AppNotification[],
): { alertNotifs: AppNotification[]; opsNotifs: AppNotification[] } {
  return {
    alertNotifs: notifs.filter((n) => notifKind(n) === "alert"),
    opsNotifs: notifs.filter((n) => notifKind(n) === "notification"),
  };
}

/** Alerts badge — count is the alert-kind subset only (genuine security
 * detections); operational "notification"-kind items render in AlertsMenu's
 * second section but never inflate this count. Danger beats warn. */
export function alertsBadge(alertNotifs: Pick<AppNotification, "severity">[]): BadgeState {
  return {
    count: alertNotifs.length,
    color: alertNotifs.some((n) => n.severity === "danger") ? "var(--danger)" : "var(--warn)",
  };
}

/** Processes badge — count is RUNNING tasks only; completed/failed tasks
 * still render in the list (their own auto-dismiss timer clears them) but
 * don't count toward "N things happening right now". Colour flags danger if
 * anything in the full list has failed, even after the running count drops. */
export function processesBadge(tasks: { status: "running" | "completed" | "failed" }[]): BadgeState {
  return {
    count: tasks.filter((t) => t.status === "running").length,
    color: tasks.some((t) => t.status === "failed") ? "var(--danger)" : "var(--accent)",
  };
}
