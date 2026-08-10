// One title-bar inbox for both security notices and running operations.
// Fleet-specific alerts stay in the Fleet console; this menu is for the local
// WinCommander session only.  It intentionally has no "All clear" badge or
// empty-state affirmation — no badge simply means there is nothing pending.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";
import { Icon } from "./ui/icon";
import { ProcessChip } from "./ui/ProcessChip";
import { NotificationRow } from "./NotificationRow";
import { TITLEBAR_ICON_BTN } from "./ui/titleBarButtonClass";
import { useTaskStatus } from "../context/TaskStatusContext";
import {
  dismissNotification,
  listNotifications,
  subscribeNotifications,
  type AppNotification,
} from "../lib/notificationStore";
import { splitNotificationsByKind } from "../lib/badgeCount";
import "./badgePulse.css";

export default function NotificationsMenu() {
  const [open, setOpen] = useState(false);
  const [notifs, setNotifs] = useState<AppNotification[]>(() => listNotifications());
  const [badgePulsing, setBadgePulsing] = useState(false);
  const previousCount = useRef(0);
  const { tasks, clearCompleted } = useTaskStatus();
  const runningTasks = useMemo(() => tasks.filter((task) => task.status === "running"), [tasks]);
  const { alertNotifs, opsNotifs } = useMemo(() => splitNotificationsByKind(notifs), [notifs]);
  const notificationCount = notifs.length + runningTasks.length;
  const hasDanger = notifs.some((notification) => notification.severity === "danger");

  useEffect(() => {
    const onToggle = () => setOpen((shown) => !shown);
    window.addEventListener("toggle-notifications", onToggle);
    return () => window.removeEventListener("toggle-notifications", onToggle);
  }, []);

  useEffect(() => {
    const sync = () => setNotifs(listNotifications());
    sync();
    return subscribeNotifications(sync);
  }, []);

  useEffect(() => {
    const previous = previousCount.current;
    previousCount.current = notificationCount;
    if (!open && previous === 0 && notificationCount > 0) {
      setBadgePulsing(true);
      const timeout = window.setTimeout(() => setBadgePulsing(false), 320);
      return () => window.clearTimeout(timeout);
    }
  }, [notificationCount, open]);

  const dismissOne = useCallback((event: React.MouseEvent, id: string) => {
    event.stopPropagation();
    dismissNotification(id);
  }, []);

  const clearNotifications = useCallback(() => {
    notifs.forEach((notification) => dismissNotification(notification.id));
  }, [notifs]);

  const clearFinished = useCallback(() => clearCompleted(), [clearCompleted]);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        className={`relative ${TITLEBAR_ICON_BTN}`}
        title="Notifications and processes"
        aria-label="Notifications and processes"
        data-tauri-drag-region={false}
      >
        <Icon icon="notifications" size={15} />
        {notificationCount > 0 && (
          <span
            key={badgePulsing ? "pulse" : "idle"}
            className={`absolute right-1 top-1 grid h-3.5 min-w-[14px] place-items-center rounded-full px-0.5 font-[family-name:var(--font-mono)] text-[8px] font-bold leading-none text-[var(--accent-contrast)]${badgePulsing ? " wc-badge-pulse" : ""}`}
            style={{ background: hasDanger ? "var(--danger)" : "var(--warn)" }}
          >
            {notificationCount}
          </span>
        )}
      </PopoverTrigger>
      <PopoverContent align="end" sideOffset={8} className="wc-popover-caret relative w-80 overflow-visible p-0 duration-[var(--dur-normal)] [animation-timing-function:var(--ease)]">
        <div className="flex items-center gap-2 border-b border-[var(--border)] px-3.5 py-2.5">
          <Icon icon="notifications" size={13} style={{ color: "var(--text-dim)" }} />
          <div className="text-[13px] font-semibold text-[var(--text)]">Notifications</div>
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
          {tasks.length > 0 && (
            <section className="mb-3 flex flex-col gap-1.5">
              <div className="flex items-center justify-between px-0.5">
                <div className="font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--text-mute)]">
                  Processes{runningTasks.length > 0 ? ` · ${runningTasks.length} running` : ""}
                </div>
                {tasks.some((task) => task.status === "completed" || task.status === "failed") && (
                  <button onClick={clearFinished} className="font-[family-name:var(--font-mono)] text-[10px] text-[var(--text-mute)] underline underline-offset-2 hover:text-[var(--text-dim)]">
                    Clear finished
                  </button>
                )}
              </div>
              {tasks.map((task) => <ProcessChip key={task.id} task={task} />)}
            </section>
          )}

          {notifs.length > 0 && (
            <section className="flex flex-col gap-1.5">
              <div className="flex items-center justify-between px-0.5">
                <div className="font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--text-mute)]">Alerts</div>
                <button onClick={clearNotifications} className="font-[family-name:var(--font-mono)] text-[10px] text-[var(--text-mute)] underline underline-offset-2 hover:text-[var(--text-dim)]">
                  Clear notifications
                </button>
              </div>
              {[...alertNotifs, ...opsNotifs].map((notification) => (
                <NotificationRow key={notification.id} notification={notification} onDismiss={dismissOne} />
              ))}
            </section>
          )}

          {tasks.length === 0 && notifs.length === 0 && (
            <div className="py-6 text-center text-[11.5px] text-[var(--text-mute)]">No pending notifications or processes.</div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
