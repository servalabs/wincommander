import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Button, Icon, InputGroup, Menu, MenuDivider, MenuItem } from "@/components/ui/bp";

interface CleanupScheduleControlProps {
  categoryLabel: string;
  minInterval: number;
  scheduleMinutes?: number | null;
  scheduleDisabled?: boolean;
  schedulePayloadVerified?: boolean | null;
  scheduleBusy?: boolean;
  scheduleState?: "checking" | "verified" | "unavailable";
  onRefreshSchedule?: () => void;
  onSetSchedule?: (minutes: number) => boolean | Promise<boolean>;
  onClearSchedule?: () => boolean | Promise<boolean>;
  onRequestScheduleAccess?: () => void;
}

const PRESET_INTERVALS = [
  { label: "Every 5 min", minutes: 5 },
  { label: "Every 15 min", minutes: 15 },
  { label: "Hourly", minutes: 60 },
  { label: "Every 6 hours", minutes: 360 },
  { label: "Every 24 hours", minutes: 1440 },
];

const SURFACE_WIDTH = 288;
const VIEWPORT_MARGIN = 8;

function formatInterval(minutes: number): string {
  if (minutes >= 1440 && minutes % 1440 === 0) return `${minutes / 1440}d`;
  if (minutes >= 60 && minutes % 60 === 0) return `${minutes / 60}h`;
  return `${minutes}m`;
}

export default function CleanupScheduleControl({
  categoryLabel,
  minInterval,
  scheduleMinutes,
  scheduleDisabled = false,
  schedulePayloadVerified,
  scheduleBusy = false,
  scheduleState = "verified",
  onRefreshSchedule,
  onSetSchedule,
  onClearSchedule,
  onRequestScheduleAccess,
}: CleanupScheduleControlProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [customMinutes, setCustomMinutes] = useState("");
  const [position, setPosition] = useState<{
    left: number;
    top: number;
    width: number;
    visibility: "hidden" | "visible";
  }>({ left: 0, top: 0, width: SURFACE_WIDTH, visibility: "hidden" });
  const triggerRef = useRef<HTMLButtonElement>(null);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const surfaceId = useId();
  const isScheduled = scheduleState === "verified" && typeof scheduleMinutes === "number" && scheduleMinutes > 0;
  const isDisabled = scheduleState === "verified" && scheduleDisabled;
  const payloadUnverified = (isScheduled || isDisabled) && schedulePayloadVerified !== true;
  const portalTarget = triggerRef.current
    ?.closest<HTMLElement>('[data-cleanup-panel-root="true"]')
    ?.querySelector<HTMLElement>('[data-cleanup-overlay-root="true"]');

  useEffect(() => {
    if (scheduleState !== "verified") setIsOpen(false);
  }, [scheduleState]);

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current;
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const surfaceHeight = surfaceRef.current?.getBoundingClientRect().height ?? 280;
    const surfaceWidth = Math.min(SURFACE_WIDTH, Math.max(0, window.innerWidth - VIEWPORT_MARGIN * 2));
    const fitsBelow = rect.bottom + 6 + surfaceHeight <= window.innerHeight - VIEWPORT_MARGIN;
    const top = fitsBelow
      ? rect.bottom + 6
      : Math.max(VIEWPORT_MARGIN, rect.top - surfaceHeight - 6);
    const left = Math.min(
      window.innerWidth - surfaceWidth - VIEWPORT_MARGIN,
      Math.max(VIEWPORT_MARGIN, rect.right - surfaceWidth),
    );
    setPosition({ left, top, width: surfaceWidth, visibility: "visible" });
  }, []);

  useLayoutEffect(() => {
    if (isOpen) updatePosition();
  }, [isOpen, updatePosition]);

  useEffect(() => {
    if (!isOpen || !portalTarget) return;
    const frame = window.requestAnimationFrame(() => {
      surfaceRef.current
        ?.querySelector<HTMLElement>('button:not([disabled]), input:not([disabled]), [role="menuitem"]:not([aria-disabled="true"])')
        ?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [isOpen, portalTarget]);

  const closeAndReturnFocus = useCallback(() => {
    setIsOpen(false);
    window.requestAnimationFrame(() => triggerRef.current?.focus());
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (triggerRef.current?.contains(target) || surfaceRef.current?.contains(target)) return;
      setIsOpen(false);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeAndReturnFocus();
      }
    };
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    document.addEventListener("pointerdown", handlePointerDown, true);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
      document.removeEventListener("pointerdown", handlePointerDown, true);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [closeAndReturnFocus, isOpen, updatePosition]);

  if (!onSetSchedule && !((isScheduled || isDisabled) && onClearSchedule)) {
    if (!onRequestScheduleAccess) return isDisabled ? <span aria-label={`${categoryLabel} scheduled wipe is disabled`} title="Scheduled wipe is disabled">OFF</span> : null;
    return (
      <button
        type="button"
        ref={triggerRef}
        className="flex h-[22px] min-w-[22px] flex-shrink-0 items-center gap-0.5 rounded px-1.5 transition-colors"
        data-trace-card-action="schedule"
        data-cleanup-schedule-control="true"
        data-active={isScheduled && !payloadUnverified ? "true" : "false"}
        aria-label={payloadUnverified ? `${categoryLabel} scheduled wipe script or interval not verified` : isDisabled ? `${categoryLabel} scheduled wipe is disabled` : isScheduled ? `Change ${categoryLabel} scheduled wipe` : `Unlock ${categoryLabel} scheduled wipe`}
        title={payloadUnverified ? "Task exists, but its cleanup script or wipe interval could not be verified · Pro required to change" : isDisabled ? "Scheduled wipe is disabled · Pro required to change" : isScheduled ? `Scheduled wipe every ${formatInterval(scheduleMinutes!)} · Pro required to change` : "Unlock scheduled wipe"}
        onClick={event => {
          event.preventDefault();
          event.stopPropagation();
          onRequestScheduleAccess();
        }}
      >
        <Icon icon="time" size={14} />
        {isScheduled && <span className="text-[8.5px] font-extrabold">{payloadUnverified ? "?" : formatInterval(scheduleMinutes!)}</span>}
        {isDisabled && <span className="text-[8.5px] font-extrabold">OFF</span>}
        <Icon icon="lock" size={9} />
      </button>
    );
  }

  const applySchedule = async (minutes: number) => {
    try {
      if (!onSetSchedule) return;
      const succeeded = await onSetSchedule(Math.max(minInterval, minutes));
      if (succeeded) {
        setCustomMinutes("");
        closeAndReturnFocus();
      }
    } catch {
      // The backend owner reports the error; keep the editor open for retry.
    }
  };

  const clearSchedule = async () => {
    try {
      if (!onClearSchedule) return;
      const succeeded = await onClearSchedule();
      if (succeeded) closeAndReturnFocus();
    } catch {
      // The backend owner reports the error; keep the editor open for retry.
    }
  };

  return (
    <>
      <button
        type="button"
        ref={triggerRef}
        className="flex h-[22px] min-w-[22px] flex-shrink-0 items-center gap-0.5 rounded px-1.5 transition-colors"
        data-trace-card-action="schedule"
        data-cleanup-schedule-control="true"
        data-active={isScheduled && !payloadUnverified ? "true" : "false"}
        aria-label={scheduleState === "unavailable" ? `Retry checking ${categoryLabel} scheduled wipe` : scheduleState === "checking" ? `Checking ${categoryLabel} scheduled wipe` : payloadUnverified ? `Manage ${categoryLabel} scheduled wipe; script or interval not verified` : isDisabled ? `Manage disabled ${categoryLabel} scheduled wipe` : isScheduled ? `${onSetSchedule ? "Change" : "Manage"} ${categoryLabel} scheduled wipe` : `Schedule a wipe for ${categoryLabel}`}
        disabled={scheduleState === "checking" || scheduleBusy}
        title={scheduleState === "checking" ? "Checking Task Scheduler…" : scheduleState === "unavailable" ? "Schedule status unavailable — click to retry" : payloadUnverified ? "Task exists, but its cleanup script or wipe interval could not be verified. Remove and recreate this schedule." : isDisabled ? "Scheduled wipe is disabled" : isScheduled ? `Scheduled wipe every ${formatInterval(scheduleMinutes!)}` : "Schedule wipe"}
        aria-haspopup="dialog"
        aria-expanded={isOpen}
        aria-controls={isOpen ? surfaceId : undefined}
        onClick={event => {
          event.preventDefault();
          event.stopPropagation();
          if (scheduleState === "unavailable") { onRefreshSchedule?.(); return; }
          setIsOpen(open => !open);
        }}
      >
        <Icon icon="time" size={14} />
        {isScheduled && <span className="text-[8.5px] font-extrabold">{payloadUnverified ? "?" : formatInterval(scheduleMinutes!)}</span>}
        {isDisabled && <span className="text-[8.5px] font-extrabold">OFF</span>}
      </button>

      {isOpen && portalTarget && createPortal(
        <div
          ref={surfaceRef}
          id={surfaceId}
          role="dialog"
          aria-label={`${categoryLabel} scheduled wipe`}
          aria-busy={scheduleBusy}
          data-cleanup-schedule-surface
          data-cleanup-schedule-menu="true"
          data-state="open"
          className="pointer-events-auto rounded-[var(--r)] border border-[var(--border-strong)] bg-[var(--surface)] p-3 text-[var(--text)] shadow-[var(--shadow)]"
          style={{
            position: "fixed",
            zIndex: "var(--z-popover)",
            width: position.width,
            maxHeight: `calc(100vh - ${VIEWPORT_MARGIN * 2}px)`,
            overflowY: "auto",
            left: position.left,
            top: position.top,
            visibility: position.visibility,
          }}
          onPointerDown={event => event.stopPropagation()}
          onClick={event => event.stopPropagation()}
        >
          <Menu>
            <MenuDivider title={payloadUnverified ? "Task exists; script or interval not verified" : isDisabled ? "Scheduled wipe is disabled" : isScheduled ? `Wipes every ${formatInterval(scheduleMinutes!)}` : "Wipe interval"} />
            {onSetSchedule && PRESET_INTERVALS.filter(preset => preset.minutes >= minInterval).map(preset => (
              <MenuItem
                key={preset.minutes}
                icon={isScheduled && scheduleMinutes === preset.minutes ? "tick" : "blank"}
                text={preset.label}
                disabled={scheduleBusy}
                onClick={() => void applySchedule(preset.minutes)}
              />
            ))}
            {onSetSchedule && <><MenuDivider />
            <li className="bp5-menu-item-custom" style={{ padding: "6px 8px" }}>
              <div className="flex items-center gap-1">
                <InputGroup
                  small
                  type="number"
                  aria-label={`${categoryLabel} custom wipe interval in minutes`}
                  placeholder={`Custom (min ${minInterval})`}
                  value={customMinutes}
                  onChange={event => setCustomMinutes(event.target.value)}
                  onKeyDown={event => {
                    if (event.key !== "Enter" || scheduleBusy) return;
                    const minutes = Number.parseInt(customMinutes, 10);
                    if (Number.isFinite(minutes)) void applySchedule(minutes);
                  }}
                  style={{ width: 140 }}
                />
                <Button
                  small
                  minimal
                  icon="tick"
                  aria-label={`Apply custom schedule for ${categoryLabel}`}
                  disabled={scheduleBusy || !customMinutes}
                  onClick={() => {
                    const minutes = Number.parseInt(customMinutes, 10);
                    if (Number.isFinite(minutes)) void applySchedule(minutes);
                  }}
                />
              </div>
              <div className="mt-1 text-[9px] text-[var(--color-text-muted)]">minutes · min {minInterval}</div>
            </li></>}
            {(isScheduled || isDisabled) && onClearSchedule && (
              <>
                <MenuDivider />
                <MenuItem
                  icon="cross"
                  text={isDisabled ? "Remove disabled schedule" : "Turn off scheduled wipe"}
                  intent="danger"
                  disabled={scheduleBusy}
                  onClick={() => void clearSchedule()}
                />
              </>
            )}
          </Menu>
        </div>,
        portalTarget,
      )}
    </>
  );
}
