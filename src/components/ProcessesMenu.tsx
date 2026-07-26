// src/components/ProcessesMenu.tsx
//
// Processes — title-bar popover for TaskStatusContext (multi-step
// runOperation tasks: self-destruct, volume create, shred, app updates...).
// Split out of the old hybrid NotificationsMenu "Processes" tab — which
// actually mixed running tasks with operational notifications — so this is
// now a pure, single-domain surface. See AlertsMenu.tsx for the
// notificationStore counterpart.
//
// This popover is the ONLY place running tasks render. The old floating
// TaskStatusBar card was already dead code (unmounted, its removal
// documented in App.tsx) and has been deleted — do not resurrect it as a
// second floating surface; that recreates the duplicate-surface problem its
// removal fixed.
import { useEffect, useMemo, useRef, useState } from "react";
import { Popover, PopoverTrigger, PopoverContent } from "./ui/popover";
import { Icon } from "./ui/icon";
import { ProcessChip } from "./ui/ProcessChip";
import { TITLEBAR_ICON_BTN } from "./ui/titleBarButtonClass";
import { useTaskStatus } from "../context/TaskStatusContext";
import { processesBadge } from "../lib/badgeCount";
import "./badgePulse.css";

export default function ProcessesMenu() {
  const [open, setOpen] = useState(false);
  const [badgePulsing, setBadgePulsing] = useState(false);
  const prevBadgeRef = useRef(0);
  const { tasks, clearCompleted } = useTaskStatus();
  const runningTasks = useMemo(() => tasks.filter((t) => t.status === "running"), [tasks]);

  useEffect(() => {
    const onToggle = () => setOpen((o) => !o);
    window.addEventListener("toggle-processes-menu", onToggle);
    return () => window.removeEventListener("toggle-processes-menu", onToggle);
  }, []);

  // Badge is the running-task count only (see badgeCount.ts). Completed/
  // failed tasks still show in the list below (their own auto-dismiss timer
  // clears them) but don't count — "Processes" should read as "N things
  // happening right now".
  const { count: badgeCount, color: badgeColor } = useMemo(() => processesBadge(tasks), [tasks]);

  useEffect(() => {
    const prev = prevBadgeRef.current;
    prevBadgeRef.current = badgeCount;
    if (!open && prev === 0 && badgeCount > 0) {
      setBadgePulsing(true);
      const id = window.setTimeout(() => setBadgePulsing(false), 320);
      return () => window.clearTimeout(id);
    }
  }, [badgeCount, open]);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className={`relative ${TITLEBAR_ICON_BTN}`} title="Processes" aria-label="Processes" data-tauri-drag-region={false}>
        <Icon icon="processes" size={15} />
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
          AlertsMenu so both popups connect to their own trigger with the
          same diamond shape. */}
      <PopoverContent align="end" sideOffset={8} className="wc-popover-caret w-80 p-0 relative overflow-visible duration-[var(--dur-normal)] [animation-timing-function:var(--ease)]">
        <div className="flex items-center gap-2 border-b border-[var(--border)] px-3.5 py-2.5">
          <Icon icon="processes" size={13} style={{ color: "var(--text-dim)" }} />
          <div className="text-[13px] font-semibold text-[var(--text)]">Processes</div>
          {badgeCount > 0 ? (
            <span className="grid h-[18px] min-w-[18px] place-items-center rounded-full bg-[var(--surface-3)] px-1.5 font-[family-name:var(--font-mono)] text-[10px] font-bold leading-none text-[var(--text-dim)]">
              {badgeCount}
            </span>
          ) : (
            <span className="font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--text-mute)]">
              Idle
            </span>
          )}
          <button
            type="button"
            onClick={() => setOpen(false)}
            title="Close"
            aria-label="Close processes"
            className="ml-auto grid h-6 w-6 shrink-0 place-items-center rounded-[var(--r-sm)] text-[var(--text-mute)] transition-colors hover:bg-[var(--surface-2)] hover:text-[var(--text)]"
          >
            <Icon icon="cross" size={12} />
          </button>
        </div>

        <div className="max-h-[420px] overflow-y-auto p-2">
          {tasks.length > 0 ? (
            <div className="mb-1 flex flex-col gap-1.5">
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
          ) : (
            <div className="flex flex-col items-center gap-2 py-8 text-center">
              <div className="grid h-10 w-10 place-items-center rounded-full border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ok)]">
                <Icon icon="tick-circle" size={20} />
              </div>
              <div className="text-[12.5px] font-semibold text-[var(--text)]">Nothing running</div>
              <div className="max-w-[220px] text-[11.5px] text-[var(--text-mute)]">
                Operations you start (shred, volume create, self-destruct…) show up here.
              </div>
            </div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
