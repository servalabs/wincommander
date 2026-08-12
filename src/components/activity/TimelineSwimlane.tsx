// src/components/activity/TimelineSwimlane.tsx
//
// AGPL-3.0, part of the Free app's own viewer layer. The Pro fleet console has
// a separate implementation under the WinCommander EULA — do NOT sync or diff
// the two; `OPEN_CORE.md` places fleet services on the proprietary side of the
// boundary and this file on the public side.
//
// `onSelectApp` is wired to a no-op by this repo's caller (see
// WinCommanderActivityProductivity.tsx): there is no per-app drilldown here,
// because one would need multi-day history and this panel only ever loads a
// single local day.
//
// Continuous 24h activity ribbon — the ActivityWatch "Timeline" tab equivalent
// the hourly bar chart (ActivityTimelineChart.tsx) does not replace. Positions
// events proportionally on a zoomable [domainStart, domainEnd) window, colours
// them from `event.color` (server-supplied) with a category-derived fallback,
// and renders every uncovered span as an idle gap (the fixed props carry no
// separate idle-period list, so "idle" is simply "nothing scheduled here").

import { useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { formatActivityDuration, type ActivityTimelineEvent } from "./activityData";
import "./awParity.css";

type Props = {
  events: ActivityTimelineEvent[];
  onSelectApp: (app: string) => void;
};

export const DAY_SECONDS = 86400;
const MIN_BLOCK_PCT = 0.35;
const FALLBACK_COLOURS = ["var(--accent)", "var(--ok)", "var(--warn)", "var(--danger)", "var(--text-mute)"];

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export interface TimelineBlock {
  event: ActivityTimelineEvent;
  startSeconds: number;
  endSeconds: number;
  leftPct: number;
  widthPct: number;
}

/**
 * Positions events on a 0..100% track for [domainStart, domainEnd), clipping
 * partial overlaps at the domain edge and flooring sub-pixel-duration events
 * to MIN_BLOCK_PCT so they stay clickable. `startSeconds`/`endSeconds` on the
 * returned block are the REAL (unclipped-to-minimum) event times, so a
 * tooltip built from them never misreports an inflated duration.
 */
export function layoutBlocks(
  events: ActivityTimelineEvent[],
  domainStart = 0,
  domainEnd = DAY_SECONDS,
): TimelineBlock[] {
  const span = Math.max(1, domainEnd - domainStart);
  return events
    .map((event) => ({
      event,
      startSeconds: clamp(event.startSeconds, 0, DAY_SECONDS),
      endSeconds: clamp(event.endSeconds, 0, DAY_SECONDS),
    }))
    .filter(
      ({ startSeconds, endSeconds }) =>
        endSeconds > startSeconds && endSeconds > domainStart && startSeconds < domainEnd,
    )
    .map(({ event, startSeconds, endSeconds }) => {
      const visStart = Math.max(startSeconds, domainStart);
      const visEnd = Math.min(endSeconds, domainEnd);
      const leftPct = ((visStart - domainStart) / span) * 100;
      const widthPct = Math.max(MIN_BLOCK_PCT, ((visEnd - visStart) / span) * 100);
      return { event, startSeconds, endSeconds, leftPct, widthPct };
    })
    .sort((a, b) => a.startSeconds - b.startSeconds);
}

export interface IdleGap {
  leftPct: number;
  widthPct: number;
}

/** The complement of every event's coverage inside [domainStart, domainEnd). */
export function computeIdleGaps(
  events: ActivityTimelineEvent[],
  domainStart = 0,
  domainEnd = DAY_SECONDS,
): IdleGap[] {
  const span = Math.max(1, domainEnd - domainStart);
  const intervals = events
    .map((event): [number, number] => [
      clamp(event.startSeconds, 0, DAY_SECONDS),
      clamp(event.endSeconds, 0, DAY_SECONDS),
    ])
    .filter(([start, end]) => end > start)
    .map(([start, end]): [number, number] => [Math.max(start, domainStart), Math.min(end, domainEnd)])
    .filter(([start, end]) => end > start)
    .sort((a, b) => a[0] - b[0]);

  const gaps: IdleGap[] = [];
  let cursor = domainStart;
  for (const [start, end] of intervals) {
    if (start > cursor) {
      gaps.push({ leftPct: ((cursor - domainStart) / span) * 100, widthPct: ((start - cursor) / span) * 100 });
    }
    cursor = Math.max(cursor, end);
  }
  if (cursor < domainEnd) {
    gaps.push({ leftPct: ((cursor - domainStart) / span) * 100, widthPct: ((domainEnd - cursor) / span) * 100 });
  }
  return gaps;
}

function hashString(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i++) hash = (hash * 31 + value.charCodeAt(i)) >>> 0;
  return hash;
}

/** Deterministic colour for events the server didn't tag with `color`, keyed
 *  by the top-level category so the same category always renders the same hue. */
export function categoryFallbackColor(categoryPath: string[]): string {
  const key = categoryPath[0] ?? "";
  return FALLBACK_COLOURS[hashString(key) % FALLBACK_COLOURS.length];
}

export function formatClock(totalSeconds: number): string {
  const seconds = Math.max(0, Math.round(totalSeconds));
  const hh = Math.floor(seconds / 3600) % 24;
  const mm = Math.floor((seconds % 3600) / 60);
  const ss = seconds % 60;
  return [hh, mm, ss].map((n) => String(n).padStart(2, "0")).join(":");
}

export interface HourMark {
  hour: number;
  pct: number;
  showLabel: boolean;
}

export function computeHourMarks(domainStart: number, domainEnd: number): HourMark[] {
  const span = Math.max(1, domainEnd - domainStart);
  const totalHours = span / 3600;
  const step = totalHours > 12 ? 3 : totalHours > 6 ? 2 : 1;
  const firstHour = Math.ceil(domainStart / 3600);
  const lastHour = Math.floor(domainEnd / 3600);
  const marks: HourMark[] = [];
  for (let hour = firstHour; hour <= lastHour; hour++) {
    marks.push({ hour: hour % 24, pct: ((hour * 3600 - domainStart) / span) * 100, showLabel: hour % step === 0 });
  }
  return marks;
}

export function TimelineSwimlane({ events, onSelectApp }: Props) {
  const [domain, setDomain] = useState({ start: 0, end: DAY_SECONDS });
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const [dragStartPct, setDragStartPct] = useState<number | null>(null);
  const [dragEndPct, setDragEndPct] = useState<number | null>(null);
  const trackRef = useRef<HTMLDivElement | null>(null);
  const buttonRefs = useRef<Array<HTMLButtonElement | null>>([]);

  const blocks = useMemo(() => layoutBlocks(events, domain.start, domain.end), [events, domain]);
  const gaps = useMemo(() => computeIdleGaps(events, domain.start, domain.end), [events, domain]);
  const hourMarks = useMemo(() => computeHourMarks(domain.start, domain.end), [domain]);
  const isZoomed = domain.start !== 0 || domain.end !== DAY_SECONDS;
  const active = activeIndex !== null ? (blocks[activeIndex] ?? null) : null;

  function resetZoom() {
    setDomain({ start: 0, end: DAY_SECONDS });
    setActiveIndex(null);
  }

  function pctFromEvent(e: PointerEvent<HTMLDivElement>): number {
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect || rect.width === 0) return 0;
    return clamp(((e.clientX - rect.left) / rect.width) * 100, 0, 100);
  }

  function onTrackPointerDown(e: PointerEvent<HTMLDivElement>) {
    if (e.target !== e.currentTarget) return; // only the bare track starts a brush, not a block
    const pct = pctFromEvent(e);
    setDragStartPct(pct);
    setDragEndPct(pct);
  }

  function onTrackPointerMove(e: PointerEvent<HTMLDivElement>) {
    if (dragStartPct === null) return;
    setDragEndPct(pctFromEvent(e));
  }

  function onTrackPointerUp() {
    if (dragStartPct !== null && dragEndPct !== null && Math.abs(dragEndPct - dragStartPct) > 1) {
      const span = domain.end - domain.start;
      const lo = Math.min(dragStartPct, dragEndPct);
      const hi = Math.max(dragStartPct, dragEndPct);
      const newStart = Math.round(domain.start + (span * lo) / 100);
      const newEnd = Math.round(domain.start + (span * hi) / 100);
      if (newEnd - newStart >= 60) {
        setDomain({ start: newStart, end: newEnd });
        setActiveIndex(null);
      }
    }
    setDragStartPct(null);
    setDragEndPct(null);
  }

  function moveFocus(delta: number) {
    if (blocks.length === 0) return;
    const next = clamp((activeIndex ?? 0) + delta, 0, blocks.length - 1);
    setActiveIndex(next);
    buttonRefs.current[next]?.focus();
  }

  function onTrackKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key === "ArrowRight") {
      e.preventDefault();
      moveFocus(1);
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      moveFocus(-1);
    }
  }

  return (
    <section className="wc-fleet-section wc-fleet-swimlane" aria-label="Activity timeline swimlane">
      <div className="wc-fleet-swimlane-head">
        <h3>Timeline</h3>
        {isZoomed && (
          <button type="button" className="wc-fleet-swimlane-reset" onClick={resetZoom}>
            Reset zoom
          </button>
        )}
      </div>
      {events.length === 0 ? (
        <p className="wc-fleet-chart-empty">No timeline activity was reported for this range.</p>
      ) : (
        <>
          <div
            className="wc-fleet-swimlane-track"
            ref={trackRef}
            role="group"
            aria-label="Timeline blocks — left/right arrows move between activities, enter or space selects"
            onKeyDown={onTrackKeyDown}
            onPointerDown={onTrackPointerDown}
            onPointerMove={onTrackPointerMove}
            onPointerUp={onTrackPointerUp}
            onPointerLeave={() => {
              setDragStartPct(null);
              setDragEndPct(null);
            }}
          >
            {hourMarks.map((mark) => (
              <i
                key={`grid-${mark.hour}`}
                className="wc-fleet-swimlane-gridline"
                style={{ left: `${mark.pct}%` }}
                aria-hidden="true"
              />
            ))}
            {hourMarks
              .filter((mark) => mark.showLabel)
              .map((mark) => (
                <span
                  key={`label-${mark.hour}`}
                  className="wc-fleet-swimlane-hour"
                  style={{ left: `${mark.pct}%` }}
                  aria-hidden="true"
                >
                  {String(mark.hour).padStart(2, "0")}:00
                </span>
              ))}
            {gaps.map((gap, i) => (
              <i
                key={`idle-${i}`}
                className="wc-fleet-swimlane-idle"
                style={{ left: `${gap.leftPct}%`, width: `${gap.widthPct}%` }}
                aria-hidden="true"
              />
            ))}
            {blocks.map((block, i) => (
              <button
                key={block.event.id ?? `${block.startSeconds}-${i}`}
                type="button"
                ref={(el) => {
                  buttonRefs.current[i] = el;
                }}
                className="wc-fleet-swimlane-block"
                style={{
                  left: `${block.leftPct}%`,
                  width: `${block.widthPct}%`,
                  background: block.event.color || categoryFallbackColor(block.event.categoryPath),
                }}
                tabIndex={i === (activeIndex ?? 0) ? 0 : -1}
                aria-label={`${block.event.app || "Unknown application"}: ${block.event.title || block.event.label} — ${formatClock(block.startSeconds)} to ${formatClock(block.endSeconds)}, ${formatActivityDuration(block.endSeconds - block.startSeconds)}`}
                onFocus={() => setActiveIndex(i)}
                onMouseEnter={() => setActiveIndex(i)}
                onClick={() => onSelectApp(block.event.app)}
              />
            ))}
            {dragStartPct !== null && dragEndPct !== null && Math.abs(dragEndPct - dragStartPct) > 0.5 && (
              <i
                className="wc-fleet-swimlane-brush"
                style={{
                  left: `${Math.min(dragStartPct, dragEndPct)}%`,
                  width: `${Math.abs(dragEndPct - dragStartPct)}%`,
                }}
                aria-hidden="true"
              />
            )}
          </div>
          <output className="wc-fleet-swimlane-detail" aria-live="polite">
            {active ? (
              <>
                <b>{active.event.app || "Unknown application"}</b>
                <span>{active.event.title || active.event.label}</span>
                <span>
                  {formatClock(active.startSeconds)}–{formatClock(active.endSeconds)}
                </span>
                <span>{formatActivityDuration(active.endSeconds - active.startSeconds)}</span>
              </>
            ) : (
              <span>Hover or focus a block for details. Drag the track to zoom.</span>
            )}
          </output>
        </>
      )}
    </section>
  );
}
