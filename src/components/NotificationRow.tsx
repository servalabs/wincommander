// src/components/NotificationRow.tsx
//
// Single dismissible notification row — severity icon, message/detail,
// relative timestamp, dismiss button. Extracted out of the old
// NotificationsMenu so AlertsMenu's two sections (Alerts / Notifications,
// see AlertsMenu.tsx) render the identical row markup from one place instead
// of two inline copies.

import { Icon } from "./ui/icon";
import type { AppNotification, NotifSeverity } from "../lib/notificationStore";

const NOTIF_TONE_VAR: Record<NotifSeverity, "danger" | "warn" | "accent"> = {
  info: "accent",
  warn: "warn",
  danger: "danger",
};

const NOTIF_ICON: Record<NotifSeverity, string> = {
  info: "info-sign",
  warn: "warning-sign",
  danger: "error",
};

function relativeTime(iso: string): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return iso;
  const secs = Math.max(0, Math.round((Date.now() - then) / 1000));
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.round(hrs / 24)}d ago`;
}

interface NotificationRowProps {
  notification: AppNotification;
  onDismiss: (e: React.MouseEvent, id: string) => void;
}

export function NotificationRow({ notification: n, onDismiss }: NotificationRowProps) {
  const toneVar = NOTIF_TONE_VAR[n.severity];
  return (
    <div className="group flex items-start gap-2.5 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-2.5">
      <div
        className="mt-0.5 grid h-7 w-7 shrink-0 place-items-center rounded-[var(--r-sm)]"
        style={{
          background: `color-mix(in srgb, var(--${toneVar}) 15%, transparent)`,
          color: `var(--${toneVar})`,
        }}
      >
        <Icon icon={NOTIF_ICON[n.severity]} size={14} />
      </div>
      <div className="flex flex-1 flex-col gap-0.5 min-w-0">
        <div className="text-[12px] text-[var(--text)] break-words">{n.message}</div>
        {n.detail ? <div className="text-[10.5px] text-[var(--text-mute)] break-words">{n.detail}</div> : null}
        <div className="font-[family-name:var(--font-mono)] text-[9.5px] text-[var(--text-mute)]">{relativeTime(n.time)}</div>
      </div>
      <button
        onClick={(e) => onDismiss(e, n.id)}
        title="Dismiss"
        aria-label="Dismiss notification"
        className="mt-0.5 grid h-5 w-5 shrink-0 place-items-center rounded-[var(--r-sm)] text-[var(--text-mute)] transition-colors hover:bg-[var(--surface-3)] hover:text-[var(--text)]"
      >
        <Icon icon="cross" size={12} />
      </button>
    </div>
  );
}
