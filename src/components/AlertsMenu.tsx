// src/components/AlertsMenu.tsx
//
// Alerts — title-bar popover for the notificationStore domain. Split out of
// the old NotificationsMenu (one bell, two tabs: "System Alerts" +
// "Processes") per owner request: "Instead of 1 notification icon with 2
// tabs, have 2 icons... Alerts and Processes." See ProcessesMenu.tsx for the
// TaskStatusContext counterpart — that domain no longer touches this file.
//
// Badge counts ONLY alert-kind items (notifKind(n) === "alert") — genuine
// security detections the user must see, mirroring the old "System Alerts"
// tab's count exactly. Operational notices (kind === "notification", e.g.
// "mount failed", "update check failed" — pushed via showError/showWarning
// with an explicit `{ kind: "notification" }` override from call sites this
// agent does not own) still need a home now that the old second tab is gone;
// they render below in their own "Notifications" section so they're never
// silently dropped, they just don't inflate the security-alert badge count.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Popover, PopoverTrigger, PopoverContent } from "./ui/popover";
import { Icon } from "./ui/icon";
import { NotificationRow } from "./NotificationRow";
import { TITLEBAR_ICON_BTN } from "./ui/titleBarButtonClass";
import {
  listNotifications,
  dismissNotification,
  subscribeNotifications,
  type AppNotification,
} from "../lib/notificationStore";
import { alertsBadge, splitNotificationsByKind } from "../lib/badgeCount";
import "./badgePulse.css";

export default function AlertsMenu() {
  const [open, setOpen] = useState(false);
  // Tracks whether the badge should play its arrive animation. Fires once
  // when badgeCount crosses 0→positive while the menu is closed (see
  // NotificationsMenu's original comment — behaviour carried over verbatim).
  const [badgePulsing, setBadgePulsing] = useState(false);
  const prevBadgeRef = useRef(0);
  const [notifs, setNotifs] = useState<AppNotification[]>(() => listNotifications());

  const { alertNotifs, opsNotifs } = useMemo(() => splitNotificationsByKind(notifs), [notifs]);
  const { count: badgeCount, color: badgeColor } = useMemo(() => alertsBadge(alertNotifs), [alertNotifs]);

  useEffect(() => {
    const onToggle = () => setOpen((o) => !o);
    window.addEventListener("toggle-alerts-menu", onToggle);
    return () => window.removeEventListener("toggle-alerts-menu", onToggle);
  }, []);

  useEffect(() => {
    const sync = () => setNotifs(listNotifications());
    sync();
    return subscribeNotifications(sync);
  }, []);

  useEffect(() => {
    const prev = prevBadgeRef.current;
    prevBadgeRef.current = badgeCount;
    if (!open && prev === 0 && badgeCount > 0) {
      setBadgePulsing(true);
      const id = window.setTimeout(() => setBadgePulsing(false), 320);
      return () => window.clearTimeout(id);
    }
  }, [badgeCount, open]);

  const dismissOne = useCallback((e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    dismissNotification(id);
  }, []);

  const clearAlerts = useCallback(() => {
    alertNotifs.forEach((n) => dismissNotification(n.id));
  }, [alertNotifs]);

  const clearOps = useCallback(() => {
    opsNotifs.forEach((n) => dismissNotification(n.id));
  }, [opsNotifs]);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className={`relative ${TITLEBAR_ICON_BTN}`} title="Alerts" aria-label="Alerts" data-tauri-drag-region={false}>
        <Icon icon="notifications" size={15} />
        {badgeCount > 0 && (
          <span
            key={badgePulsing ? "pulse" : "idle"}
            className={`absolute right-1 top-1 grid h-3.5 min-w-[14px] place-items-center rounded-full px-0.5 font-[family-name:var(--font-mono)] text-[8px] font-bold leading-none text-[var(--accent-contrast)]${badgePulsing ? " wc-badge-pulse" : ""}`}
            style={{ background: badgeColor }}
          >
            {badgeCount}
          </span>
        )}
      </PopoverTrigger>
      {/* Caret is .wc-popover-caret::before (index.css) — shared with
          ProcessesMenu so both popups connect to their own trigger with the
          same diamond shape. */}
      <PopoverContent align="end" sideOffset={8} className="wc-popover-caret w-80 p-0 relative overflow-visible duration-[var(--dur-normal)] [animation-timing-function:var(--ease)]">
        <div className="flex items-center gap-2 border-b border-[var(--border)] px-3.5 py-2.5">
          <Icon icon="notifications" size={13} style={{ color: "var(--text-dim)" }} />
          <div className="text-[13px] font-semibold text-[var(--text)]">Alerts</div>
          {badgeCount > 0 && (
            <span className="grid h-[18px] min-w-[18px] place-items-center rounded-full bg-[var(--surface-3)] px-1.5 font-[family-name:var(--font-mono)] text-[10px] font-bold leading-none text-[var(--text-dim)]">
              {badgeCount}
            </span>
          )}
          <button
            type="button"
            onClick={() => setOpen(false)}
            title="Close"
            aria-label="Close alerts"
            className="ml-auto grid h-6 w-6 shrink-0 place-items-center rounded-[var(--r-sm)] text-[var(--text-mute)] transition-colors hover:bg-[var(--surface-2)] hover:text-[var(--text)]"
          >
            <Icon icon="cross" size={12} />
          </button>
        </div>

        <div className="max-h-[420px] overflow-y-auto p-2">
          {alertNotifs.length > 0 && (
            <div className="mb-2 flex flex-col gap-1.5">
              <div className="flex items-center justify-between px-0.5">
                <div className="font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--text-mute)]">
                  Alerts
                </div>
                <button
                  onClick={clearAlerts}
                  className="font-[family-name:var(--font-mono)] text-[10px] text-[var(--text-mute)] underline underline-offset-2 transition-colors hover:text-[var(--text-dim)]"
                >
                  Clear all
                </button>
              </div>
              {alertNotifs.map((n) => (
                <NotificationRow key={n.id} notification={n} onDismiss={dismissOne} />
              ))}
            </div>
          )}

          {opsNotifs.length > 0 && (
            <div className="mb-2 flex flex-col gap-1.5">
              <div className="flex items-center justify-between px-0.5">
                <div className="font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--text-mute)]">
                  Notifications
                </div>
                <button
                  onClick={clearOps}
                  className="font-[family-name:var(--font-mono)] text-[10px] text-[var(--text-mute)] underline underline-offset-2 transition-colors hover:text-[var(--text-dim)]"
                >
                  Clear all
                </button>
              </div>
              {opsNotifs.map((n) => (
                <NotificationRow key={n.id} notification={n} onDismiss={dismissOne} />
              ))}
            </div>
          )}

          {alertNotifs.length === 0 && opsNotifs.length === 0 && (
            <div className="py-8 text-center text-[11.5px] text-[var(--text-mute)]">No pending alerts or notifications.</div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
