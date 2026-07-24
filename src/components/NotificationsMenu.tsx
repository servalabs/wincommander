import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Popover, PopoverTrigger, PopoverContent } from "./ui/popover";
import { Icon } from "./ui/icon";
import { useSovereigntyScore } from "../hooks/useSovereigntyScore";
import { useTaskStatus } from "../context/TaskStatusContext";
import {
  listNotifications,
  dismissNotification,
  subscribeNotifications,
  notifKind,
  type AppNotification,
  type NotifSeverity,
} from "../lib/notificationStore";
import { ProcessChip } from "./ui/ProcessChip";
import "./NotificationsMenu.css";

const NOTIF_TONE: Record<NotifSeverity, "ok" | "warn" | "danger"> = {
  info: "ok",
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

/**
 * NotificationsMenu — V2 title-bar alert dropdown. Bell-anchored popover with two
 * tabs: System Alerts (security detections the user MUST see) and Notifications
 * (operational/app items + live task progress). Responds to the global
 * `toggle-notifications` event (fired by the command palette).
 */
export default function NotificationsMenu({ className }: { className?: string }) {
  const [open, setOpen] = useState(false);
  const [section, setSection] = useState<"alerts" | "notifications">("alerts");
  // Tracks whether the badge should play its arrive animation. We fire it once
  // when badgeCount crosses 0→positive while the menu is closed, then clear it
  // so a subsequent identical count doesn't re-trigger (animation: none resets
  // the CSS animation state on the next render cycle).
  const [badgePulsing, setBadgePulsing] = useState(false);
  const prevBadgeRef = useRef(0);
  const [notifs, setNotifs] = useState<AppNotification[]>(() => listNotifications());
  const score = useSovereigntyScore();
  const { tasks, clearCompleted } = useTaskStatus();
  const runningTasks = useMemo(() => tasks.filter((t) => t.status === "running"), [tasks]);
  // Kind-derived split for the two tabs — no new data model, just a view over
  // the existing notifs array via the shared notifKind() classifier.
  const alertNotifs = useMemo(() => notifs.filter((n) => notifKind(n) === "alert"), [notifs]);
  const notificationNotifs = useMemo(() => notifs.filter((n) => notifKind(n) === "notification"), [notifs]);
  const activeNotifs = section === "alerts" ? alertNotifs : notificationNotifs;

  useEffect(() => {
    const onToggle = () => setOpen((o) => !o);
    window.addEventListener("toggle-notifications", onToggle);
    return () => window.removeEventListener("toggle-notifications", onToggle);
  }, []);

  useEffect(() => {
    const sync = () => setNotifs(listNotifications());
    sync();
    return subscribeNotifications(sync);
  }, []);

  const badgeCount = notifs.length + runningTasks.length;

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

  // Clear only the notifs visible in the active tab — a global clear would wipe
  // both tabs at once, which isn't the per-tab "clear this section" intent.
  const clearActiveSection = useCallback(() => {
    activeNotifs.forEach((n) => dismissNotification(n.id));
  }, [activeNotifs]);

  // Tasks live at the top of the Notifications tab only.
  const showTasks = section === "notifications" && tasks.length > 0;

  const winBtn = "grid place-items-center w-8 h-8 rounded-[var(--r-sm)] text-[var(--text-mute)] hover:text-[var(--text)] hover:bg-[var(--surface-2)] transition-colors duration-150";

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className={`relative ${className ?? winBtn}`} title="Notifications" aria-label="Notifications">
        <Icon icon="notifications" size={15} />
        {badgeCount > 0 && (
          <span
            /*
             * wc-badge-pulse fires once on 0→positive transition (controlled by
             * badgePulsing state). Key changes when pulsing so React unmounts/
             * remounts the element, guaranteeing the CSS animation restarts even
             * if the count stays the same value after a clear+re-add cycle.
             */
            key={badgePulsing ? "pulse" : "idle"}
            className={`absolute right-1 top-1 grid h-3.5 min-w-[14px] place-items-center rounded-full px-0.5 font-[family-name:var(--font-mono)] text-[8px] font-bold leading-none text-[var(--accent-contrast)]${badgePulsing ? " wc-badge-pulse" : ""}`}
            style={{
              background: notifs.some((n) => n.severity === "danger")
                ? "var(--danger)"
                : notifs.length === 0 && runningTasks.length > 0
                  ? "var(--accent)"
                  : "var(--warn)",
            }}
          >
            {badgeCount}
          </span>
        )}
      </PopoverTrigger>
      {/*
        * duration-[var(--dur-normal)] + [animation-timing-function:var(--ease)]
        * align this overlay with the dialog feel (see ui/dialog.tsx).
        * The base animate-in/out fade+zoom classes come from ui/popover.tsx;
        * we only add the canonical token overrides here so this surface uses
        * the same 200ms / standard-curve cadence as every other overlay.
        */}
      <PopoverContent align="end" sideOffset={8} className="wc-notif-caret w-80 p-0 relative overflow-visible duration-[var(--dur-normal)] [animation-timing-function:var(--ease)]">
        {/* Caret is .wc-notif-caret::before (index.css) — the SAME rotated-square
            diamond the near-bell toast uses, so both popups connect to the bell
            with one identical shape instead of two different carets. */}
        <div className="flex items-center gap-2 border-b border-[var(--border)] px-3.5 py-2.5">
          {/* Bell echoes the trigger so the panel reads as "the bell's contents". */}
          <Icon icon="notifications" size={13} style={{ color: "var(--text-dim)" }} />
          <div className="text-[13px] font-semibold text-[var(--text)]">Notifications</div>
          {badgeCount > 0 ? (
            <span className="grid h-[18px] min-w-[18px] place-items-center rounded-full bg-[var(--surface-3)] px-1.5 font-[family-name:var(--font-mono)] text-[10px] font-bold leading-none text-[var(--text-dim)]">
              {badgeCount}
            </span>
          ) : (
            <span className="font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--text-mute)]">
              All clear
            </span>
          )}
          {/* Close sits alone in the far corner (ml-auto) so it clearly means
              "close the panel" — not "clear" — separated from the row's count. */}
          <button
            type="button"
            onClick={() => setOpen(false)}
            title="Close"
            aria-label="Close notifications"
            className="ml-auto grid h-6 w-6 shrink-0 place-items-center rounded-[var(--r-sm)] text-[var(--text-mute)] transition-colors hover:bg-[var(--surface-2)] hover:text-[var(--text)]"
          >
            <Icon icon="cross" size={12} />
          </button>
        </div>

        <div className="max-h-[420px] overflow-y-auto p-2">
          {/* Two-tab toggle: System Alerts (security) vs Notifications (app/ops). */}
          <div className="wc-notif-tabs mb-2">
            <button
              type="button"
              onClick={() => setSection("alerts")}
              className={`wc-notif-tab${section === "alerts" ? " wc-notif-tab-active" : ""}`}
            >
              System Alerts
              {alertNotifs.length > 0 && <span className="wc-notif-tab-count">{alertNotifs.length}</span>}
            </button>
            <button
              type="button"
              onClick={() => setSection("notifications")}
              className={`wc-notif-tab${section === "notifications" ? " wc-notif-tab-active" : ""}`}
            >
              Processes
              {notificationNotifs.length + runningTasks.length > 0 && (
                <span className="wc-notif-tab-count">{notificationNotifs.length + runningTasks.length}</span>
              )}
            </button>
          </div>

          {/* Tasks — in-flight + recently-finished (runOperation pipeline).
              Notifications tab only; they are not System Alerts. */}
          {showTasks && (
            <div className="mb-2 flex flex-col gap-1.5">
              <div className="flex items-center justify-between px-0.5">
                <div className="font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--text-mute)]">
                  Active {runningTasks.length > 0 ? `· ${runningTasks.length} running` : ""}
                </div>
                {tasks.some((t) => t.status === "completed" || t.status === "failed") && (
                  <button
                    onClick={() => clearCompleted()}
                    className="font-[family-name:var(--font-mono)] text-[10px] text-[var(--text-mute)] underline underline-offset-2 transition-colors hover:text-[var(--text-dim)]"
                  >
                    Clear finished
                  </button>
                )}
              </div>
              {tasks.map((task) => (
                <ProcessChip key={task.id} task={task} />
              ))}
            </div>
          )}

          {/* Active tab's notifs — dismissible */}
          {activeNotifs.length > 0 && (
            <div className="mb-2 flex flex-col gap-1.5">
              <div className="flex items-center justify-between px-0.5">
                <div className="font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--text-mute)]">
                  {section === "alerts" ? "Alerts" : "Notifications"}
                </div>
                <button
                  onClick={clearActiveSection}
                  className="font-[family-name:var(--font-mono)] text-[10px] text-[var(--text-mute)] underline underline-offset-2 transition-colors hover:text-[var(--text-dim)]"
                >
                  Clear all
                </button>
              </div>
              {activeNotifs.map((n) => (
                <div
                  key={n.id}
                  className="group flex items-start gap-2.5 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-2.5"
                >
                  <div
                    className="mt-0.5 grid h-7 w-7 shrink-0 place-items-center rounded-[var(--r-sm)]"
                    style={{
                      background: `color-mix(in srgb, var(--${NOTIF_TONE[n.severity] === "danger" ? "danger" : NOTIF_TONE[n.severity] === "warn" ? "warn" : "accent"}) 15%, transparent)`,
                      color: `var(--${NOTIF_TONE[n.severity] === "danger" ? "danger" : NOTIF_TONE[n.severity] === "warn" ? "warn" : "accent"})`,
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
                    onClick={(e) => dismissOne(e, n.id)}
                    title="Dismiss"
                    aria-label="Dismiss notification"
                    className="mt-0.5 grid h-5 w-5 shrink-0 place-items-center rounded-[var(--r-sm)] text-[var(--text-mute)] transition-colors hover:bg-[var(--surface-3)] hover:text-[var(--text)]"
                  >
                    <Icon icon="cross" size={12} />
                  </button>
                </div>
              ))}
            </div>
          )}

          {/* Per-tab empty state */}
          {activeNotifs.length === 0 && !showTasks && (
            <div className="flex flex-col items-center gap-2 py-8 text-center">
              <div className="grid h-10 w-10 place-items-center rounded-full border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ok)]">
                <Icon icon="tick-circle" size={20} />
              </div>
              <div className="text-[12.5px] font-semibold text-[var(--text)]">All clear</div>
              <div className="max-w-[220px] text-[11.5px] text-[var(--text-mute)]">
                {section === "alerts"
                  ? `No security alerts. Health ${score.total}%.`
                  : "No app notifications right now."}
              </div>
            </div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
