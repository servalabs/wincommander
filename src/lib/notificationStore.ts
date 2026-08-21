// src/lib/notificationStore.ts
//
// Lightweight, frontend-only notification store for the title-bar bell.
//
// WHY THIS EXISTS (owner decision 2026-06): alerts used to fire as Sonner
// toasts in the bottom corner — transient and undismissible. They now live in
// the bell dropdown as dismissible items ("bell-center only"); only brief
// action confirmations (success/info) still toast.
//
// This is deliberately SEPARATE from the evidence ledger (src/lib/evidence.ts).
// That ledger is the backend-persisted, cleanup-grade security timeline
// (monitors, lockdown, network detections) and must not be polluted with
// transient UI errors. This store is plain localStorage, frontend-only.

export type NotifSeverity = "info" | "warn" | "danger";

// Which section of the title-bar Alerts popover an item belongs to. "alert" =
// genuine security alerts the user MUST see; "notification" = WinCommander
// operational/app items (updates, toggle results, mounts, license notices).
// Both render in AlertsMenu (see notifKind() for the default routing); this
// no longer selects between two SEPARATE surfaces (there used to be a
// Processes tab that mixed "notification"-kind items with running tasks —
// Processes is now its own icon backed solely by TaskStatusContext, see
// ProcessesMenu.tsx) — it only decides the Alerts-badge count (alert-kind
// only) vs. which of AlertsMenu's two in-panel sections an item lands in.
export type NotifKind = "alert" | "notification";

export interface AppNotification {
  id: string;
  severity: NotifSeverity;
  message: string;
  detail?: string;
  /** Explicit section override; absent falls back to the severity-based default. */
  kind?: NotifKind;
  /** ISO-8601 timestamp. */
  time: string;
}

/**
 * Classify a notification for AlertsMenu. Explicit `kind` wins; otherwise
 * info → notification (operational), warn|danger → alert (security).
 * KT: shared by the menu and the toast callers so routing can't drift.
 */
export function notifKind(n: Pick<AppNotification, "severity" | "kind">): NotifKind {
  return n.kind ?? (n.severity === "info" ? "notification" : "alert");
}

const LS_NOTIFS = "wc-notifications";
const LS_NA_DISMISSED = "wc-needs-attention-dismissed";
/** Fired on any mutation so the bell re-reads. Also listen to `storage` for cross-window. */
export const NOTIF_CHANGED_EVENT = "wc-notifications-changed";
const MAX_NOTIFS = 50;

function readNotifs(): AppNotification[] {
  try {
    const raw = localStorage.getItem(LS_NOTIFS);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as AppNotification[]) : [];
  } catch {
    return [];
  }
}

function writeNotifs(list: AppNotification[]): void {
  try {
    localStorage.setItem(LS_NOTIFS, JSON.stringify(list.slice(0, MAX_NOTIFS)));
  } catch {
    /* quota / disabled storage — non-fatal */
  }
  window.dispatchEvent(new Event(NOTIF_CHANGED_EVENT));
}

/** Add a notification to the bell. Returns its id. Newest first. */
export function pushNotification(severity: NotifSeverity, message: string, detail?: string, kind?: NotifKind): string {
  const list = readNotifs();
  const id = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  // Dedup identical back-to-back messages (same severity + text as the newest).
  if (list[0] && list[0].severity === severity && list[0].message === message) {
    return list[0].id;
  }
  list.unshift({ id, severity, message, detail, kind, time: new Date().toISOString() });
  writeNotifs(list);
  return id;
}

/** All current bell notifications, newest first. */
export function listNotifications(): AppNotification[] {
  return readNotifs();
}

/** Remove a single notification by id. */
export function dismissNotification(id: string): void {
  writeNotifs(readNotifs().filter((n) => n.id !== id));
}

/** Clear every bell notification. */
export function clearNotifications(): void {
  writeNotifs([]);
}

// ── Popup-alert preference ───────────────────────────────────────────────
// When true (default), showError/showWarning/showInfo ALSO fire a Sonner
// toast in addition to writing to the bell. When false, they're silent —
// bell only. Stored in localStorage; toggled from Secret Settings.
const LS_POPUP_ALERTS = "wc-popup-alerts";
export const POPUP_PREF_CHANGED_EVENT = "wc-popup-alerts-changed";

export function getPopupAlertsEnabled(): boolean {
  try {
    const val = localStorage.getItem(LS_POPUP_ALERTS);
    return val === null ? true : val === "1";
  } catch {
    return true;
  }
}

export function setPopupAlertsEnabled(enabled: boolean): void {
  try {
    localStorage.setItem(LS_POPUP_ALERTS, enabled ? "1" : "0");
  } catch {
    /* quota / disabled storage — non-fatal */
  }
  window.dispatchEvent(new Event(POPUP_PREF_CHANGED_EVENT));
}

// ── Bell-hidden flag (owner 2026-06-12) ─────────────────────────────────
// Mirrors settings.app.hideNotificationBell into a synchronous, module-level
// store so the toast helpers (which aren't React components and can't read
// AppContext) can suppress popups when the bell is hidden. TitleBar keeps this
// in sync whenever the setting changes.
const LS_BELL_HIDDEN = "wc-bell-hidden";

export function getNotificationsHidden(): boolean {
  try {
    return localStorage.getItem(LS_BELL_HIDDEN) === "1";
  } catch {
    return false;
  }
}

export function setNotificationsHidden(hidden: boolean): void {
  try {
    if (hidden) localStorage.setItem(LS_BELL_HIDDEN, "1");
    else localStorage.removeItem(LS_BELL_HIDDEN);
  } catch {
    /* quota / disabled storage — non-fatal */
  }
}

// ── Popup-alerts borrowed-mode suppression ──────────────────────────────
// Separate from the user's explicit on/off pref. TitleBar sets this when
// Borrowed Mode is active and "popup-alerts" is in borrowedHidden so toasts
// are silenced without touching the user's permanent preference.
const LS_POPUP_SUPPRESSED = "wc-popup-suppressed";

export function getPopupAlertsSuppressed(): boolean {
  try {
    return localStorage.getItem(LS_POPUP_SUPPRESSED) === "1";
  } catch {
    return false;
  }
}

export function setPopupAlertsSuppressed(suppressed: boolean): void {
  try {
    if (suppressed) localStorage.setItem(LS_POPUP_SUPPRESSED, "1");
    else localStorage.removeItem(LS_POPUP_SUPPRESSED);
  } catch {
    /* non-fatal */
  }
}

// ── "Needs attention" dismissals (score-derived items) ──────────────────
// These items are recomputed live from the sovereignty score, so we can't
// delete them — we record the keys the user has dismissed and filter them out.
// A dismissed key reappears only if explicitly un-dismissed (resolving the
// underlying issue removes it from the score breakdown anyway).

function readDismissedNA(): string[] {
  try {
    const raw = localStorage.getItem(LS_NA_DISMISSED);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as string[]) : [];
  } catch {
    return [];
  }
}

export function getDismissedNeedsAttention(): Set<string> {
  return new Set(readDismissedNA());
}

export function dismissNeedsAttention(key: string): void {
  const set = new Set(readDismissedNA());
  set.add(key);
  try {
    localStorage.setItem(LS_NA_DISMISSED, JSON.stringify([...set]));
  } catch {
    /* non-fatal */
  }
  window.dispatchEvent(new Event(NOTIF_CHANGED_EVENT));
}

/** Re-show all dismissed needs-attention items. */
export function resetDismissedNeedsAttention(): void {
  try {
    localStorage.removeItem(LS_NA_DISMISSED);
  } catch {
    /* non-fatal */
  }
  window.dispatchEvent(new Event(NOTIF_CHANGED_EVENT));
}

/** Subscribe to any store change (this window + other windows). Returns an unsubscribe fn. */
export function subscribeNotifications(cb: () => void): () => void {
  window.addEventListener(NOTIF_CHANGED_EVENT, cb);
  window.addEventListener("storage", cb);
  return () => {
    window.removeEventListener(NOTIF_CHANGED_EVENT, cb);
    window.removeEventListener("storage", cb);
  };
}
