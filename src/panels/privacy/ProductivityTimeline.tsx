// src/panels/privacy/ProductivityTimeline.tsx
//
// Chronological timeline + day-rollup view for Argus App-Usage windows.
// Receives already-fetched data from ArgusAppUsageSection — NO separate
// Tauri invoke here. Privacy invariant preserved: only aggregate scalars.

import { useState } from 'react';
import { AlignJustify, Clock } from 'lucide-react';
import './ProductivityTimeline.css';
import type { ArgusWindowSlot } from './ArgusAppUsageSection';

// ── Re-export so consumers can import from a single location ──────────
export type { ArgusWindowSlot };

// ── Props ─────────────────────────────────────────────────────────────

export interface ProductivityTimelineProps {
  /** Already-fetched window slots from argus_app_usage_recent (newest→oldest) */
  windows: ArgusWindowSlot[];
}

// ── Helpers ───────────────────────────────────────────────────────────

type RatioTier = 'high' | 'mid' | 'low';

function ratioTier(ratio: number): RatioTier {
  if (ratio >= 0.7) return 'high';
  if (ratio >= 0.4) return 'mid';
  return 'low';
}

function activeRatio(slot: ArgusWindowSlot): number {
  const total = slot.activeSeconds + slot.idleSeconds;
  return total > 0 ? slot.activeSeconds / total : 0;
}

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  const m = Math.floor(seconds / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return rem === 0 ? `${h}h` : `${h}h ${rem}m`;
}

/** Returns "HH:MM" from an ISO string */
function hhmm(iso: string): string {
  const t = iso.slice(11, 16);
  return t.length === 5 ? t : '';
}

/** Returns a stable day key ("YYYY-MM-DD") from an ISO string */
function dayKey(iso: string): string {
  return iso.slice(0, 10);
}

/** Human-readable day label: "Today", "Yesterday", or short date */
function dayLabel(key: string): string {
  const today = new Date().toISOString().slice(0, 10);
  if (key === today) return 'Today';
  const yest = new Date(Date.now() - 86_400_000).toISOString().slice(0, 10);
  if (key === yest) return 'Yesterday';
  // e.g. "Jun 24"
  const d = new Date(key + 'T00:00:00');
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

// ── Sub-components ────────────────────────────────────────────────────

interface DaySummaryProps {
  slots: ArgusWindowSlot[];
  label: string;
}

function DaySummary({ slots, label }: DaySummaryProps) {
  const totalActive = slots.reduce((s, w) => s + w.activeSeconds, 0);
  const totalIdle = slots.reduce((s, w) => s + w.idleSeconds, 0);
  const total = totalActive + totalIdle;
  const ratio = total > 0 ? totalActive / total : 0;
  const tier = ratioTier(ratio);
  const pct = Math.round(ratio * 100);

  return (
    <div className="pt-day-header">
      <span className="pt-day-label">{label}</span>
      <div className="pt-day-bar-wrap">
        <div className="pt-day-bar-track">
          <div
            className={`pt-day-bar-fill pt-day-bar-fill--${tier}`}
            style={{ width: `${pct}%` }}
          />
        </div>
      </div>
      <span className="pt-day-meta">
        {formatDuration(totalActive)} active · {formatDuration(totalIdle)} idle
      </span>
    </div>
  );
}

interface TimelineSlotProps {
  slot: ArgusWindowSlot;
  isLast: boolean;
}

function TimelineSlot({ slot, isLast }: TimelineSlotProps) {
  const ratio = activeRatio(slot);
  const tier = ratioTier(ratio);
  const total = slot.activeSeconds + slot.idleSeconds;
  const pct = Math.round(ratio * 100);
  const topScores = Object.entries(slot.categoryScores)
    .sort(([, a], [, b]) => b - a)
    .slice(0, 3);

  return (
    <div className={`pt-slot${isLast ? ' pt-slot--last' : ''}`}>
      <div className="pt-slot-time">
        <div className={`pt-slot-dot pt-slot-dot--${tier}`} />
      </div>

      <div className="pt-slot-card">
        {/* Header: time range + category badge */}
        <div className="pt-slot-head">
          <span className="pt-slot-range">
            {hhmm(slot.windowStart)}–{hhmm(slot.windowEnd)}
          </span>
          <span className="pt-slot-category">{slot.topCategory}</span>
        </div>

        {/* Active/idle proportion bar */}
        <div className="pt-slot-bar-track">
          <div
            className={`pt-slot-bar-fill pt-slot-bar-fill--${tier}`}
            style={{ width: `${pct}%` }}
          />
        </div>

        {/* Stats row */}
        <div className="pt-slot-stats">
          <span className="pt-slot-stat--active">{formatDuration(slot.activeSeconds)} active</span>
          <span className="pt-slot-stat--idle">{formatDuration(slot.idleSeconds)} idle</span>
          <span className="pt-slot-stat--total">/ {formatDuration(total)}</span>
        </div>

        {/* Category score chips — top 3 */}
        {topScores.length > 0 && (
          <div className="pt-score-chips">
            {topScores.map(([cat, score]) => (
              <span key={cat} className="pt-score-chip">
                {cat} {Math.round(score * 100)}%
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ── List view (flat, identical to original but without duplication) ───

interface FlatListProps {
  windows: ArgusWindowSlot[];
}

function FlatList({ windows }: FlatListProps) {
  return (
    <div className="max-h-[260px] overflow-auto rounded border border-[var(--color-border)]" role="region" aria-label="Recent app-usage window records">
      <table className="w-full text-left text-[11px]" aria-label="Recent app-usage window records">
        <thead className="sticky top-0 bg-[var(--color-bg-secondary)] text-[var(--shield-text-muted)]">
          <tr>
            <th scope="col" className="px-2 py-1.5">Time</th>
            <th scope="col" className="px-2 py-1.5">Category</th>
            <th scope="col" className="px-2 py-1.5 text-right">Active</th>
            <th scope="col" className="px-2 py-1.5 text-right">Idle</th>
            <th scope="col" className="px-2 py-1.5 text-right">Active share</th>
            <th scope="col" className="px-2 py-1.5">Top scores</th>
          </tr>
        </thead>
        <tbody>
      {[...windows].reverse().map((slot, i) => {
        const ratio = activeRatio(slot);
        const pct = Math.round(ratio * 100);
        const topScores = Object.entries(slot.categoryScores)
          .sort(([, a], [, b]) => b - a)
          .slice(0, 3);

        return (
          <tr key={`${slot.windowStart}-${i}`} className="border-t border-[var(--color-border)]">
            <td className="whitespace-nowrap px-2 py-1.5 font-mono opacity-70">{slot.windowStart.slice(0, 16).replace('T', ' ')}–{slot.windowEnd.slice(11, 16)}</td>
            <td className="px-2 py-1.5">{slot.topCategory}</td>
            <td className="px-2 py-1.5 text-right font-mono">{formatDuration(slot.activeSeconds)}</td>
            <td className="px-2 py-1.5 text-right font-mono">{formatDuration(slot.idleSeconds)}</td>
            <td className="px-2 py-1.5 text-right font-mono">{pct}%</td>
            <td className="px-2 py-1.5 font-mono opacity-70">{topScores.map(([cat, score]) => `${cat} ${Math.round(score * 100)}%`).join(', ') || '—'}</td>
          </tr>
        );
      })}
        </tbody>
      </table>
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────

type ViewMode = 'list' | 'timeline';

export default function ProductivityTimeline({ windows }: ProductivityTimelineProps) {
  const [view, setView] = useState<ViewMode>('timeline');

  if (windows.length === 0) return null;

  // Sort newest→oldest (the backend may or may not guarantee order)
  const sorted = [...windows].sort(
    (a, b) => Date.parse(b.windowStart) - Date.parse(a.windowStart),
  );

  // Aggregate totals across all visible windows
  const totalActive = sorted.reduce((s, w) => s + w.activeSeconds, 0);
  const totalIdle = sorted.reduce((s, w) => s + w.idleSeconds, 0);
  const grandTotal = totalActive + totalIdle;
  const grandRatio = grandTotal > 0 ? totalActive / grandTotal : 0;
  const grandPct = Math.round(grandRatio * 100);
  const grandActiveWidth = `${grandPct}%`;
  const grandIdleWidth = `${Math.round((1 - grandRatio) * 100)}%`;

  // Group by day for timeline view
  const dayGroups: Array<{ key: string; slots: ArgusWindowSlot[] }> = [];
  for (const slot of sorted) {
    const k = dayKey(slot.windowStart);
    const last = dayGroups[dayGroups.length - 1];
    if (last && last.key === k) {
      last.slots.push(slot);
    } else {
      dayGroups.push({ key: k, slots: [slot] });
    }
  }

  return (
    <div className="flex flex-col gap-3">
      {/* ── Section header + view toggle ── */}
      <div className="flex items-center justify-between gap-2 flex-wrap">
        <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
          Recent windows ({windows.length})
        </span>
        <div className="pt-toggle" role="group" aria-label="View mode">
          <button
            type="button"
            className={`pt-toggle-btn${view === 'list' ? ' pt-toggle-btn--active' : ''}`}
            onClick={() => setView('list')}
            aria-pressed={view === 'list'}
            aria-label="Show app-usage table"
          >
            <AlignJustify size={10} />
            List
          </button>
          <button
            type="button"
            className={`pt-toggle-btn${view === 'timeline' ? ' pt-toggle-btn--active' : ''}`}
            onClick={() => setView('timeline')}
            aria-pressed={view === 'timeline'}
            aria-label="Show app-usage timeline"
          >
            <Clock size={10} />
            Timeline
          </button>
        </div>
      </div>

      {/* ── Compact summary bar (always visible) ── */}
      <div className="pt-summary">
        <span className="pt-summary-label">Total — {windows.length} windows</span>
        <div className="pt-summary-bar-row">
          <div className="pt-summary-bar-track">
            <div className="pt-summary-bar-active" style={{ width: grandActiveWidth }} />
            <div className="pt-summary-bar-idle" style={{ width: grandIdleWidth }} />
          </div>
        </div>
        <div className="pt-summary-stats">
          <span className="pt-summary-stat--active">{formatDuration(totalActive)} active</span>
          <span className="pt-summary-stat--idle">{formatDuration(totalIdle)} idle</span>
        </div>
      </div>

      {/* ── Main view ── */}
      {view === 'list' ? (
        <FlatList windows={windows} />
      ) : (
        <div className="flex flex-col gap-0 max-h-[340px] overflow-y-auto pr-1">
          {dayGroups.map((group) => (
            <div key={group.key} className="flex flex-col gap-0">
              {/* Day header with rollup bar only when multiple days exist */}
              {dayGroups.length > 1 && (
                <DaySummary slots={group.slots} label={dayLabel(group.key)} />
              )}
              {group.slots.map((slot, i) => (
                <TimelineSlot
                  key={`${slot.windowStart}-${i}`}
                  slot={slot}
                  isLast={i === group.slots.length - 1}
                />
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
